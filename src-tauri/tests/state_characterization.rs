//! The nine probes of STATE_FEEDBACK_PLAN.md (section 1), as they behave now.
//!
//! Phase 0 committed this file as the "before" half: it asserted what the
//! schema 3 runtime did (events routed to `bindings`, the last event the only
//! truth). The state core changes each of those behaviours, so each test below
//! was rewritten in the same diff as the change, keeps its probe number, and
//! says in its comment what it used to assert. Everything runs on the real
//! runtime, the real virtual Launchpad, the real Demo plugin and, for P7, the
//! real compiled Counter plugin.

#[path = "support/state_scenario.rs"]
mod scenario;

use std::time::{Duration, Instant};

use launchdeck_lib::actions::definition::Assignment;
use launchdeck_lib::core::model::{ControlPos, Visual};
use launchdeck_lib::devices::virtual_device::VIRTUAL_ID;
use launchdeck_lib::events::{Event, EventBus};
use launchdeck_lib::persistence::Store;
use launchdeck_lib::state::hub::Status;
use launchdeck_lib::state::{ObservableDefinition, Scalar, StateAddress};
use launchdeck_lib::surfaces::default_workspace;
use scenario::*;
use serde_json::{json, Value};

fn options() -> Options {
    Options::default()
}

/// A state rule on a Demo agent: when it holds `value`, show `look`.
fn when_demo(agent: &str, value: &str, look: &str) -> Value {
    json!({
        "when": { "state": { "provider": "demo", "id": agent },
                  "is": { "op": "equals", "value": value } },
        "show": look,
    })
}

/// An event rule that keeps its look until the next event.
fn on_event(source: &str, kind: &str, look: &str) -> Value {
    json!({
        "when": { "event": { "provider": source, "id": kind } },
        "show": look,
        "hold": "latch",
    })
}

fn pad_with(col: u8, row: u8, rules: Vec<Value>) -> launchdeck_lib::core::model::Control {
    let mut pad = Pad::at(col, row);
    for r in rules {
        pad = pad.rule(r);
    }
    pad.build()
}

fn alpha() -> StateAddress {
    StateAddress::new("demo", "alpha")
}

/// P1. Used to assert: restart the application and the pad forgets what an
/// event said, because nothing stored the thing the event was about.
///
/// Now: the file holds the rules and never a value. After a restart the Demo
/// provider has not reported, so the state is `unknown` and the pad rests; it
/// does not show the value it had. The moment the provider reports again, the
/// pad that already existed shows it.
#[tokio::test(flavor = "multi_thread")]
async fn p1_a_restart_never_shows_an_old_value_and_shows_the_new_one_at_once() {
    let dir = tempfile::tempdir().unwrap();
    let rt = start(Options {
        store: Some(Store::new(dir.path())),
        ..options()
    })
    .await;
    let (s, p) = new_surface(&rt, "Probe");
    rt.activate_surface(&s, None).unwrap();
    rt.update_control(&s, &p, demo_pad(0, 0, "alpha")).unwrap();
    demo(&rt, "startTask", "alpha").await;
    wait("the pad shows running", || colour(&rt, 0, 0) == GREEN).await;

    // The file holds the rules. A value the provider reported (`running`) is
    // not a rule, a look or a label, so it appears nowhere as text.
    let file = std::fs::read_to_string(dir.path().join("workspace.json")).unwrap();
    assert!(file.contains("\"feedback\""), "the rules are saved");
    fn texts<'a>(v: &'a Value, out: &mut Vec<&'a str>) {
        match v {
            Value::String(s) => out.push(s),
            Value::Array(a) => a.iter().for_each(|x| texts(x, out)),
            Value::Object(o) => o.values().for_each(|x| texts(x, out)),
            _ => {}
        }
    }
    let document: Value = serde_json::from_str(&file).unwrap();
    let mut all = Vec::new();
    texts(&document, &mut all);
    assert!(
        !all.contains(&"running"),
        "no state value is saved with the workspace"
    );
    assert!(!file.contains("updatedAt"));

    let saved = Store::new(dir.path()).load_or_default(default_workspace);
    let rt2 = start(Options {
        workspace: Some(saved),
        store: Some(Store::new(dir.path())),
        ..options()
    })
    .await;
    assert_eq!(rt2.state().active_surface_id, s, "the same surface returns");
    wait("the pad is drawn", || colour(&rt2, 0, 0) == REST).await;
    assert_eq!(
        rt2.state_hub().read_address(&alpha()).status,
        Status::Unknown,
        "the restarted provider has reported nothing yet"
    );
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(
        colour(&rt2, 0, 0),
        REST,
        "the red/green of the last run is gone"
    );

    demo(&rt2, "startTask", "alpha").await;
    assert!(within(250, || colour(&rt2, 0, 0) == GREEN).await);
}

