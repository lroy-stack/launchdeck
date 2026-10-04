//! Read models sent to the desktop UI.
//!
//! The UI never recomputes anything: statuses, availability and slot
//! assignment all arrive here already derived from the single runtime state in
//! `AgentControl`. The same derivation drives the pads, so the app, the
//! virtual Launchpad and a physical one cannot disagree.

use serde::Serialize;

use super::adapter::{AdapterInfo, AdapterState, DiagnosticLevel};
use super::audit::AuditEntry;
use super::model::{Agent, CommandKind, Session, Status};
use super::visual::{CommandState, PaletteEntry};

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CommandView {
    pub kind: CommandKind,
    pub state: CommandState,
    /// Why the command is unavailable, in words. Absent otherwise.
    pub reason: Option<String>,
    /// Machine-readable form of `reason` (`unsupported`, `monitor_only`, ...).
    pub reason_code: Option<super::model::Unavailable>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentView {
    #[serde(flatten)]
    pub agent: Agent,
    /// Aggregate status derived from the agent's sessions.
    pub status: Status,
    pub session_ids: Vec<String>,
    pub pending_approvals: usize,
    /// Agent-level commands (`spawn`).
    pub commands: Vec<CommandView>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionView {
    /// `status` is the effective status: `offline` when the agent is.
    #[serde(flatten)]
    pub session: Session,
    pub agent_name: String,
    pub slot: Option<usize>,
    pub commands: Vec<CommandView>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SlotView {
    pub index: usize,
    pub status: Option<Status>,
    pub agent_id: Option<String>,
    pub session_id: Option<String>,
    pub label: Option<String>,
    pub focused: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Summary {
    pub agents_online: usize,
    pub agents_total: usize,
    pub active_sessions: usize,
    pub pending_approvals: usize,
    pub failed_sessions: usize,
    pub recent_completions: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticView {
    pub ts: u64,
    pub level: DiagnosticLevel,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AdapterView {
    #[serde(flatten)]
    pub info: AdapterInfo,
    pub state: AdapterState,
    pub detail: Option<String>,
    pub agent_ids: Vec<String>,
    pub diagnostics: Vec<DiagnosticView>,
}

/// Settings the UI may see. The hook token is deliberately absent; it is only
/// returned by the dedicated setup command.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsView {
    pub notifications: bool,
    pub hooks_enabled: bool,
    pub hooks_port: u16,
    /// Hook adapter id to whether its route is served.
    pub hooks_harnesses: std::collections::BTreeMap<String, bool>,
    pub hooks_hold_secs: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentsSnapshot {
    /// Increases with every change; the UI ignores anything older than what
    /// it already shows.
    pub rev: u64,
    pub agents: Vec<AgentView>,
    pub sessions: Vec<SessionView>,
    pub slots: Vec<SlotView>,
    pub focus: Option<usize>,
    pub adapters: Vec<AdapterView>,
    pub summary: Summary,
    pub palette: Vec<PaletteEntry>,
    pub audit: Vec<AuditEntry>,
    pub settings: SettingsView,
}
