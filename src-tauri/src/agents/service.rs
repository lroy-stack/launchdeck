//! `AgentControl`: the single source of truth for agents, sessions, approvals,
//! slots, focus and command lifecycle.
//!
//! ```text
//!   adapters --(AdapterHost)--> AgentControl --> events on the bus   (source "agent-control")
//!                                    |       \-> projection events    (slot.changed, focus.changed, command.state)
//!                                    |       \-> AgentsSnapshot       (desktop UI)
//!   pads / UI / bridge --(command)---+--(AgentAdapter::execute)--> adapters
//! ```
//!
//! Everything that anyone sees (the desktop app, the virtual Launchpad, a
//! physical Launchpad) is derived from the state in this file through
//! [`AgentControl::flush`]. There is no second copy.
//!
//! Locking: one mutex. Events and the UI snapshot are published while the lock
//! is held so that their order always matches the order of state changes (a
//! stale pad state can never overwrite a fresh one). Adapters are only ever
//! called with the lock released, and the sink must not call back in.

use std::collections::{BTreeMap, HashMap, VecDeque};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use serde_json::{json, Value};
use tokio::time::Instant;

use super::adapter::{
    AdapterCommand, AdapterHost, AdapterKind, AdapterState, AgentAdapter, AgentSpec, ApprovalSpec,
    DiagnosticLevel, HostError, RequestId, SessionUpdate,
};
use super::audit::{Actor, Audit, AuditEntry};
use super::model::*;
use super::notify::{self, Notification};
use super::settings::{AgentsStorage, Settings};
use super::view::*;
use super::visual::{self, CommandState};
use crate::events::{now_ms, Event, EventBus};

/// Event source for everything Agent Control publishes.
pub const SOURCE: &str = "agent-control";
/// Pads per row on the surface: one track per slot.
pub const SLOT_COUNT: usize = 8;

/// Projection events exist so that surface bindings can follow the state.
/// They carry no news for a person reading an event log.
pub fn is_projection(event: &Event) -> bool {
    event.source == SOURCE
        && matches!(
            event.kind.as_str(),
            "slot.changed" | "focus.changed" | "command.state"
        )
}

const MAX_AGENTS: usize = 64;
const MAX_SESSIONS: usize = 120;
const MAX_SESSIONS_PER_AGENT: usize = 32;
const MAX_DIAGNOSTICS: usize = 20;
pub const MAX_TEXT: usize = 2000;
pub const MAX_CWD: usize = 512;
const AUDIT_IN_SNAPSHOT: usize = 40;

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy)]
pub struct Timing {
    /// An accepted command with no result after this long fails.
    pub command_timeout: Duration,
    /// How long a pad shows a succeeded command.
    pub succeeded_feedback: Duration,
    /// How long a pad shows a failed command.
    pub failed_feedback: Duration,
    /// Completions after at least this long raise a notification.
    pub long_running: Duration,
}

impl Default for Timing {
    fn default() -> Self {
        Self {
            command_timeout: Duration::from_secs(15),
            succeeded_feedback: Duration::from_millis(2500),
            failed_feedback: Duration::from_millis(4000),
            long_running: Duration::from_secs(60),
        }
    }
}

/// Receives UI snapshots and notifications. Called with the state lock held:
/// implementations must not call back into [`AgentControl`].
pub trait AgentsSink: Send + Sync {
    fn changed(&self, snapshot: &AgentsSnapshot);
    fn notify(&self, notification: &Notification);
}

pub struct NullSink;
impl AgentsSink for NullSink {
    fn changed(&self, _snapshot: &AgentsSnapshot) {}
    fn notify(&self, _notification: &Notification) {}
}

/// Which track a command is aimed at.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    /// Whatever is selected on the surface.
    Focus,
    Session(SessionId),
    Agent(AgentId),
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CommandRefused {
    #[error("{}", .0.label())]
    Unavailable(Unavailable),
    #[error("nothing is selected")]
    NoTarget,
    #[error("a command for this session is already pending")]
    AlreadyPending,
    #[error("invalid {0}")]
    Invalid(&'static str),
}

/// What occupies a slot: a session, or an agent that has none.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Track {
    Session {
        session_id: SessionId,
        agent_id: AgentId,
    },
    Agent {
        agent_id: AgentId,
    },
}

impl Track {
    fn agent_id(&self) -> &str {
        match self {
            Track::Session { agent_id, .. } | Track::Agent { agent_id } => agent_id,
        }
    }
}

// ---------------------------------------------------------------------------
// Internal state
// ---------------------------------------------------------------------------

struct AgentRec {
    agent: Agent,
    ordinal: u64,
}

struct SessionRec {
    session: Session,
    ordinal: u64,
    /// Start of the current run, for the long-running notification.
    run_started: Instant,
}

struct PendingCmd {
    agent_id: AgentId,
    session_id: Option<SessionId>,
    kind: CommandKind,
}

struct Flash {
    kind: CommandKind,
    outcome: OutcomeState,
    token: u64,
}

struct Diagnostic {
    ts: u64,
    level: DiagnosticLevel,
    message: String,
}

struct AdapterEntry {
    adapter: Arc<dyn AgentAdapter>,
    info: super::adapter::AdapterInfo,
    state: AdapterState,
    detail: Option<String>,
    diagnostics: VecDeque<Diagnostic>,
}

#[derive(Clone, PartialEq)]
struct SlotProj {
    status: &'static str,
    agent_id: Option<String>,
    session_id: Option<String>,
    label: String,
}

#[derive(Clone, PartialEq)]
struct Projection {
    slots: Vec<SlotProj>,
    focus: Option<usize>,
    commands: BTreeMap<CommandKind, CommandState>,
}

struct State {
    rev: u64,
    seq: u64,
    agents: BTreeMap<AgentId, AgentRec>,
    sessions: BTreeMap<SessionId, SessionRec>,
    slots: Vec<Option<Track>>,
    focus: Option<usize>,
    pending: HashMap<RequestId, PendingCmd>,
    flashes: HashMap<String, Flash>,
    adapters: BTreeMap<String, AdapterEntry>,
    settings: Settings,
    audit: Audit,
    published: Option<Projection>,
}

impl State {
    fn next(&mut self) -> u64 {
        self.seq += 1;
        self.seq
    }
}

enum Schedule {
    ClearFlash {
        key: String,
        token: u64,
        after: Duration,
    },
    CommandTimeout {
        request_id: RequestId,
    },
}

#[derive(Default)]
struct Out {
    events: Vec<(String, Value)>,
    audit: Vec<AuditEntry>,
    notifications: Vec<Notification>,
    autofocus: Option<SessionId>,
    schedule: Vec<Schedule>,
}

impl Out {
    fn event(&mut self, kind: &str, payload: Value) {
        self.events.push((kind.to_string(), payload));
    }
    fn audit(&mut self, entry: AuditEntry) {
        self.audit.push(entry);
    }
}

// ---------------------------------------------------------------------------
// The service
// ---------------------------------------------------------------------------

pub struct AgentControl {
    state: Mutex<State>,
    bus: EventBus,
    sink: Arc<dyn AgentsSink>,
    storage: Option<AgentsStorage>,
    timing: Timing,
    handle: tokio::runtime::Handle,
}

