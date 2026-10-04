//! Provider-neutral agent domain model.
//!
//! Nothing here knows a vendor, a harness, MIDI, pads or LEDs. Adapters
//! translate provider-specific behaviour into these types.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

pub type AgentId = String;
pub type SessionId = String;
pub type ApprovalId = String;

pub const MAX_ID_LEN: usize = 64;
pub const MAX_NAME_LEN: usize = 60;
pub const MAX_TASK_LEN: usize = 160;
pub const MAX_MESSAGE_LEN: usize = 300;
pub const MAX_ERROR_LEN: usize = 200;
pub const MAX_DETAIL_LEN: usize = 120;

/// Identifiers are short and boring on purpose: they end up in events,
/// logs and file names.
pub fn valid_id(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= MAX_ID_LEN
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | ':' | '-'))
}

/// Truncate to `max` characters, collapsing control characters and
/// whitespace runs so that user-facing summaries stay one concise line.
pub fn summarize(text: &str, max: usize) -> String {
    let flat: String = text
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    let flat = flat.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() <= max {
        return flat;
    }
    let mut out: String = flat.chars().take(max.saturating_sub(1)).collect();
    out.push('…');
    out
}

// ---------------------------------------------------------------------------
// Status
// ---------------------------------------------------------------------------

/// Normalized status. Adapters translate provider states into these; the UI
/// and the surfaces only ever see these.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Idle,
    Starting,
    Running,
    ToolUse,
    Waiting,
    ApprovalRequired,
    Paused,
    Completed,
    Failed,
    Cancelled,
    Offline,
}

impl Status {
    pub const ALL: [Status; 11] = [
        Status::Idle,
        Status::Starting,
        Status::Running,
        Status::ToolUse,
        Status::Waiting,
        Status::ApprovalRequired,
        Status::Paused,
        Status::Completed,
        Status::Failed,
        Status::Cancelled,
        Status::Offline,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Status::Idle => "idle",
            Status::Starting => "starting",
            Status::Running => "running",
            Status::ToolUse => "tool_use",
            Status::Waiting => "waiting",
            Status::ApprovalRequired => "approval_required",
            Status::Paused => "paused",
            Status::Completed => "completed",
            Status::Failed => "failed",
            Status::Cancelled => "cancelled",
            Status::Offline => "offline",
        }
    }

    /// Accepts the normalized names plus a small, documented alias table so
    /// that simple clients can say `done` or `error`. Case, `-` and `_` are
    /// interchangeable. Anything else is rejected, never guessed.
    pub fn normalize(raw: &str) -> Option<Status> {
        let key: String = raw
            .trim()
            .chars()
            .map(|c| {
                if c == '-' || c == ' ' {
                    '_'
                } else {
                    c.to_ascii_lowercase()
                }
            })
            .collect();
        Some(match key.as_str() {
            "idle" | "ready" => Status::Idle,
            "starting" | "pending" | "queued" | "initializing" => Status::Starting,
            "running" | "working" | "thinking" | "in_progress" | "active" => Status::Running,
            "tool_use" | "tooluse" | "tool" | "using_tool" => Status::ToolUse,
            "waiting" | "waiting_for_input" | "input_required" | "blocked" => Status::Waiting,
            "approval_required" | "awaiting_approval" | "approval" | "needs_approval" => {
                Status::ApprovalRequired
            }
            "paused" | "suspended" => Status::Paused,
            "completed" | "complete" | "done" | "success" | "succeeded" | "finished" => {
                Status::Completed
            }
            "failed" | "failure" | "error" | "errored" => Status::Failed,
            "cancelled" | "canceled" | "stopped" | "aborted" => Status::Cancelled,
            "offline" | "disconnected" => Status::Offline,
            _ => return None,
        })
    }

    /// Work is in flight or the session is waiting on someone.
    pub const fn is_active(self) -> bool {
        matches!(
            self,
            Status::Starting
                | Status::Running
                | Status::ToolUse
                | Status::Waiting
                | Status::ApprovalRequired
                | Status::Paused
        )
    }

    pub const fn is_terminal(self) -> bool {
        matches!(self, Status::Completed | Status::Failed | Status::Cancelled)
    }

    /// Higher means the human should look sooner. Used to pick the status
    /// that represents an agent with several sessions.
    pub const fn urgency(self) -> u8 {
        match self {
            Status::ApprovalRequired => 100,
            Status::Failed => 90,
            Status::Waiting => 80,
            Status::Running => 70,
            Status::ToolUse => 65,
            Status::Starting => 60,
            Status::Paused => 50,
            Status::Completed => 40,
            Status::Cancelled => 30,
            Status::Idle => 20,
            Status::Offline => 0,
        }
    }
}

