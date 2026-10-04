//! A deliberately small HTTP/1.1 endpoint for harness hooks.
//!
//! Harnesses such as coding agents call user-configured "hooks" at lifecycle
//! points. Some can POST JSON to a URL, others run a command (which can use
//! `curl`). Both end up here. The server is generic: it knows nothing about
//! any harness, only about routes, a bearer token and how to hold a request
//! open while a human decides.
//!
//! Security, in the order the checks run:
//! * binds `127.0.0.1` only and drops non-loopback peers;
//! * `Host` must be this exact loopback origin (defeats DNS rebinding);
//! * any `Origin` header is refused: hooks are not browsers, browsers are;
//! * `POST` to a known route only; bounded headers, body and connections;
//! * `Authorization: Bearer <token>` compared in constant time;
//! * nothing about a request body is ever logged.
//!
//! A held request is released when the human answers, when the hold times out
//! or when the harness hangs up. The server never answers on anyone's behalf:
//! timeout and hang-up produce an empty 200, which every supported harness
//! reads as "this hook made no decision".

use std::collections::HashMap;
use std::net::{Ipv4Addr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;

use serde_json::Value;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{oneshot, watch, Semaphore};

pub const MAX_HEADER_BYTES: usize = 16 * 1024;
pub const MAX_BODY_BYTES: usize = 2 * 1024 * 1024;
pub const MAX_CONNECTIONS: usize = 16;
const READ_TIMEOUT: Duration = Duration::from_secs(10);

/// What a route decided to do with a request.
pub enum Outcome {
    /// Answer now. `None` is an empty 200 ("no decision").
    Reply(Option<Value>),
    /// Keep the request open until `rx` yields, `timeout` passes or the client
    /// hangs up. A timeout or hang-up answers with an empty 200 and runs
    /// `abandoned`.
    Hold {
        rx: oneshot::Receiver<Option<Value>>,
        timeout: Duration,
        abandoned: Box<dyn FnOnce() + Send>,
    },
}

pub trait Route: Send + Sync {
    /// Called with the parsed JSON body of an authenticated POST.
    fn handle(&self, payload: Value) -> Outcome;
}

pub struct Server {
    addr: SocketAddr,
    stop: watch::Sender<bool>,
}

impl Server {
    pub fn addr(&self) -> SocketAddr {
        self.addr
    }

    /// Stop accepting and release every held request.
    pub fn stop(&self) {
        let _ = self.stop.send(true);
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.stop();
    }
}

/// Start the server. `routes` maps a path (`/hooks/x`) to its handler.
pub async fn serve(
    port: u16,
    token: String,
    routes: HashMap<String, Arc<dyn Route>>,
) -> std::io::Result<Server> {
    if token.len() < 16 {
        return Err(std::io::Error::other("hook token is too short"));
    }
    let listener = TcpListener::bind(SocketAddr::from((Ipv4Addr::LOCALHOST, port))).await?;
    let addr = listener.local_addr()?;
    let (stop_tx, stop_rx) = watch::channel(false);
    let shared = Arc::new(Shared {
        token,
        routes,
        port: addr.port(),
    });
    let permits = Arc::new(Semaphore::new(MAX_CONNECTIONS));
    let mut stop = stop_rx.clone();
    tokio::spawn(async move {
        loop {
            let (stream, peer) = tokio::select! {
                accepted = listener.accept() => match accepted {
                    Ok(x) => x,
                    Err(_) => continue,
                },
                _ = stop.changed() => break,
            };
            if !peer.ip().is_loopback() {
                continue;
            }
            let Ok(permit) = permits.clone().try_acquire_owned() else {
                continue;
            };
            let shared = shared.clone();
            let stop = stop_rx.clone();
            tokio::spawn(async move {
                let _permit = permit;
                let _ = connection(stream, &shared, stop).await;
            });
        }
    });
    Ok(Server {
        addr,
        stop: stop_tx,
    })
}

struct Shared {
    token: String,
    routes: HashMap<String, Arc<dyn Route>>,
    port: u16,
}

struct Request {
    method: String,
    path: String,
    headers: HashMap<String, String>,
    body: Vec<u8>,
}

enum Fail {
    Status(u16, &'static str),
    Closed,
}

async fn connection(
    mut stream: TcpStream,
    shared: &Shared,
    mut stop: watch::Receiver<bool>,
) -> std::io::Result<()> {
    let request = match tokio::time::timeout(READ_TIMEOUT, read_request(&mut stream)).await {
        Ok(Ok(r)) => r,
        Ok(Err(Fail::Status(code, why))) => return respond(&mut stream, code, why, None).await,
        Ok(Err(Fail::Closed)) | Err(_) => return Ok(()),
    };

    if let Err((code, why)) = authorize(&request, shared) {
        return respond(&mut stream, code, why, None).await;
    }
    let Some(route) = shared.routes.get(&request.path) else {
        return respond(&mut stream, 404, "unknown route", None).await;
    };
    let payload: Value = match serde_json::from_slice(&request.body) {
        Ok(v) => v,
        Err(_) => return respond(&mut stream, 400, "body is not JSON", None).await,
    };

    match route.handle(payload) {
        Outcome::Reply(body) => respond(&mut stream, 200, "OK", body.as_ref()).await,
        Outcome::Hold {
            mut rx,
            timeout,
            abandoned,
        } => {
            let mut probe = [0u8; 1];
            let decided = tokio::select! {
                r = &mut rx => Some(r.ok().flatten()),
                _ = tokio::time::sleep(timeout) => None,
                // The harness gave up (or crashed): nobody is listening.
                read = stream.read(&mut probe) => { let _ = read; None }
                _ = stop.changed() => None,
            };
            match decided {
                Some(body) => respond(&mut stream, 200, "OK", body.as_ref()).await,
                None => {
                    abandoned();
                    respond(&mut stream, 200, "OK", None).await
                }
            }
        }
    }
}

fn authorize(request: &Request, shared: &Shared) -> Result<(), (u16, &'static str)> {
    let host_ok = request.headers.get("host").is_some_and(|h| {
        let h = h.to_ascii_lowercase();
        h == format!("127.0.0.1:{}", shared.port) || h == format!("localhost:{}", shared.port)
    });
    if !host_ok {
        return Err((403, "host not allowed"));
    }
    if request.headers.contains_key("origin") {
        return Err((403, "browser requests are not accepted"));
    }
    if request.method != "POST" {
        return Err((405, "POST only"));
    }
    let presented = request
        .headers
        .get("authorization")
        .and_then(|v| {
            v.strip_prefix("Bearer ")
                .or_else(|| v.strip_prefix("bearer "))
        })
        .unwrap_or("");
    if !constant_time_eq(presented.as_bytes(), shared.token.as_bytes()) {
        return Err((401, "missing or wrong token"));
    }
    Ok(())
}

/// Compare without leaking where the first difference is.
pub fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    let mut diff = (a.len() ^ b.len()) as u8;
    for i in 0..a.len().max(b.len()) {
        diff |= a.get(i).copied().unwrap_or(0) ^ b.get(i).copied().unwrap_or(0);
    }
    diff == 0
}

async fn read_request(stream: &mut TcpStream) -> Result<Request, Fail> {
    let mut buf: Vec<u8> = Vec::with_capacity(2048);
    let mut chunk = [0u8; 4096];
    let head_end = loop {
        if let Some(pos) = find(&buf, b"\r\n\r\n") {
            break pos;
        }
        if buf.len() > MAX_HEADER_BYTES {
            return Err(Fail::Status(431, "headers too large"));
        }
        let n = stream.read(&mut chunk).await.map_err(|_| Fail::Closed)?;
        if n == 0 {
            return Err(Fail::Closed);
        }
        buf.extend_from_slice(&chunk[..n]);
    };
    let head =
        std::str::from_utf8(&buf[..head_end]).map_err(|_| Fail::Status(400, "bad headers"))?;
    let mut lines = head.split("\r\n");
    let request_line = lines.next().unwrap_or("");
    let mut parts = request_line.split(' ');
    let method = parts.next().unwrap_or("").to_string();
    let target = parts.next().unwrap_or("");
    let path = target.split('?').next().unwrap_or("").to_string();
    let mut headers = HashMap::new();
    for line in lines {
        if let Some((k, v)) = line.split_once(':') {
            headers.insert(k.trim().to_ascii_lowercase(), v.trim().to_string());
        }
    }
    if headers.contains_key("transfer-encoding") {
        return Err(Fail::Status(411, "send a Content-Length"));
    }
    let length: usize = match headers.get("content-length") {
        Some(v) => v
            .parse()
            .map_err(|_| Fail::Status(400, "bad Content-Length"))?,
        None => 0,
    };
    if length > MAX_BODY_BYTES {
        return Err(Fail::Status(413, "body too large"));
    }
    if headers
        .get("expect")
        .is_some_and(|v| v.eq_ignore_ascii_case("100-continue"))
    {
        stream
            .write_all(b"HTTP/1.1 100 Continue\r\n\r\n")
            .await
            .map_err(|_| Fail::Closed)?;
    }
    let mut body = buf[head_end + 4..].to_vec();
    while body.len() < length {
        let n = stream.read(&mut chunk).await.map_err(|_| Fail::Closed)?;
        if n == 0 {
            return Err(Fail::Closed);
        }
        body.extend_from_slice(&chunk[..n]);
        if body.len() > MAX_BODY_BYTES {
            return Err(Fail::Status(413, "body too large"));
        }
    }
    body.truncate(length);
    Ok(Request {
        method,
        path,
        headers,
        body,
    })
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

async fn respond(
    stream: &mut TcpStream,
    code: u16,
    reason: &str,
    body: Option<&Value>,
) -> std::io::Result<()> {
    let payload = if code == 200 {
        body.map(|b| b.to_string()).unwrap_or_default()
    } else {
        format!("{{\"error\":\"{reason}\"}}")
    };
    let head = format!(
        "HTTP/1.1 {code} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n",
        payload.len()
    );
    stream.write_all(head.as_bytes()).await?;
    stream.write_all(payload.as_bytes()).await?;
    stream.shutdown().await
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Mutex;

    const TOKEN: &str = "0123456789abcdef0123456789abcdef";

    /// Replies with what the test installed; records payloads.
    struct Echo {
        seen: Mutex<Vec<Value>>,
        hold: Mutex<Option<oneshot::Receiver<Option<Value>>>>,
        abandoned: Arc<AtomicBool>,
        hold_for: Duration,
    }

    impl Route for Echo {
        fn handle(&self, payload: Value) -> Outcome {
            self.seen.lock().unwrap().push(payload.clone());
            if let Some(rx) = self.hold.lock().unwrap().take() {
                let flag = self.abandoned.clone();
                return Outcome::Hold {
                    rx,
                    timeout: self.hold_for,
                    abandoned: Box::new(move || flag.store(true, Ordering::SeqCst)),
                };
            }
            Outcome::Reply(Some(json!({"echo": payload})))
        }
    }

    fn echo(hold: Option<oneshot::Receiver<Option<Value>>>, hold_for: Duration) -> Arc<Echo> {
        Arc::new(Echo {
            seen: Mutex::new(vec![]),
            hold: Mutex::new(hold),
            abandoned: Arc::new(AtomicBool::new(false)),
            hold_for,
        })
    }

    async fn start(route: Arc<Echo>) -> Server {
        let mut routes: HashMap<String, Arc<dyn Route>> = HashMap::new();
        routes.insert("/hooks/test".into(), route);
        serve(0, TOKEN.into(), routes).await.unwrap()
    }

    /// Raw request, so the tests control every header.
    async fn raw(addr: SocketAddr, request: String) -> String {
        let mut s = TcpStream::connect(addr).await.unwrap();
        s.write_all(request.as_bytes()).await.unwrap();
        let mut out = Vec::new();
        let _ = tokio::time::timeout(Duration::from_secs(5), s.read_to_end(&mut out)).await;
        String::from_utf8_lossy(&out).to_string()
    }

    fn post(addr: SocketAddr, path: &str, extra: &str, body: &str) -> String {
        format!(
            "POST {path} HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n{extra}\r\n{body}",
            addr.port(),
            body.len()
        )
    }

    fn auth() -> String {
        format!("Authorization: Bearer {TOKEN}\r\n")
    }

    fn status(response: &str) -> u16 {
        response
            .split(' ')
            .nth(1)
            .and_then(|s| s.parse().ok())
            .unwrap_or(0)
    }

    fn body_of(response: &str) -> &str {
        response.split("\r\n\r\n").nth(1).unwrap_or("")
    }

    #[tokio::test]
    async fn an_authenticated_post_reaches_its_route() {
        let route = echo(None, Duration::from_secs(1));
        let server = start(route.clone()).await;
        assert!(server.addr().ip().is_loopback());
        let r = raw(
            server.addr(),
            post(server.addr(), "/hooks/test", &auth(), r#"{"a":1}"#),
        )
        .await;
        assert_eq!(status(&r), 200, "{r}");
        assert_eq!(
            serde_json::from_str::<Value>(body_of(&r)).unwrap(),
            json!({"echo":{"a":1}})
        );
        assert_eq!(route.seen.lock().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn requests_without_the_token_never_reach_a_route() {
        let route = echo(None, Duration::from_secs(1));
        let server = start(route.clone()).await;
        let a = server.addr();
        assert_eq!(status(&raw(a, post(a, "/hooks/test", "", "{}")).await), 401);
        assert_eq!(
            status(
                &raw(
                    a,
                    post(a, "/hooks/test", "Authorization: Bearer nope\r\n", "{}")
                )
                .await
            ),
            401
        );
        let almost = format!("Authorization: Bearer {}x\r\n", TOKEN);
        assert_eq!(
            status(&raw(a, post(a, "/hooks/test", &almost, "{}")).await),
            401
        );
        assert!(route.seen.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn browsers_and_rebinding_hosts_are_refused_even_with_the_token() {
        let route = echo(None, Duration::from_secs(1));
        let server = start(route.clone()).await;
        let a = server.addr();
        let with_origin = format!("{}Origin: https://evil.example\r\n", auth());
        assert_eq!(
            status(&raw(a, post(a, "/hooks/test", &with_origin, "{}")).await),
            403
        );

        let rebinding = format!(
            "POST /hooks/test HTTP/1.1\r\nHost: evil.example:{}\r\nContent-Length: 2\r\n{}\r\n{{}}",
            a.port(),
            auth()
        );
        assert_eq!(status(&raw(a, rebinding).await), 403);
        assert!(route.seen.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn only_post_to_known_routes_with_json_and_sane_sizes() {
        let route = echo(None, Duration::from_secs(1));
        let server = start(route.clone()).await;
        let a = server.addr();
        assert_eq!(
            status(&raw(a, post(a, "/elsewhere", &auth(), "{}")).await),
            404
        );
        assert_eq!(
            status(&raw(a, post(a, "/hooks/test", &auth(), "not json")).await),
            400
        );
        let get = format!(
            "GET /hooks/test HTTP/1.1\r\nHost: 127.0.0.1:{}\r\n{}\r\n",
            a.port(),
            auth()
        );
        assert_eq!(status(&raw(a, get).await), 405);
        let huge = format!(
            "POST /hooks/test HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nContent-Length: {}\r\n{}\r\n",
            a.port(),
            MAX_BODY_BYTES + 1,
            auth()
        );
        assert_eq!(status(&raw(a, huge).await), 413);
        let chunked = format!(
            "POST /hooks/test HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nTransfer-Encoding: chunked\r\n{}\r\n0\r\n\r\n",
            a.port(),
            auth()
        );
        assert_eq!(status(&raw(a, chunked).await), 411);
        assert!(route.seen.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn expect_continue_bodies_are_read() {
        let route = echo(None, Duration::from_secs(1));
        let server = start(route.clone()).await;
        let a = server.addr();
        let mut s = TcpStream::connect(a).await.unwrap();
        let head = format!(
            "POST /hooks/test HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nExpect: 100-continue\r\nContent-Length: 7\r\n{}\r\n",
            a.port(),
            auth()
        );
        s.write_all(head.as_bytes()).await.unwrap();
        let mut first = [0u8; 25];
        s.read_exact(&mut first).await.unwrap();
        assert!(String::from_utf8_lossy(&first).starts_with("HTTP/1.1 100 Continue"));
        s.write_all(br#"{"a":2}"#).await.unwrap();
        let mut rest = Vec::new();
        s.read_to_end(&mut rest).await.unwrap();
        assert!(String::from_utf8_lossy(&rest).contains("200 OK"));
    }

    #[tokio::test]
    async fn a_held_request_is_answered_by_the_decision() {
        let (tx, rx) = oneshot::channel();
        let route = echo(Some(rx), Duration::from_secs(5));
        let server = start(route.clone()).await;
        let a = server.addr();
        let pending = tokio::spawn(raw(a, post(a, "/hooks/test", &auth(), r#"{"ask":true}"#)));
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert!(!pending.is_finished(), "the request must stay open");
        tx.send(Some(json!({"decision":"allow"}))).unwrap();
        let r = pending.await.unwrap();
        assert_eq!(
            serde_json::from_str::<Value>(body_of(&r)).unwrap(),
            json!({"decision":"allow"})
        );
        assert!(!route.abandoned.load(Ordering::SeqCst));
    }

    #[tokio::test]
    async fn a_hold_that_times_out_answers_empty_and_reports_abandonment() {
        let (_tx, rx) = oneshot::channel();
        let route = echo(Some(rx), Duration::from_millis(150));
        let server = start(route.clone()).await;
        let a = server.addr();
        let r = raw(a, post(a, "/hooks/test", &auth(), "{}")).await;
        assert_eq!(status(&r), 200);
        assert_eq!(body_of(&r), "", "a timeout must never look like a decision");
        assert!(route.abandoned.load(Ordering::SeqCst));
    }

    #[tokio::test]
    async fn a_client_that_hangs_up_abandons_the_hold() {
        let (_tx, rx) = oneshot::channel();
        let route = echo(Some(rx), Duration::from_secs(30));
        let server = start(route.clone()).await;
        let a = server.addr();
        let mut s = TcpStream::connect(a).await.unwrap();
        s.write_all(post(a, "/hooks/test", &auth(), "{}").as_bytes())
            .await
            .unwrap();
        tokio::time::sleep(Duration::from_millis(100)).await;
        drop(s);
        for _ in 0..100 {
            if route.abandoned.load(Ordering::SeqCst) {
                return;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        panic!("hang-up was not noticed");
    }

    #[tokio::test]
    async fn stopping_the_server_releases_held_requests() {
        let (_tx, rx) = oneshot::channel();
        let route = echo(Some(rx), Duration::from_secs(30));
        let server = start(route.clone()).await;
        let a = server.addr();
        let pending = tokio::spawn(raw(a, post(a, "/hooks/test", &auth(), "{}")));
        tokio::time::sleep(Duration::from_millis(100)).await;
        server.stop();
        let r = tokio::time::timeout(Duration::from_secs(3), pending)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(body_of(&r), "");
        assert!(route.abandoned.load(Ordering::SeqCst));
    }

    #[tokio::test]
    async fn a_weak_token_is_refused_at_startup() {
        let routes: HashMap<String, Arc<dyn Route>> = HashMap::new();
        assert!(serve(0, "short".into(), routes).await.is_err());
    }

    #[test]
    fn constant_time_comparison_is_correct() {
        assert!(constant_time_eq(b"abc", b"abc"));
        assert!(!constant_time_eq(b"abc", b"abd"));
        assert!(!constant_time_eq(b"abc", b"abcd"));
        assert!(!constant_time_eq(b"", b"x"));
        assert!(constant_time_eq(b"", b""));
    }
}