/// P2. Used to assert: a pad created after the event never learns it. That is
/// still true of an event, which is a moment that passed. It is no longer true
/// of a state, which is how things are.
#[tokio::test(flavor = "multi_thread")]
async fn p2_a_pad_created_late_misses_an_event_but_learns_a_state() {
    let rt = start(options()).await;
    let (s, p) = new_surface(&rt, "Probe");
    rt.activate_surface(&s, None).unwrap();
    publish(&rt, "ci", "build.finished", json!({}));
    demo(&rt, "failTask", "alpha").await;
    wait("both went by", || {
        seen(&rt, "build.finished") && seen(&rt, "plugin.state.changed")
    })
    .await;

    rt.update_control(
        &s,
        &p,
        pad_with(0, 0, vec![on_event("ci", "build.finished", "failed")]),
    )
    .unwrap();
    rt.update_control(&s, &p, demo_pad(1, 0, "alpha")).unwrap();

    assert!(
        within(250, || colour(&rt, 1, 0) == RED).await,
        "the state is shown at once"
    );
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(
        colour(&rt, 0, 0),
        REST,
        "the event is not state: a pad that did not exist then rests"
    );
}

/// P3a. Already right before and still right: a surface or a page that is not
/// on screen is correct the moment it is opened, now without any event.
#[tokio::test(flavor = "multi_thread")]
async fn p3a_inactive_surfaces_and_pages_are_correct_when_opened() {
    let rt = start(options()).await;
    let (a, _) = new_surface(&rt, "A");
    let (b, pb) = new_surface(&rt, "B");
    let page2 = rt.add_page(&b, "Second").unwrap();
    rt.update_control(&b, &pb, demo_pad(0, 0, "alpha")).unwrap();
    rt.update_control(&b, &page2, demo_pad(0, 0, "alpha"))
        .unwrap();

    rt.activate_surface(&a, None).unwrap();
    demo(&rt, "failTask", "alpha").await;
    wait("processed", || seen(&rt, "plugin.state.changed")).await;
    rt.activate_surface(&b, None).unwrap();
    wait("the other surface paints failed at once", || {
        colour(&rt, 0, 0) == RED
    })
    .await;

    rt.activate_page(&page2).unwrap();
    wait("a page that was never shown paints failed at once", || {
        colour(&rt, 0, 0) == RED
    })
    .await;
}

/// P3b. A device that comes back is repainted from the current state, not from
/// the looks it had when it left.
#[tokio::test(flavor = "multi_thread")]
async fn p3b_a_reconnected_device_is_repainted_from_the_current_state() {
    let rt = start(options()).await;
    let (s, p) = new_surface(&rt, "Probe");
    rt.activate_surface(&s, None).unwrap();
    rt.update_control(&s, &p, demo_pad(0, 0, "alpha")).unwrap();
    demo(&rt, "failTask", "alpha").await;
    wait("red", || colour(&rt, 0, 0) == RED).await;

    rt.disconnect_device(VIRTUAL_ID).unwrap();
    demo(&rt, "startTask", "alpha").await;
    wait("running was processed while unplugged", || {
        rt.state_hub()
            .read_address(&alpha())
            .value
            .is_some_and(|v| v.same_as(&Scalar::from("running")))
    })
    .await;
    rt.connect_device(VIRTUAL_ID).unwrap();
    wait("repainted green after the reconnect", || {
        colour_if_drawn(&rt, 0, 0) == Some(GREEN)
    })
    .await;
}