// ---------------------------------------------------------------------------
// Capabilities and access
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Capability {
    Spawn,
    SendInstruction,
    Pause,
    Resume,
    Cancel,
    Retry,
    Approve,
    Reject,
    GetStatus,
    GetLogs,
    GetDiff,
    OpenSession,
}

impl Capability {
    pub const ALL: [Capability; 12] = [
        Capability::Spawn,
        Capability::SendInstruction,
        Capability::Pause,
        Capability::Resume,
        Capability::Cancel,
        Capability::Retry,
        Capability::Approve,
        Capability::Reject,
        Capability::GetStatus,
        Capability::GetLogs,
        Capability::GetDiff,
        Capability::OpenSession,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Capability::Spawn => "spawn",
            Capability::SendInstruction => "send_instruction",
            Capability::Pause => "pause",
            Capability::Resume => "resume",
            Capability::Cancel => "cancel",
            Capability::Retry => "retry",
            Capability::Approve => "approve",
            Capability::Reject => "reject",
            Capability::GetStatus => "get_status",
            Capability::GetLogs => "get_logs",
            Capability::GetDiff => "get_diff",
            Capability::OpenSession => "open_session",
        }
    }

    pub fn parse(raw: &str) -> Option<Capability> {
        let key = raw.trim().to_ascii_lowercase().replace('-', "_");
        Capability::ALL.into_iter().find(|c| c.as_str() == key)
    }
}

pub type Capabilities = BTreeSet<Capability>;

/// Read-only monitoring versus permission to send commands. Agents that
/// register themselves start as `Monitor`; the user grants `Control`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AccessLevel {
    #[default]
    Monitor,
    Control,
}

impl AccessLevel {
    pub const fn as_str(self) -> &'static str {
        match self {
            AccessLevel::Monitor => "monitor",
            AccessLevel::Control => "control",
        }
    }
}

