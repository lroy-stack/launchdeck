//! Local WebSocket interface for external events.
//!
//! Security properties (see docs/architecture.md):
//! * binds to `127.0.0.1` only, never `0.0.0.0`
//! * accepts *events* only. There is no message that executes an action,
//!   a shell command or anything else; at most it changes pad state
//! * rejects browser handshakes whose `Origin` is not local
//! * bounded message size and connection count
//!
//! One extension point exists: an [`Extension`] may claim messages whose
//! `type` starts with `agent.` (the Agent Bridge, see
//! docs/agent-control/bridge-protocol.md) and may push frames back to the
//! connection it came from. The extension decides nothing about transport
//! security; everything above applies to it unchanged.

use std::net::{Ipv4Addr, SocketAddr};
use std::sync::Arc;

use futures_util::{SinkExt, StreamExt};
use std::sync::atomic::{AtomicU64, Ordering};

use serde::Deserialize;
use serde_json::{json, Value};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{mpsc, Semaphore};
use tokio_tungstenite::tungstenite::handshake::server::{ErrorResponse, Request, Response};
use tokio_tungstenite::tungstenite::http::StatusCode;
use tokio_tungstenite::tungstenite::protocol::{Message, WebSocketConfig};

use super::{Event, EventBus};

const MAX_MESSAGE_BYTES: usize = 64 * 1024;
const MAX_CONNECTIONS: usize = 16;
/// Frames queued for one connection before the client counts as not reading.
pub const OUTBOUND_QUEUE: usize = 64;

/// Handler for the `agent.*` message family. Implemented by the Agent Bridge.
pub trait Extension: Send + Sync {
    /// A client connected. `outbound` carries text frames to send to it.
    fn connected(&self, conn: u64, outbound: mpsc::Sender<String>);
    /// A message whose `type` starts with `agent.`. Returns the replies to send.
    fn message(&self, conn: u64, message: &Value) -> Vec<Value>;
    /// The client went away.
    fn disconnected(&self, conn: u64);
}

static NEXT_CONN: AtomicU64 = AtomicU64::new(1);

/// Sources the runtime uses for its own events; external clients may not spoof them.
const RESERVED_SOURCES: &[&str] = &["runtime", "device", "agent-control"];

#[derive(Debug, Deserialize)]
struct Inbound {
    #[serde(rename = "type")]
    kind: String,
    #[serde(default)]
    source: Option<String>,
    #[serde(default)]
    event: Option<String>,
    #[serde(default)]
    payload: Option<Value>,
}

#[derive(Debug, PartialEq)]
pub enum Parsed {
    Event(Event),
    Ping,
}

/// Identifier grammar shared by event sources, event types and plugin names.
pub(crate) fn valid_ident(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 64
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | ':' | '-'))
}

/// Tells whether a source name is already taken by something inside LaunchDeck
/// (an installed plugin, for example), so a client may not publish as it.
pub type SourceGuard = Arc<dyn Fn(&str) -> bool + Send + Sync>;

/// Parse one inbound text frame. Pure, so it is unit tested.
pub fn parse_inbound(text: &str) -> Result<Parsed, String> {
    parse_inbound_with(text, None)
}

/// [`parse_inbound`] with an extra rule for which sources are reserved.
pub fn parse_inbound_with(text: &str, taken: Option<&SourceGuard>) -> Result<Parsed, String> {
    if text.len() > MAX_MESSAGE_BYTES {
        return Err("message too large".into());
    }
    let msg: Inbound = serde_json::from_str(text).map_err(|e| format!("invalid JSON: {e}"))?;
    match msg.kind.as_str() {
        "ping" => Ok(Parsed::Ping),
        "event" => {
            let source = msg.source.ok_or("missing `source`")?;
            let kind = msg.event.ok_or("missing `event`")?;
            if !valid_ident(&source) || !valid_ident(&kind) {
                return Err("`source` and `event` must be 1-64 chars of [A-Za-z0-9_.:-]".into());
            }
            if RESERVED_SOURCES.contains(&source.as_str())
                || taken.is_some_and(|is_taken| is_taken(&source))
            {
                return Err(format!("source `{source}` is reserved"));
            }
            Ok(Parsed::Event(Event::new(
                source,
                kind,
                msg.payload.unwrap_or(Value::Null),
            )))
        }
        other => Err(format!(
            "unsupported message type `{other}`; only `event` and `ping` are accepted"
        )),
    }
}