impl AgentControl {
    /// Must be called from within a Tokio runtime (timers are spawned on it).
    pub fn new(
        bus: EventBus,
        sink: Arc<dyn AgentsSink>,
        storage: Option<AgentsStorage>,
        timing: Timing,
    ) -> Arc<Self> {
        let settings = storage.as_ref().map(|s| s.load()).unwrap_or_default();
        let audit = Audit::new(storage.as_ref().map(|s| s.audit_path()));
        Arc::new(Self {
            state: Mutex::new(State {
                rev: 0,
                seq: 0,
                agents: BTreeMap::new(),
                sessions: BTreeMap::new(),
                slots: vec![None; SLOT_COUNT],
                focus: None,
                pending: HashMap::new(),
                flashes: HashMap::new(),
                adapters: BTreeMap::new(),
                settings,
                audit,
                published: None,
            }),
            bus,
            sink,
            storage,
            timing,
            handle: tokio::runtime::Handle::current(),
        })
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// Run a mutation, then publish everything derived from the new state.
    fn mutate<R>(self: &Arc<Self>, f: impl FnOnce(&mut State, &mut Out) -> R) -> R {
        let (result, schedule) = {
            let mut st = self.lock();
            let mut out = Out::default();
            let result = f(&mut st, &mut out);
            let schedule = self.flush(&mut st, out);
            (result, schedule)
        };
        for s in schedule {
            self.run_schedule(s);
        }
        result
    }

    fn run_schedule(self: &Arc<Self>, s: Schedule) {
        let this = self.clone();
        match s {
            Schedule::ClearFlash { key, token, after } => {
                self.handle.spawn(async move {
                    tokio::time::sleep(after).await;
                    this.mutate(|st, _| {
                        if st.flashes.get(&key).is_some_and(|f| f.token == token) {
                            st.flashes.remove(&key);
                        }
                    });
                });
            }
            Schedule::CommandTimeout { request_id } => {
                let after = self.timing.command_timeout;
                self.handle.spawn(async move {
                    tokio::time::sleep(after).await;
                    this.complete_command(
                        &request_id,
                        Err("the adapter did not answer in time".to_string()),
                    );
                });
            }
        }
    }

    /// Reconcile derived state, then publish events, audit, notifications,
    /// projection and the UI snapshot. Returns timers to start.
    fn flush(&self, st: &mut State, mut out: Out) -> Vec<Schedule> {
        reconcile_slots(st);
        if let Some(sid) = out.autofocus.take() {
            if st.focus.is_none() {
                st.focus = slot_of_session(st, &sid);
            }
        }
        if st.focus.is_some_and(|i| st.slots[i].is_none()) {
            st.focus = None;
        }

        for (kind, payload) in out.events.drain(..) {
            self.bus.publish(Event::new(SOURCE, kind, payload));
        }
        for entry in out.audit.drain(..) {
            st.audit.record(entry);
        }
        if st.settings.notifications {
            for n in &out.notifications {
                self.sink.notify(n);
            }
        }

        let projection = project(st);
        self.publish_projection(st.published.as_ref(), &projection);
        st.published = Some(projection);

        st.rev += 1;
        let snapshot = build_snapshot(st);
        self.sink.changed(&snapshot);
        out.schedule
    }

    fn publish_projection(&self, old: Option<&Projection>, new: &Projection) {
        let publish = |kind: &str, payload: Value| {
            self.bus.publish(Event::new(SOURCE, kind, payload));
        };
        for (i, slot) in new.slots.iter().enumerate() {
            if old.is_some_and(|o| o.slots.get(i) == Some(slot)) {
                continue;
            }
            publish(
                "slot.changed",
                json!({
                    "slot": i,
                    "status": slot.status,
                    "agentId": slot.agent_id,
                    "sessionId": slot.session_id,
                    "label": slot.label,
                }),
            );
        }
        if old.is_none_or_differs(|o| o.focus != new.focus) {
            publish("focus.changed", json!({ "slot": new.focus }));
        }
        for (kind, state) in &new.commands {
            if old.is_some_and(|o| o.commands.get(kind) == Some(state)) {
                continue;
            }
            publish(
                "command.state",
                json!({ "command": kind.as_str(), "state": state.as_str() }),
            );
        }
    }

    // -----------------------------------------------------------------------
    // Observation
    // -----------------------------------------------------------------------

    pub fn snapshot(&self) -> AgentsSnapshot {
        build_snapshot(&self.lock())
    }

    pub fn session(&self, id: &str) -> Option<Session> {
        self.lock().sessions.get(id).map(|r| r.session.clone())
    }

    pub fn agent(&self, id: &str) -> Option<Agent> {
        self.lock().agents.get(id).map(|r| r.agent.clone())
    }

    pub fn slots(&self) -> Vec<Option<Track>> {
        self.lock().slots.clone()
    }

    pub fn focus(&self) -> Option<usize> {
        self.lock().focus
    }

    pub fn settings(&self) -> Settings {
        self.lock().settings.clone()
    }

    /// Publish the complete projection again (startup, workspace reset, a
    /// surface that was just created). Cheap and idempotent.
    pub fn republish(self: &Arc<Self>) {
        self.mutate(|st, _| st.published = None);
    }

    // -----------------------------------------------------------------------
    // Adapters
    // -----------------------------------------------------------------------

    pub fn register_adapter(self: &Arc<Self>, adapter: Arc<dyn AgentAdapter>) {
        let info = adapter.info();
        let id = info.id.clone();
        self.mutate(|st, _| {
            st.adapters.insert(
                id.clone(),
                AdapterEntry {
                    adapter: adapter.clone(),
                    info,
                    state: AdapterState::Active,
                    detail: None,
                    diagnostics: VecDeque::new(),
                },
            );
        });
        adapter.attach(AdapterHost::new(&id, self.clone()));
    }

    pub(crate) fn host_set_adapter_state(
        self: &Arc<Self>,
        adapter: &str,
        state: AdapterState,
        detail: Option<&str>,
    ) {
        let detail = detail.map(|d| summarize(d, MAX_ERROR_LEN));
        self.mutate(|st, _| {
            if let Some(a) = st.adapters.get_mut(adapter) {
                a.state = state;
                a.detail = detail;
            }
        });
    }

    pub(crate) fn host_diagnostic(
        self: &Arc<Self>,
        adapter: &str,
        level: DiagnosticLevel,
        message: &str,
    ) {
        match level {
            DiagnosticLevel::Info => log::info!("[{adapter}] {message}"),
            DiagnosticLevel::Warn => log::warn!("[{adapter}] {message}"),
            DiagnosticLevel::Error => log::error!("[{adapter}] {message}"),
        }
        let message = summarize(message, 400);
        self.mutate(|st, _| {
            if let Some(a) = st.adapters.get_mut(adapter) {
                a.diagnostics.push_back(Diagnostic {
                    ts: now_ms(),
                    level,
                    message,
                });
                while a.diagnostics.len() > MAX_DIAGNOSTICS {
                    a.diagnostics.pop_front();
                }
            }
        });
    }

    // -----------------------------------------------------------------------
    // Host operations (what adapters call)
    // -----------------------------------------------------------------------

    pub(crate) fn host_register_agent(
        self: &Arc<Self>,
        adapter: &str,
        spec: AgentSpec,
    ) -> Result<(), HostError> {
        check_id(&spec.id, "agent id")?;
        let name = summarize(&spec.name, MAX_NAME_LEN);
        if name.is_empty() {
            return Err(HostError::Invalid("agent name"));
        }
        let provider = spec.provider.as_deref().map(|p| summarize(p, 40));
        let harness = spec.harness.as_deref().map(|p| summarize(p, 40));
        self.mutate(|st, out| {
            let now = now_ms();
            if let Some(rec) = st.agents.get_mut(&spec.id) {
                if rec.agent.adapter_id != adapter {
                    return Err(HostError::IdTaken(spec.id.clone()));
                }
                rec.agent.name = name;
                rec.agent.provider = provider;
                rec.agent.harness = harness;
                rec.agent.capabilities = spec.capabilities.clone();
                rec.agent.last_seen = now;
                if !rec.agent.online {
                    rec.agent.online = true;
                    out.event("agent.online", json!({ "agentId": spec.id }));
                }
                return Ok(());
            }
            if st.agents.len() >= MAX_AGENTS {
                return Err(HostError::LimitReached("too many agents"));
            }
            let default_access = match st.adapters.get(adapter).map(|a| a.info.kind) {
                Some(AdapterKind::Simulated) => AccessLevel::Control,
                _ => AccessLevel::Monitor,
            };
            let access = st
                .settings
                .access
                .get(&spec.id)
                .copied()
                .unwrap_or(default_access);
            let ordinal = st.next();
            st.agents.insert(
                spec.id.clone(),
                AgentRec {
                    agent: Agent {
                        id: spec.id.clone(),
                        name,
                        provider,
                        harness,
                        adapter_id: adapter.to_string(),
                        capabilities: spec.capabilities.clone(),
                        access,
                        online: true,
                        registered_at: now,
                        last_seen: now,
                    },
                    ordinal,
                },
            );
            out.event(
                "agent.registered",
                json!({ "agentId": spec.id, "adapterId": adapter, "access": access }),
            );
            out.event("agent.online", json!({ "agentId": spec.id }));
            out.audit(
                AuditEntry::new("agent.registered", Actor::Agent)
                    .agent(&spec.id)
                    .detail(adapter),
            );
            Ok(())
        })
    }

    pub(crate) fn host_agent_access(
        &self,
        adapter: &str,
        agent_id: &str,
    ) -> Result<AccessLevel, HostError> {
        let st = self.lock();
        owned_agent(&st, adapter, agent_id)?;
        Ok(st.agents[agent_id].agent.access)
    }

    pub(crate) fn host_set_online(
        self: &Arc<Self>,
        adapter: &str,
        agent_id: &str,
        online: bool,
    ) -> Result<(), HostError> {
        let timing = self.timing;
        self.mutate(|st, out| {
            owned_agent(st, adapter, agent_id)?;
            let rec = st.agents.get_mut(agent_id).expect("checked");
            rec.agent.last_seen = now_ms();
            if rec.agent.online == online {
                return Ok(());
            }
            rec.agent.online = online;
            let name = rec.agent.name.clone();
            if online {
                out.event("agent.online", json!({ "agentId": agent_id }));
            } else {
                out.event("agent.offline", json!({ "agentId": agent_id }));
                out.notifications
                    .push(notify::adapter_disconnected(agent_id, &name));
                let ids: Vec<RequestId> = st
                    .pending
                    .iter()
                    .filter(|(_, p)| p.agent_id == agent_id)
                    .map(|(id, _)| id.clone())
                    .collect();
                for id in ids {
                    complete_in(st, out, &timing, &id, Err("the agent went offline".into()));
                }
                // Nobody is left to receive a decision: drop the approval gate
                // instead of leaving a pad that can never be answered.
                let stuck: Vec<SessionId> = st
                    .sessions
                    .values()
                    .filter(|r| {
                        r.session.agent_id == agent_id && r.session.pending_approval.is_some()
                    })
                    .map(|r| r.session.id.clone())
                    .collect();
                for sid in stuck {
                    apply_status(st, out, &timing, &sid, Status::Waiting, false);
                }
            }
            Ok(())
        })
    }

    pub(crate) fn host_update_session(
        self: &Arc<Self>,
        adapter: &str,
        agent_id: &str,
        update: SessionUpdate,
    ) -> Result<(), HostError> {
        check_id(&update.session_id, "session id")?;
        if update.status == Status::Offline {
            return Err(HostError::Invalid("status (use set_online for agents)"));
        }
        if update.status == Status::ApprovalRequired {
            // An approval gate without an approval could never be answered.
            return Err(HostError::Invalid("status (use request_approval)"));
        }
        let progress = match update.progress {
            Some(p) if p.is_finite() => Some(p.clamp(0.0, 1.0)),
            Some(_) => return Err(HostError::Invalid("progress")),
            None => None,
        };
        let task = update.task.as_deref().map(|t| summarize(t, MAX_TASK_LEN));
        let detail = update
            .detail
            .as_deref()
            .map(|t| summarize(t, MAX_DETAIL_LEN));
        let error = update.error.as_deref().map(|t| summarize(t, MAX_ERROR_LEN));
        let timing = self.timing;
        self.mutate(|st, out| {
            owned_agent(st, adapter, agent_id)?;
            let sid = update.session_id.as_str();
            let created = match st.sessions.get(sid) {
                Some(r) if r.session.agent_id != agent_id => {
                    return Err(HostError::SessionOwnedByOther(sid.to_string()))
                }
                Some(_) => false,
                None => true,
            };
            if created {
                make_room(st, out, agent_id)?;
                let ordinal = st.next();
                st.sessions.insert(
                    sid.to_string(),
                    SessionRec {
                        session: Session {
                            id: sid.to_string(),
                            agent_id: agent_id.to_string(),
                            task: None,
                            status: Status::Idle,
                            started_at: None,
                            updated_at: now_ms(),
                            pending_approval: None,
                            progress: None,
                            detail: None,
                            error: None,
                            last_command: None,
                        },
                        ordinal,
                        run_started: Instant::now(),
                    },
                );
                out.event(
                    "session.created",
                    json!({ "agentId": agent_id, "sessionId": sid }),
                );
            }
            let rec = st.sessions.get_mut(sid).expect("exists");
            let changing = created || rec.session.status != update.status;
            if let Some(t) = task {
                rec.session.task = Some(t);
            }
            rec.session.progress = progress.or(if changing { None } else { rec.session.progress });
            rec.session.detail = detail.or(if changing {
                None
            } else {
                rec.session.detail.take()
            });
            rec.session.error = if update.status == Status::Failed {
                error.or(rec.session.error.take())
            } else {
                None
            };
            rec.session.updated_at = now_ms();
            if let Some(a) = st.agents.get_mut(agent_id) {
                a.agent.last_seen = rec.session.updated_at;
            }
            apply_status(st, out, &timing, sid, update.status, created);
            Ok(())
        })
    }

    pub(crate) fn host_request_approval(
        self: &Arc<Self>,
        adapter: &str,
        agent_id: &str,
        session_id: &str,
        spec: ApprovalSpec,
    ) -> Result<ApprovalId, HostError> {
        let message = summarize(&spec.message, MAX_MESSAGE_LEN);
        if message.is_empty() {
            return Err(HostError::Invalid("approval message"));
        }
        if let Some(id) = &spec.id {
            check_id(id, "approval id")?;
        }
        let tool = spec.tool.as_deref().map(|t| summarize(t, 60));
        let timing = self.timing;
        self.mutate(|st, out| {
            owned_agent(st, adapter, agent_id)?;
            match st.sessions.get(session_id) {
                None => return Err(HostError::UnknownSession(session_id.to_string())),
                Some(r) if r.session.agent_id != agent_id => {
                    return Err(HostError::SessionOwnedByOther(session_id.to_string()))
                }
                Some(_) => {}
            }
            // One pending approval per session: a newer request replaces the older.
            resolve_approval(st, out, session_id, "superseded", Actor::System);
            let id = spec
                .id
                .clone()
                .unwrap_or_else(|| format!("appr-{}", st.next()));
            let name = st
                .agents
                .get(agent_id)
                .map(|a| a.agent.name.clone())
                .unwrap_or_default();
            let rec = st.sessions.get_mut(session_id).expect("checked");
            rec.session.pending_approval = Some(Approval {
                id: id.clone(),
                session_id: session_id.to_string(),
                message,
                tool: tool.clone(),
                created_at: now_ms(),
            });
            out.event(
                "approval.created",
                json!({
                    "approvalId": id,
                    "sessionId": session_id,
                    "agentId": agent_id,
                    "tool": tool,
                }),
            );
            out.audit(
                AuditEntry::new("approval.requested", Actor::Agent)
                    .agent(agent_id)
                    .session(session_id)
                    .tool(tool.as_deref()),
            );
            out.notifications.push(notify::approval_required(
                agent_id,
                session_id,
                &name,
                tool.as_deref(),
            ));
            if st.focus.is_none() {
                out.autofocus = Some(session_id.to_string());
            }
            apply_status(
                st,
                out,
                &timing,
                session_id,
                Status::ApprovalRequired,
                false,
            );
            Ok(id)
        })
    }

    pub(crate) fn host_withdraw_approval(
        self: &Arc<Self>,
        adapter: &str,
        session_id: &str,
        approval_id: &str,
    ) -> Result<(), HostError> {
        self.mutate(|st, out| {
            let agent_id = st
                .sessions
                .get(session_id)
                .map(|r| r.session.agent_id.clone())
                .ok_or_else(|| HostError::UnknownSession(session_id.to_string()))?;
            owned_agent(st, adapter, &agent_id)?;
            let matches = st
                .sessions
                .get(session_id)
                .and_then(|r| r.session.pending_approval.as_ref())
                .is_some_and(|a| a.id == approval_id);
            if matches {
                resolve_approval(st, out, session_id, "withdrawn", Actor::Agent);
            }
            Ok(())
        })
    }

    pub(crate) fn host_end_session(
        self: &Arc<Self>,
        adapter: &str,
        session_id: &str,
    ) -> Result<(), HostError> {
        self.mutate(|st, out| {
            let agent_id = st
                .sessions
                .get(session_id)
                .map(|r| r.session.agent_id.clone())
                .ok_or_else(|| HostError::UnknownSession(session_id.to_string()))?;
            owned_agent(st, adapter, &agent_id)?;
            remove_session(st, out, session_id, "closed by the agent");
            Ok(())
        })
    }

    pub(crate) fn host_command_result(
        self: &Arc<Self>,
        adapter: &str,
        request_id: &str,
        result: Result<(), String>,
    ) -> Result<(), HostError> {
        {
            let st = self.lock();
            let p = st
                .pending
                .get(request_id)
                .ok_or_else(|| HostError::UnknownRequest(request_id.to_string()))?;
            owned_agent(&st, adapter, &p.agent_id)?;
        }
        self.complete_command(request_id, result);
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Commands (what people do)
    // -----------------------------------------------------------------------

    /// Issue a command. Returns once it has been handed to the adapter; the
    /// outcome (pending, succeeded, failed) is visible in the state and as
    /// `command.completed`.
    pub fn command(
        self: &Arc<Self>,
        target: Target,
        command: Command,
    ) -> Result<RequestId, CommandRefused> {
        validate_command(&command)?;
        let kind = command.kind();
        let (adapter, adapter_cmd) = self.mutate(
            |st, out| -> Result<(Arc<dyn AgentAdapter>, AdapterCommand), CommandRefused> {
                let (agent_id, session_id) =
                    resolve_target(st, &target, kind).ok_or(CommandRefused::NoTarget)?;
                let arec = st.agents.get(&agent_id).ok_or(CommandRefused::NoTarget)?;
                let session = session_id
                    .as_deref()
                    .and_then(|s| st.sessions.get(s))
                    .map(|r| &r.session);
                if let Err(u) = availability(&arec.agent, session, kind) {
                    if u == Unavailable::MonitorOnly && kind.is_critical() {
                        // A blocked intervention is worth a line: it means
                        // something tried to control an agent it may not.
                        out.audit(
                            AuditEntry::new("command.refused", Actor::System)
                                .agent(&agent_id)
                                .detail(kind.as_str())
                                .tool(None),
                        );
                    }
                    return Err(CommandRefused::Unavailable(u));
                }
                if st
                    .pending
                    .values()
                    .any(|p| conflicts(p, &agent_id, session_id.as_deref(), kind))
                {
                    return Err(CommandRefused::AlreadyPending);
                }
                let adapter = st
                    .adapters
                    .get(&arec.agent.adapter_id)
                    .map(|a| a.adapter.clone())
                    .ok_or(CommandRefused::NoTarget)?;
                let approval_id = session
                    .and_then(|s| s.pending_approval.as_ref())
                    .map(|a| a.id.clone())
                    .filter(|_| matches!(kind, CommandKind::Approve | CommandKind::Reject));
                let request_id = format!("req-{}", st.next());
                st.pending.insert(
                    request_id.clone(),
                    PendingCmd {
                        agent_id: agent_id.clone(),
                        session_id: session_id.clone(),
                        kind,
                    },
                );
                if let Some(rec) = session_id.as_deref().and_then(|s| st.sessions.get_mut(s)) {
                    rec.session.last_command = Some(CommandOutcome {
                        command: kind,
                        state: OutcomeState::Pending,
                        error: None,
                        at: now_ms(),
                    });
                }
                st.flashes
                    .remove(&flash_key(&agent_id, session_id.as_deref()));
                out.event(
                    "command.sent",
                    json!({
                        "requestId": request_id,
                        "command": kind.as_str(),
                        "agentId": agent_id,
                        "sessionId": session_id,
                    }),
                );
                out.schedule.push(Schedule::CommandTimeout {
                    request_id: request_id.clone(),
                });
                Ok((
                    adapter,
                    AdapterCommand {
                        request_id,
                        agent_id,
                        session_id,
                        approval_id,
                        command: command.clone(),
                    },
                ))
            },
        )?;

        let request_id = adapter_cmd.request_id.clone();
        if let Err(e) = adapter.execute(adapter_cmd) {
            self.complete_command(&request_id, Err(e.0));
        }
        Ok(request_id)
    }

    fn complete_command(self: &Arc<Self>, request_id: &str, result: Result<(), String>) {
        let timing = self.timing;
        self.mutate(|st, out| {
            complete_in(st, out, &timing, request_id, result);
        });
    }

    // -----------------------------------------------------------------------
    // Selection and housekeeping
    // -----------------------------------------------------------------------

    /// Whether `kind` could be issued to the focused track right now, and if not,
    /// why. The Action Library asks this, so a pad and the library can never
    /// disagree: it is the same `availability` rule the pads use.
    pub fn focus_availability(&self, kind: CommandKind) -> Availability {
        let st = self.lock();
        let Some((agent_id, session_id)) = resolve_target(&st, &Target::Focus, kind) else {
            return Err(Unavailable::NoSession);
        };
        let Some(agent) = st.agents.get(&agent_id) else {
            return Err(Unavailable::NoSession);
        };
        let session = session_id
            .as_deref()
            .and_then(|id| st.sessions.get(id))
            .map(|r| &r.session);
        availability(&agent.agent, session, kind)
    }

    pub fn focus_slot(self: &Arc<Self>, slot: usize) -> Result<(), String> {
        self.mutate(|st, _| {
            if slot >= SLOT_COUNT {
                return Err(format!("slot must be 0-{}", SLOT_COUNT - 1));
            }
            if st.slots[slot].is_none() {
                return Err("that slot is empty".to_string());
            }
            st.focus = Some(slot);
            Ok(())
        })
    }

    pub fn clear_focus(self: &Arc<Self>) {
        self.mutate(|st, _| st.focus = None);
    }

    /// Remove a finished session (or an idle one) from the live set.
    pub fn dismiss_session(self: &Arc<Self>, session_id: &str) -> Result<(), String> {
        self.mutate(|st, out| {
            let status = st
                .sessions
                .get(session_id)
                .map(|r| r.session.status)
                .ok_or_else(|| "unknown session".to_string())?;
            if status.is_active() {
                return Err("the session is still active".to_string());
            }
            remove_session(st, out, session_id, "dismissed");
            Ok(())
        })
    }

    pub fn dismiss_slot(self: &Arc<Self>, slot: usize) -> Result<(), String> {
        let track = self.lock().slots.get(slot).cloned().flatten();
        match track {
            Some(Track::Session { session_id, .. }) => self.dismiss_session(&session_id),
            Some(Track::Agent { .. }) => Err("nothing to dismiss: no session on this slot".into()),
            None => Err("that slot is empty".into()),
        }
    }

    /// Remove every finished session. Returns how many.
    pub fn dismiss_finished(self: &Arc<Self>) -> usize {
        self.mutate(|st, out| {
            let ids: Vec<SessionId> = st
                .sessions
                .iter()
                .filter(|(_, r)| r.session.status.is_terminal())
                .map(|(id, _)| id.clone())
                .collect();
            for id in &ids {
                remove_session(st, out, id, "dismissed");
            }
            ids.len()
        })
    }

    /// Grant or revoke control. Persisted per agent id.
    pub fn set_access(self: &Arc<Self>, agent_id: &str, access: AccessLevel) -> Result<(), String> {
        let storage = self.storage.clone();
        let changed = self.mutate(|st, out| {
            let rec = st
                .agents
                .get_mut(agent_id)
                .ok_or_else(|| "unknown agent".to_string())?;
            let mut notify_adapter = None;
            if rec.agent.access != access {
                rec.agent.access = access;
                notify_adapter = st
                    .adapters
                    .get(&st.agents[agent_id].agent.adapter_id)
                    .map(|a| a.adapter.clone());
                st.settings.access.insert(agent_id.to_string(), access);
                save_settings(&storage, &st.settings);
                let kind = match access {
                    AccessLevel::Control => "access.granted",
                    AccessLevel::Monitor => "access.revoked",
                };
                out.event(
                    "agent.access",
                    json!({ "agentId": agent_id, "access": access }),
                );
                out.audit(AuditEntry::new(kind, Actor::User).agent(agent_id));
            }
            Ok::<_, String>(notify_adapter)
        })?;
        // With the lock released, like every call into an adapter.
        if let Some(adapter) = changed {
            adapter.access_changed(agent_id, access);
        }
        Ok(())
    }

    /// Edit and persist settings (notifications, hook endpoint).
    pub fn update_settings(self: &Arc<Self>, f: impl FnOnce(&mut Settings)) -> Settings {
        let storage = self.storage.clone();
        self.mutate(|st, _| {
            f(&mut st.settings);
            save_settings(&storage, &st.settings);
            st.settings.clone()
        })
    }
}

// ---------------------------------------------------------------------------
// Pure-ish helpers over State
// ---------------------------------------------------------------------------

trait OptionProjectionExt {
    fn is_none_or_differs(&self, f: impl FnOnce(&Projection) -> bool) -> bool;
}
impl OptionProjectionExt for Option<&Projection> {
    fn is_none_or_differs(&self, f: impl FnOnce(&Projection) -> bool) -> bool {
        match self {
            None => true,
            Some(p) => f(p),
        }
    }
}

fn save_settings(storage: &Option<AgentsStorage>, settings: &Settings) {
    if let Some(s) = storage {
        if let Err(e) = s.save(settings) {
            log::error!("could not save agents.json: {e}");
        }
    }
}

fn check_id(id: &str, what: &'static str) -> Result<(), HostError> {
    if valid_id(id) {
        Ok(())
    } else {
        Err(HostError::Invalid(what))
    }
}

fn owned_agent(st: &State, adapter: &str, agent_id: &str) -> Result<(), HostError> {
    match st.agents.get(agent_id) {
        None => Err(HostError::UnknownAgent(agent_id.to_string())),
        Some(r) if r.agent.adapter_id != adapter => Err(HostError::NotOwner(agent_id.to_string())),
        Some(_) => Ok(()),
    }
}

fn validate_command(command: &Command) -> Result<(), CommandRefused> {
    match command {
        Command::SendInstruction { text } => {
            if text.trim().is_empty() || text.len() > MAX_TEXT {
                return Err(CommandRefused::Invalid("instruction"));
            }
        }
        Command::Spawn { task, cwd } => {
            if task.trim().is_empty() || task.len() > MAX_TEXT {
                return Err(CommandRefused::Invalid("task"));
            }
            if let Some(c) = cwd {
                if c.len() > MAX_CWD || c.chars().any(char::is_control) {
                    return Err(CommandRefused::Invalid("working directory"));
                }
            }
        }
        _ => {}
    }
    Ok(())
}

fn flash_key(agent_id: &str, session_id: Option<&str>) -> String {
    match session_id {
        Some(s) => format!("s:{s}"),
        None => format!("a:{agent_id}"),
    }
}

/// Approve and reject exclude each other; anything else only conflicts with itself.
fn conflicts(p: &PendingCmd, agent_id: &str, session_id: Option<&str>, kind: CommandKind) -> bool {
    let decision = |k| matches!(k, CommandKind::Approve | CommandKind::Reject);
    p.agent_id == agent_id
        && p.session_id.as_deref() == session_id
        && (p.kind == kind || (decision(p.kind) && decision(kind)))
}

fn resolve_target(
    st: &State,
    target: &Target,
    kind: CommandKind,
) -> Option<(AgentId, Option<SessionId>)> {
    let agent_level = kind == CommandKind::Spawn;
    match target {
        Target::Session(sid) => {
            let r = st.sessions.get(sid)?;
            Some((
                r.session.agent_id.clone(),
                (!agent_level).then(|| sid.clone()),
            ))
        }
        Target::Agent(aid) => st.agents.contains_key(aid).then(|| (aid.clone(), None)),
        Target::Focus => match st.focus.and_then(|i| st.slots.get(i)).cloned().flatten() {
            Some(Track::Session {
                session_id,
                agent_id,
            }) => Some((agent_id, (!agent_level).then_some(session_id))),
            Some(Track::Agent { agent_id }) => Some((agent_id, None)),
            None if agent_level => st
                .agents
                .values()
                .filter(|a| {
                    a.agent.online
                        && a.agent.access == AccessLevel::Control
                        && a.agent.capabilities.contains(&Capability::Spawn)
                })
                .min_by_key(|a| a.ordinal)
                .map(|a| (a.agent.id.clone(), None)),
            None => None,
        },
    }
}

/// The one derivation of a command's state, used by pads and by the app.
fn command_state(
    st: &State,
    agent_id: &str,
    session_id: Option<&str>,
    kind: CommandKind,
) -> (CommandState, Option<Unavailable>) {
    let Some(arec) = st.agents.get(agent_id) else {
        return (CommandState::Unavailable, Some(Unavailable::NoSession));
    };
    let session_id = if kind == CommandKind::Spawn {
        None
    } else {
        session_id
    };
    if st
        .pending
        .values()
        .any(|p| conflicts(p, agent_id, session_id, kind))
    {
        return (CommandState::Pending, None);
    }
    if let Some(f) = st.flashes.get(&flash_key(agent_id, session_id)) {
        if f.kind == kind {
            let s = match f.outcome {
                OutcomeState::Succeeded => CommandState::Succeeded,
                OutcomeState::Failed => CommandState::Failed,
                OutcomeState::Pending => CommandState::Pending,
            };
            return (s, None);
        }
    }
    let session = session_id
        .and_then(|s| st.sessions.get(s))
        .map(|r| &r.session);
    match availability(&arec.agent, session, kind) {
        Err(u) => (CommandState::Unavailable, Some(u)),
        Ok(()) if kind == CommandKind::Approve => (CommandState::Attention, None),
        Ok(()) => (CommandState::Available, None),
    }
}

fn effective_status(st: &State, rec: &SessionRec) -> Status {
    match st.agents.get(&rec.session.agent_id) {
        Some(a) if a.agent.online => rec.session.status,
        _ => Status::Offline,
    }
}

fn track_status(st: &State, track: &Track) -> Status {
    match track {
        Track::Session { session_id, .. } => st
            .sessions
            .get(session_id)
            .map(|r| effective_status(st, r))
            .unwrap_or(Status::Idle),
        Track::Agent { agent_id } => match st.agents.get(agent_id) {
            Some(a) if a.agent.online => Status::Idle,
            _ => Status::Offline,
        },
    }
}

fn slot_of_session(st: &State, session_id: &str) -> Option<usize> {
    st.slots
        .iter()
        .position(|t| matches!(t, Some(Track::Session { session_id: s, .. }) if s == session_id))
}

fn sessions_of<'a>(st: &'a State, agent_id: &str) -> Vec<&'a SessionRec> {
    let mut v: Vec<&SessionRec> = st
        .sessions
        .values()
        .filter(|r| r.session.agent_id == agent_id)
        .collect();
    v.sort_by_key(|r| r.ordinal);
    v
}

