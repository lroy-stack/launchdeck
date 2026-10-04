//! What Agent Control lets a control observe.
//!
//! Three states, all of them *parameterised*: there is one track status per
//! slot, one selected marker per slot and one command state per command. They
//! are ordinary [`ObservableDefinition`]s; the state hub treats them like any
//! other provider's and nothing in the hub or the evaluator knows this module.
//!
//! The domain (`service.rs`) stays authoritative. Its projection is the one
//! derivation of these values; `publish_projection` writes what differs into
//! the hub, and a surface reads them from there.

use super::model::{AccessLevel, CommandKind, OutcomeState, Status};
use super::service::{SLOT_COUNT, SOURCE};
use super::visual::{CommandState, EMPTY};
use crate::actions::params::ParamDef;
use crate::state::definition::{ObservableDefinition, Role, ValueOption, ValueSpec, ValueType};
use crate::state::StateAddress;

pub const PROVIDER: &str = SOURCE;
/// The status of what a track holds: one of the eleven statuses, or `empty`.
pub const SLOT_STATUS: &str = "slot.status";
/// Whether a track is the selected one.
pub const SLOT_FOCUSED: &str = "slot.focused";
/// The state of one command for whatever is selected.
pub const COMMAND_STATE: &str = "command.state";

pub fn slot_status(slot: usize) -> StateAddress {
    StateAddress::new(PROVIDER, SLOT_STATUS).with("slot", slot as i64)
}

pub fn slot_focused(slot: usize) -> StateAddress {
    StateAddress::new(PROVIDER, SLOT_FOCUSED).with("slot", slot as i64)
}

pub fn command_state(kind: CommandKind) -> StateAddress {
    StateAddress::new(PROVIDER, COMMAND_STATE).with("command", kind.as_str())
}

fn status_label(status: &str) -> &'static str {
    match status {
        "idle" => "Idle",
        "starting" => "Starting",
        "running" => "Running",
        "tool_use" => "Using a tool",
        "waiting" => "Waiting",
        "approval_required" => "Needs approval",
        "paused" => "Paused",
        "completed" => "Completed",
        "failed" => "Failed",
        "cancelled" => "Cancelled",
        "offline" => "Offline",
        _ => "Empty",
    }
}

fn status_role(status: &str) -> Role {
    match status {
        "starting" | "running" | "tool_use" => Role::Active,
        "waiting" | "paused" => Role::Warning,
        "approval_required" => Role::Attention,
        "completed" => Role::Ok,
        "failed" => Role::Error,
        _ => Role::Inactive,
    }
}

fn command_label(kind: CommandKind) -> &'static str {
    match kind {
        CommandKind::Approve => "Approve",
        CommandKind::Reject => "Reject",
        CommandKind::Pause => "Pause",
        CommandKind::Resume => "Resume",
        CommandKind::Cancel => "Stop",
        CommandKind::Retry => "Retry",
        CommandKind::SendInstruction => "Continue",
        CommandKind::Spawn => "Start a task",
    }
}

fn command_state_role(state: CommandState) -> Role {
    match state {
        CommandState::Unavailable => Role::Inactive,
        CommandState::Available | CommandState::Succeeded => Role::Ok,
        CommandState::Attention => Role::Attention,
        CommandState::Pending => Role::Active,
        CommandState::Failed => Role::Error,
    }
}

fn command_state_label(state: CommandState) -> &'static str {
    match state {
        CommandState::Unavailable => "Not available",
        CommandState::Available => "Available",
        CommandState::Attention => "Needs you",
        CommandState::Pending => "Working",
        CommandState::Succeeded => "Done",
        CommandState::Failed => "Failed",
    }
}

