//! The `agent-control` plugin: how pads reach Agent Control.
//!
//! Surfaces use ordinary `Action::Plugin { plugin: "agent-control", ... }`
//! actions. The plugin knows no pad, colour or MIDI message; it translates an
//! action into a command on the focused track (or a selection change).

use std::sync::Arc;

use serde_json::{json, Value};

use super::demo::DemoAgentAdapter;
use super::model::{Command, CommandKind, Status};
use super::service::{AgentControl, Target, MAX_CWD, MAX_TEXT, SLOT_COUNT};
use super::visual::{self, CommandState};
use crate::actions::definition::{Availability, Cause, ProviderKind};
use crate::actions::params::ParamDef;
use crate::plugins::{
    str_arg, ActionDef, ActionError, Plugin, PluginContext, PluginManifest, StateDef,
};

pub const PLUGIN_ID: &str = "agent-control";
/// The action that sends the focused session an instruction.
pub const CONTINUE: &str = "continue";
/// The action that starts a new agent session on a task.
pub const NEW_TASK: &str = "newTask";
/// The actions that answer the focused session's pending request, and
/// that retry a failed or cancelled one.
pub const APPROVE: &str = "approve";
pub const REJECT: &str = "reject";
pub const RETRY: &str = "retry";

pub struct AgentControlPlugin {
    ctl: Arc<AgentControl>,
    demo: Arc<DemoAgentAdapter>,
}

impl AgentControlPlugin {
    pub fn new(ctl: Arc<AgentControl>, demo: Arc<DemoAgentAdapter>) -> Self {
        Self { ctl, demo }
    }

    fn command(&self, command: Command) -> Result<Value, ActionError> {
        self.ctl
            .command(Target::Focus, command)
            .map(|request_id| json!({ "requestId": request_id }))
            .map_err(|e| ActionError::Failed(e.to_string()))
    }
}

fn slot_arg(args: &Value) -> Result<usize, ActionError> {
    args.get("slot")
        .and_then(Value::as_u64)
        .map(|n| n as usize)
        .filter(|n| *n < SLOT_COUNT)
        .ok_or_else(|| ActionError::InvalidArgs(format!("`slot` must be 0-{}", SLOT_COUNT - 1)))
}

const CATEGORY: &str = "Agent Control";

fn def(id: &str, label: &str, description: &str) -> ActionDef {
    ActionDef::new(id, label, description).category(CATEGORY)
}

fn slot_param() -> ParamDef {
    ParamDef::number("slot", "Track")
        .required()
        .integer()
        .range(0.0, (SLOT_COUNT - 1) as f64)
        .help("0 is the first track, 7 the last.")
}

/// Which command an action issues, for availability. Actions that only change
/// the selection or the demo are always available.
fn command_of(action: &str) -> Option<CommandKind> {
    Some(match action {
        APPROVE => CommandKind::Approve,
        REJECT => CommandKind::Reject,
        "pause" => CommandKind::Pause,
        "resume" => CommandKind::Resume,
        "cancel" => CommandKind::Cancel,
        RETRY => CommandKind::Retry,
        CONTINUE => CommandKind::SendInstruction,
        NEW_TASK => CommandKind::Spawn,
        _ => return None,
    })
}

/// The instruction `continue` sends: `text`, or "continue" when it is
/// blank. An artifact's review shows the same text, read here.
pub(crate) fn instruction_of(args: &Value) -> &str {
    args.get("text")
        .and_then(Value::as_str)
        .filter(|t| !t.trim().is_empty())
        .unwrap_or(CONTINUE)
}

/// The task `newTask` starts and the folder it starts in (none when
/// blank). An artifact's review shows the same, read here.
pub(crate) fn new_task_of(args: &Value) -> Result<(String, Option<String>), ActionError> {
    let task = str_arg(args, "task")?.to_string();
    let cwd = args
        .get("cwd")
        .and_then(Value::as_str)
        .filter(|c| !c.trim().is_empty())
        .map(str::to_string);
    Ok((task, cwd))
}

fn sentence(label: &str) -> String {
    let mut chars = label.chars();
    match chars.next() {
        Some(first) => format!("{}{}.", first.to_uppercase(), chars.as_str()),
        None => String::new(),
    }
}

impl Plugin for AgentControlPlugin {
    fn observables(&self) -> Vec<crate::state::ObservableDefinition> {
        super::observables::all()
    }