// ---------------------------------------------------------------------------
// Slot assignment
// ---------------------------------------------------------------------------

/// Keep slots stable: a track keeps its pad until it disappears. New tracks
/// take a free slot, or replace a finished/idle one that is less urgent.
fn reconcile_slots(st: &mut State) {
    // 1. Drop tracks that no longer exist. A vanished session falls back to its agent.
    for i in 0..st.slots.len() {
        let replacement = match &st.slots[i] {
            Some(Track::Session {
                session_id,
                agent_id,
            }) if !st.sessions.contains_key(session_id) => {
                if st.agents.contains_key(agent_id) {
                    Some(Some(Track::Agent {
                        agent_id: agent_id.clone(),
                    }))
                } else {
                    Some(None)
                }
            }
            Some(Track::Agent { agent_id }) if !st.agents.contains_key(agent_id) => Some(None),
            _ => None,
        };
        if let Some(r) = replacement {
            st.slots[i] = r;
        }
    }

    // 2. An agent track gives way to the agent's first unslotted session, in place.
    for i in 0..st.slots.len() {
        let Some(Track::Agent { agent_id }) = st.slots[i].clone() else {
            continue;
        };
        let next = sessions_of(st, &agent_id)
            .into_iter()
            .find(|r| slot_of_session(st, &r.session.id).is_none())
            .map(|r| r.session.id.clone());
        if let Some(session_id) = next {
            st.slots[i] = Some(Track::Session {
                session_id,
                agent_id,
            });
        } else if !sessions_of(st, &agent_id).is_empty() {
            // The agent has sessions, all already on other slots.
            st.slots[i] = None;
        }
    }

    // 3. Place everything that is still missing, in agent registration order.
    let mut wanted: Vec<Track> = Vec::new();
    let mut agents: Vec<&AgentRec> = st.agents.values().collect();
    agents.sort_by_key(|a| a.ordinal);
    for a in agents {
        let sessions = sessions_of(st, &a.agent.id);
        if sessions.is_empty() {
            wanted.push(Track::Agent {
                agent_id: a.agent.id.clone(),
            });
        } else {
            for r in sessions {
                wanted.push(Track::Session {
                    session_id: r.session.id.clone(),
                    agent_id: a.agent.id.clone(),
                });
            }
        }
    }
    for track in wanted {
        if st.slots.iter().any(|t| t.as_ref() == Some(&track)) {
            continue;
        }
        if let Some(free) = st.slots.iter().position(Option::is_none) {
            st.slots[free] = Some(track);
            continue;
        }
        let incoming = track_status(st, &track).urgency();
        let victim = st
            .slots
            .iter()
            .enumerate()
            .filter_map(|(i, t)| t.as_ref().map(|t| (i, t)))
            .filter(|(_, t)| {
                matches!(
                    track_status(st, t),
                    Status::Completed | Status::Cancelled | Status::Idle | Status::Offline
                ) && track_status(st, t).urgency() < incoming
            })
            .min_by_key(|(_, t)| (track_status(st, t).urgency(), updated_at(st, t)))
            .map(|(i, _)| i);
        if let Some(i) = victim {
            st.slots[i] = Some(track);
        }
        // Otherwise the track has no pad (overflow); it remains visible in the app.
    }
}