/// The states Agent Control offers.
pub fn definitions() -> Vec<ObservableDefinition> {
    let last_slot = (SLOT_COUNT - 1) as f64;
    let track = || {
        ParamDef::number("slot", "Track")
            .range(0.0, last_slot)
            .integer()
            .required()
            .help("Which of the eight tracks, counting from 0.")
    };

    let mut status = ObservableDefinition::state(
        PROVIDER,
        SLOT_STATUS,
        "Agent track status",
        "What the agent on one track is doing.",
    )
    .category("Agent Control")
    .keywords(&["agent", "session", "track", "status"])
    .param(track());
    status.value = Some(ValueSpec {
        kind: ValueType::Enum,
        values: Status::ALL
            .iter()
            .map(|s| s.as_str())
            .chain([EMPTY])
            .map(|s| ValueOption::new(s, status_label(s), Some(status_role(s))))
            .collect(),
    });

    let focused = ObservableDefinition::state(
        PROVIDER,
        SLOT_FOCUSED,
        "Agent track selected",
        "Whether one track is the selected one.",
    )
    .category("Agent Control")
    .keywords(&["agent", "focus", "selected", "track"])
    .param(track())
    .on_off(
        ("Selected", Some(Role::Active)),
        ("Not selected", Some(Role::Inactive)),
    );

    let mut command = ObservableDefinition::state(
        PROVIDER,
        COMMAND_STATE,
        "Agent command state",
        "Whether a command can be used on the selected track, and how it went.",
    )
    .category("Agent Control")
    .keywords(&["agent", "approve", "reject", "command"])
    .param(
        ParamDef::choice(
            "command",
            "Command",
            &CommandKind::ALL
                .iter()
                .map(|k| (k.as_str(), command_label(*k)))
                .collect::<Vec<_>>(),
        )
        .required(),
    );
    command.value = Some(ValueSpec {
        kind: ValueType::Enum,
        values: CommandState::ALL
            .iter()
            .map(|s| {
                ValueOption::new(
                    s.as_str(),
                    command_state_label(*s),
                    Some(command_state_role(*s)),
                )
            })
            .collect(),
    });

    vec![status, focused, command]
}

/// What Agent Control announces on the bus, as an event a rule can name.
///
/// The three `*.changed` / `command.state` projection events are not listed:
/// they exist for older subscribers of the bus, and the states above say the
/// same thing as a current value, which is what a control should read.
pub fn events() -> Vec<ObservableDefinition> {
    let event = |kind: &str, name: &str, description: &str| {
        ObservableDefinition::event(PROVIDER, kind, name, description)
            .category("Agent Control")
            .keywords(&["agent"])
    };
    let status_values: Vec<(&str, &str)> = Status::ALL
        .iter()
        .map(|s| (s.as_str(), status_label(s.as_str())))
        .collect();
    let commands: Vec<(&str, &str)> = CommandKind::ALL
        .iter()
        .map(|k| (k.as_str(), command_label(*k)))
        .collect();

    let mut out = vec![
        event(
            "agent.online",
            "Agent came online",
            "An agent connected or was registered.",
        )
        .field("agentId", "Agent"),
        event(
            "agent.offline",
            "Agent went offline",
            "An agent disconnected.",
        )
        .field("agentId", "Agent"),
        event(
            "agent.access",
            "Agent access changed",
            "The access an agent has was granted or revoked.",
        )
        .field("agentId", "Agent")
        .field_one_of(
            "access",
            "Access",
            &[
                (AccessLevel::Monitor.as_str(), "Monitor only"),
                (AccessLevel::Control.as_str(), "Can be controlled"),
            ],
        ),
        event("session.created", "Session created", "A session appeared.")
            .field("agentId", "Agent")
            .field("sessionId", "Session"),
        event(
            "session.started",
            "Session started working",
            "A session began or resumed work.",
        )
        .field("agentId", "Agent")
        .field("sessionId", "Session")
        .field_one_of("status", "Status", &status_values)
        .field_one_of("previous", "Previous status", &status_values),
        event("session.ended", "Session ended", "A session was removed.")
            .field("agentId", "Agent")
            .field("sessionId", "Session")
            .field("reason", "Reason"),
        event(
            "approval.created",
            "Approval requested",
            "An agent asked for permission to use a tool.",
        )
        .field("approvalId", "Approval")
        .field("sessionId", "Session")
        .field("agentId", "Agent")
        .field("tool", "Tool"),
        event(
            "approval.resolved",
            "Approval resolved",
            "A request for permission was answered, withdrawn or replaced.",
        )
        .field("approvalId", "Approval")
        .field("sessionId", "Session")
        .field("agentId", "Agent")
        .field_one_of(
            "resolution",
            "Resolution",
            &[
                ("approved", "Approved"),
                ("rejected", "Rejected"),
                ("withdrawn", "Withdrawn"),
                ("superseded", "Superseded"),
            ],
        ),
        event(
            "command.sent",
            "Command sent",
            "A command was sent to an agent.",
        )
        .field("requestId", "Request")
        .field_one_of("command", "Command", &commands)
        .field("agentId", "Agent")
        .field("sessionId", "Session"),
        event(
            "command.completed",
            "Command finished",
            "An agent answered a command.",
        )
        .field("requestId", "Request")
        .field_one_of("command", "Command", &commands)
        .field("agentId", "Agent")
        .field("sessionId", "Session")
        .field_one_of(
            "state",
            "Outcome",
            &[
                (OutcomeState::Pending.as_str(), "Working"),
                (OutcomeState::Succeeded.as_str(), "Done"),
                (OutcomeState::Failed.as_str(), "Failed"),
            ],
        )
        .field("error", "Error"),
    ];
    // One event per status a session can enter: `session.running`, `session.failed`...
    for s in Status::ALL {
        out.push(
            event(
                &format!("session.{}", s.as_str()),
                &format!("Session became {}", status_label(s.as_str()).to_lowercase()),
                "A session entered this status.",
            )
            .field("agentId", "Agent")
            .field("sessionId", "Session")
            .field_one_of("status", "Status", &status_values)
            .field_one_of("previous", "Previous status", &status_values),
        );
    }
    out
}

