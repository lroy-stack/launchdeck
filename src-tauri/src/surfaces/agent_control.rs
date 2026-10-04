//! The Agent Control surface: a glanceable physical control surface for
//! autonomous agents.
//!
//! ```text
//! row 0  [1][2][3][4][5][6][7][8]     one track per pad (session, or an agent with none)
//! row 1  [ ][ ][ ][ ][ ][ ][ ][ ]     focus marker under the selected track
//! row 2  [APPROVE ][REJECT  ][Pause][Resume][Retry][Stop]
//! row 3  [   2x2  ][   2x2  ][     ][Continue]
//! row 4  [Run tests][Review diff][Summarize]               start a task
//! row 7  [Run Agent Demo][Reset][ ... ][Open Agents]
//! ```
//!
//! Nothing here is provider specific and nothing here is about a device. The
//! surface only carries *feedback rules* on the states Agent Control offers
//! (`slot.status`, `slot.focused`, `command.state`) and presses call plugin
//! actions. Colours come from `agents::visual`, the single status mapping.

use serde_json::{json, Value};

use super::*;
use crate::agents::model::{CommandKind, Status};
use crate::agents::observables;
use crate::agents::plugin::PLUGIN_ID;
use crate::agents::service::SLOT_COUNT;
use crate::agents::visual::{self, command_visual, CommandState};
use crate::state::feedback::{FeedbackRule, Test};
use crate::state::Scalar;

pub const SURFACE_ID: &str = "agent-control";

fn act(action: &str, args: Value) -> Assignment {
    plugin(PLUGIN_ID, action, args)
}

/// One pad per slot. Its look is the status of whatever the slot holds.
fn slot_pad(i: u8) -> Control {
    let mut c = ctl(
        g(i, 0),
        &format!("Agent {}", i + 1),
        &(i + 1).to_string(),
        visual::GRAY,
        act("focusSlot", json!({ "slot": i })),
    );
    c.description = "Shows the status of one agent session. Press to select it; the command \
                     pads then act on it. Long-press dismisses a finished session."
        .into();
    c.long_press = Some(act("dismissSlot", json!({ "slot": i })));
    c.visual = visual::empty_visual();
    c.states
        .insert(visual::EMPTY.into(), visual::empty_visual());
    for status in Status::ALL {
        c.states
            .insert(status.as_str().into(), visual::status_visual(status));
    }
    // One rule per pad: show the look named after the track's status.
    c.feedback = vec![
        FeedbackRule::state(observables::slot_status(i.into()), Test::HasValue).show_from_value(),
    ];
    c
}

/// The marker under a slot lights up when that slot is selected.
fn focus_marker(i: u8) -> Control {
    let mut c = ctl(
        g(i, 1),
        &format!("Select {}", i + 1),
        "▲",
        visual::WHITE,
        act("focusSlot", json!({ "slot": i })),
    );
    c.description = "Lit while this track is selected. Press to select it.".into();
    c.visual = visual::focus_visual(false);
    c.states
        .insert("focused".into(), visual::focus_visual(true));
    // Lit while this track is selected; otherwise no rule is true and it rests.
    c.feedback = vec![FeedbackRule::state(
        observables::slot_focused(i.into()),
        Test::Equals {
            value: Scalar::Bool(true),
        },
    )
    .show("focused")];
    c
}

/// A pad for one command. It shows the six command states: unavailable
/// (dim), available, attention (flashing), pending, succeeded, failed.
#[allow(clippy::too_many_arguments)]
fn command_pad(
    pos: ControlPos,
    span: Span,
    label: &str,
    icon: &str,
    base: Rgb,
    command: CommandKind,
    action: Assignment,
    description: &str,
) -> Control {
    let mut c = ctl(pos, label, icon, base, action);
    c.span = span;
    c.description = description.into();
    c.visual = command_visual(base, CommandState::Unavailable);
    for state in CommandState::ALL {
        c.states
            .insert(state.as_str().into(), command_visual(base, state));
    }
    c.feedback = vec![
        FeedbackRule::state(observables::command_state(command), Test::HasValue).show_from_value(),
    ];
    c
}

fn one() -> Span {
    Span { w: 1, h: 1 }
}

fn big() -> Span {
    Span { w: 2, h: 2 }
}