fn updated_at(st: &State, track: &Track) -> u64 {
    match track {
        Track::Session { session_id, .. } => st
            .sessions
            .get(session_id)
            .map(|r| r.session.updated_at)
            .unwrap_or(0),
        Track::Agent { agent_id } => st
            .agents
            .get(agent_id)
            .map(|a| a.agent.last_seen)
            .unwrap_or(0),
    }
}

// ---------------------------------------------------------------------------
// Session transitions
// ---------------------------------------------------------------------------

fn resolve_approval(
    st: &mut State,
    out: &mut Out,
    session_id: &str,
    resolution: &str,
    actor: Actor,
) -> Option<Approval> {
    let rec = st.sessions.get_mut(session_id)?;
    let approval = rec.session.pending_approval.take()?;
    let agent_id = rec.session.agent_id.clone();
    out.event(
        "approval.resolved",
        json!({
            "approvalId": approval.id,
            "sessionId": session_id,
            "agentId": agent_id,
            "resolution": resolution,
        }),
    );
    out.audit(
        AuditEntry::new(&format!("approval.{resolution}"), actor)
            .agent(&agent_id)
            .session(session_id)
            .tool(approval.tool.as_deref()),
    );
    Some(approval)
}

/// Move a session to `new`, emitting events, audit entries and notifications.
fn apply_status(
    st: &mut State,
    out: &mut Out,
    timing: &Timing,
    session_id: &str,
    new: Status,
    created: bool,
) {
    let now = now_ms();
    let Some(rec) = st.sessions.get_mut(session_id) else {
        return;
    };
    let prev = rec.session.status;
    if prev == new && !created {
        return;
    }
    let agent_id = rec.session.agent_id.clone();
    let begins = new.is_active()
        && (rec.session.started_at.is_none() || prev.is_terminal() || prev == Status::Idle);
    let mut elapsed = Duration::ZERO;
    if begins {
        rec.session.started_at = Some(now);
        rec.run_started = Instant::now();
    } else if new.is_terminal() {
        elapsed = rec.run_started.elapsed();
    }
    rec.session.status = new;
    rec.session.updated_at = now;
    let error = rec.session.error.clone();

    let payload = json!({
        "agentId": agent_id,
        "sessionId": session_id,
        "status": new.as_str(),
        "previous": prev.as_str(),
    });
    if begins {
        out.event("session.started", payload.clone());
        out.audit(
            AuditEntry::new("session.started", Actor::Agent)
                .agent(&agent_id)
                .session(session_id),
        );
    }
    out.event(&format!("session.{}", new.as_str()), payload);

    let name = st
        .agents
        .get(&agent_id)
        .map(|a| a.agent.name.clone())
        .unwrap_or_default();
    match new {
        Status::Completed => {
            out.audit(
                AuditEntry::new("session.completed", Actor::Agent)
                    .agent(&agent_id)
                    .session(session_id),
            );
            if elapsed >= timing.long_running {
                out.notifications.push(notify::long_running_completed(
                    &agent_id,
                    session_id,
                    &name,
                    elapsed.as_secs() / 60,
                ));
            }
        }
        Status::Failed => {
            out.audit(
                AuditEntry::new("session.failed", Actor::Agent)
                    .agent(&agent_id)
                    .session(session_id),
            );
            out.notifications.push(notify::agent_failed(
                &agent_id,
                session_id,
                &name,
                error.as_deref(),
            ));
            if st.focus.is_none() {
                out.autofocus = Some(session_id.to_string());
            }
        }
        Status::Cancelled => {
            out.audit(
                AuditEntry::new("session.cancelled", Actor::Agent)
                    .agent(&agent_id)
                    .session(session_id),
            );
        }
        _ => {}
    }
    // The approval gate is gone once the session moves on without a decision.
    if new != Status::ApprovalRequired {
        resolve_approval(st, out, session_id, "withdrawn", Actor::Agent);
    }
}