/// P7. Used to assert: turn an installed plugin off and on again and its memory
/// is gone while the pad still shows the value it had, for as long as the app
/// runs.
///
/// Now: the plugin's states are `unavailable` while it is off and `unknown`
/// when it returns, until it reports again. Nothing it said is kept.
///
/// The Counter plugin declares no states yet (manifest schema 3 is a later
/// milestone), so the test declares one on its provider and plays the part of
/// the plugin: what matters here is the lifecycle the runtime applies.
#[tokio::test(flavor = "multi_thread")]
async fn p7_a_plugin_restart_never_leaves_the_pad_showing_the_old_value() {
    let rt = start(Options {
        plugins: true,
        ..options()
    })
    .await;
    let level = StateAddress::new("example.counter", "level");
    rt.state_hub()
        .declare(
            "example.counter",
            vec![ObservableDefinition::state(
                "example.counter",
                "level",
                "Counter level",
                "The level",
            )
            .one_of(&[
                ("zero", "Zero", None),
                ("low", "Low", None),
                ("high", "High", None),
            ])],
        )
        .unwrap();
    let (s, p) = new_surface(&rt, "Probe");
    rt.activate_surface(&s, None).unwrap();
    let mut pad = pad_with(
        0,
        0,
        vec![json!({
            "when": { "state": { "provider": "example.counter", "id": "level" },
                      "is": { "op": "hasValue" } },
            "showFromValue": true,
        })],
    );
    pad.states.insert("high".into(), Visual::solid(AMBER));
    rt.update_control(&s, &p, pad).unwrap();

    let increment = Assignment::new("example.counter", "counter.increment", json!({}));
    for _ in 0..5 {
        rt.run_action(increment.clone()).await.unwrap();
    }
    rt.state_hub().set(&level, "high".into()).unwrap();
    wait("the pad shows high", || colour(&rt, 0, 0) == AMBER).await;

    rt.plugin_host()
        .set_enabled("example.counter", false)
        .unwrap();
    assert!(
        within(250, || colour(&rt, 0, 0) == REST).await,
        "the plugin is off: the pad no longer says high"
    );
    assert_eq!(
        rt.state_hub().read_address(&level).status,
        Status::Unavailable
    );
    assert!(
        rt.state_hub().set(&level, "high".into()).is_err(),
        "a provider that is off cannot report"
    );

    rt.plugin_host()
        .set_enabled("example.counter", true)
        .unwrap();
    let observed = rt.state_hub().read_address(&level);
    assert_eq!(observed.status, Status::Unknown, "back, but silent");
    assert_eq!(observed.value, None, "nothing it said before is kept");
    tokio::time::sleep(Duration::from_millis(250)).await;
    let decrement = Assignment::new("example.counter", "counter.decrement", json!({}));
    assert!(
        !rt.run_action(decrement).await.unwrap_err().is_empty(),
        "the counter is back at 0, it cannot go lower"
    );
    assert_eq!(colour(&rt, 0, 0), REST, "and the pad no longer says high");

    rt.state_hub().set(&level, "high".into()).unwrap();
    assert!(within(250, || colour(&rt, 0, 0) == AMBER).await);
    // A reload is a restart too.
    rt.plugin_host().reload("example.counter").unwrap();
    assert_eq!(
        rt.state_hub().read_address(&level).status,
        Status::Unknown,
        "a reloaded plugin starts again from unknown"
    );
    assert!(within(250, || colour(&rt, 0, 0) == REST).await);
}

/// P8. Still true: the bus is a bounded broadcast, so a reader that falls
/// behind loses events. What changed is that a state no longer travels on it.
#[tokio::test]
async fn p8_a_slow_bus_reader_still_loses_the_oldest_events() {
    let bus = EventBus::new();
    let mut rx = bus.subscribe();
    for i in 0..2000 {
        bus.publish(Event::new("ci", "tick", json!({ "i": i })));
    }
    assert!(matches!(
        rx.try_recv(),
        Err(tokio::sync::broadcast::error::TryRecvError::Lagged(976))
    ));
    assert_eq!(rx.try_recv().unwrap().payload["i"], 976);
}

/// P8, the other half: a flood of events cannot change a state. The runtime's
/// own reader may fall behind (it counts what it missed); the values in the hub
/// and the pads that show them are untouched.
#[tokio::test(flavor = "multi_thread")]
async fn p8_a_flood_of_events_cannot_change_a_state() {
    let rt = start(options()).await;
    let (s, p) = new_surface(&rt, "Probe");
    rt.activate_surface(&s, None).unwrap();
    rt.update_control(&s, &p, demo_pad(0, 0, "alpha")).unwrap();
    demo(&rt, "startTask", "alpha").await;
    wait("running", || colour(&rt, 0, 0) == GREEN).await;

    for i in 0..5000 {
        publish(&rt, "noise", "tick", json!({ "i": i }));
    }
    publish(&rt, "noise", "done", json!({}));
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(colour(&rt, 0, 0), GREEN);
    assert_eq!(
        rt.state_hub().read_address(&alpha()).value,
        Some(Scalar::from("running"))
    );
    // Whatever the reader missed was counted, not hidden.
    let stats = rt.feedback_stats();
    eprintln!(
        "P8 flood: the event loop missed {} events",
        stats.bus_lagged
    );
}