fn origin_allowed(origin: &str) -> bool {
    let rest = origin.split_once("://").map(|(_, r)| r).unwrap_or(origin);
    let authority = rest.split('/').next().unwrap_or("");
    let host = match authority.rsplit_once(':') {
        Some((h, p)) if !p.is_empty() && p.chars().all(|c| c.is_ascii_digit()) => h,
        _ => authority,
    };
    matches!(
        host,
        "localhost" | "127.0.0.1" | "tauri.localhost" | "[::1]"
    )
}

// The error type is fixed by the tungstenite `Callback` trait.
#[allow(clippy::result_large_err)]
fn check_handshake(req: &Request, resp: Response) -> Result<Response, ErrorResponse> {
    if let Some(origin) = req.headers().get("origin").and_then(|v| v.to_str().ok()) {
        if !origin_allowed(origin) {
            let mut err = ErrorResponse::new(Some("origin not allowed".to_string()));
            *err.status_mut() = StatusCode::FORBIDDEN;
            return Err(err);
        }
    }
    Ok(resp)
}

/// Start the server. Returns the bound address (useful when `port == 0`).
pub async fn serve(port: u16, bus: EventBus) -> std::io::Result<SocketAddr> {
    serve_with(port, bus, None, None).await
}

/// Like [`serve`], with an optional `agent.*` extension and an optional guard
/// that reserves more event sources than the fixed list.
pub async fn serve_with(
    port: u16,
    bus: EventBus,
    extension: Option<Arc<dyn Extension>>,
    guard: Option<SourceGuard>,
) -> std::io::Result<SocketAddr> {
    let listener = TcpListener::bind(SocketAddr::from((Ipv4Addr::LOCALHOST, port))).await?;
    let addr = listener.local_addr()?;
    let permits = Arc::new(Semaphore::new(MAX_CONNECTIONS));
    tokio::spawn(async move {
        loop {
            let Ok((stream, peer)) = listener.accept().await else {
                continue;
            };
            if !peer.ip().is_loopback() {
                continue;
            }
            let Ok(permit) = permits.clone().try_acquire_owned() else {
                continue;
            };
            let bus = bus.clone();
            let extension = extension.clone();
            let guard = guard.clone();
            tokio::spawn(async move {
                let _permit = permit;
                handle(stream, bus, extension, guard).await;
            });
        }
    });
    Ok(addr)
}

async fn handle(
    stream: TcpStream,
    bus: EventBus,
    extension: Option<Arc<dyn Extension>>,
    guard: Option<SourceGuard>,
) {
    let config = WebSocketConfig::default()
        .max_message_size(Some(MAX_MESSAGE_BYTES))
        .max_frame_size(Some(MAX_MESSAGE_BYTES));
    let Ok(ws) =
        tokio_tungstenite::accept_hdr_async_with_config(stream, check_handshake, Some(config))
            .await
    else {
        return;
    };
    let (mut tx, mut rx) = ws.split();
    let conn = NEXT_CONN.fetch_add(1, Ordering::Relaxed);
    let (out_tx, mut out_rx) = mpsc::channel::<String>(OUTBOUND_QUEUE);
    // This task keeps one sender alive, so `recv` pends (rather than yielding
    // `None`) when no extension is attached or the extension dropped its copy.
    let _keepalive = out_tx.clone();
    if let Some(ext) = &extension {
        ext.connected(conn, out_tx);
    }
    loop {
        tokio::select! {
            outbound = out_rx.recv() => {
                let Some(text) = outbound else { break };
                if tx.send(Message::text(text)).await.is_err() {
                    break;
                }
            }
            inbound = rx.next() => {
                let Some(Ok(msg)) = inbound else { break };
                let Message::Text(text) = msg else {
                    if matches!(msg, Message::Close(_)) {
                        break;
                    }
                    continue;
                };
                let replies = reply_to(
                    text.as_str(),
                    conn,
                    &bus,
                    extension.as_deref(),
                    guard.as_ref(),
                );
                let mut failed = false;
                for reply in replies {
                    if tx.send(Message::text(reply.to_string())).await.is_err() {
                        failed = true;
                        break;
                    }
                }
                if failed {
                    break;
                }
            }
        }
    }
    if let Some(ext) = &extension {
        ext.disconnected(conn);
    }
}

