//! The state core under load and at its edges, on the real runtime: what the
//! index evaluates, how event overlays expire, the unknown and unavailable
//! states, the gate that checks rules, Agent Control's states against its own
//! snapshot, and the hub read while the runtime is busy.

#[path = "support/state_scenario.rs"]
mod scenario;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use launchdeck_lib::agents::model::{CommandKind, Status as AgentStatus};
use launchdeck_lib::agents::observables as agent_states;
use launchdeck_lib::agents::service::is_projection;
use launchdeck_lib::agents::visual::{self, EMPTY};
use launchdeck_lib::core::model::{Control, Page, Surface};
use launchdeck_lib::state::hub::Status;
use launchdeck_lib::state::{ObservableDefinition, Scalar, StateAddress};
use launchdeck_lib::surfaces::{blank_surface, default_workspace};
use scenario::*;
use serde_json::{json, Value};

fn pad_with(col: u8, row: u8, rules: Vec<Value>) -> Control {
    let mut pad = Pad::at(col, row);
    for r in rules {
        pad = pad.rule(r);
    }
    pad.build()
}

fn state_rule(provider: &str, id: &str, is: Value, look: Value) -> Value {
    let mut rule = json!({
        "when": { "state": { "provider": provider, "id": id }, "is": is },
    });
    match look {
        Value::Bool(true) => rule["showFromValue"] = json!(true),
        other => rule["show"] = other,
    }
    rule
}

fn event_rule(source: &str, kind: &str, look: &str, hold: Value) -> Value {
    json!({
        "when": { "event": { "provider": source, "id": kind } },
        "show": look,
        "hold": hold,
    })
}

// ---------------------------------------------------------------------------
// The index
// ---------------------------------------------------------------------------

/// 2,560 pads, 40 of which read one Demo state and 40 listen to one event. A
/// state or an event nobody reads evaluates nothing; one that 40 pads read
/// evaluates exactly those 40.
#[tokio::test(flavor = "multi_thread")]
async fn an_unrelated_change_evaluates_nothing() {
    let mut ws = default_workspace();
    ws.surfaces.clear();
    for n in 0..10 {
        let mut s: Surface = blank_surface(&format!("syn{n}"), "Syn");
        s.pages = (0..4)
            .map(|pi| Page {
                id: format!("p{pi}"),
                name: format!("P{pi}"),
                controls: (0..64u8)
                    .map(|i| match (pi, i) {
                        (0, 0..=3) => pad_with(
                            i % 8,
                            i / 8,
                            vec![state_rule(
                                "demo",
                                "gamma",
                                json!({ "op": "hasValue" }),
                                json!(true),
                            )],
                        ),
                        (0, 4..=7) => pad_with(
                            i % 8,
                            i / 8,
                            vec![event_rule("ci", "e", "ok", json!("latch"))],
                        ),
                        _ => Pad::at(i % 8, i / 8).build(),
                    })
                    .collect(),
            })
            .collect();
        ws.surfaces.push(s);
    }
    ws.last_active_surface = Some("syn0".into());
    let rt = start(Options {
        workspace: Some(ws),
        ..Options::default()
    })
    .await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    let base = rt.feedback_stats();
    assert_eq!(
        base.evaluations, 40,
        "at start only the 40 state pads are evaluated, once each"
    );

    // Nobody reads Alpha.
    demo(&rt, "startTask", "alpha").await;
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(rt.feedback_stats().evaluations, base.evaluations);

    // Writing the same value again is not a change.
    let gamma = StateAddress::new("demo", "gamma");
    demo(&rt, "startTask", "gamma").await;
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(
        rt.feedback_stats().evaluations,
        base.evaluations + 40,
        "forty pads read Gamma"
    );
    rt.state_hub().set(&gamma, "running".into()).unwrap();
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(rt.feedback_stats().evaluations, base.evaluations + 40);

    // Events: a thousand nobody listens to cost no routing; one that 40 pads
    // listen to is routed to those 40.
    for i in 0..1000 {
        publish(&rt, "noise", "tick", json!({ "i": i }));
    }
    publish(&rt, "noise", "done", json!({}));
    wait("noise went by", || seen(&rt, "done")).await;
    assert_eq!(rt.feedback_stats().events_routed, 0);
    publish(&rt, "ci", "e", json!({}));
    wait("ci/e was routed", || {
        rt.feedback_stats().events_routed == 40
    })
    .await;
}

