//! Characterization of how LaunchDeck represents external state TODAY
//! (workspace schema 3: events routed to `bindings`), written before the
//! state hub exists. See STATE_FEEDBACK_PLAN.md, section 1, probes P1 to P9.
//!
//! Every test asserts what the code does now, on the real runtime, the virtual
//! Launchpad and (for P7) the real compiled Counter plugin. They are the
//! "before" half of the milestone: each one names the behaviour that the state
//! hub replaces, and `tests/state_feedback.rs` holds the "after" half.
//!
//! When a behaviour is fixed, the commit that fixes it rewrites the matching test
//! here in the same diff, so the history shows exactly what changed.

#[path = "support/state_scenario.rs"]
mod scenario;

use std::time::{Duration, Instant};

use launchdeck_lib::actions::definition::Assignment;
use launchdeck_lib::core::model::{Control, ControlPos};
use launchdeck_lib::devices::virtual_device::VIRTUAL_ID;
use launchdeck_lib::events::{Event, EventBus};
use launchdeck_lib::persistence::Store;
use launchdeck_lib::surfaces::default_workspace;
use scenario::*;
use serde_json::{json, Value};

fn rule(event: &str, when: Value, set: &str) -> Value {
    json!({ "event": event, "when": when, "setState": set })
}

fn ci(rules: Vec<Value>) -> Value {
    json!({ "source": "ci", "rules": rules })
}

fn ci_pad(col: u8, row: u8, rules: Vec<Value>) -> Control {
    Pad::at(col, row).binding(ci(rules)).build()
}

fn options() -> Options {
    Options::default()
}

/// P1. A pad shows what the last event said. Restart the application and the
/// pad forgets it: nothing stores the thing the event was about.
///
/// After the milestone: a value that a provider reports again is shown from the
/// first frame, and a value nobody reports is `unknown`, never the old one.
#[tokio::test(flavor = "multi_thread")]
async fn today_a_restart_loses_what_the_pad_was_showing() {
    let dir = tempfile::tempdir().unwrap();
    let rt = start(Options {
        store: Some(Store::new(dir.path())),
        ..options()
    })
    .await;
    let (s, p) = new_surface(&rt, "Probe");
    rt.activate_surface(&s, None).unwrap();
    rt.update_control(
        &s,
        &p,
        ci_pad(0, 0, vec![rule("build.finished", json!([]), "failed")]),
    )
    .unwrap();
    publish(&rt, "ci", "build.finished", json!({}));
    wait("the pad shows failed", || colour(&rt, 0, 0) == RED).await;

    let saved = Store::new(dir.path()).load_or_default(default_workspace);
    let rt2 = start(Options {
        workspace: Some(saved),
        store: Some(Store::new(dir.path())),
        ..options()
    })
    .await;
    assert_eq!(rt2.state().active_surface_id, s, "the same surface returns");
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(colour(&rt2, 0, 0), REST, "the pad rests: the red is gone");
}

/// P2. A pad created after the event never learns what is already true.
///
/// After the milestone: it shows the current value at once (see
/// `a_pad_created_after_the_value_shows_it_at_once` in `state_feedback.rs`).
#[tokio::test(flavor = "multi_thread")]
async fn today_a_pad_created_after_the_event_never_learns_the_value() {
    let rt = start(options()).await;
    let (s, p) = new_surface(&rt, "Probe");
    rt.activate_surface(&s, None).unwrap();
    publish(&rt, "ci", "build.finished", json!({}));
    wait("the event was processed", || seen(&rt, "build.finished")).await;
    rt.update_control(
        &s,
        &p,
        ci_pad(0, 0, vec![rule("build.finished", json!([]), "failed")]),
    )
    .unwrap();
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(colour(&rt, 0, 0), REST, "resting until the next event");
}