    fn manifest(&self) -> PluginManifest {
        PluginManifest {
            id: PLUGIN_ID.into(),
            name: "Agent Control".into(),
            version: env!("CARGO_PKG_VERSION").into(),
            description: "Observe, intervene in and orchestrate agents from any surface. \
                          Commands act on the focused track and respect each agent's capabilities."
                .into(),
            kind: ProviderKind::Module,
            actions: vec![
                def(
                    APPROVE,
                    "Approve",
                    "Approve the focused session's pending request.",
                )
                .keywords(&["yes", "allow", "accept", "permission", "confirm", "agent"])
                .look("Approve", visual::GREEN, "✔"),
                def(
                    REJECT,
                    "Reject",
                    "Reject the focused session's pending request.",
                )
                .keywords(&["no", "deny", "refuse", "decline", "agent"])
                .look("Reject", visual::RED, "✕"),
                def(
                    "pause",
                    "Pause",
                    "Pause the focused session (if the agent supports it).",
                )
                .keywords(&["hold", "wait", "agent"])
                .look("Pause", visual::ORANGE, "⏸"),
                def("resume", "Resume", "Resume the focused session.")
                    .keywords(&["unpause", "go", "agent"])
                    .look("Resume", visual::GREEN, "▶"),
                def("cancel", "Cancel", "Cancel the focused session.")
                    .keywords(&["stop", "abort", "kill", "end", "agent"])
                    .look("Stop", visual::RED, "■"),
                def(RETRY, "Retry", "Retry a failed or cancelled session.")
                    .keywords(&["again", "rerun", "restart", "agent"])
                    .look("Retry", visual::YELLOW, "↻"),
                def(
                    CONTINUE,
                    "Continue",
                    "Send an instruction to the focused session.",
                )
                .keywords(&["instruction", "prompt", "message", "send", "tell", "agent"])
                .param(
                    ParamDef::string("text", "Instruction")
                        .multiline()
                        .max_length(MAX_TEXT)
                        .default_value(json!("continue"))
                        .placeholder("continue"),
                )
                .look("Continue", visual::PURPLE, "⏵⏵"),
                def(
                    NEW_TASK,
                    "New task",
                    "Start a task on the focused agent, or on the first agent that can.",
                )
                .keywords(&["start", "spawn", "prompt", "run", "begin", "agent", "job"])
                .param(
                    ParamDef::string("task", "Task")
                        .required()
                        .multiline()
                        .max_length(MAX_TEXT)
                        .placeholder("Run the test suite and report failures"),
                )
                .param(
                    ParamDef::string("cwd", "Working folder")
                        .max_length(MAX_CWD)
                        .help("Optional. Where the agent should work."),
                )
                .look("New task", visual::CYAN, "✓"),
                def(
                    "focusSlot",
                    "Select track",
                    "Select the agent track in a slot; the command pads then act on it.",
                )
                .keywords(&["focus", "choose", "slot", "agent"])
                .param(slot_param()),
                def(
                    "clearFocus",
                    "Clear selection",
                    "Deselect the focused track.",
                )
                .keywords(&["unfocus", "none", "agent"]),
                def(
                    "dismissSlot",
                    "Dismiss track",
                    "Remove a finished session from a slot.",
                )
                .keywords(&["remove", "clear", "close", "slot", "agent"])
                .param(slot_param()),
                def(
                    "runDemo",
                    "Run Agent Demo",
                    "Start four simulated agents that exercise the whole workflow.",
                )
                .keywords(&["simulated", "sample", "try", "play"]),
                def(
                    "resetDemo",
                    "Reset demo",
                    "Stop the simulated agents and clear their sessions.",
                )
                .keywords(&["simulated", "sample", "clear", "stop"]),
                def(
                    "openApp",
                    "Open Agents view",
                    "Ask the desktop app to show the Agents section.",
                )
                .keywords(&["window", "show", "sessions", "logs"]),
            ],
            states: vec![
                StateDef {
                    key: "slot.changed".into(),
                    description: "Status of the track in a slot (`slot`, `status`).".into(),
                    values: Status::ALL
                        .iter()
                        .map(|s| s.as_str().to_string())
                        .chain(["empty".to_string()])
                        .collect(),
                },
                StateDef {
                    key: "command.state".into(),
                    description: "State of a command for the focused track (`command`, `state`)."
                        .into(),
                    values: CommandState::ALL
                        .iter()
                        .map(|s| s.as_str().to_string())
                        .collect(),
                },
                StateDef {
                    key: "focus.changed".into(),
                    description: "Which slot is focused (`slot`, or null).".into(),
                    values: vec![],
                },
            ],
            permissions: vec![],
            external: None,
        }
    }

    fn availability(&self, action: &str) -> Availability {
        match command_of(action) {
            Some(kind) => match self.ctl.focus_availability(kind) {
                Ok(()) => Availability::Available,
                Err(why) => Availability::unavailable(Cause::State, sentence(why.label())),
            },
            None => Availability::Available,
        }
    }

    fn invoke(
        &self,
        ctx: &PluginContext,
        action: &str,
        args: &Value,
    ) -> Result<Value, ActionError> {
        match action {
            APPROVE => self.command(Command::Approve),
            REJECT => self.command(Command::Reject),
            "pause" => self.command(Command::Pause),
            "resume" => self.command(Command::Resume),
            "cancel" => self.command(Command::Cancel),
            RETRY => self.command(Command::Retry),
            CONTINUE => self.command(Command::SendInstruction {
                text: instruction_of(args).into(),
            }),
            NEW_TASK => {
                let (task, cwd) = new_task_of(args)?;
                self.command(Command::Spawn { task, cwd })
            }
            "focusSlot" => {
                let slot = slot_arg(args)?;
                self.ctl.focus_slot(slot).map_err(ActionError::Failed)?;
                Ok(json!({ "slot": slot }))
            }
            "clearFocus" => {
                self.ctl.clear_focus();
                Ok(Value::Null)
            }
            "dismissSlot" => {
                let slot = slot_arg(args)?;
                self.ctl.dismiss_slot(slot).map_err(ActionError::Failed)?;
                Ok(Value::Null)
            }
            "runDemo" => {
                self.demo.run_demo().map_err(ActionError::Failed)?;
                Ok(Value::Null)
            }
            "resetDemo" => {
                self.demo.reset_demo();
                self.ctl.clear_focus();
                Ok(Value::Null)
            }
            "openApp" => {
                ctx.publish("ui.navigate", json!({ "view": "agents" }));
                Ok(Value::Null)
            }
            other => Err(ActionError::UnknownAction {
                plugin: PLUGIN_ID.into(),
                action: other.into(),
            }),
        }
    }
}