fn remove_session(st: &mut State, out: &mut Out, session_id: &str, why: &str) {
    resolve_approval(st, out, session_id, "withdrawn", Actor::System);
    let Some(rec) = st.sessions.remove(session_id) else {
        return;
    };
    let ids: Vec<RequestId> = st
        .pending
        .iter()
        .filter(|(_, p)| p.session_id.as_deref() == Some(session_id))
        .map(|(id, _)| id.clone())
        .collect();
    for id in ids {
        st.pending.remove(&id);
    }
    st.flashes
        .remove(&flash_key(&rec.session.agent_id, Some(session_id)));
    out.event(
        "session.ended",
        json!({ "agentId": rec.session.agent_id, "sessionId": session_id, "reason": why }),
    );
}

/// Keep the session collections bounded by dropping the oldest finished ones.
fn make_room(st: &mut State, out: &mut Out, agent_id: &str) -> Result<(), HostError> {
    let evict = |st: &mut State, out: &mut Out, agent: Option<&str>| -> bool {
        let victim = st
            .sessions
            .values()
            .filter(|r| r.session.status.is_terminal())
            .filter(|r| agent.is_none_or_eq(&r.session.agent_id))
            .min_by_key(|r| r.ordinal)
            .map(|r| r.session.id.clone());
        match victim {
            Some(id) => {
                remove_session(st, out, &id, "evicted");
                true
            }
            None => false,
        }
    };
    while sessions_of(st, agent_id).len() >= MAX_SESSIONS_PER_AGENT {
        if !evict(st, out, Some(agent_id)) {
            return Err(HostError::LimitReached(
                "too many live sessions for this agent",
            ));
        }
    }
    while st.sessions.len() >= MAX_SESSIONS {
        if !evict(st, out, None) {
            return Err(HostError::LimitReached("too many live sessions"));
        }
    }
    Ok(())
}