// ---------------------------------------------------------------------------
// Event overlays
// ---------------------------------------------------------------------------

/// An event shows its look for a while and then the pad returns to what the
/// state says. The event itself is never stored as a state.
#[tokio::test(flavor = "multi_thread")]
async fn an_event_look_expires_and_the_pad_returns_to_its_state() {
    let rt = start(Options::default()).await;
    let (s, p) = new_surface(&rt, "Probe");
    rt.activate_surface(&s, None).unwrap();
    let control = pad_with(
        0,
        0,
        vec![
            state_rule(
                "demo",
                "alpha",
                json!({ "op": "equals", "value": "running" }),
                json!("a"),
            ),
            event_rule("ci", "alert", "failed", json!({ "ms": 500 })),
        ],
    );
    let id = control.id.clone();
    rt.update_control(&s, &p, control).unwrap();
    demo(&rt, "startTask", "alpha").await;
    wait("the state shows", || colour(&rt, 0, 0) == AMBER).await;

    publish(&rt, "ci", "alert", json!({}));
    assert!(
        within(250, || colour(&rt, 0, 0) == RED).await,
        "the event shows"
    );
    let why = rt.explain_control(&s, &p, &id).unwrap();
    assert_eq!(why.layer, "overlay");
    assert_eq!(why.overlay.as_deref(), Some("failed"));
    assert_eq!(
        why.state_rules.result.look.as_deref(),
        Some("a"),
        "underneath, the state rules still say what they said"
    );

    assert!(
        within(1500, || colour(&rt, 0, 0) == AMBER).await,
        "after the hold the pad is back to the state"
    );
    assert_eq!(rt.explain_control(&s, &p, &id).unwrap().layer, "state");
    // The event left nothing behind in the hub.
    assert!(rt
        .state_hub()
        .known()
        .iter()
        .all(|(k, _)| k.provider() != "ci"));
}

/// The newest event replaces an older temporary look, and the older one's timer
/// cannot take the newer look away.
#[tokio::test(flavor = "multi_thread")]
async fn a_newer_event_look_is_not_cleared_by_an_older_timer() {
    let rt = start(Options::default()).await;
    let (s, p) = new_surface(&rt, "Probe");
    rt.activate_surface(&s, None).unwrap();
    rt.update_control(
        &s,
        &p,
        pad_with(
            0,
            0,
            vec![
                event_rule("ci", "first", "failed", json!({ "ms": 400 })),
                event_rule("ci", "second", "warn", json!({ "ms": 1500 })),
            ],
        ),
    )
    .unwrap();
    publish(&rt, "ci", "first", json!({}));
    assert!(within(250, || colour(&rt, 0, 0) == RED).await);
    tokio::time::sleep(Duration::from_millis(200)).await;
    publish(&rt, "ci", "second", json!({}));
    assert!(within(250, || colour(&rt, 0, 0) == AMBER).await);
    // The first hold ended 200 ms ago.
    tokio::time::sleep(Duration::from_millis(400)).await;
    assert_eq!(colour(&rt, 0, 0), AMBER);
    assert!(within(2500, || colour(&rt, 0, 0) == REST).await);
}

/// A latched look (what a migrated binding does) stays until another event
/// replaces it, `idle` clears it, or the control is edited.
#[tokio::test(flavor = "multi_thread")]
async fn a_latched_look_stays_until_replaced_cleared_or_edited() {
    let rt = start(Options::default()).await;
    let (s, p) = new_surface(&rt, "Probe");
    rt.activate_surface(&s, None).unwrap();
    let control = pad_with(
        0,
        0,
        vec![
            event_rule("ci", "sticky", "other", json!("latch")),
            event_rule("ci", "clear", "idle", json!("latch")),
        ],
    );
    rt.update_control(&s, &p, control.clone()).unwrap();
    publish(&rt, "ci", "sticky", json!({}));
    assert!(within(250, || colour(&rt, 0, 0) == PURPLE).await);
    tokio::time::sleep(Duration::from_millis(700)).await;
    assert_eq!(colour(&rt, 0, 0), PURPLE, "nothing expires a latch");

    publish(&rt, "ci", "clear", json!({}));
    assert!(
        within(250, || colour(&rt, 0, 0) == REST).await,
        "idle clears it"
    );

    publish(&rt, "ci", "sticky", json!({}));
    assert!(within(250, || colour(&rt, 0, 0) == PURPLE).await);
    let mut edited = control;
    edited.label = "Renamed".into();
    rt.update_control(&s, &p, edited).unwrap();
    assert!(
        within(250, || colour(&rt, 0, 0) == REST).await,
        "editing the control starts it over"
    );
}

