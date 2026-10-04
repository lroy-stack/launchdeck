//! Notification policy.
//!
//! Exactly four things may interrupt the user: an approval is required, an
//! agent failed, a long-running session completed, an adapter disconnected.
//! Everything else stays ambient (pads and the Agents view).
//!
//! The policy lives here and produces plain [`Notification`] values. Delivery
//! is a separate concern (`AgentsSink::notify`): the desktop shell shows them
//! in the app today, and an OS-level delivery can be plugged in without
//! touching this file. Notification text never contains prompts, tool inputs
//! or command lines; it names the agent and, for approvals, the tool.

use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum NotificationKind {
    ApprovalRequired,
    AgentFailed,
    LongRunningCompleted,
    AdapterDisconnected,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Notification {
    pub kind: NotificationKind,
    pub title: String,
    pub body: String,
    pub agent_id: Option<String>,
    pub session_id: Option<String>,
}

pub fn approval_required(
    agent_id: &str,
    session_id: &str,
    agent: &str,
    tool: Option<&str>,
) -> Notification {
    Notification {
        kind: NotificationKind::ApprovalRequired,
        title: format!("{agent} needs your approval"),
        body: match tool {
            Some(t) => format!("Permission requested for {t}."),
            None => "Permission requested.".to_string(),
        },
        agent_id: Some(agent_id.into()),
        session_id: Some(session_id.into()),
    }
}

pub fn agent_failed(
    agent_id: &str,
    session_id: &str,
    agent: &str,
    error: Option<&str>,
) -> Notification {
    Notification {
        kind: NotificationKind::AgentFailed,
        title: format!("{agent} failed"),
        body: error
            .unwrap_or("The session ended with an error.")
            .to_string(),
        agent_id: Some(agent_id.into()),
        session_id: Some(session_id.into()),
    }
}

pub fn long_running_completed(
    agent_id: &str,
    session_id: &str,
    agent: &str,
    minutes: u64,
) -> Notification {
    Notification {
        kind: NotificationKind::LongRunningCompleted,
        title: format!("{agent} finished"),
        body: if minutes == 0 {
            "The long-running session completed.".to_string()
        } else {
            format!("Completed after {minutes} min.")
        },
        agent_id: Some(agent_id.into()),
        session_id: Some(session_id.into()),
    }
}

pub fn adapter_disconnected(agent_id: &str, agent: &str) -> Notification {
    Notification {
        kind: NotificationKind::AdapterDisconnected,
        title: format!("{agent} disconnected"),
        body: "The agent went offline. Pending commands were cancelled.".to_string(),
        agent_id: Some(agent_id.into()),
        session_id: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn approval_notification_names_the_tool_but_nothing_else() {
        let n = approval_required("a", "s", "Atlas", Some("Bash"));
        assert_eq!(n.kind, NotificationKind::ApprovalRequired);
        assert!(n.title.contains("Atlas"));
        assert!(n.body.contains("Bash"));
    }

    #[test]
    fn long_running_text_handles_short_and_long_runs() {
        assert!(long_running_completed("a", "s", "A", 0)
            .body
            .contains("long-running"));
        assert!(long_running_completed("a", "s", "A", 7)
            .body
            .contains("7 min"));
    }
}