trait OptionStrExt {
    fn is_none_or_eq(&self, other: &str) -> bool;
}
impl OptionStrExt for Option<&str> {
    fn is_none_or_eq(&self, other: &str) -> bool {
        match self {
            None => true,
            Some(s) => *s == other,
        }
    }
}

/// Record the outcome of a command. Returns whether the request was known.
fn complete_in(
    st: &mut State,
    out: &mut Out,
    timing: &Timing,
    request_id: &str,
    result: Result<(), String>,
) -> bool {
    let Some(p) = st.pending.remove(request_id) else {
        return false;
    };
    let (state, error) = match &result {
        Ok(()) => (OutcomeState::Succeeded, None),
        Err(e) => (OutcomeState::Failed, Some(summarize(e, MAX_ERROR_LEN))),
    };
    if let Some(sid) = &p.session_id {
        if let Some(rec) = st.sessions.get_mut(sid) {
            rec.session.last_command = Some(CommandOutcome {
                command: p.kind,
                state,
                error: error.clone(),
                at: now_ms(),
            });
        }
    }
    let key = flash_key(&p.agent_id, p.session_id.as_deref());
    let token = st.next();
    st.flashes.insert(
        key.clone(),
        Flash {
            kind: p.kind,
            outcome: state,
            token,
        },
    );
    out.schedule.push(Schedule::ClearFlash {
        key,
        token,
        after: if state == OutcomeState::Succeeded {
            timing.succeeded_feedback
        } else {
            timing.failed_feedback
        },
    });
    out.event(
        "command.completed",
        json!({
            "requestId": request_id,
            "command": p.kind.as_str(),
            "agentId": p.agent_id,
            "sessionId": p.session_id,
            "state": state,
            "error": error,
        }),
    );

    let entry = |kind: &str| {
        let mut e = AuditEntry::new(kind, Actor::User).agent(&p.agent_id);
        if let Some(s) = &p.session_id {
            e = e.session(s);
        }
        e
    };
    match (&result, p.kind) {
        (Err(_), _) => {
            let detail = error.clone().unwrap_or_default();
            out.audit(entry("command.failed").detail(&format!("{}: {detail}", p.kind.as_str())));
        }
        (Ok(()), CommandKind::Approve | CommandKind::Reject) => {
            if let Some(sid) = &p.session_id {
                let resolution = if p.kind == CommandKind::Approve {
                    "approved"
                } else {
                    "rejected"
                };
                resolve_approval(st, out, sid, resolution, Actor::User);
                // The gate is open; the adapter's next report refines the status.
                if st
                    .sessions
                    .get(sid)
                    .is_some_and(|r| r.session.status == Status::ApprovalRequired)
                {
                    apply_status(st, out, timing, sid, Status::Running, false);
                }
            }
        }
        (Ok(()), CommandKind::SendInstruction) => {}
        (Ok(()), kind) => out.audit(entry(&format!("intervention.{}", kind.as_str()))),
    }
    true
}