/// A rule whose look comes from a payload field, and an event that lacks it.
#[tokio::test(flavor = "multi_thread")]
async fn an_event_can_name_its_look_in_a_payload_field() {
    let rt = start(Options::default()).await;
    let (s, p) = new_surface(&rt, "Probe");
    rt.activate_surface(&s, None).unwrap();
    rt.update_control(
        &s,
        &p,
        pad_with(
            0,
            0,
            vec![json!({
                "when": { "event": { "provider": "ci", "id": "result" },
                          "where": [{ "field": "branch", "equals": "main" }] },
                "showFromField": "outcome",
                "hold": "latch",
            })],
        ),
    )
    .unwrap();
    publish(
        &rt,
        "ci",
        "result",
        json!({ "branch": "dev", "outcome": "failed" }),
    );
    publish(&rt, "ci", "result", json!({ "branch": "main" }));
    tokio::time::sleep(Duration::from_millis(150)).await;
    assert_eq!(
        colour(&rt, 0, 0),
        REST,
        "wrong branch, then no field: nothing shown"
    );
    publish(
        &rt,
        "ci",
        "result",
        json!({ "branch": "main", "outcome": "failed" }),
    );
    assert!(within(250, || colour(&rt, 0, 0) == RED).await);
}

// ---------------------------------------------------------------------------
// Unknown, unavailable and the gate
// ---------------------------------------------------------------------------

/// The three statuses are explicit and a rule can ask for each one.
#[tokio::test(flavor = "multi_thread")]
async fn unknown_and_unavailable_are_states_a_rule_can_show() {
    let rt = start(Options::default()).await;
    let hub = rt.state_hub().clone();
    let defs = || {
        vec![
            ObservableDefinition::state("probe", "mode", "Mode", "A mode")
                .one_of(&[("ok", "Fine", None), ("other", "Other", None)]),
        ]
    };
    hub.ensure_provider("probe", defs()).unwrap();
    let mode = StateAddress::new("probe", "mode");
    let (s, p) = new_surface(&rt, "Probe");
    rt.activate_surface(&s, None).unwrap();
    rt.update_control(
        &s,
        &p,
        pad_with(
            0,
            0,
            vec![
                state_rule(
                    "probe",
                    "mode",
                    json!({ "op": "unavailable" }),
                    json!("failed"),
                ),
                state_rule("probe", "mode", json!({ "op": "noValue" }), json!("warn")),
                state_rule("probe", "mode", json!({ "op": "hasValue" }), json!(true)),
            ],
        ),
    )
    .unwrap();

    assert!(within(250, || colour(&rt, 0, 0) == AMBER).await, "unknown");
    hub.set(&mode, "ok".into()).unwrap();
    assert!(within(250, || colour(&rt, 0, 0) == GREEN).await, "known");
    hub.provider_down("probe");
    assert!(
        within(250, || colour(&rt, 0, 0) == RED).await,
        "unavailable"
    );
    assert_eq!(hub.read_address(&mode).value, None, "the old value is gone");
    hub.provider_up("probe");
    assert!(
        within(250, || colour(&rt, 0, 0) == AMBER).await,
        "back, but unknown until it reports again"
    );
    hub.set(&mode, "other".into()).unwrap();
    assert!(within(250, || colour(&rt, 0, 0) == PURPLE).await);
    hub.remove_provider("probe");
    assert!(
        within(250, || colour(&rt, 0, 0) == RED).await,
        "uninstalled"
    );
    assert_eq!(hub.read_address(&mode).status, Status::Unavailable);
}

