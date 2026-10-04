//! Acceptance flows for Agent Control, run end to end on the real runtime:
//! simulated agents, the real Agent Control surface, the real renderer and the
//! virtual Launchpad. The assertions read the virtual device's LED frame, so
//! they check exactly what a person (or a physical Launchpad) would see.
//!
//! The simulated agents run at 5% of their documented durations to keep the
//! suite fast; the sequence is identical.

use std::sync::Arc;
use std::time::Duration;

use launchdeck_lib::agents::model::{CommandKind, Status};
use launchdeck_lib::agents::visual::{self, command_visual, CommandState};
use launchdeck_lib::core::model::{Animation, ControlPos, Rgb, Visual};
use launchdeck_lib::core::runtime::{NullEmitter, Runtime, RuntimeConfig};
use launchdeck_lib::devices::Led;
use launchdeck_lib::plugins::keyboard::{Combo, Keyboard};
use launchdeck_lib::plugins::ActionError;
use launchdeck_lib::surfaces::default_workspace;

struct NoKeyboard;
impl Keyboard for NoKeyboard {
    fn press(&self, _combo: &Combo) -> Result<(), ActionError> {
        Ok(())
    }
}

async fn start() -> Arc<Runtime> {
    let rt = Runtime::start(RuntimeConfig {
        workspace: default_workspace(),
        store: None,
        emitter: Arc::new(NullEmitter),
        keyboard: Arc::new(NoKeyboard),
        scan_hardware: false,
        serve_external: false,
        demo_pace: 0.05,
        plugin_dirs: vec![],
    })
    .await;
    assert_eq!(rt.state().active_surface_id, "agent-control");
    wait_until("first frame", || {
        !rt.virtual_handle().frame().leds.is_empty()
    })
    .await;
    rt
}

