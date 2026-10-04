//! The Agent Adapter SDK, Rust form.
//!
//! One model, two faces:
//!
//! * In process, an adapter implements [`AgentAdapter`] and talks to
//!   LaunchDeck through an [`AdapterHost`].
//! * Out of process, the Agent Bridge (a localhost WebSocket protocol) exposes
//!   exactly the same operations, one message per method. The bridge adapter
//!   is simply an `AgentAdapter` that forwards.
//!
//! The contract between the two directions:
//!
//! ```text
//! adapter  --- AdapterHost ----->  AgentControl   register_agent, set_online,
//!                                                 update_session, request_approval,
//!                                                 withdraw_approval, end_session,
//!                                                 command_result, diagnostic
//! adapter  <--- AgentAdapter ----  AgentControl   execute(AdapterCommand)
//! ```
//!
//! An adapter never sees the event bus, a device, a pad, a colour or a MIDI
//! message. It reports what its agent is doing in normalized terms and carries
//! out commands the human issued. It can only touch agents it registered.

use std::sync::Arc;

use serde::Serialize;

use super::model::{AccessLevel, ApprovalId, Capabilities, Command, SessionId, Status};
use super::service::AgentControl;

pub type RequestId = String;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AdapterKind {
    /// In-process and harmless. Its agents are trusted with `control` by default.
    Simulated,
    /// A local HTTP endpoint that a harness calls from its hooks.
    Hooks,
    /// Any local process speaking the Agent Bridge protocol.
    Bridge,
}