/// What the gate refuses, and what it lets a saved pad keep.
#[tokio::test(flavor = "multi_thread")]
async fn the_gate_refuses_rules_that_cannot_work_and_keeps_saved_ones_editable() {
    let rt = start(Options::default()).await;
    let (s, p) = new_surface(&rt, "Probe");
    rt.activate_surface(&s, None).unwrap();
    let controls = || {
        rt.snapshot().workspace.surface(&s).unwrap().pages[0]
            .controls
            .len()
    };
    let before = controls();
    let refuse = |rule: Value, needle: &str| {
        let err = rt
            .update_control(&s, &p, pad_with(0, 0, vec![rule]))
            .expect_err("the rule must be refused");
        assert!(err.contains(needle), "`{needle}` not in `{err}`");
    };
    refuse(
        state_rule("nobody", "x", json!({ "op": "hasValue" }), json!(true)),
        "not a state anything offers",
    );
    refuse(
        state_rule(
            "agent-control",
            "slot.status",
            json!({ "op": "hasValue" }),
            json!(true),
        ),
        "Track",
    );
    let mut bad_track = state_rule(
        "agent-control",
        "slot.status",
        json!({ "op": "hasValue" }),
        json!(true),
    );
    bad_track["when"]["state"]["args"] = json!({ "slot": 99 });
    refuse(bad_track, "Track");
    refuse(
        state_rule(
            "demo",
            "alpha",
            json!({ "op": "equals", "value": "bogus" }),
            json!("a"),
        ),
        "not one of the values",
    );
    refuse(
        state_rule(
            "demo",
            "alpha",
            json!({ "op": "atLeast", "value": 3 }),
            json!("a"),
        ),
        "not a number",
    );
    refuse(
        json!({
            "when": { "state": { "provider": "demo", "id": "alpha" }, "is": { "op": "hasValue" } },
            "show": "a", "showFromValue": true,
        }),
        "exactly one look",
    );
    refuse(
        event_rule("ci", "e", "a", json!({ "ms": 10 })),
        "a hold lasts",
    );
    let too_many: Vec<Value> = (0..33)
        .map(|_| event_rule("ci", "e", "a", json!("latch")))
        .collect();
    assert!(rt
        .update_control(&s, &p, pad_with(0, 0, too_many))
        .unwrap_err()
        .contains("at most 32"));
    assert_eq!(controls(), before, "nothing refused was saved");

    // A saved rule stays editable when its provider goes away.
    rt.state_hub()
        .ensure_provider(
            "tmp",
            vec![ObservableDefinition::state("tmp", "x", "X", "x")
                .on_off(("On", None), ("Off", None))],
        )
        .unwrap();
    let mut pad = pad_with(
        0,
        0,
        vec![state_rule(
            "tmp",
            "x",
            json!({ "op": "equals", "value": true }),
            json!("a"),
        )],
    );
    rt.update_control(&s, &p, pad.clone()).unwrap();
    rt.state_hub().remove_provider("tmp");
    pad.label = "Still editable".into();
    rt.update_control(&s, &p, pad).unwrap();
    assert!(
        within(250, || colour(&rt, 0, 0) == REST).await,
        "and it rests: its state is unavailable"
    );
}

// ---------------------------------------------------------------------------
// Agent Control
// ---------------------------------------------------------------------------

/// The states Agent Control keeps in the hub are its own snapshot, and the
/// shipped surface shows exactly them.
#[tokio::test(flavor = "multi_thread")]
async fn agent_control_states_are_its_snapshot_and_the_surface_shows_them() {
    let rt = start(Options::default()).await;
    let idle = |slot: u8| led(&rt, slot, 0) == visual_led(AgentStatus::Idle);
    wait("the four simulated agents are idle", || (0..4).all(idle)).await;
    tokio::time::sleep(Duration::from_millis(300)).await;

    let snapshot = rt.agents().snapshot();
    let hub = rt.state_hub();
    for slot in &snapshot.slots {
        let want = slot.status.map_or(EMPTY, |s| s.as_str());
        let got = hub.read_address(&agent_states::slot_status(slot.index));
        assert_eq!(got.value, Some(Scalar::from(want)), "track {}", slot.index);
        let focused = hub.read_address(&agent_states::slot_focused(slot.index));
        assert_eq!(
            focused.value,
            Some(Scalar::Bool(snapshot.focus == Some(slot.index))),
            "marker {}",
            slot.index
        );
    }
    for kind in CommandKind::ALL {
        let c = hub.read_address(&agent_states::command_state(kind));
        assert_eq!(c.status, Status::Known, "{}", kind.as_str());
    }
    for i in 0..4u8 {
        assert_eq!(led(&rt, i, 0), visual_led(AgentStatus::Idle));
    }
}

fn visual_led(status: AgentStatus) -> launchdeck_lib::devices::Led {
    let v = visual::status_visual(status);
    launchdeck_lib::devices::Led {
        color: v.color,
        animation: v.animation,
    }
}