// ---------------------------------------------------------------------------
// Entities
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Agent {
    pub id: AgentId,
    pub name: String,
    pub provider: Option<String>,
    pub harness: Option<String>,
    pub adapter_id: String,
    pub capabilities: Capabilities,
    pub access: AccessLevel,
    pub online: bool,
    pub registered_at: u64,
    pub last_seen: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Approval {
    pub id: ApprovalId,
    pub session_id: SessionId,
    /// What the agent asks permission for, one concise line.
    pub message: String,
    /// Tool or operation name, when known.
    pub tool: Option<String>,
    pub created_at: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OutcomeState {
    Pending,
    Succeeded,
    Failed,
}

impl OutcomeState {
    pub const fn as_str(self) -> &'static str {
        match self {
            OutcomeState::Pending => "pending",
            OutcomeState::Succeeded => "succeeded",
            OutcomeState::Failed => "failed",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CommandOutcome {
    pub command: CommandKind,
    pub state: OutcomeState,
    pub error: Option<String>,
    pub at: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Session {
    pub id: SessionId,
    pub agent_id: AgentId,
    pub task: Option<String>,
    pub status: Status,
    pub started_at: Option<u64>,
    pub updated_at: u64,
    pub pending_approval: Option<Approval>,
    pub progress: Option<f32>,
    /// Current tool or step, for example `Bash`.
    pub detail: Option<String>,
    /// Concise failure summary. Raw diagnostics go to the log, not here.
    pub error: Option<String>,
    pub last_command: Option<CommandOutcome>,
}

// ---------------------------------------------------------------------------
// Commands
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CommandKind {
    Approve,
    Reject,
    Pause,
    Resume,
    Cancel,
    Retry,
    SendInstruction,
    Spawn,
}

impl CommandKind {
    pub const ALL: [CommandKind; 8] = [
        CommandKind::Approve,
        CommandKind::Reject,
        CommandKind::Pause,
        CommandKind::Resume,
        CommandKind::Cancel,
        CommandKind::Retry,
        CommandKind::SendInstruction,
        CommandKind::Spawn,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            CommandKind::Approve => "approve",
            CommandKind::Reject => "reject",
            CommandKind::Pause => "pause",
            CommandKind::Resume => "resume",
            CommandKind::Cancel => "cancel",
            CommandKind::Retry => "retry",
            CommandKind::SendInstruction => "send_instruction",
            CommandKind::Spawn => "spawn",
        }
    }

    pub fn parse(raw: &str) -> Option<CommandKind> {
        let key = raw.trim().to_ascii_lowercase().replace('-', "_");
        CommandKind::ALL.into_iter().find(|c| c.as_str() == key)
    }

    pub const fn capability(self) -> Capability {
        match self {
            CommandKind::Approve => Capability::Approve,
            CommandKind::Reject => Capability::Reject,
            CommandKind::Pause => Capability::Pause,
            CommandKind::Resume => Capability::Resume,
            CommandKind::Cancel => Capability::Cancel,
            CommandKind::Retry => Capability::Retry,
            CommandKind::SendInstruction => Capability::SendInstruction,
            CommandKind::Spawn => Capability::Spawn,
        }
    }

    /// Interventions worth a line in the audit log.
    pub const fn is_critical(self) -> bool {
        !matches!(self, CommandKind::SendInstruction)
    }
}

/// A command with its payload. Free text (instructions, tasks) is delivered
/// to the adapter but never written to the audit log.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Command {
    Approve,
    Reject,
    Pause,
    Resume,
    Cancel,
    Retry,
    SendInstruction { text: String },
    Spawn { task: String, cwd: Option<String> },
}

impl Command {
    pub fn kind(&self) -> CommandKind {
        match self {
            Command::Approve => CommandKind::Approve,
            Command::Reject => CommandKind::Reject,
            Command::Pause => CommandKind::Pause,
            Command::Resume => CommandKind::Resume,
            Command::Cancel => CommandKind::Cancel,
            Command::Retry => CommandKind::Retry,
            Command::SendInstruction { .. } => CommandKind::SendInstruction,
            Command::Spawn { .. } => CommandKind::Spawn,
        }
    }
}

// ---------------------------------------------------------------------------
// Availability: one function decides what may be done, for pads, UI and dispatch
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Unavailable {
    /// The agent does not declare this capability.
    Unsupported,
    Offline,
    /// The agent is read-only until the user grants control.
    MonitorOnly,
    NoSession,
    /// Supported, but not meaningful in the current status.
    WrongState,
    NothingPending,
}

impl Unavailable {
    pub const fn label(self) -> &'static str {
        match self {
            Unavailable::Unsupported => "not supported by this agent",
            Unavailable::Offline => "agent is offline",
            Unavailable::MonitorOnly => "monitor only (control not granted)",
            Unavailable::NoSession => "no session selected",
            Unavailable::WrongState => "not possible in the current state",
            Unavailable::NothingPending => "nothing is waiting for approval",
        }
    }
}

pub type Availability = Result<(), Unavailable>;

/// Whether `kind` may be issued now. `session` is `None` for agent-level
/// commands (spawn) or when nothing is selected.
pub fn availability(agent: &Agent, session: Option<&Session>, kind: CommandKind) -> Availability {
    if !agent.capabilities.contains(&kind.capability()) {
        return Err(Unavailable::Unsupported);
    }
    if !agent.online {
        return Err(Unavailable::Offline);
    }
    if agent.access != AccessLevel::Control {
        return Err(Unavailable::MonitorOnly);
    }
    if kind == CommandKind::Spawn {
        return Ok(());
    }
    let session = session.ok_or(Unavailable::NoSession)?;
    let s = session.status;
    let ok = match kind {
        CommandKind::Approve | CommandKind::Reject => {
            return if session.pending_approval.is_some() {
                Ok(())
            } else {
                Err(Unavailable::NothingPending)
            };
        }
        CommandKind::Pause => matches!(s, Status::Running | Status::ToolUse),
        CommandKind::Resume => s == Status::Paused,
        CommandKind::Cancel => s.is_active(),
        CommandKind::Retry => matches!(s, Status::Failed | Status::Cancelled),
        CommandKind::SendInstruction => {
            matches!(
                s,
                Status::Idle
                    | Status::Running
                    | Status::ToolUse
                    | Status::Waiting
                    | Status::Paused
                    | Status::Completed
            )
        }
        CommandKind::Spawn => true,
    };
    if ok {
        Ok(())
    } else {
        Err(Unavailable::WrongState)
    }
}

/// The status that stands for an agent as a whole.
pub fn aggregate_status<'a>(
    agent: &Agent,
    sessions: impl IntoIterator<Item = &'a Session>,
) -> Status {
    if !agent.online {
        return Status::Offline;
    }
    sessions
        .into_iter()
        .map(|s| s.status)
        .max_by_key(|s| s.urgency())
        .unwrap_or(Status::Idle)
}

// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn agent(caps: &[Capability], access: AccessLevel) -> Agent {
        Agent {
            id: "a".into(),
            name: "A".into(),
            provider: None,
            harness: None,
            adapter_id: "x".into(),
            capabilities: caps.iter().copied().collect(),
            access,
            online: true,
            registered_at: 0,
            last_seen: 0,
        }
    }

    fn session(status: Status, approval: bool) -> Session {
        Session {
            id: "s".into(),
            agent_id: "a".into(),
            task: None,
            status,
            started_at: None,
            updated_at: 0,
            pending_approval: approval.then(|| Approval {
                id: "ap".into(),
                session_id: "s".into(),
                message: "m".into(),
                tool: None,
                created_at: 0,
            }),
            progress: None,
            detail: None,
            error: None,
            last_command: None,
        }
    }

    #[test]
    fn normalizes_documented_names_and_aliases() {
        for s in Status::ALL {
            assert_eq!(Status::normalize(s.as_str()), Some(s), "{s:?}");
        }
        assert_eq!(Status::normalize("Tool-Use"), Some(Status::ToolUse));
        assert_eq!(Status::normalize("error"), Some(Status::Failed));
        assert_eq!(Status::normalize("done"), Some(Status::Completed));
        assert_eq!(Status::normalize("canceled"), Some(Status::Cancelled));
        assert_eq!(
            Status::normalize("awaiting-approval"),
            Some(Status::ApprovalRequired)
        );
        // Never guessed.
        assert_eq!(Status::normalize("vibing"), None);
        assert_eq!(Status::normalize(""), None);
    }

    #[test]
    fn statuses_serialize_as_snake_case() {
        assert_eq!(
            serde_json::to_string(&Status::ApprovalRequired).unwrap(),
            "\"approval_required\""
        );
        assert_eq!(
            serde_json::to_string(&Capability::SendInstruction).unwrap(),
            "\"send_instruction\""
        );
        let s: Status = serde_json::from_str("\"tool_use\"").unwrap();
        assert_eq!(s, Status::ToolUse);
    }

    #[test]
    fn urgency_orders_what_a_human_should_see_first() {
        let mut by_urgency = Status::ALL.to_vec();
        by_urgency.sort_by_key(|s| std::cmp::Reverse(s.urgency()));
        assert_eq!(by_urgency[0], Status::ApprovalRequired);
        assert_eq!(by_urgency[1], Status::Failed);
        assert_eq!(*by_urgency.last().unwrap(), Status::Offline);
    }

    #[test]
    fn capabilities_parse_loosely_but_not_wildly() {
        assert_eq!(Capability::parse("approve"), Some(Capability::Approve));
        assert_eq!(
            Capability::parse("send-instruction"),
            Some(Capability::SendInstruction)
        );
        assert_eq!(Capability::parse("fly"), None);
    }

    #[test]
    fn availability_follows_capability_access_and_state() {
        let full = [
            Capability::Pause,
            Capability::Resume,
            Capability::Cancel,
            Capability::Retry,
            Capability::Approve,
            Capability::Reject,
            Capability::SendInstruction,
            Capability::Spawn,
        ];
        let a = agent(&full, AccessLevel::Control);
        let running = session(Status::Running, false);
        assert_eq!(availability(&a, Some(&running), CommandKind::Pause), Ok(()));
        assert_eq!(
            availability(&a, Some(&running), CommandKind::Resume),
            Err(Unavailable::WrongState)
        );
        assert_eq!(
            availability(&a, Some(&running), CommandKind::Approve),
            Err(Unavailable::NothingPending)
        );
        assert_eq!(
            availability(
                &a,
                Some(&session(Status::Paused, false)),
                CommandKind::Resume
            ),
            Ok(())
        );
        assert_eq!(
            availability(
                &a,
                Some(&session(Status::Failed, false)),
                CommandKind::Retry
            ),
            Ok(())
        );
        assert_eq!(
            availability(
                &a,
                Some(&session(Status::Failed, false)),
                CommandKind::Cancel
            ),
            Err(Unavailable::WrongState)
        );
        let waiting = session(Status::ApprovalRequired, true);
        assert_eq!(
            availability(&a, Some(&waiting), CommandKind::Approve),
            Ok(())
        );
        assert_eq!(
            availability(&a, Some(&waiting), CommandKind::Reject),
            Ok(())
        );
        assert_eq!(
            availability(&a, None, CommandKind::Pause),
            Err(Unavailable::NoSession)
        );
        assert_eq!(availability(&a, None, CommandKind::Spawn), Ok(()));
    }

    #[test]
    fn unsupported_beats_everything_and_monitor_only_blocks_control() {
        let limited = agent(&[Capability::Approve], AccessLevel::Control);
        let s = session(Status::Running, false);
        assert_eq!(
            availability(&limited, Some(&s), CommandKind::Pause),
            Err(Unavailable::Unsupported)
        );
        let monitor = agent(&[Capability::Pause], AccessLevel::Monitor);
        assert_eq!(
            availability(&monitor, Some(&s), CommandKind::Pause),
            Err(Unavailable::MonitorOnly)
        );
        let mut offline = agent(&[Capability::Pause], AccessLevel::Control);
        offline.online = false;
        assert_eq!(
            availability(&offline, Some(&s), CommandKind::Pause),
            Err(Unavailable::Offline)
        );
    }

    #[test]
    fn agent_status_is_the_most_urgent_session_status() {
        let a = agent(&[], AccessLevel::Control);
        let sessions = [
            session(Status::Running, false),
            session(Status::ApprovalRequired, true),
            session(Status::Completed, false),
        ];
        assert_eq!(
            aggregate_status(&a, sessions.iter()),
            Status::ApprovalRequired
        );
        assert_eq!(aggregate_status(&a, std::iter::empty()), Status::Idle);
        let mut off = a.clone();
        off.online = false;
        assert_eq!(aggregate_status(&off, sessions.iter()), Status::Offline);
    }

    #[test]
    fn summaries_are_single_line_and_bounded() {
        assert_eq!(summarize("a\n  b\tc", 20), "a b c");
        let long = "x".repeat(500);
        let s = summarize(&long, 40);
        assert_eq!(s.chars().count(), 40);
        assert!(s.ends_with('…'));
    }

    #[test]
    fn ids_are_validated() {
        assert!(valid_id("claude-code:main_1.x"));
        assert!(!valid_id(""));
        assert!(!valid_id("a b"));
        assert!(!valid_id(&"a".repeat(65)));
        assert!(!valid_id("../etc"));
    }
}
