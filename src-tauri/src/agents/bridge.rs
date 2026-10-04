//! The Agent Bridge: a versioned, provider-neutral protocol that lets any
//! local process appear in Agent Control.
//!
//! It is the out-of-process face of the Adapter SDK. Every client message maps
//! to one [`AdapterHost`] method, and every command LaunchDeck sends back is an
//! [`AdapterCommand`] rendered as JSON. There is no second model.
//!
//! Transport: the existing localhost WebSocket (`events::external`), which
//! owns the security properties (127.0.0.1 only, origin check, size and
//! connection caps). This file adds the rules that belong to the protocol:
//!
//! * a client must `agent.register` before it can report anything;
//! * a connection can only speak for the agents it registered, and an agent id
//!   is held by one live connection at a time;
//! * agents start as monitor-only: commands are delivered only after the user
//!   grants control, and the service enforces that regardless of this file;
//! * an agent whose connection ends goes offline, and commands in flight fail;
//! * unknown message types, statuses and capabilities are rejected, never
//!   guessed; unknown *fields* are ignored so the protocol can grow;
//! * a token bucket per connection limits message rate.
//!
//! Nothing here can execute anything on the machine: the only thing a client
//! can do to LaunchDeck is describe the state of agents it registered.

use std::collections::{BTreeSet, HashMap};
use std::net::SocketAddr;
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};
use std::time::Instant;

use serde::de::DeserializeOwned;
use serde::Deserialize;
use serde_json::{json, Map, Value};
use tokio::sync::mpsc;

use super::adapter::*;
use super::model::*;
use crate::events::external::Extension;

pub const ADAPTER_ID: &str = "bridge";
/// The only protocol version this build speaks.
pub const PROTOCOL: u64 = 1;
pub const MAX_AGENTS_PER_CONNECTION: usize = 8;

/// Messages a connection may send in a burst, and per second on average.
const BURST: f64 = 60.0;
const REFILL_PER_SEC: f64 = 30.0;

// ---------------------------------------------------------------------------
// Rate limiting
// ---------------------------------------------------------------------------

struct Bucket {
    tokens: f64,
    at: Instant,
}

impl Bucket {
    fn new(now: Instant) -> Self {
        Self {
            tokens: BURST,
            at: now,
        }
    }

    fn take(&mut self, now: Instant) -> bool {
        let dt = now.saturating_duration_since(self.at).as_secs_f64();
        self.tokens = (self.tokens + dt * REFILL_PER_SEC).min(BURST);
        self.at = now;
        if self.tokens >= 1.0 {
            self.tokens -= 1.0;
            true
        } else {
            false
        }
    }
}