async fn wait_until(what: &str, mut cond: impl FnMut() -> bool) {
    for _ in 0..600 {
        if cond() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("timed out waiting for: {what}");
}

fn led(rt: &Runtime, col: u8, row: u8) -> Led {
    rt.virtual_handle()
        .frame()
        .get(ControlPos::grid(col, row))
        .expect("pad exists")
}

fn tap(rt: &Runtime, col: u8, row: u8) {
    let h = rt.virtual_handle();
    h.press(ControlPos::grid(col, row), 100).unwrap();
    h.release(ControlPos::grid(col, row)).unwrap();
}

/// What the virtual device shows for a visual (it has native pulse and flash).
fn shows(rt: &Runtime, col: u8, row: u8, v: Visual) -> bool {
    led(rt, col, row)
        == Led {
            color: v.color,
            animation: v.animation,
        }
}

fn slot_shows(rt: &Runtime, slot: u8, status: Status) -> bool {
    shows(rt, slot, 0, visual::status_visual(status))
}

fn pad_shows(rt: &Runtime, col: u8, row: u8, base: Rgb, state: CommandState) -> bool {
    shows(rt, col, row, command_visual(base, state))
}

// Pad positions on the Agent Control surface.
const APPROVE: (u8, u8) = (0, 2);
const REJECT: (u8, u8) = (2, 2);
const PAUSE: (u8, u8) = (4, 2);
const RESUME: (u8, u8) = (5, 2);
const RETRY: (u8, u8) = (6, 2);
const STOP: (u8, u8) = (7, 2);
const RUN_DEMO: (u8, u8) = (0, 7);
const RESET_DEMO: (u8, u8) = (2, 7);

const ATLAS: u8 = 0;
const BEACON: u8 = 1;
const CINDER: u8 = 2;
const DELTA: u8 = 3;

fn approve_pad(rt: &Runtime, s: CommandState) -> bool {
    pad_shows(rt, APPROVE.0, APPROVE.1, visual::GREEN, s)
}
fn reject_pad(rt: &Runtime, s: CommandState) -> bool {
    pad_shows(rt, REJECT.0, REJECT.1, visual::RED, s)
}

#[tokio::test(flavor = "multi_thread")]
async fn the_surface_starts_calm_with_every_command_unavailable() {
    let rt = start().await;
    wait_until("agents idle on their pads", || {
        [ATLAS, BEACON, CINDER, DELTA]
            .iter()
            .all(|s| slot_shows(&rt, *s, Status::Idle))
    })
    .await;
    // Slots without an agent are quiet.
    assert!(shows(&rt, 4, 0, visual::empty_visual()));
    assert!(approve_pad(&rt, CommandState::Unavailable));
    assert!(reject_pad(&rt, CommandState::Unavailable));
    assert!(pad_shows(
        &rt,
        PAUSE.0,
        PAUSE.1,
        visual::ORANGE,
        CommandState::Unavailable
    ));
    // No focus marker is lit.
    assert!(shows(&rt, 0, 1, visual::focus_visual(false)));
}

/// Flow 1: four agents change state at the same time and the pads follow.
#[tokio::test(flavor = "multi_thread")]
async fn flow_1_observation_four_agents_change_state_simultaneously() {
    let rt = start().await;
    tap(&rt, RUN_DEMO.0, RUN_DEMO.1);

    // Over time every pad walks through its own sequence; at some instant the
    // four tracks show at least three different states side by side.
    let mut seen_at_once = 0usize;
    let mut states_seen = std::collections::BTreeSet::new();
    for _ in 0..400 {
        let snap = rt.agents().snapshot();
        let mut now = std::collections::BTreeSet::new();
        for slot in [ATLAS, BEACON, CINDER, DELTA] {
            if let Some(status) = snap.slots[slot as usize].status {
                // The pad matches the runtime state it was derived from.
                if shows(&rt, slot, 0, visual::status_visual(status)) {
                    now.insert(status);
                    states_seen.insert(status);
                }
            }
        }
        seen_at_once = seen_at_once.max(now.len());
        if seen_at_once >= 3 && states_seen.contains(&Status::ApprovalRequired) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert!(seen_at_once >= 3, "pads moved in lockstep: {seen_at_once}");
    for expected in [
        Status::Starting,
        Status::Running,
        Status::ToolUse,
        Status::ApprovalRequired,
    ] {
        assert!(
            states_seen.contains(&expected),
            "never saw {expected:?}: {states_seen:?}"
        );
    }
    // The app, the virtual device and a physical device read one state: the
    // snapshot's slot list is what the pads were just checked against.
    assert_eq!(rt.snapshot().agents.slots.len(), 8);
}

/// Flow 2: approval required, the APPROVE pad flashes, the human presses it,
/// the session resumes and completes, the pad goes green.
#[tokio::test(flavor = "multi_thread")]
async fn flow_2_approval_pad_flashes_press_resumes_and_completes() {
    let rt = start().await;
    tap(&rt, RUN_DEMO.0, RUN_DEMO.1);

    wait_until("Atlas asks for approval", || {
        slot_shows(&rt, ATLAS, Status::ApprovalRequired)
    })
    .await;
    // The first approval is selected automatically; APPROVE flashes, REJECT is available.
    wait_until("APPROVE flashes", || {
        approve_pad(&rt, CommandState::Attention)
    })
    .await;
    assert!(reject_pad(&rt, CommandState::Available));
    assert!(
        shows(&rt, ATLAS, 1, visual::focus_visual(true)),
        "focus marker under Atlas"
    );
    // All four cells of the big pad flash together.
    for (c, r) in [(0, 2), (1, 2), (0, 3), (1, 3)] {
        assert!(approve_pad_cell(&rt, c, r), "cell {c},{r}");
    }

    // Nothing happens by itself: stay pending for a while.
    tokio::time::sleep(Duration::from_millis(600)).await;
    assert!(
        slot_shows(&rt, ATLAS, Status::ApprovalRequired),
        "never auto-approved"
    );

    // Press any cell of the big APPROVE pad.
    tap(&rt, 1, 3);
    wait_until("Atlas resumes", || {
        slot_shows(&rt, ATLAS, Status::Running) || slot_shows(&rt, ATLAS, Status::ToolUse)
    })
    .await;
    wait_until("APPROVE goes back to unavailable", || {
        approve_pad(&rt, CommandState::Unavailable)
    })
    .await;
    wait_until("Atlas completes: solid green", || {
        slot_shows(&rt, ATLAS, Status::Completed)
    })
    .await;
    // The audit trail has the decision, without any request text.
    let audit = rt.agents().snapshot().audit;
    assert!(audit.iter().any(|e| e.kind == "approval.requested"));
    assert!(audit.iter().any(|e| e.kind == "approval.approved"));
}

fn approve_pad_cell(rt: &Runtime, c: u8, r: u8) -> bool {
    pad_shows(rt, c, r, visual::GREEN, CommandState::Attention)
}

#[tokio::test(flavor = "multi_thread")]
async fn rejecting_leaves_the_agent_waiting_for_instructions() {
    let rt = start().await;
    tap(&rt, RUN_DEMO.0, RUN_DEMO.1);
    wait_until("APPROVE flashes", || {
        approve_pad(&rt, CommandState::Attention)
    })
    .await;
    tap(&rt, REJECT.0, REJECT.1);
    wait_until("Atlas waits", || slot_shows(&rt, ATLAS, Status::Waiting)).await;
    let audit = rt.agents().snapshot().audit;
    assert!(audit.iter().any(|e| e.kind == "approval.rejected"));
    // CONTINUE (send instruction) is available for a waiting agent.
    wait_until("Continue available", || {
        pad_shows(&rt, 4, 3, visual::PURPLE, CommandState::Available)
    })
    .await;
    tap(&rt, 4, 3);
    wait_until("Atlas carries on", || {
        slot_shows(&rt, ATLAS, Status::Running)
    })
    .await;
    wait_until("and completes", || {
        slot_shows(&rt, ATLAS, Status::Completed)
    })
    .await;
}

/// Flow 3: running -> PAUSE -> paused -> RESUME -> continues.
#[tokio::test(flavor = "multi_thread")]
async fn flow_3_intervention_pause_and_resume() {
    let rt = start().await;
    tap(&rt, RUN_DEMO.0, RUN_DEMO.1);

    wait_until("Delta runs", || slot_shows(&rt, DELTA, Status::Running)).await;
    // Select Delta (press its slot): PAUSE becomes available, RESUME and RETRY do not.
    tap(&rt, DELTA, 0);
    wait_until("focus on Delta", || {
        shows(&rt, DELTA, 1, visual::focus_visual(true))
    })
    .await;
    wait_until("PAUSE available", || {
        pad_shows(
            &rt,
            PAUSE.0,
            PAUSE.1,
            visual::ORANGE,
            CommandState::Available,
        )
    })
    .await;
    assert!(pad_shows(
        &rt,
        RESUME.0,
        RESUME.1,
        visual::GREEN,
        CommandState::Unavailable
    ));

    tap(&rt, PAUSE.0, PAUSE.1);
    wait_until("Delta paused", || slot_shows(&rt, DELTA, Status::Paused)).await;
    wait_until("RESUME available", || {
        pad_shows(
            &rt,
            RESUME.0,
            RESUME.1,
            visual::GREEN,
            CommandState::Available,
        )
    })
    .await;
    // While paused it makes no progress.
    let progress = |rt: &Runtime| {
        rt.agents()
            .snapshot()
            .sessions
            .into_iter()
            .find(|s| s.session.agent_id == "demo-delta")
            .and_then(|s| s.session.progress)
    };
    let frozen = progress(&rt);
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(progress(&rt), frozen, "progress moved while paused");

    tap(&rt, RESUME.0, RESUME.1);
    wait_until("Delta runs again", || {
        slot_shows(&rt, DELTA, Status::Running) || slot_shows(&rt, DELTA, Status::ToolUse)
    })
    .await;
    wait_until("Delta completes", || {
        slot_shows(&rt, DELTA, Status::Completed)
    })
    .await;
}

#[tokio::test(flavor = "multi_thread")]
async fn commands_the_agent_does_not_support_stay_dim() {
    let rt = start().await;
    tap(&rt, RUN_DEMO.0, RUN_DEMO.1);
    wait_until("Cinder runs", || {
        slot_shows(&rt, CINDER, Status::Running) || slot_shows(&rt, CINDER, Status::ToolUse)
    })
    .await;
    // Cinder cannot be paused: selecting it leaves PAUSE unavailable, STOP available.
    tap(&rt, CINDER, 0);
    wait_until("STOP available", || {
        pad_shows(&rt, STOP.0, STOP.1, visual::RED, CommandState::Available)
    })
    .await;
    assert!(pad_shows(
        &rt,
        PAUSE.0,
        PAUSE.1,
        visual::ORANGE,
        CommandState::Unavailable
    ));
    // Pressing the dim pad does nothing.
    tap(&rt, PAUSE.0, PAUSE.1);
    tokio::time::sleep(Duration::from_millis(150)).await;
    assert!(!slot_shows(&rt, CINDER, Status::Paused));
}

/// Flow 4: a task fails, the pad flashes red, RETRY is available and works.
#[tokio::test(flavor = "multi_thread")]
async fn flow_4_failure_flashes_red_and_retry_recovers() {
    let rt = start().await;
    tap(&rt, RUN_DEMO.0, RUN_DEMO.1);

    wait_until("Cinder fails: red flash", || {
        slot_shows(&rt, CINDER, Status::Failed)
    })
    .await;
    // The app has the same fact, with a concise error.
    let snap = rt.agents().snapshot();
    let cinder = snap
        .sessions
        .iter()
        .find(|s| s.session.agent_id == "demo-cinder")
        .unwrap();
    assert_eq!(cinder.session.status, Status::Failed);
    assert_eq!(
        cinder.session.error.as_deref(),
        Some("3 of 42 tests failed")
    );
    let retry = cinder
        .commands
        .iter()
        .find(|c| c.kind == CommandKind::Retry)
        .unwrap();
    assert_eq!(retry.state, CommandState::Available);
    assert_eq!(snap.summary.failed_sessions, 1);

    // Select the failed track: RETRY lights up.
    tap(&rt, CINDER, 0);
    wait_until("RETRY available", || {
        pad_shows(
            &rt,
            RETRY.0,
            RETRY.1,
            visual::YELLOW,
            CommandState::Available,
        )
    })
    .await;
    tap(&rt, RETRY.0, RETRY.1);
    wait_until("Cinder starts again", || {
        slot_shows(&rt, CINDER, Status::Starting) || slot_shows(&rt, CINDER, Status::Running)
    })
    .await;
    wait_until("Cinder succeeds this time", || {
        slot_shows(&rt, CINDER, Status::Completed)
    })
    .await;
    assert!(rt
        .agents()
        .snapshot()
        .audit
        .iter()
        .any(|e| e.kind == "session.failed"));
}

#[tokio::test(flavor = "multi_thread")]
async fn cancelling_a_session_from_a_pad() {
    let rt = start().await;
    tap(&rt, RUN_DEMO.0, RUN_DEMO.1);
    wait_until("Beacon runs", || slot_shows(&rt, BEACON, Status::Running)).await;
    tap(&rt, BEACON, 0);
    wait_until("STOP available", || {
        pad_shows(&rt, STOP.0, STOP.1, visual::RED, CommandState::Available)
    })
    .await;
    tap(&rt, STOP.0, STOP.1);
    wait_until("Beacon cancelled", || {
        slot_shows(&rt, BEACON, Status::Cancelled)
    })
    .await;
    let audit = rt.agents().snapshot().audit;
    assert!(audit.iter().any(|e| e.kind == "intervention.cancel"));
    assert!(audit.iter().any(|e| e.kind == "session.cancelled"));
}

#[tokio::test(flavor = "multi_thread")]
async fn a_new_task_can_be_started_from_a_template_pad() {
    let rt = start().await;
    // Nothing is running: the first agent that can spawn is used.
    wait_until("New task pad is available", || {
        pad_shows(&rt, 0, 4, visual::CYAN, CommandState::Available)
    })
    .await;
    tap(&rt, 0, 4);
    wait_until("a session appears", || {
        rt.agents()
            .snapshot()
            .sessions
            .iter()
            .any(|s| s.session.agent_id == "demo-atlas" && s.session.task.is_some())
    })
    .await;
    let snap = rt.agents().snapshot();
    let session = snap
        .sessions
        .iter()
        .find(|s| s.session.agent_id == "demo-atlas")
        .unwrap();
    assert_eq!(
        session.session.task.as_deref(),
        Some("Run the test suite and report failures")
    );
    wait_until("it completes", || slot_shows(&rt, ATLAS, Status::Completed)).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn state_survives_switching_surfaces_and_back() {
    let rt = start().await;
    tap(&rt, RUN_DEMO.0, RUN_DEMO.1);
    wait_until("Atlas asks", || {
        slot_shows(&rt, ATLAS, Status::ApprovalRequired)
    })
    .await;
    wait_until("APPROVE flashes", || {
        approve_pad(&rt, CommandState::Attention)
    })
    .await;

    rt.activate_surface("development", None).unwrap();
    tokio::time::sleep(Duration::from_millis(200)).await;
    // The agents did not notice: the state lives in the runtime, not on a surface.
    assert_eq!(rt.agents().snapshot().summary.pending_approvals, 1);

    rt.activate_surface("agent-control", None).unwrap();
    wait_until("APPROVE still flashing", || {
        approve_pad(&rt, CommandState::Attention)
    })
    .await;
    assert!(slot_shows(&rt, ATLAS, Status::ApprovalRequired));
}

#[tokio::test(flavor = "multi_thread")]
async fn reset_clears_the_demo_and_the_pads_calm_down() {
    let rt = start().await;
    tap(&rt, RUN_DEMO.0, RUN_DEMO.1);
    wait_until("Atlas asks", || {
        slot_shows(&rt, ATLAS, Status::ApprovalRequired)
    })
    .await;
    tap(&rt, RESET_DEMO.0, RESET_DEMO.1);
    wait_until("agents idle again", || {
        [ATLAS, BEACON, CINDER, DELTA]
            .iter()
            .all(|s| slot_shows(&rt, *s, Status::Idle))
    })
    .await;
    wait_until("APPROVE unavailable", || {
        approve_pad(&rt, CommandState::Unavailable)
    })
    .await;
    assert_eq!(rt.agents().snapshot().sessions.len(), 0);
    assert!(shows(&rt, ATLAS, 1, visual::focus_visual(false)));
    // And the animation machinery is not left running.
    assert_eq!(led(&rt, APPROVE.0, APPROVE.1).animation, Animation::None);
}
