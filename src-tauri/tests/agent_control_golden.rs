//! Golden LEDs of the shipped Agent Control surface.
//!
//! Moving Agent Control from projection events and bindings to the state hub
//! and feedback rules must not change what a person sees. This test drives every
//! status of a track, both states of a focus marker and every command state of
//! every command pad through the real runtime, reads the LEDs of the virtual
//! Launchpad and compares them with `tests/fixtures/agent-control-golden.json`.
//!
//! The fixture was recorded from the workspace schema 3 implementation, before
//! the change (`UPDATE_GOLDEN=1 cargo test --test agent_control_golden`). It
//! must still match after it.

#[path = "support/state_scenario.rs"]
mod scenario;

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

use launchdeck_lib::agents::model::{CommandKind, Status};
use launchdeck_lib::agents::visual::{self, CommandState};
use launchdeck_lib::core::runtime::Runtime;
use launchdeck_lib::events::Event;
use scenario::*;
use serde_json::{json, Value};

const GOLDEN: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/agent-control-golden.json"
);

/// Track 6 holds no agent in the shipped setup, so a test can drive it freely.
const SLOT: u8 = 6;

/// The top-left cell of each command pad on the shipped surface.
const PADS: [(CommandKind, (u8, u8)); 8] = [
    (CommandKind::Approve, (0, 2)),
    (CommandKind::Reject, (2, 2)),
    (CommandKind::Pause, (4, 2)),
    (CommandKind::Resume, (5, 2)),
    (CommandKind::Retry, (6, 2)),
    (CommandKind::Cancel, (7, 2)),
    (CommandKind::SendInstruction, (4, 3)),
    (CommandKind::Spawn, (0, 4)),
];

fn describe(rt: &Runtime, col: u8, row: u8) -> Value {
    let l = led(rt, col, row);
    json!({
        "color": l.color.to_hex(),
        "animation": serde_json::to_value(l.animation).unwrap(),
    })
}

// ---------------------------------------------------------------------------
// How the test drives Agent Control's state. This is the only part that
// depends on the implementation.
// ---------------------------------------------------------------------------

fn drive_slot(rt: &Runtime, slot: u8, status: &str) {
    // Schema 3: the projection event the surface is bound to.
    rt.publish_external(Event::new(
        "agent-control",
        "slot.changed",
        json!({ "slot": slot, "status": status }),
    ));
}

fn drive_focus(rt: &Runtime, slot: Option<u8>) {
    rt.publish_external(Event::new(
        "agent-control",
        "focus.changed",
        json!({ "slot": slot }),
    ));
}

fn drive_command(rt: &Runtime, command: CommandKind, state: CommandState) {
    rt.publish_external(Event::new(
        "agent-control",
        "command.state",
        json!({ "command": command.as_str(), "state": state.as_str() }),
    ));
}

async fn settle() {
    tokio::time::sleep(Duration::from_millis(140)).await;
}

async fn capture(rt: &Arc<Runtime>) -> Value {
    let mut slots = BTreeMap::new();
    let all_statuses = Status::ALL
        .iter()
        .map(|s| s.as_str())
        .chain([visual::EMPTY])
        .collect::<Vec<_>>();
    for status in all_statuses {
        drive_slot(rt, SLOT, status);
        settle().await;
        slots.insert(status.to_string(), describe(rt, SLOT, 0));
    }
    drive_slot(rt, SLOT, visual::EMPTY);

    let mut focus = BTreeMap::new();
    drive_focus(rt, Some(SLOT));
    settle().await;
    focus.insert("selected".to_string(), describe(rt, SLOT, 1));
    drive_focus(rt, None);
    settle().await;
    focus.insert("not selected".to_string(), describe(rt, SLOT, 1));

    let mut commands: BTreeMap<String, BTreeMap<String, Value>> = BTreeMap::new();
    for state in CommandState::ALL {
        for (command, _) in PADS {
            drive_command(rt, command, state);
        }
        settle().await;
        for (command, (col, row)) in PADS {
            commands
                .entry(command.as_str().to_string())
                .or_default()
                .insert(state.as_str().to_string(), describe(rt, col, row));
        }
    }
    json!({ "slot.status": slots, "slot.focused": focus, "command.state": commands })
}

#[tokio::test(flavor = "multi_thread")]
async fn the_agent_control_surface_looks_exactly_like_the_recorded_one() {
    let rt = start(Options::default()).await;
    assert_eq!(rt.state().active_surface_id, "agent-control");
    let now = capture(&rt).await;

    if std::env::var("UPDATE_GOLDEN").is_ok() {
        std::fs::write(GOLDEN, serde_json::to_string_pretty(&now).unwrap() + "\n").unwrap();
        eprintln!("recorded {GOLDEN}");
        return;
    }
    let golden: Value = serde_json::from_str(
        &std::fs::read_to_string(GOLDEN).expect("the golden file is committed"),
    )
    .unwrap();
    assert_eq!(
        serde_json::to_string_pretty(&now).unwrap(),
        serde_json::to_string_pretty(&golden).unwrap(),
        "the surface no longer looks like the recorded one"
    );
}