// ---------------------------------------------------------------------------
// Projection and read models
// ---------------------------------------------------------------------------

fn project(st: &State) -> Projection {
    let slots = st
        .slots
        .iter()
        .map(|t| match t {
            None => SlotProj {
                status: visual::EMPTY,
                agent_id: None,
                session_id: None,
                label: String::new(),
            },
            Some(track) => SlotProj {
                status: track_status(st, track).as_str(),
                agent_id: Some(track.agent_id().to_string()),
                session_id: match track {
                    Track::Session { session_id, .. } => Some(session_id.clone()),
                    Track::Agent { .. } => None,
                },
                label: st
                    .agents
                    .get(track.agent_id())
                    .map(|a| a.agent.name.clone())
                    .unwrap_or_default(),
            },
        })
        .collect();
    let commands = CommandKind::ALL
        .into_iter()
        .map(|kind| {
            let state = resolve_target(st, &Target::Focus, kind)
                .map(|(a, s)| command_state(st, &a, s.as_deref(), kind).0)
                .unwrap_or(CommandState::Unavailable);
            (kind, state)
        })
        .collect();
    Projection {
        slots,
        focus: st.focus,
        commands,
    }
}

fn command_views(
    st: &State,
    agent_id: &str,
    session_id: Option<&str>,
    kinds: &[CommandKind],
) -> Vec<CommandView> {
    kinds
        .iter()
        .map(|&kind| {
            let (state, why) = command_state(st, agent_id, session_id, kind);
            CommandView {
                kind,
                state,
                reason: why.map(|u| u.label().to_string()),
                reason_code: why,
            }
        })
        .collect()
}