/// Route one inbound frame and return the replies. Pure apart from the bus
/// publish and the extension call, so it is unit tested directly.
fn reply_to(
    text: &str,
    conn: u64,
    bus: &EventBus,
    ext: Option<&dyn Extension>,
    guard: Option<&SourceGuard>,
) -> Vec<Value> {
    if text.len() > MAX_MESSAGE_BYTES {
        return vec![
            json!({ "type": "error", "code": "too_large", "message": "message too large" }),
        ];
    }
    if let Some(ext) = ext {
        if let Ok(value) = serde_json::from_str::<Value>(text) {
            let claimed = value
                .get("type")
                .and_then(Value::as_str)
                .is_some_and(|t| t.starts_with("agent."));
            if claimed {
                return ext.message(conn, &value);
            }
        }
    }
    vec![match parse_inbound_with(text, guard) {
        Ok(Parsed::Event(event)) => {
            bus.publish(event);
            json!({ "type": "ack" })
        }
        Ok(Parsed::Ping) => json!({ "type": "pong" }),
        Err(message) => json!({ "type": "error", "message": message }),
    }]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_documented_event_shapes() {
        let p =
            parse_inbound(r#"{"type":"event","source":"codex","event":"task.started"}"#).unwrap();
        match p {
            Parsed::Event(e) => {
                assert_eq!(e.source, "codex");
                assert_eq!(e.kind, "task.started");
            }
            Parsed::Ping => panic!("expected event"),
        }
        let p = parse_inbound(
            r#"{"type":"event","source":"codex","event":"task.completed","payload":{"success":true}}"#,
        )
        .unwrap();
        assert!(matches!(p, Parsed::Event(e) if e.payload["success"] == true));
    }

    #[test]
    fn rejects_anything_that_is_not_an_event() {
        assert!(parse_inbound(r#"{"type":"exec","command":"rm -rf /"}"#).is_err());
        assert!(parse_inbound(r#"{"type":"action","action":"shell.execute"}"#).is_err());
        assert!(parse_inbound("not json").is_err());
        assert!(parse_inbound(r#"{"type":"event","source":"a b","event":"x"}"#).is_err());
        assert!(parse_inbound(r#"{"type":"event","source":"runtime","event":"x"}"#).is_err());
    }

    #[test]
    fn origin_policy() {
        assert!(origin_allowed("http://localhost:1420"));
        assert!(origin_allowed("http://127.0.0.1:3000"));
        assert!(origin_allowed("https://tauri.localhost"));
        assert!(!origin_allowed("https://evil.example"));
        assert!(!origin_allowed("https://evil.example:443"));
    }

    #[tokio::test]
    async fn ws_roundtrip_publishes_on_bus() {
        use tokio_tungstenite::connect_async;
        let bus = EventBus::new();
        let mut sub = bus.subscribe();
        let addr = serve(0, bus).await.unwrap();
        assert!(addr.ip().is_loopback());
        let (mut ws, _) = connect_async(format!("ws://{addr}")).await.unwrap();
        ws.send(Message::text(
            r#"{"type":"event","source":"codex","event":"task.started"}"#,
        ))
        .await
        .unwrap();
        let reply = ws.next().await.unwrap().unwrap();
        assert!(reply.to_text().unwrap().contains("ack"));
        let ev = sub.recv().await.unwrap();
        assert_eq!(ev.kind, "task.started");
    }
}