/// P4. Used to assert: who wins depended on the order of the rules inside a
/// binding, the order of the bindings and, across sources, on which event
/// arrived last.
///
/// Now: one ordered list, the first rule that is true wins, and the answer is a
/// function of the saved order and the values, never of how they arrived.
#[tokio::test(flavor = "multi_thread")]
async fn p4_the_first_true_rule_wins_and_the_saved_order_is_the_only_priority() {
    let rt = start(options()).await;
    let (s, p) = new_surface(&rt, "Probe");
    rt.activate_surface(&s, None).unwrap();
    let ok = when_demo("alpha", "running", "a");
    let other = when_demo("alpha", "running", "b");
    rt.update_control(&s, &p, pad_with(0, 0, vec![ok.clone(), other.clone()]))
        .unwrap();
    // The same two rules, the other way round.
    rt.update_control(&s, &p, pad_with(1, 0, vec![other, ok]))
        .unwrap();
    // Two event rules for one event: the first one wins.
    rt.update_control(
        &s,
        &p,
        pad_with(
            2,
            0,
            vec![on_event("ci", "e", "ok"), on_event("ci", "e", "failed")],
        ),
    )
    .unwrap();

    demo(&rt, "startTask", "alpha").await;
    publish(&rt, "ci", "e", json!({}));
    wait("first rule", || colour(&rt, 0, 0) == AMBER).await;
    wait("reversed list, reversed answer", || {
        colour(&rt, 1, 0) == PURPLE
    })
    .await;
    wait("first event rule", || colour(&rt, 2, 0) == GREEN).await;
}

/// P5. Used to assert: equality was JSON structural equality, so a rule written
/// with `1` never matched a plugin that sent `1.0`.
///
/// Now: numbers compare as numbers, in event conditions and in state tests, and
/// the gate refuses a comparison that cannot be right (a text with a number).
#[tokio::test(flavor = "multi_thread")]
async fn p5_numbers_compare_as_numbers() {
    assert_ne!(
        json!(1),
        serde_json::from_str::<Value>("1.0").unwrap(),
        "serde_json keeps 1 and 1.0 apart, which is why the evaluator cannot use it"
    );
    let rt = start(options()).await;
    let (s, p) = new_surface(&rt, "Probe");
    rt.activate_surface(&s, None).unwrap();

    // An event condition written with 1 matches a payload that says 1.0.
    rt.update_control(
        &s,
        &p,
        pad_with(
            0,
            0,
            vec![json!({
                "when": { "event": { "provider": "ci", "id": "level" },
                          "where": [{ "field": "n", "equals": 1 }] },
                "show": "ok",
                "hold": "latch",
            })],
        ),
    )
    .unwrap();
    publish(&rt, "ci", "level", json!({ "n": 1.0 }));
    wait("1.0 matched 1", || colour(&rt, 0, 0) == GREEN).await;

    // A state test on a number.
    rt.state_hub()
        .ensure_provider(
            "probe",
            vec![ObservableDefinition::state("probe", "level", "Level", "A number").number()],
        )
        .unwrap();
    let level = StateAddress::new("probe", "level");
    let num = |op: &str, v: f64, look: &str| {
        json!({
            "when": { "state": { "provider": "probe", "id": "level" },
                      "is": { "op": op, "value": v } },
            "show": look,
        })
    };
    rt.update_control(
        &s,
        &p,
        pad_with(
            1,
            0,
            vec![
                num("equals", 2.0, "ok"),
                num("atLeast", 3.0, "warn"),
                num("lessThan", 1.0, "failed"),
            ],
        ),
    )
    .unwrap();
    let set = |v: Value| {
        rt.state_hub()
            .set(&level, Scalar::from_json(&v).unwrap())
            .unwrap();
    };
    set(json!(2));
    assert!(
        within(250, || colour(&rt, 1, 0) == GREEN).await,
        "2 equals 2.0"
    );
    set(json!(3.0));
    assert!(
        within(250, || colour(&rt, 1, 0) == AMBER).await,
        "3.0 is at least 3"
    );
    set(json!(0.5));
    assert!(
        within(250, || colour(&rt, 1, 0) == RED).await,
        "0.5 is below 1"
    );
    set(json!(1.5));
    assert!(
        within(250, || colour(&rt, 1, 0) == REST).await,
        "no rule is true"
    );

    // A text is never a number.
    let bad = rt.update_control(
        &s,
        &p,
        pad_with(
            2,
            0,
            vec![json!({
                "when": { "state": { "provider": "probe", "id": "level" },
                          "is": { "op": "equals", "value": "2" } },
                "show": "ok",
            })],
        ),
    );
    let message = bad.unwrap_err();
    assert!(message.contains("cannot be compared"), "{message}");
}