/// A workspace reset puts the shipped surface back and it is right at once,
/// with nothing replayed to make it so.
#[tokio::test(flavor = "multi_thread")]
async fn a_reset_workspace_is_correct_without_a_replay() {
    let rt = start(Options::default()).await;
    let idle = |slot: u8| led(&rt, slot, 0) == visual_led(AgentStatus::Idle);
    wait("the four simulated agents are idle", || (0..4).all(idle)).await;
    tokio::time::sleep(Duration::from_millis(300)).await;

    let (s, p) = new_surface(&rt, "Scratch");
    rt.update_control(&s, &p, demo_pad(0, 0, "alpha")).unwrap();
    rt.activate_surface(&s, None).unwrap();

    let mut bus = rt.bus().subscribe();
    rt.reset_workspace();
    assert_eq!(rt.state().active_surface_id, "agent-control");
    assert!(within(250, || (0..4).all(idle)).await);
    let mut replay = Vec::new();
    while let Ok(e) = bus.try_recv() {
        if is_projection(&e) {
            replay.push(e.kind);
        }
    }
    assert!(replay.is_empty(), "Agent Control replayed {replay:?}");
}

// ---------------------------------------------------------------------------
// The hub while the runtime is busy
// ---------------------------------------------------------------------------

/// Writers hammer the hub, a provider keeps restarting, and meanwhile the
/// runtime builds snapshots and catalogs, explains a control and edits a pad.
/// Nothing deadlocks and, when the writers stop, the pad shows the last value.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_hub_and_the_runtime_do_not_block_each_other() {
    let rt = start(Options::default()).await;
    let hub = rt.state_hub().clone();
    hub.ensure_provider(
        "probe",
        vec![ObservableDefinition::state("probe", "n", "N", "A number").number()],
    )
    .unwrap();
    let n = StateAddress::new("probe", "n");
    let (s, p) = new_surface(&rt, "Probe");
    rt.activate_surface(&s, None).unwrap();
    let control = pad_with(
        0,
        0,
        vec![
            state_rule(
                "probe",
                "n",
                json!({ "op": "atLeast", "value": 1000 }),
                json!("warn"),
            ),
            state_rule("probe", "n", json!({ "op": "hasValue" }), json!("ok")),
        ],
    );
    let id = control.id.clone();
    rt.update_control(&s, &p, control.clone()).unwrap();

    let stop = Arc::new(AtomicBool::new(false));
    let mut writers = Vec::new();
    for t in 0..3u32 {
        let (hub, n, stop) = (hub.clone(), n.clone(), stop.clone());
        writers.push(std::thread::spawn(move || {
            let mut i = 0u32;
            while !stop.load(Ordering::Relaxed) {
                // Errors are expected while the provider is restarting.
                let _ = hub.set(&n, Scalar::from(f64::from(t * 1000 + i % 500)));
                i += 1;
            }
            i
        }));
    }
    let restarter = {
        let (hub, stop) = (hub.clone(), stop.clone());
        std::thread::spawn(move || {
            while !stop.load(Ordering::Relaxed) {
                hub.provider_down("probe");
                hub.provider_up("probe");
                std::thread::sleep(Duration::from_millis(2));
            }
        })
    };

    let busy = {
        let rt = rt.clone();
        let (s, p, id, control) = (s.clone(), p.clone(), id.clone(), control.clone());
        tokio::task::spawn_blocking(move || {
            for i in 0..150 {
                let snapshot = rt.snapshot();
                assert!(snapshot.catalog.actions.len() > 10);
                let _ = rt.catalog();
                let _ = rt.state();
                rt.explain_control(&s, &p, &id).unwrap();
                if i % 10 == 0 {
                    let mut c = control.clone();
                    c.label = format!("Edit {i}");
                    rt.update_control(&s, &p, c).unwrap();
                }
            }
        })
    };
    tokio::time::timeout(Duration::from_secs(60), busy)
        .await
        .expect("the runtime did not deadlock against the hub")
        .unwrap();

    stop.store(true, Ordering::Relaxed);
    let written: u32 = writers.into_iter().map(|w| w.join().unwrap()).sum();
    restarter.join().unwrap();
    assert!(written > 1000, "the writers really ran ({written} writes)");

    hub.provider_up("probe");
    hub.set(&n, Scalar::from(3.0)).unwrap();
    assert!(within(500, || colour(&rt, 0, 0) == GREEN).await);
    hub.set(&n, Scalar::from(5000.0)).unwrap();
    assert!(within(500, || colour(&rt, 0, 0) == AMBER).await);
}