/// Everything Agent Control offers to observe: its states and its events.
pub fn all() -> Vec<ObservableDefinition> {
    let mut all = definitions();
    all.extend(events());
    all
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_definitions_are_valid_and_complete() {
        let defs = definitions();
        assert_eq!(defs.len(), 3);
        for d in &defs {
            d.check().unwrap_or_else(|e| panic!("{e}"));
            assert_eq!(d.provider, PROVIDER);
            assert_eq!(d.params.len(), 1);
        }
        let status = defs.iter().find(|d| d.local == SLOT_STATUS).unwrap();
        let values = &status.value.as_ref().unwrap().values;
        assert_eq!(values.len(), Status::ALL.len() + 1);
        assert!(values
            .iter()
            .all(|o| o.role.is_some() && !o.label.is_empty()));
        let command = defs.iter().find(|d| d.local == COMMAND_STATE).unwrap();
        assert_eq!(
            command.value.as_ref().unwrap().values.len(),
            CommandState::ALL.len()
        );
        assert_eq!(command.params[0].options.len(), CommandKind::ALL.len());
    }

    #[test]
    fn every_status_and_command_state_the_domain_can_produce_is_declared() {
        let defs = definitions();
        let spec = |local: &str| {
            defs.iter()
                .find(|d| d.local == local)
                .unwrap()
                .value
                .clone()
                .unwrap()
        };
        for s in Status::ALL {
            assert!(spec(SLOT_STATUS)
                .values
                .iter()
                .any(|o| o.value.as_text() == Some(s.as_str())));
        }
        for s in CommandState::ALL {
            assert!(spec(COMMAND_STATE)
                .values
                .iter()
                .any(|o| o.value.as_text() == Some(s.as_str())));
        }
    }

    #[test]
    fn addresses_name_the_instance() {
        assert_eq!(
            slot_status(2).to_string(),
            "agent-control.slot.status(slot=2)"
        );
        assert_eq!(
            command_state(CommandKind::SendInstruction).to_string(),
            "agent-control.command.state(command=send_instruction)"
        );
        assert_ne!(slot_status(1).key(), slot_focused(1).key());
    }
}