/// P3a. What already works and must keep working: a surface or a page that is
/// not on screen follows events, so opening it shows the right look.
#[tokio::test(flavor = "multi_thread")]
async fn today_inactive_surfaces_and_pages_are_correct_when_opened() {
    let rt = start(options()).await;
    let (a, _) = new_surface(&rt, "A");
    let (b, pb) = new_surface(&rt, "B");
    let page2 = rt.add_page(&b, "Second").unwrap();
    let rules = vec![
        rule("build.finished", json!([]), "failed"),
        rule("build.fixed", json!([]), "ok"),
    ];
    rt.update_control(&b, &pb, ci_pad(0, 0, rules.clone()))
        .unwrap();
    rt.update_control(&b, &page2, ci_pad(0, 0, rules)).unwrap();

    rt.activate_surface(&a, None).unwrap();
    publish(&rt, "ci", "build.finished", json!({}));
    wait("processed", || seen(&rt, "build.finished")).await;
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

/// P3b. A device that comes back is repainted from the current looks.
#[tokio::test(flavor = "multi_thread")]
async fn today_a_reconnected_device_is_repainted_from_the_current_state() {
    let rt = start(options()).await;
    let (s, p) = new_surface(&rt, "Probe");
    rt.activate_surface(&s, None).unwrap();
    rt.update_control(
        &s,
        &p,
        ci_pad(
            0,
            0,
            vec![
                rule("build.finished", json!([]), "failed"),
                rule("build.fixed", json!([]), "ok"),
            ],
        ),
    )
    .unwrap();
    publish(&rt, "ci", "build.finished", json!({}));
    wait("red", || colour(&rt, 0, 0) == RED).await;

    rt.disconnect_device(VIRTUAL_ID).unwrap();
    publish(&rt, "ci", "build.fixed", json!({}));
    wait("fixed was processed while unplugged", || {
        seen(&rt, "build.fixed")
    })
    .await;
    rt.connect_device(VIRTUAL_ID).unwrap();
    wait("repainted green after the reconnect", || {
        colour_if_drawn(&rt, 0, 0) == Some(GREEN)
    })
    .await;
}

/// P7. Turn an installed plugin off and on again: its memory is gone, the pad
/// still shows the value it had. A pad can lie for as long as the app runs.
///
/// After the milestone: the plugin's states are `unavailable` while it is off
/// and `unknown` when it returns, until it reports a value again.
#[tokio::test(flavor = "multi_thread")]
async fn today_a_plugin_restart_leaves_the_pad_showing_the_old_value() {
    let rt = start(Options {
        plugins: true,
        ..options()
    })
    .await;
    let (s, p) = new_surface(&rt, "Probe");
    rt.activate_surface(&s, None).unwrap();
    let mut pad = Pad::at(0, 0)
        .binding(json!({
            "source": "example.counter",
            "rules": [rule("counter.changed", json!([]), "$payload.level")],
        }))
        .build();
    pad.states.insert(
        "high".into(),
        launchdeck_lib::core::model::Visual::solid(AMBER),
    );
    rt.update_control(&s, &p, pad).unwrap();
    let increment = Assignment::new("example.counter", "counter.increment", json!({}));
    for _ in 0..5 {
        rt.run_action(increment.clone()).await.unwrap();
    }
    wait("the pad shows high", || colour(&rt, 0, 0) == AMBER).await;

    rt.plugin_host()
        .set_enabled("example.counter", false)
        .unwrap();
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(
        colour(&rt, 0, 0),
        AMBER,
        "the plugin is off, the pad says high"
    );

    rt.plugin_host()
        .set_enabled("example.counter", true)
        .unwrap();
    tokio::time::sleep(Duration::from_millis(300)).await;
    let decrement = Assignment::new("example.counter", "counter.decrement", json!({}));
    let err = rt.run_action(decrement).await.unwrap_err();
    assert!(
        !err.is_empty(),
        "the counter is back at 0, it cannot go lower"
    );
    assert_eq!(colour(&rt, 0, 0), AMBER, "yet the pad still says high");
}

/// P8. The bus is a bounded broadcast: a reader that falls behind loses events,
/// and a state carried only by events is lost with them.
///
/// After the milestone the loss is still true of events (it is logged and
/// counted), but states travel through the hub, which does not lose anything.
#[tokio::test]
async fn today_a_slow_bus_reader_loses_the_oldest_events() {
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

/// P4. With several rules, who wins depends on the order of the rules inside a
/// binding, the order of the bindings, and, across sources, on which event
/// arrived last.
///
/// After the milestone: one ordered list, first match wins, no dependence on
/// arrival order.
#[tokio::test(flavor = "multi_thread")]
async fn today_precedence_depends_on_rule_order_binding_order_and_arrival_order() {
    let rt = start(options()).await;
    let (s, p) = new_surface(&rt, "Probe");
    rt.activate_surface(&s, None).unwrap();
    // (a) two rules of one binding match one event: the first wins.
    rt.update_control(
        &s,
        &p,
        ci_pad(
            0,
            0,
            vec![rule("e", json!([]), "ok"), rule("e", json!([]), "failed")],
        ),
    )
    .unwrap();
    // (b) two bindings on one source match one event: the last wins.
    rt.update_control(
        &s,
        &p,
        Pad::at(1, 0)
            .binding(ci(vec![rule("e", json!([]), "ok")]))
            .binding(ci(vec![rule("e", json!([]), "failed")]))
            .build(),
    )
    .unwrap();
    // (c) two sources: whichever event arrived last wins.
    rt.update_control(
        &s,
        &p,
        Pad::at(2, 0)
            .binding(ci(vec![rule("e", json!([]), "ok")]))
            .binding(json!({ "source": "other", "rules": [rule("e", json!([]), "failed")] }))
            .build(),
    )
    .unwrap();
    publish(&rt, "ci", "e", json!({}));
    wait("a: first rule", || colour(&rt, 0, 0) == GREEN).await;
    wait("b: last binding", || colour(&rt, 1, 0) == RED).await;
    wait("c: ci arrived first", || colour(&rt, 2, 0) == GREEN).await;
    publish(&rt, "other", "e", json!({}));
    wait("c: other arrived second", || colour(&rt, 2, 0) == RED).await;
    publish(&rt, "ci", "e", json!({}));
    wait("c: ci arrived third", || colour(&rt, 2, 0) == GREEN).await;
}

/// P5. Equality is JSON structural equality, so a rule written with `1` never
/// matches a plugin that sends `1.0`.
///
/// After the milestone: numbers compare as numbers.
#[tokio::test(flavor = "multi_thread")]
async fn today_numeric_conditions_are_structural() {
    assert_ne!(
        json!(1),
        serde_json::from_str::<Value>("1.0").unwrap(),
        "serde_json keeps 1 and 1.0 apart"
    );
    let rt = start(options()).await;
    let (s, p) = new_surface(&rt, "Probe");
    rt.activate_surface(&s, None).unwrap();
    rt.update_control(
        &s,
        &p,
        ci_pad(
            0,
            0,
            vec![rule("level", json!([{ "field": "n", "equals": 1 }]), "ok")],
        ),
    )
    .unwrap();
    publish(&rt, "ci", "level", json!({ "n": 1.0 }));
    wait("processed", || seen(&rt, "level")).await;
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(colour(&rt, 0, 0), REST, "1 did not match 1.0");
    publish(&rt, "ci", "level", json!({ "n": 1 }));
    wait("the integer matches", || colour(&rt, 0, 0) == GREEN).await;
}

/// P6. A rule that names a look the pad does not have is accepted. The state is
/// recorded and the LED shows the resting look. Nothing says why.
///
/// After the milestone: the same pad is explained as "no look named x".
#[tokio::test(flavor = "multi_thread")]
async fn today_a_missing_look_is_silent() {
    let rt = start(options()).await;
    let (s, p) = new_surface(&rt, "Probe");
    rt.activate_surface(&s, None).unwrap();
    rt.update_control(
        &s,
        &p,
        ci_pad(0, 0, vec![rule("e", json!([]), "no-such-look")]),
    )
    .unwrap();
    publish(&rt, "ci", "e", json!({}));
    wait("the state is recorded", || {
        rt.state()
            .control_states
            .values()
            .any(|v| v == "no-such-look")
    })
    .await;
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(colour(&rt, 0, 0), REST, "the LED rests and nobody is told");
}

/// P9. Every event is matched against every control of every surface. This is a
/// measurement, not a gate: it prints the cost so the index can be compared
/// with it. Run with `cargo test --test state_characterization -- --ignored --nocapture`.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "benchmark: prints timings"]
async fn today_routing_cost_grows_with_every_control_in_the_workspace() {
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
                            ci_pad(i % 8, i / 8, vec![rule("e", json!([]), "ok")])
                        })
                        .collect(),
                })
                .collect();
            ws.surfaces.push(s);
        }
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
        let n = 5000;
        let t = Instant::now();
        for i in 0..n {
            publish(&rt, "noise", "tick", json!({ "i": i }));
        }
        publish(&rt, "noise", "done", json!({}));
        wait("all routed", || seen(&rt, "done")).await;
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