pub(super) fn agent_control() -> Surface {
    let mut controls: Vec<Control> = Vec::new();
    for i in 0..SLOT_COUNT as u8 {
        controls.push(slot_pad(i));
        controls.push(focus_marker(i));
    }

    controls.push(command_pad(
        g(0, 2),
        big(),
        "Approve",
        "✔",
        visual::GREEN,
        CommandKind::Approve,
        act("approve", json!({})),
        "Approve the selected session's pending request. Flashes while the selected \
         session is waiting for you. Never pressed for you.",
    ));
    controls.push(command_pad(
        g(2, 2),
        big(),
        "Reject",
        "✕",
        visual::RED,
        CommandKind::Reject,
        act("reject", json!({})),
        "Reject the selected session's pending request.",
    ));
    controls.push(command_pad(
        g(4, 2),
        one(),
        "Pause",
        "⏸",
        visual::ORANGE,
        CommandKind::Pause,
        act("pause", json!({})),
        "Pause the selected session, if its agent supports pausing.",
    ));
    controls.push(command_pad(
        g(5, 2),
        one(),
        "Resume",
        "▶",
        visual::GREEN,
        CommandKind::Resume,
        act("resume", json!({})),
        "Resume the selected paused session.",
    ));
    controls.push(command_pad(
        g(6, 2),
        one(),
        "Retry",
        "↻",
        visual::YELLOW,
        CommandKind::Retry,
        act("retry", json!({})),
        "Retry the selected failed or cancelled session.",
    ));
    controls.push(command_pad(
        g(7, 2),
        one(),
        "Stop",
        "■",
        visual::RED,
        CommandKind::Cancel,
        act("cancel", json!({})),
        "Cancel the selected session.",
    ));
    controls.push(command_pad(
        g(4, 3),
        one(),
        "Continue",
        "⏵⏵",
        visual::PURPLE,
        CommandKind::SendInstruction,
        act("continue", json!({ "text": "continue" })),
        "Tell the selected session to continue. Edit the text in the inspector.",
    ));

    for (col, label, icon, task) in [
        (
            0,
            "Run tests",
            "✓",
            "Run the test suite and report failures",
        ),
        (1, "Review diff", "±", "Review the uncommitted changes"),
        (2, "Summarize", "≡", "Summarize the recent changes"),
    ] {
        controls.push(command_pad(
            g(col, 4),
            one(),
            label,
            icon,
            visual::CYAN,
            CommandKind::Spawn,
            act("newTask", json!({ "task": task })),
            "Start this task on the selected agent (or the first agent that can). \
             Edit the task text in the inspector.",
        ));
    }

    let mut demo = ctl(
        g(0, 7),
        "Run Agent Demo",
        "▶",
        visual::GREEN.scaled(0.7),
        act("runDemo", json!({})),
    );
    demo.span = Span { w: 2, h: 1 };
    demo.description = "Start four simulated agents: tool use, an approval to answer, a pause \
                        to try, a failure to retry. Nothing real runs."
        .into();
    let mut reset = ctl(
        g(2, 7),
        "Reset demo",
        "↺",
        visual::RED.scaled(0.45),
        act("resetDemo", json!({})),
    );
    reset.description = "Stop the simulated agents and clear their sessions.".into();
    let mut open = ctl(
        g(6, 7),
        "Open Agents",
        "☰",
        visual::GRAY,
        act("openApp", json!({})),
    );
    open.span = Span { w: 2, h: 1 };
    open.description =
        "Show the Agents section in the desktop app: sessions, logs, adapters.".into();
    controls.extend([demo, reset, open]);
    controls.extend(surface_switchers(SURFACE_ID));

    Surface {
        id: SURFACE_ID.into(),
        name: "Agent Control".into(),
        description: "A physical control surface for autonomous agents. Watch every session at \
                      a glance, approve or reject, pause and resume, start a task."
            .into(),
        icon: Some("◉".into()),
        category: "agents".into(),
        pages: vec![Page {
            id: "home".into(),
            name: "Home".into(),
            controls,
        }],
        context: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn surface() -> Surface {
        agent_control()
    }

    fn pad<'a>(s: &'a Surface, label: &str) -> &'a Control {
        s.pages[0]
            .controls
            .iter()
            .find(|c| c.label == label)
            .unwrap_or_else(|| panic!("no pad `{label}`"))
    }

    #[test]
    fn slot_pads_cover_every_status_from_the_central_mapping() {
        let s = surface();
        let p = pad(&s, "Agent 1");
        for status in Status::ALL {
            assert_eq!(
                p.states[status.as_str()],
                visual::status_visual(status),
                "{status:?}"
            );
        }
        assert_eq!(p.states[visual::EMPTY], visual::empty_visual());
    }

    #[test]
    fn there_is_one_slot_pad_and_one_focus_marker_per_slot() {
        let s = surface();
        let slots = s.pages[0]
            .controls
            .iter()
            .filter(|c| c.label.starts_with("Agent "))
            .count();
        let markers = s.pages[0]
            .controls
            .iter()
            .filter(|c| c.label.starts_with("Select "))
            .count();
        assert_eq!(slots, SLOT_COUNT);
        assert_eq!(markers, SLOT_COUNT);
    }

    #[test]
    fn command_pads_show_all_six_states() {
        let s = surface();
        for label in ["Approve", "Reject", "Pause", "Resume", "Retry", "Stop"] {
            let p = pad(&s, label);
            for state in CommandState::ALL {
                assert!(p.states.contains_key(state.as_str()), "{label}/{state:?}");
            }
        }
        assert_eq!(pad(&s, "Approve").span, Span { w: 2, h: 2 });
        assert_eq!(pad(&s, "Reject").span, Span { w: 2, h: 2 });
    }

    #[test]
    fn the_surface_has_no_provider_names_and_reads_only_agent_control_states() {
        let s = surface();
        let text = serde_json::to_string(&s).unwrap().to_lowercase();
        for banned in ["claude", "codex", "chatgpt", "openai", "anthropic"] {
            assert!(!text.contains(banned), "{banned} leaked into the surface");
        }
        for c in &s.pages[0].controls {
            for rule in &c.feedback {
                let crate::state::feedback::When::State(cond) = &rule.when else {
                    panic!("{} has an event rule", c.label);
                };
                assert_eq!(cond.state.provider, observables::PROVIDER);
            }
        }
    }

    #[test]
    fn pads_only_call_the_agent_control_plugin_or_navigate() {
        let s = surface();
        for c in &s.pages[0].controls {
            let a = c.press.as_ref().unwrap();
            match a.provider.as_str() {
                PLUGIN_ID | "navigation" => {}
                other => panic!("{} runs {other}", c.label),
            }
        }
    }
}