/// P6. Used to assert: a rule that names a look the pad does not have was
/// accepted, the LED rested, and nobody was told.
///
/// Now: the LED still rests (a renderer never invents a colour), but the pad is
/// explained: which rule decided, which look it named, that the look is missing.
#[tokio::test(flavor = "multi_thread")]
async fn p6_a_missing_look_is_detectable() {
    use launchdeck_lib::state::eval::Diagnostic;

    let rt = start(options()).await;
    let (s, p) = new_surface(&rt, "Probe");
    rt.activate_surface(&s, None).unwrap();
    let control = pad_with(0, 0, vec![when_demo("alpha", "running", "no-such-look")]);
    let id = control.id.clone();
    rt.update_control(&s, &p, control).unwrap();
    demo(&rt, "startTask", "alpha").await;
    wait("the rule decided", || {
        rt.explain_control(&s, &p, &id).unwrap().shown.as_deref() == Some("no-such-look")
    })
    .await;
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(colour(&rt, 0, 0), REST, "the LED rests");

    let why = rt.explain_control(&s, &p, &id).unwrap();
    assert_eq!(why.layer, "state");
    assert_eq!(why.state_rules.result.winner, Some(0));
    assert_eq!(
        why.state_rules.result.diagnostics,
        vec![Diagnostic::MissingLook {
            rule: 0,
            look: "no-such-look".into()
        }]
    );
    assert_eq!(why.state_rules.rules[0].look_exists, Some(false));
}

/// P9. Every event used to be matched against every control of every surface.
/// This is a measurement, not a gate: it prints the cost of an unrelated event
/// on the starter workspace and on one 25 times larger. With the index the two
/// should be close; the deterministic proof that nothing unrelated is evaluated
/// is `an_unrelated_change_evaluates_nothing` in `state_hub.rs`.
///
/// The end of the run is read from the runtime's own counter (one pad listens
/// to the last event), so the measurement does not poll the workspace.
///
/// Run with `cargo test --test state_characterization -- --ignored --nocapture`.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "benchmark: prints timings"]
async fn p9_routing_cost_does_not_grow_with_the_workspace() {
    for (surfaces, label) in [
        (0usize, "starter workspace"),
        (10, "starter + 10x4x64 pads"),
    ] {
        let mut ws = default_workspace();
        let mut synthetic = 0usize;
        for n in 0..surfaces {
            let mut s = launchdeck_lib::surfaces::blank_surface(&format!("syn{n}"), "Syn");
            s.pages = (0..4)
                .map(|pi| launchdeck_lib::core::model::Page {
                    id: format!("p{pi}"),
                    name: format!("P{pi}"),
                    controls: (0..64u8)
                        .map(|i| {
                            synthetic += 1;
                            pad_with(i % 8, i / 8, vec![on_event("ci", "e", "ok")])
                        })
                        .collect(),
                })
                .collect();
            ws.surfaces.push(s);
        }
        let watcher = pad_with(7, 7, vec![on_event("noise", "done", "ok")]);
        ws.surfaces[0].pages[0]
            .controls
            .retain(|c| c.pos != watcher.pos);
        ws.surfaces[0].pages[0].controls.push(watcher);
        let total: usize = ws
            .surfaces
            .iter()
            .flat_map(|s| &s.pages)
            .map(|p| p.controls.len())
            .sum();
        let rt = start(Options {
            workspace: Some(ws),
            ..options()
        })
        .await;
        tokio::time::sleep(Duration::from_millis(300)).await;
        let n = 50_000;
        let t = Instant::now();
        for i in 0..n {
            publish(&rt, "noise", "tick", json!({ "i": i }));
        }
        publish(&rt, "noise", "done", json!({}));
        while rt.feedback_stats().events_routed == 0 {
            tokio::time::sleep(Duration::from_millis(2)).await;
        }
        let elapsed = t.elapsed();
        eprintln!(
            "P9 {label}: {total} controls ({synthetic} synthetic): {n} unrelated events in {elapsed:?} = {:.1} us/event",
            elapsed.as_micros() as f64 / f64::from(n)
        );
    }
}

#[test]
fn the_scenario_pad_builder_makes_a_valid_control() {
    let c = Pad::at(3, 4).build();
    assert_eq!(c.pos, ControlPos::grid(3, 4));
}