const SESSION_COMMANDS: [CommandKind; 7] = [
    CommandKind::Approve,
    CommandKind::Reject,
    CommandKind::Pause,
    CommandKind::Resume,
    CommandKind::Cancel,
    CommandKind::Retry,
    CommandKind::SendInstruction,
];

fn build_snapshot(st: &State) -> AgentsSnapshot {
    let mut agents: Vec<&AgentRec> = st.agents.values().collect();
    agents.sort_by_key(|a| a.ordinal);
    let mut sessions: Vec<&SessionRec> = st.sessions.values().collect();
    // Most recent first: what happened last is what the human wants to see.
    sessions.sort_by_key(|r| std::cmp::Reverse(r.ordinal));

    let agent_views: Vec<AgentView> = agents
        .iter()
        .map(|a| {
            let own = sessions_of(st, &a.agent.id);
            AgentView {
                agent: a.agent.clone(),
                status: aggregate_status(&a.agent, own.iter().map(|r| &r.session)),
                session_ids: own.iter().map(|r| r.session.id.clone()).collect(),
                pending_approvals: own
                    .iter()
                    .filter(|r| r.session.pending_approval.is_some())
                    .count(),
                commands: command_views(st, &a.agent.id, None, &[CommandKind::Spawn]),
            }
        })
        .collect();

    let session_views: Vec<SessionView> = sessions
        .iter()
        .map(|r| {
            let mut session = r.session.clone();
            session.status = effective_status(st, r);
            SessionView {
                agent_name: st
                    .agents
                    .get(&session.agent_id)
                    .map(|a| a.agent.name.clone())
                    .unwrap_or_default(),
                slot: slot_of_session(st, &session.id),
                commands: command_views(
                    st,
                    &session.agent_id,
                    Some(&session.id),
                    &SESSION_COMMANDS,
                ),
                session,
            }
        })
        .collect();

    let slots: Vec<SlotView> = st
        .slots
        .iter()
        .enumerate()
        .map(|(index, t)| SlotView {
            index,
            status: t.as_ref().map(|t| track_status(st, t)),
            agent_id: t.as_ref().map(|t| t.agent_id().to_string()),
            session_id: match t {
                Some(Track::Session { session_id, .. }) => Some(session_id.clone()),
                _ => None,
            },
            label: t
                .as_ref()
                .and_then(|t| st.agents.get(t.agent_id()))
                .map(|a| a.agent.name.clone()),
            focused: st.focus == Some(index),
        })
        .collect();

    let summary = Summary {
        agents_online: agents.iter().filter(|a| a.agent.online).count(),
        agents_total: agents.len(),
        active_sessions: session_views
            .iter()
            .filter(|s| s.session.status.is_active())
            .count(),
        pending_approvals: session_views
            .iter()
            .filter(|s| s.session.pending_approval.is_some())
            .count(),
        failed_sessions: session_views
            .iter()
            .filter(|s| s.session.status == Status::Failed)
            .count(),
        recent_completions: session_views
            .iter()
            .filter(|s| s.session.status == Status::Completed)
            .count(),
    };

    let adapters = st
        .adapters
        .values()
        .map(|a| AdapterView {
            info: a.info.clone(),
            state: a.state,
            detail: a.detail.clone(),
            agent_ids: agents
                .iter()
                .filter(|g| g.agent.adapter_id == a.info.id)
                .map(|g| g.agent.id.clone())
                .collect(),
            diagnostics: a
                .diagnostics
                .iter()
                .map(|d| DiagnosticView {
                    ts: d.ts,
                    level: d.level,
                    message: d.message.clone(),
                })
                .collect(),
        })
        .collect();

    let hooks = &st.settings.hooks;
    AgentsSnapshot {
        rev: st.rev,
        agents: agent_views,
        sessions: session_views,
        slots,
        focus: st.focus,
        adapters,
        summary,
        palette: visual::palette(),
        audit: st.audit.recent(AUDIT_IN_SNAPSHOT),
        settings: SettingsView {
            notifications: st.settings.notifications,
            hooks_enabled: hooks.enabled,
            hooks_port: hooks.port,
            hooks_harnesses: hooks.harnesses.clone(),
            hooks_hold_secs: hooks.hold_secs,
        },
    }
}