/// Static description of an adapter, shown in the Adapters view. `limits`
/// states honestly what the integration cannot observe or control.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AdapterInfo {
    pub id: String,
    pub name: String,
    pub kind: AdapterKind,
    pub description: String,
    pub limits: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AdapterState {
    Active,
    /// Present but switched off by the user (for example the hook endpoint).
    Disabled,
    Error,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AgentSpec {
    pub id: String,
    pub name: String,
    pub provider: Option<String>,
    pub harness: Option<String>,
    pub capabilities: Capabilities,
}

/// A state report for one session. The first report for an unknown session id
/// creates it.
#[derive(Debug, Clone, PartialEq)]
pub struct SessionUpdate {
    pub session_id: SessionId,
    pub status: Status,
    /// One-line task description. Held in memory for display, never logged.
    pub task: Option<String>,
    /// `0.0..=1.0`.
    pub progress: Option<f32>,
    /// Current step or tool name.
    pub detail: Option<String>,
    /// Concise failure summary for `failed`. Keep raw output in `diagnostic`.
    pub error: Option<String>,
}

impl SessionUpdate {
    pub fn new(session_id: &str, status: Status) -> Self {
        Self {
            session_id: session_id.to_string(),
            status,
            task: None,
            progress: None,
            detail: None,
            error: None,
        }
    }
    pub fn task(mut self, task: &str) -> Self {
        self.task = Some(task.to_string());
        self
    }
    pub fn progress(mut self, p: f32) -> Self {
        self.progress = Some(p);
        self
    }
    pub fn detail(mut self, d: &str) -> Self {
        self.detail = Some(d.to_string());
        self
    }
    pub fn error(mut self, e: &str) -> Self {
        self.error = Some(e.to_string());
        self
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ApprovalSpec {
    /// Adapter-chosen id. Generated when absent.
    pub id: Option<ApprovalId>,
    /// What is being asked, one concise line the human can decide on.
    pub message: String,
    pub tool: Option<String>,
}

/// A command the human issued, already checked by `availability`.
#[derive(Debug, Clone, PartialEq)]
pub struct AdapterCommand {
    pub request_id: RequestId,
    pub agent_id: String,
    pub session_id: Option<SessionId>,
    /// For approve and reject: the approval being decided.
    pub approval_id: Option<ApprovalId>,
    pub command: Command,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{0}")]
pub struct CommandError(pub String);

impl CommandError {
    pub fn new(msg: impl Into<String>) -> Self {
        Self(msg.into())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum HostError {
    #[error("invalid {0}")]
    Invalid(&'static str),
    #[error("agent `{0}` is registered by another adapter")]
    IdTaken(String),
    #[error("agent `{0}` is not registered by this adapter")]
    NotOwner(String),
    #[error("unknown agent `{0}`")]
    UnknownAgent(String),
    #[error("unknown session `{0}`")]
    UnknownSession(String),
    #[error("session `{0}` belongs to another agent")]
    SessionOwnedByOther(String),
    #[error("unknown command request `{0}`")]
    UnknownRequest(String),
    #[error("limit reached: {0}")]
    LimitReached(&'static str),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DiagnosticLevel {
    Info,
    Warn,
    Error,
}

/// The adapter's way into LaunchDeck. Cheap to clone.
#[derive(Clone)]
pub struct AdapterHost {
    adapter_id: String,
    core: Arc<AgentControl>,
}

impl AdapterHost {
    pub(crate) fn new(adapter_id: &str, core: Arc<AgentControl>) -> Self {
        Self {
            adapter_id: adapter_id.to_string(),
            core,
        }
    }

    pub fn adapter_id(&self) -> &str {
        &self.adapter_id
    }

    /// Register (or re-register) an agent this adapter owns. Marks it online.
    pub fn register_agent(&self, spec: AgentSpec) -> Result<(), HostError> {
        self.core.host_register_agent(&self.adapter_id, spec)
    }

    /// Whether the human currently lets LaunchDeck control this agent. Agents
    /// from non-simulated adapters start as `Monitor`; adapters should not
    /// advertise or attempt control the user has not granted (the service
    /// refuses commands anyway, this is for honest UX on the agent's side).
    pub fn agent_access(&self, agent_id: &str) -> Result<AccessLevel, HostError> {
        self.core.host_agent_access(&self.adapter_id, agent_id)
    }

    pub fn set_online(&self, agent_id: &str, online: bool) -> Result<(), HostError> {
        self.core
            .host_set_online(&self.adapter_id, agent_id, online)
    }

    /// Report a session's state. Creates the session on first use.
    pub fn update_session(&self, agent_id: &str, update: SessionUpdate) -> Result<(), HostError> {
        self.core
            .host_update_session(&self.adapter_id, agent_id, update)
    }

    /// Ask the human to decide. The session moves to `approval_required`.
    pub fn request_approval(
        &self,
        agent_id: &str,
        session_id: &str,
        spec: ApprovalSpec,
    ) -> Result<ApprovalId, HostError> {
        self.core
            .host_request_approval(&self.adapter_id, agent_id, session_id, spec)
    }

    /// The approval no longer applies (answered elsewhere, timed out).
    pub fn withdraw_approval(&self, session_id: &str, approval_id: &str) -> Result<(), HostError> {
        self.core
            .host_withdraw_approval(&self.adapter_id, session_id, approval_id)
    }

    /// The harness closed the session; it leaves the live set.
    pub fn end_session(&self, session_id: &str) -> Result<(), HostError> {
        self.core.host_end_session(&self.adapter_id, session_id)
    }

    /// Outcome of a command previously handed to `execute`.
    pub fn command_result(
        &self,
        request_id: &str,
        result: Result<(), String>,
    ) -> Result<(), HostError> {
        self.core
            .host_command_result(&self.adapter_id, request_id, result)
    }

    /// Raw detail for logs and the Adapters view. Not shown on cards.
    pub fn diagnostic(&self, level: DiagnosticLevel, message: &str) {
        self.core.host_diagnostic(&self.adapter_id, level, message);
    }

    pub fn set_state(&self, state: AdapterState, detail: Option<&str>) {
        self.core
            .host_set_adapter_state(&self.adapter_id, state, detail);
    }
}

pub trait AgentAdapter: Send + Sync {
    fn info(&self) -> AdapterInfo;

    /// Called once, when the adapter is registered with Agent Control. Keep
    /// the host; register agents when they exist.
    fn attach(&self, host: AdapterHost);

    /// Carry out a command. Return `Ok` once the command is accepted; report
    /// the real outcome with `AdapterHost::command_result` (immediately or
    /// later). Return `Err` to fail right away. Must not block.
    fn execute(&self, command: AdapterCommand) -> Result<(), CommandError>;

    /// The human granted or revoked control of one of this adapter's agents.
    /// Called with no locks held. Optional.
    fn access_changed(&self, _agent_id: &str, _access: AccessLevel) {}
}