// ---------------------------------------------------------------------------
// Wire messages (client to LaunchDeck)
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RegisterMsg {
    #[serde(default)]
    protocol: Option<u64>,
    agent: AgentDecl,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct AgentDecl {
    id: String,
    name: String,
    #[serde(default)]
    provider: Option<String>,
    #[serde(default)]
    harness: Option<String>,
    #[serde(default)]
    capabilities: Vec<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SessionMsg {
    agent_id: String,
    session: SessionDecl,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SessionDecl {
    id: String,
    status: String,
    #[serde(default)]
    task: Option<String>,
    #[serde(default)]
    progress: Option<f64>,
    #[serde(default)]
    detail: Option<String>,
    #[serde(default)]
    error: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ApprovalRequestMsg {
    agent_id: String,
    session_id: String,
    approval: ApprovalDecl,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ApprovalDecl {
    #[serde(default)]
    id: Option<String>,
    message: String,
    #[serde(default)]
    tool: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ApprovalWithdrawMsg {
    agent_id: String,
    session_id: String,
    approval_id: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SessionEndMsg {
    agent_id: String,
    session_id: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CommandResultMsg {
    agent_id: String,
    request_id: String,
    ok: bool,
    #[serde(default)]
    error: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct UnregisterMsg {
    agent_id: String,
}

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

struct Rejected {
    code: &'static str,
    message: String,
}

fn rejected(code: &'static str, message: impl Into<String>) -> Rejected {
    Rejected {
        code,
        message: message.into(),
    }
}

impl From<HostError> for Rejected {
    fn from(e: HostError) -> Self {
        let code = match &e {
            HostError::Invalid(_) => "invalid_field",
            HostError::IdTaken(_) => "agent_in_use",
            HostError::NotOwner(_) | HostError::SessionOwnedByOther(_) => "not_owner",
            HostError::UnknownAgent(_) => "not_registered",
            HostError::UnknownSession(_) => "unknown_session",
            HostError::UnknownRequest(_) => "unknown_request",
            HostError::LimitReached(_) => "limit_reached",
        };
        rejected(code, e.to_string())
    }
}

fn parse<T: DeserializeOwned>(value: &Value) -> Result<T, Rejected> {
    serde_json::from_value(value.clone())
        .map_err(|e| rejected("invalid_message", summarize(&e.to_string(), 160)))
}

// ---------------------------------------------------------------------------
// Connections
// ---------------------------------------------------------------------------

struct Conn {
    out: mpsc::Sender<String>,
    agents: BTreeSet<AgentId>,
    bucket: Bucket,
}

#[derive(Default)]
struct Inner {
    conns: HashMap<u64, Conn>,
    /// Which live connection currently speaks for an agent id.
    owners: HashMap<AgentId, u64>,
    /// Commands handed to a connection and not yet answered.
    inflight: HashMap<RequestId, (u64, AgentId)>,
}

pub struct BridgeAdapter {
    inner: Mutex<Inner>,
    host: OnceLock<AdapterHost>,
}

impl BridgeAdapter {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            inner: Mutex::new(Inner::default()),
            host: OnceLock::new(),
        })
    }

    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// The WebSocket is up: show where clients should connect.
    pub fn mark_listening(&self, addr: SocketAddr) {
        if let Some(h) = self.host.get() {
            h.set_state(AdapterState::Active, Some(&format!("ws://{addr}")));
        }
    }

    /// The WebSocket is off or failed; say why in the Adapters view.
    pub fn mark_unavailable(&self, state: AdapterState, detail: &str) {
        if let Some(h) = self.host.get() {
            h.set_state(state, Some(detail));
        }
    }

    fn send_to_owner(&self, agent_id: &str, frame: Value) -> Result<(), &'static str> {
        let inner = self.lock();
        let conn = inner.owners.get(agent_id).ok_or("not connected")?;
        let out = &inner.conns.get(conn).ok_or("not connected")?.out;
        out.try_send(frame.to_string()).map_err(|e| match e {
            mpsc::error::TrySendError::Full(_) => "not reading its connection",
            mpsc::error::TrySendError::Closed(_) => "connection closed",
        })
    }

    fn require_owner(&self, conn: u64, agent_id: &str) -> Result<(), Rejected> {
        let inner = self.lock();
        if inner
            .conns
            .get(&conn)
            .is_some_and(|c| c.agents.contains(agent_id))
        {
            Ok(())
        } else {
            Err(rejected(
                "not_registered",
                format!(
                    "agent `{}` is not registered on this connection; send agent.register first",
                    summarize(agent_id, MAX_ID_LEN)
                ),
            ))
        }
    }

    fn dispatch(&self, conn: u64, message: &Value) -> Result<Value, Rejected> {
        let host = self
            .host
            .get()
            .ok_or_else(|| rejected("unavailable", "the bridge is not attached yet"))?;
        {
            let mut inner = self.lock();
            let c = inner
                .conns
                .get_mut(&conn)
                .ok_or_else(|| rejected("not_connected", "unknown connection"))?;
            if !c.bucket.take(Instant::now()) {
                return Err(rejected("rate_limited", "too many messages; slow down"));
            }
        }
        let kind = message.get("type").and_then(Value::as_str).unwrap_or("");
        match kind {
            "agent.register" => self.register(host, conn, parse(message)?),
            "agent.session.update" => {
                let m: SessionMsg = parse(message)?;
                self.require_owner(conn, &m.agent_id)?;
                let status = Status::normalize(&m.session.status).ok_or_else(|| {
                    rejected(
                        "invalid_status",
                        format!(
                            "unknown status `{}`; use one of {}",
                            summarize(&m.session.status, 24),
                            Status::ALL
                                .iter()
                                .map(|s| s.as_str())
                                .collect::<Vec<_>>()
                                .join(", ")
                        ),
                    )
                })?;
                if status == Status::ApprovalRequired {
                    return Err(rejected(
                        "invalid_status",
                        "approvals are requested with agent.approval.request",
                    ));
                }
                if status == Status::Offline {
                    return Err(rejected(
                        "invalid_status",
                        "a session cannot be offline; close the connection or send agent.unregister",
                    ));
                }
                let mut update = SessionUpdate::new(&m.session.id, status);
                update.task = m.session.task;
                update.detail = m.session.detail;
                update.error = m.session.error;
                update.progress = m.session.progress.map(|p| p as f32);
                host.update_session(&m.agent_id, update)?;
                Ok(json!({ "type": "ack" }))
            }
            "agent.approval.request" => {
                let m: ApprovalRequestMsg = parse(message)?;
                self.require_owner(conn, &m.agent_id)?;
                let id = host.request_approval(
                    &m.agent_id,
                    &m.session_id,
                    ApprovalSpec {
                        id: m.approval.id,
                        message: m.approval.message,
                        tool: m.approval.tool,
                    },
                )?;
                Ok(json!({ "type": "ack", "approvalId": id }))
            }
            "agent.approval.withdraw" => {
                let m: ApprovalWithdrawMsg = parse(message)?;
                self.require_owner(conn, &m.agent_id)?;
                host.withdraw_approval(&m.session_id, &m.approval_id)?;
                Ok(json!({ "type": "ack" }))
            }
            "agent.session.end" => {
                let m: SessionEndMsg = parse(message)?;
                self.require_owner(conn, &m.agent_id)?;
                host.end_session(&m.session_id)?;
                Ok(json!({ "type": "ack" }))
            }
            "agent.command.result" => {
                let m: CommandResultMsg = parse(message)?;
                self.require_owner(conn, &m.agent_id)?;
                {
                    let mut inner = self.lock();
                    match inner.inflight.get(&m.request_id) {
                        Some((c, a)) if *c == conn && *a == m.agent_id => {
                            inner.inflight.remove(&m.request_id);
                        }
                        _ => {
                            return Err(rejected(
                                "unknown_request",
                                "no command with that requestId is waiting for this agent",
                            ))
                        }
                    }
                }
                let result = if m.ok {
                    Ok(())
                } else {
                    Err(m
                        .error
                        .unwrap_or_else(|| "the agent reported a failure".into()))
                };
                host.command_result(&m.request_id, result)?;
                Ok(json!({ "type": "ack" }))
            }
            "agent.unregister" => {
                let m: UnregisterMsg = parse(message)?;
                self.require_owner(conn, &m.agent_id)?;
                {
                    let mut inner = self.lock();
                    if let Some(c) = inner.conns.get_mut(&conn) {
                        c.agents.remove(&m.agent_id);
                    }
                    inner.owners.remove(&m.agent_id);
                    inner.inflight.retain(|_, (_, a)| *a != m.agent_id);
                }
                host.set_online(&m.agent_id, false)?;
                Ok(json!({ "type": "ack" }))
            }
            other => Err(rejected(
                "unsupported_type",
                format!("unknown message type `{}`", summarize(other, 40)),
            )),
        }
    }

    fn register(&self, host: &AdapterHost, conn: u64, m: RegisterMsg) -> Result<Value, Rejected> {
        if m.protocol != Some(PROTOCOL) {
            return Err(rejected(
                "unsupported_protocol",
                format!(
                    "this LaunchDeck speaks protocol {PROTOCOL}; send \"protocol\": {PROTOCOL}"
                ),
            ));
        }
        let mut capabilities = Capabilities::new();
        for raw in &m.agent.capabilities {
            let cap = Capability::parse(raw).ok_or_else(|| {
                rejected(
                    "invalid_capability",
                    format!("unknown capability `{}`", summarize(raw, 32)),
                )
            })?;
            capabilities.insert(cap);
        }
        let id = m.agent.id.clone();
        // Claim the id first so two connections cannot race for it.
        let newly_claimed = {
            let mut inner = self.lock();
            if let Some(&owner) = inner.owners.get(&id) {
                if owner != conn {
                    return Err(rejected(
                        "agent_in_use",
                        format!(
                            "agent `{}` is connected from another process",
                            summarize(&id, MAX_ID_LEN)
                        ),
                    ));
                }
            }
            let c = inner
                .conns
                .get_mut(&conn)
                .ok_or_else(|| rejected("not_connected", "unknown connection"))?;
            let newly = !c.agents.contains(&id);
            if newly && c.agents.len() >= MAX_AGENTS_PER_CONNECTION {
                return Err(rejected(
                    "limit_reached",
                    format!("a connection can register at most {MAX_AGENTS_PER_CONNECTION} agents"),
                ));
            }
            c.agents.insert(id.clone());
            inner.owners.insert(id.clone(), conn);
            newly
        };
        let registered = host.register_agent(AgentSpec {
            id: id.clone(),
            name: m.agent.name,
            provider: m.agent.provider,
            harness: m.agent.harness,
            capabilities: capabilities.clone(),
        });
        if let Err(e) = registered {
            if newly_claimed {
                let mut inner = self.lock();
                if let Some(c) = inner.conns.get_mut(&conn) {
                    c.agents.remove(&id);
                }
                inner.owners.remove(&id);
            }
            return Err(e.into());
        }
        let access = host.agent_access(&id)?;
        Ok(json!({
            "type": "agent.registered",
            "protocol": PROTOCOL,
            "agentId": id,
            "access": access,
            "capabilities": capabilities.iter().map(|c| c.as_str()).collect::<Vec<_>>(),
        }))
    }
}

/// The JSON frame for a command. Free text (instructions, tasks) travels to
/// the agent it is meant for and is never written to a log by LaunchDeck.
pub fn command_frame(c: &AdapterCommand) -> Value {
    let mut command = Map::new();
    command.insert("kind".into(), json!(c.command.kind().as_str()));
    match &c.command {
        Command::SendInstruction { text } => {
            command.insert("text".into(), json!(text));
        }
        Command::Spawn { task, cwd } => {
            command.insert("task".into(), json!(task));
            if let Some(cwd) = cwd {
                command.insert("cwd".into(), json!(cwd));
            }
        }
        _ => {}
    }
    json!({
        "type": "agent.command",
        "requestId": c.request_id,
        "agentId": c.agent_id,
        "sessionId": c.session_id,
        "approvalId": c.approval_id,
        "command": command,
    })
}

impl AgentAdapter for BridgeAdapter {
    fn info(&self) -> AdapterInfo {
        AdapterInfo {
            id: ADAPTER_ID.into(),
            name: "Agent Bridge".into(),
            kind: AdapterKind::Bridge,
            description: "Any local process that speaks the Agent Bridge protocol over the \
                          localhost WebSocket. Nothing connects unless it registers."
                .into(),
            limits: vec![
                "Agents are monitor-only until you grant control.".into(),
                "A client must stay connected: when it disconnects its agents go offline.".into(),
                "LaunchDeck delivers commands; whether the agent honours them is up to the client."
                    .into(),
                "Local processes are not authenticated, only localhost-bound. Registration is \
                 explicit and control is always your decision."
                    .into(),
            ],
        }
    }

    fn attach(&self, host: AdapterHost) {
        let _ = self.host.set(host);
    }

    fn execute(&self, c: AdapterCommand) -> Result<(), CommandError> {
        let mut inner = self.lock();
        let conn = *inner
            .owners
            .get(&c.agent_id)
            .ok_or_else(|| CommandError::new("the agent is not connected"))?;
        let out = inner
            .conns
            .get(&conn)
            .map(|c| c.out.clone())
            .ok_or_else(|| CommandError::new("the agent is not connected"))?;
        out.try_send(command_frame(&c).to_string())
            .map_err(|e| match e {
                mpsc::error::TrySendError::Full(_) => {
                    CommandError::new("the agent is not reading its connection")
                }
                mpsc::error::TrySendError::Closed(_) => {
                    CommandError::new("the agent's connection is closed")
                }
            })?;
        // Still under the lock: a fast reply cannot arrive before this entry.
        inner.inflight.insert(c.request_id, (conn, c.agent_id));
        Ok(())
    }

    fn access_changed(&self, agent_id: &str, access: AccessLevel) {
        let frame = json!({ "type": "agent.access", "agentId": agent_id, "access": access });
        // Informational only; a client that is gone or slow just misses it.
        let _ = self.send_to_owner(agent_id, frame);
    }
}

impl Extension for BridgeAdapter {
    fn connected(&self, conn: u64, outbound: mpsc::Sender<String>) {
        self.lock().conns.insert(
            conn,
            Conn {
                out: outbound,
                agents: BTreeSet::new(),
                bucket: Bucket::new(Instant::now()),
            },
        );
    }

    fn message(&self, conn: u64, message: &Value) -> Vec<Value> {
        // Optional correlation id, echoed on the reply.
        let id = message
            .get("id")
            .filter(|v| v.is_number() || v.as_str().is_some_and(|s| s.len() <= MAX_ID_LEN))
            .cloned();
        let mut reply = match self.dispatch(conn, message) {
            Ok(v) => v,
            Err(r) => json!({ "type": "error", "code": r.code, "message": r.message }),
        };
        if let (Some(id), Some(obj)) = (id, reply.as_object_mut()) {
            obj.insert("id".into(), id);
        }
        vec![reply]
    }

    fn disconnected(&self, conn: u64) {
        let agents = {
            let mut inner = self.lock();
            let Some(c) = inner.conns.remove(&conn) else {
                return;
            };
            for a in &c.agents {
                if inner.owners.get(a) == Some(&conn) {
                    inner.owners.remove(a);
                }
            }
            inner.inflight.retain(|_, (cn, _)| *cn != conn);
            c.agents
        };
        if let Some(host) = self.host.get() {
            for a in agents {
                let _ = host.set_online(&a, false);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agents::service::{AgentControl, NullSink, Target, Timing};
    use crate::events::{Event, EventBus};
    use std::time::Duration;
    use tokio::sync::broadcast::Receiver;

    struct Rig {
        ctl: Arc<AgentControl>,
        bridge: Arc<BridgeAdapter>,
        rx: Receiver<Event>,
    }

    fn rig() -> Rig {
        let bus = EventBus::new();
        let rx = bus.subscribe();
        let ctl = AgentControl::new(bus, Arc::new(NullSink), None, Timing::default());
        let bridge = BridgeAdapter::new();
        ctl.register_adapter(bridge.clone());
        Rig { ctl, bridge, rx }
    }

    impl Rig {
        /// Connect a client; returns its connection id and the frames sent to it.
        fn connect(&self, conn: u64) -> mpsc::Receiver<String> {
            let (tx, rx) = mpsc::channel(8);
            self.bridge.connected(conn, tx);
            rx
        }

        fn send(&self, conn: u64, msg: Value) -> Value {
            let mut replies = self.bridge.message(conn, &msg);
            assert_eq!(replies.len(), 1);
            replies.remove(0)
        }

        fn events(&mut self) -> Vec<Event> {
            let mut v = Vec::new();
            while let Ok(e) = self.rx.try_recv() {
                v.push(e);
            }
            v
        }
    }

    fn register(id: &str, caps: &[&str]) -> Value {
        json!({
            "type": "agent.register",
            "protocol": 1,
            "agent": { "id": id, "name": id.to_uppercase(), "provider": "acme", "harness": "script", "capabilities": caps }
        })
    }

    fn session(agent: &str, id: &str, status: &str) -> Value {
        json!({ "type": "agent.session.update", "agentId": agent, "session": { "id": id, "status": status, "task": "do the thing" } })
    }

    fn code(reply: &Value) -> &str {
        assert_eq!(reply["type"], "error", "expected an error, got {reply}");
        reply["code"].as_str().unwrap()
    }

    fn status_of(rig: &Rig, session: &str) -> Status {
        rig.ctl.session(session).expect("session exists").status
    }

    #[tokio::test]
    async fn registration_is_explicit_and_starts_monitor_only() {
        let mut rig = rig();
        let _out = rig.connect(1);
        // Nothing can be reported before registering.
        let early = rig.send(1, session("bot", "s1", "running"));
        assert_eq!(code(&early), "not_registered");

        let reply = rig.send(1, register("bot", &["approve", "reject", "pause"]));
        assert_eq!(reply["type"], "agent.registered");
        assert_eq!(reply["protocol"], 1);
        assert_eq!(reply["access"], "monitor");
        assert_eq!(reply["capabilities"], json!(["pause", "approve", "reject"]));

        let agent = rig.ctl.agent("bot").unwrap();
        assert_eq!(agent.provider.as_deref(), Some("acme"));
        assert_eq!(agent.adapter_id, ADAPTER_ID);
        assert_eq!(agent.access, AccessLevel::Monitor);

        let evs = rig.events();
        assert!(evs
            .iter()
            .any(|e| e.source == "agent-control" && e.kind == "agent.registered"));
    }

    #[tokio::test]
    async fn protocol_version_and_message_shape_are_validated() {
        let rig = rig();
        let _out = rig.connect(1);
        let mut v2 = register("bot", &[]);
        v2["protocol"] = json!(2);
        assert_eq!(code(&rig.send(1, v2)), "unsupported_protocol");
        let mut none = register("bot", &[]);
        none.as_object_mut().unwrap().remove("protocol");
        assert_eq!(code(&rig.send(1, none)), "unsupported_protocol");

        assert_eq!(
            code(&rig.send(1, json!({"type":"agent.register","protocol":1}))),
            "invalid_message"
        );
        assert_eq!(
            code(&rig.send(1, json!({"type":"agent.frobnicate"}))),
            "unsupported_type"
        );
        let mut bad_id = register("has space", &[]);
        bad_id["agent"]["id"] = json!("has space");
        assert_eq!(code(&rig.send(1, bad_id)), "invalid_field");
        assert!(rig.ctl.agent("has space").is_none());
    }

    #[tokio::test]
    async fn unknown_capabilities_and_statuses_are_rejected_not_guessed() {
        let rig = rig();
        let _out = rig.connect(1);
        let r = rig.send(1, register("bot", &["approve", "teleport"]));
        assert_eq!(code(&r), "invalid_capability");
        assert!(rig.ctl.agent("bot").is_none());

        rig.send(1, register("bot", &["pause"]));
        assert_eq!(
            code(&rig.send(1, session("bot", "s1", "vibing"))),
            "invalid_status"
        );
        // An approval gate cannot be faked through a status.
        assert_eq!(
            code(&rig.send(1, session("bot", "s1", "approval_required"))),
            "invalid_status"
        );
        assert_eq!(
            code(&rig.send(1, session("bot", "s1", "offline"))),
            "invalid_status"
        );
        assert!(rig.ctl.session("s1").is_none());
    }

    #[tokio::test]
    async fn session_updates_normalize_status_and_support_many_sessions() {
        let rig = rig();
        let _out = rig.connect(1);
        rig.send(1, register("bot", &[]));
        assert_eq!(rig.send(1, session("bot", "s1", "working"))["type"], "ack");
        rig.send(1, session("bot", "s2", "tool_use"));
        rig.send(1, session("bot", "s3", "done"));
        assert_eq!(status_of(&rig, "s1"), Status::Running);
        assert_eq!(status_of(&rig, "s2"), Status::ToolUse);
        assert_eq!(status_of(&rig, "s3"), Status::Completed);

        // Progress is clamped and details are kept short.
        let long = "x".repeat(5000);
        rig.send(
            1,
            json!({"type":"agent.session.update","agentId":"bot","session":{
                "id":"s1","status":"running","progress":7.5,"detail":long}}),
        );
        let s = rig.ctl.session("s1").unwrap();
        assert_eq!(s.progress, Some(1.0));
        assert!(s.detail.unwrap().chars().count() <= MAX_DETAIL_LEN);
        // A wrongly typed field is a protocol error, not a crash.
        assert_eq!(
            code(&rig.send(
                1,
                json!({"type":"agent.session.update","agentId":"bot","session":{
                    "id":"s1","status":"running","progress":"half"}})
            )),
            "invalid_message"
        );
    }

    #[tokio::test]
    async fn unknown_fields_are_ignored_for_forward_compatibility() {
        let rig = rig();
        let _out = rig.connect(1);
        let mut reg = register("bot", &[]);
        reg["future"] = json!({"anything": true});
        reg["agent"]["color"] = json!("teal");
        assert_eq!(rig.send(1, reg)["type"], "agent.registered");
        let mut up = session("bot", "s1", "running");
        up["session"]["etaSeconds"] = json!(90);
        assert_eq!(rig.send(1, up)["type"], "ack");
    }

    #[tokio::test]
    async fn a_connection_cannot_speak_for_or_take_over_another_agent() {
        let rig = rig();
        let _a = rig.connect(1);
        let _b = rig.connect(2);
        rig.send(1, register("bot", &[]));
        rig.send(1, session("bot", "s1", "running"));

        assert_eq!(code(&rig.send(2, register("bot", &[]))), "agent_in_use");
        assert_eq!(
            code(&rig.send(2, session("bot", "s1", "failed"))),
            "not_registered"
        );
        assert_eq!(status_of(&rig, "s1"), Status::Running);

        // Registered under its own id it still cannot touch someone else's session.
        rig.send(2, register("other", &[]));
        assert_eq!(
            code(&rig.send(2, session("other", "s1", "failed"))),
            "not_owner"
        );
        assert_eq!(status_of(&rig, "s1"), Status::Running);
    }

    #[tokio::test]
    async fn a_connection_registers_a_bounded_number_of_agents() {
        let rig = rig();
        let _out = rig.connect(1);
        for i in 0..MAX_AGENTS_PER_CONNECTION {
            assert_eq!(
                rig.send(1, register(&format!("a{i}"), &[]))["type"],
                "agent.registered"
            );
        }
        assert_eq!(
            code(&rig.send(1, register("one-more", &[]))),
            "limit_reached"
        );
        assert!(rig.ctl.agent("one-more").is_none());
        // Re-registering an existing one is fine.
        assert_eq!(
            rig.send(1, register("a0", &["pause"]))["type"],
            "agent.registered"
        );
    }

    #[tokio::test]
    async fn replies_echo_the_correlation_id() {
        let rig = rig();
        let _out = rig.connect(1);
        let mut m = register("bot", &[]);
        m["id"] = json!("req-7");
        assert_eq!(rig.send(1, m)["id"], "req-7");
        let mut bad = json!({"type":"agent.nope"});
        bad["id"] = json!(42);
        let r = rig.send(1, bad);
        assert_eq!(r["id"], 42);
        assert_eq!(code(&r), "unsupported_type");
    }

    #[tokio::test]
    async fn approval_lifecycle_over_the_bridge() {
        let mut rig = rig();
        let mut out = rig.connect(1);
        rig.send(1, register("bot", &["approve", "reject"]));
        rig.send(1, session("bot", "s1", "running"));
        let ack = rig.send(
            1,
            json!({"type":"agent.approval.request","agentId":"bot","sessionId":"s1",
                   "approval":{"id":"ap1","message":"Run the migration?","tool":"db"}}),
        );
        assert_eq!(ack["approvalId"], "ap1");
        assert_eq!(status_of(&rig, "s1"), Status::ApprovalRequired);

        // Monitor-only: the human cannot approve yet, and nothing is sent.
        let refused = rig
            .ctl
            .command(Target::Session("s1".into()), Command::Approve);
        assert!(refused.is_err(), "monitor-only agents take no commands");
        assert!(out.try_recv().is_err());

        // The user grants control; the client is told.
        rig.ctl.set_access("bot", AccessLevel::Control).unwrap();
        let frame: Value = serde_json::from_str(&out.try_recv().unwrap()).unwrap();
        assert_eq!(frame["type"], "agent.access");
        assert_eq!(frame["access"], "control");

        let request_id = rig
            .ctl
            .command(Target::Session("s1".into()), Command::Approve)
            .unwrap();
        let frame: Value = serde_json::from_str(&out.try_recv().unwrap()).unwrap();
        assert_eq!(frame["type"], "agent.command");
        assert_eq!(frame["requestId"], request_id.as_str());
        assert_eq!(frame["agentId"], "bot");
        assert_eq!(frame["sessionId"], "s1");
        assert_eq!(frame["approvalId"], "ap1");
        assert_eq!(frame["command"], json!({"kind":"approve"}));
        // Still pending: approval only resolves when the agent confirms.
        assert!(rig.ctl.session("s1").unwrap().pending_approval.is_some());

        let ok = rig.send(
            1,
            json!({"type":"agent.command.result","agentId":"bot","requestId":request_id,"ok":true}),
        );
        assert_eq!(ok["type"], "ack");
        assert!(rig.ctl.session("s1").unwrap().pending_approval.is_none());
        rig.send(1, session("bot", "s1", "running"));
        let kinds: Vec<String> = rig
            .events()
            .into_iter()
            .filter(|e| e.source == "agent-control")
            .map(|e| e.kind)
            .collect();
        for expected in ["approval.created", "command.sent", "approval.resolved"] {
            assert!(kinds.iter().any(|k| k == expected), "missing {expected}");
        }
        // The same answer cannot be replayed.
        let again = rig.send(
            1,
            json!({"type":"agent.command.result","agentId":"bot","requestId":request_id,"ok":true}),
        );
        assert_eq!(code(&again), "unknown_request");
    }

    #[tokio::test]
    async fn a_failed_command_result_reaches_the_session() {
        let rig = rig();
        let mut out = rig.connect(1);
        rig.send(1, register("bot", &["pause"]));
        rig.send(1, session("bot", "s1", "running"));
        rig.ctl.set_access("bot", AccessLevel::Control).unwrap();
        let _ = out.try_recv(); // agent.access
        let id = rig
            .ctl
            .command(Target::Session("s1".into()), Command::Pause)
            .unwrap();
        rig.send(
            1,
            json!({"type":"agent.command.result","agentId":"bot","requestId":id,"ok":false,"error":"cannot pause during commit"}),
        );
        let outcome = rig.ctl.session("s1").unwrap().last_command.unwrap();
        assert_eq!(outcome.state, OutcomeState::Failed);
        assert_eq!(outcome.error.as_deref(), Some("cannot pause during commit"));
        // The status is whatever the agent reports; a failed pause did not pause it.
        assert_eq!(status_of(&rig, "s1"), Status::Running);
    }

    #[tokio::test]
    async fn only_the_receiving_connection_may_answer_a_command() {
        let rig = rig();
        let mut out = rig.connect(1);
        let _other = rig.connect(2);
        rig.send(1, register("bot", &["cancel"]));
        rig.send(2, register("rival", &[]));
        rig.send(1, session("bot", "s1", "running"));
        rig.ctl.set_access("bot", AccessLevel::Control).unwrap();
        let _ = out.try_recv();
        let id = rig
            .ctl
            .command(Target::Session("s1".into()), Command::Cancel)
            .unwrap();
        let forged = rig.send(
            2,
            json!({"type":"agent.command.result","agentId":"rival","requestId":id,"ok":true}),
        );
        assert_eq!(code(&forged), "unknown_request");
        let forged = rig.send(
            2,
            json!({"type":"agent.command.result","agentId":"bot","requestId":id,"ok":true}),
        );
        assert_eq!(code(&forged), "not_registered");
        assert_eq!(
            rig.ctl.session("s1").unwrap().last_command.unwrap().state,
            OutcomeState::Pending
        );
    }

    #[tokio::test]
    async fn spawn_and_instruction_payloads_reach_the_client() {
        let rig = rig();
        let mut out = rig.connect(1);
        rig.send(1, register("bot", &["spawn", "send_instruction"]));
        rig.send(1, session("bot", "s1", "waiting"));
        rig.ctl.set_access("bot", AccessLevel::Control).unwrap();
        let _ = out.try_recv();
        rig.ctl
            .command(
                Target::Agent("bot".into()),
                Command::Spawn {
                    task: "summarize the repo".into(),
                    cwd: Some("/work/app".into()),
                },
            )
            .unwrap();
        let frame: Value = serde_json::from_str(&out.try_recv().unwrap()).unwrap();
        assert_eq!(
            frame["command"],
            json!({"kind":"spawn","task":"summarize the repo","cwd":"/work/app"})
        );
        assert_eq!(frame["sessionId"], Value::Null);
    }

    #[tokio::test]
    async fn disconnecting_takes_agents_offline_and_fails_inflight_commands() {
        let mut rig = rig();
        let mut out = rig.connect(1);
        rig.send(1, register("bot", &["pause"]));
        rig.send(1, session("bot", "s1", "running"));
        rig.ctl.set_access("bot", AccessLevel::Control).unwrap();
        let _ = out.try_recv();
        rig.ctl
            .command(Target::Session("s1".into()), Command::Pause)
            .unwrap();
        rig.events();

        rig.bridge.disconnected(1);
        assert!(!rig.ctl.agent("bot").unwrap().online);
        assert_eq!(
            rig.ctl.session("s1").unwrap().last_command.unwrap().state,
            OutcomeState::Failed
        );
        let evs = rig.events();
        assert!(evs.iter().any(|e| e.kind == "agent.offline"));

        // A new process can take the id over, and it comes back online.
        let _out2 = rig.connect(5);
        assert_eq!(
            rig.send(5, register("bot", &["pause"]))["type"],
            "agent.registered"
        );
        assert!(rig.ctl.agent("bot").unwrap().online);
        // Access granted earlier is remembered for this agent id.
        assert_eq!(rig.ctl.agent("bot").unwrap().access, AccessLevel::Control);
    }

    #[tokio::test]
    async fn an_offline_agent_does_not_keep_an_unanswerable_approval() {
        let rig = rig();
        let _out = rig.connect(1);
        rig.send(1, register("bot", &["approve"]));
        rig.send(1, session("bot", "s1", "running"));
        rig.send(
            1,
            json!({"type":"agent.approval.request","agentId":"bot","sessionId":"s1",
                   "approval":{"message":"Delete the build directory?"}}),
        );
        assert!(rig.ctl.session("s1").unwrap().pending_approval.is_some());
        rig.bridge.disconnected(1);
        let s = rig.ctl.session("s1").unwrap();
        assert!(s.pending_approval.is_none());
        assert_ne!(s.status, Status::ApprovalRequired);
    }

    #[tokio::test]
    async fn commands_to_an_unreachable_client_fail_cleanly() {
        let rig = rig();
        let (tx, _rx) = mpsc::channel(1);
        rig.bridge.connected(1, tx);
        rig.send(1, register("bot", &["pause"]));
        rig.send(1, session("bot", "s1", "running"));
        // Capacity 1: the access frame fills the queue, so the command cannot be queued.
        rig.ctl.set_access("bot", AccessLevel::Control).unwrap();
        let id = rig
            .ctl
            .command(Target::Session("s1".into()), Command::Pause)
            .unwrap();
        let outcome = rig.ctl.session("s1").unwrap().last_command.unwrap();
        assert_eq!(outcome.state, OutcomeState::Failed, "request {id}");
        assert!(outcome.error.unwrap().contains("not reading"));
    }

    #[tokio::test]
    async fn unregister_releases_the_id_and_sets_the_agent_offline() {
        let rig = rig();
        let _a = rig.connect(1);
        let _b = rig.connect(2);
        rig.send(1, register("bot", &[]));
        assert_eq!(
            rig.send(1, json!({"type":"agent.unregister","agentId":"bot"}))["type"],
            "ack"
        );
        assert!(!rig.ctl.agent("bot").unwrap().online);
        assert_eq!(
            code(&rig.send(1, session("bot", "s1", "running"))),
            "not_registered"
        );
        assert_eq!(
            rig.send(2, register("bot", &[]))["type"],
            "agent.registered"
        );
    }

    #[test]
    fn the_rate_limiter_allows_bursts_then_throttles_then_recovers() {
        let t0 = Instant::now();
        let mut b = Bucket::new(t0);
        let allowed = (0..200).filter(|_| b.take(t0)).count();
        assert_eq!(allowed as f64, BURST);
        assert!(!b.take(t0));
        // Half a second refills about fifteen messages.
        let later = t0 + Duration::from_millis(500);
        let refilled = (0..100).filter(|_| b.take(later)).count();
        assert!((14..=16).contains(&refilled), "refilled {refilled}");
    }

    #[tokio::test]
    async fn flooding_a_connection_is_rate_limited() {
        let rig = rig();
        let _out = rig.connect(1);
        rig.send(1, register("bot", &[]));
        let mut limited = false;
        for i in 0..500 {
            let r = rig.send(
                1,
                session("bot", "s1", if i % 2 == 0 { "running" } else { "tool_use" }),
            );
            if r["type"] == "error" {
                assert_eq!(code(&r), "rate_limited");
                limited = true;
                break;
            }
        }
        assert!(limited, "a flood was not throttled");
    }

    #[test]
    fn command_frames_have_a_stable_shape() {
        let f = command_frame(&AdapterCommand {
            request_id: "req-1".into(),
            agent_id: "bot".into(),
            session_id: Some("s1".into()),
            approval_id: None,
            command: Command::SendInstruction {
                text: "go on".into(),
            },
        });
        assert_eq!(
            f,
            json!({
                "type": "agent.command",
                "requestId": "req-1",
                "agentId": "bot",
                "sessionId": "s1",
                "approvalId": null,
                "command": { "kind": "send_instruction", "text": "go on" }
            })
        );
    }
}
