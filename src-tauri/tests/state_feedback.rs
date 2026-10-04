//! What LaunchDeck must do with external state (STATE_FEEDBACK_PLAN.md,
//! sections 8, 9 and 18): the "after" half of `state_characterization.rs`.
//!
//! The mandatory acceptance test is the first one: a provider publishes its
//! current state, a pad that did not exist yet is created later, no event
//! happens, and the virtual Launchpad shows the right look at once. Everything
//! runs on the real runtime, the real Demo plugin and Agent Control, and the
//! real virtual device. Nothing is mocked.
//!
//! Phase 0 status: every test that describes new behaviour is `#[ignore]`d with
//! the reason "red until phase 1". Run them with `cargo test --test
//! state_feedback -- --include-ignored` to watch them fail on the schema 3
//! implementation; the phase 1 commit that implements the state core removes
//! the attributes in the same diff.
//!
//! Pads are described in both schemas (see `support/state_scenario.rs`), so
//! the tests that must hold in both worlds (inactive surfaces, pages, a device
//! that reconnects) pass before and after, and the ones that describe the new
//! behaviour fail before.

#[path = "support/state_scenario.rs"]
mod scenario;

use launchdeck_lib::agents::service::is_projection;
use launchdeck_lib::agents::visual::{self};
use launchdeck_lib::devices::virtual_device::VIRTUAL_ID;
use launchdeck_lib::devices::Led;
use scenario::*;

/// Every event published after `rx` was created that a provider could have
/// sent: anything that is not the runtime's own navigation and device news.
fn provider_events(
    rx: &mut tokio::sync::broadcast::Receiver<launchdeck_lib::events::Event>,
) -> Vec<String> {
    let mut out = Vec::new();
    while let Ok(e) = rx.try_recv() {
        if e.source != "runtime" && e.source != "device" {
            out.push(format!("{}/{}", e.source, e.kind));
        }
    }
    out
}

/// THE ACCEPTANCE TEST.
///
/// ```text
/// provider publishes current state
///   -> no new event occurs
///   -> a pad is created afterwards
///   -> its feedback is evaluated from the state that exists
///   -> the virtual device shows the right look straight away
/// ```
#[tokio::test(flavor = "multi_thread")]
#[ignore = "red until phase 1: the feedback model does not exist yet"]
async fn acceptance_a_pad_created_after_the_value_shows_it_with_no_new_event() {
    let rt = start(Options::default()).await;
    demo(&rt, "startTask", "alpha").await;
    wait("the provider event went by", || {
        seen(&rt, "plugin.state.changed")
    })
    .await;

    // From here on nothing else may be published by a provider.
    let mut bus = rt.bus().subscribe();
    let (s, p) = new_surface(&rt, "Probe");
    rt.update_control(&s, &p, demo_pad(0, 0, "alpha")).unwrap();
    rt.activate_surface(&s, None).unwrap();

    assert!(
        within(250, || colour(&rt, 0, 0) == GREEN).await,
        "the pad shows running within a render tick or two, but shows {:?}",
        colour(&rt, 0, 0)
    );
    assert_eq!(
        provider_events(&mut bus),
        Vec::<String>::new(),
        "no provider published anything after the pad was created"
    );
}

/// The same, when the pad existed but its feedback was added afterwards.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "red until phase 1: the feedback model does not exist yet"]
async fn a_pad_edited_after_the_value_shows_it_at_once() {
    let rt = start(Options::default()).await;
    let (s, p) = new_surface(&rt, "Probe");
    rt.activate_surface(&s, None).unwrap();
    rt.update_control(&s, &p, Pad::at(0, 0).build()).unwrap();
    wait("the plain pad is drawn", || colour(&rt, 0, 0) == REST).await;
    demo(&rt, "failTask", "beta").await;
    wait("the event went by", || seen(&rt, "plugin.state.changed")).await;
    assert_eq!(colour(&rt, 0, 0), REST);

    rt.update_control(&s, &p, demo_pad(0, 0, "beta")).unwrap();
    assert!(
        within(250, || colour(&rt, 0, 0) == RED).await,
        "shows {:?}",
        colour(&rt, 0, 0)
    );
}

/// Variant: a page that was never shown. The value is known before the pad
/// that mirrors it exists, and the person opens the page afterwards.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "red until phase 1: the feedback model does not exist yet"]
async fn acceptance_variant_a_page_switch_needs_no_event() {
    let rt = start(Options::default()).await;
    let (s, home) = new_surface(&rt, "Probe");
    let second = rt.add_page(&s, "Second").unwrap();
    rt.activate_surface(&s, None).unwrap();
    demo(&rt, "requestApproval", "gamma").await;
    wait("the event went by", || seen(&rt, "plugin.state.changed")).await;
    rt.update_control(&s, &second, demo_pad(0, 0, "gamma"))
        .unwrap();
    assert_eq!(rt.state().active_page_id, home);

    let mut bus = rt.bus().subscribe();
    rt.activate_page(&second).unwrap();
    assert!(
        within(250, || colour(&rt, 0, 0) == WHITE).await,
        "shows {:?}",
        colour(&rt, 0, 0)
    );
    assert_eq!(provider_events(&mut bus), Vec::<String>::new());
}

/// Variant: a device that is unplugged while the value changes and the pad is
/// created is painted with the value as it is now when it comes back.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "red until phase 1: the feedback model does not exist yet"]
async fn acceptance_variant_a_device_reconnect_needs_no_event() {
    let rt = start(Options::default()).await;
    let (s, p) = new_surface(&rt, "Probe");
    rt.activate_surface(&s, None).unwrap();

    rt.disconnect_device(VIRTUAL_ID).unwrap();
    demo(&rt, "failTask", "alpha").await;
    wait("the event went by", || seen(&rt, "plugin.state.changed")).await;
    rt.update_control(&s, &p, demo_pad(0, 0, "alpha")).unwrap();

    let mut bus = rt.bus().subscribe();
    rt.connect_device(VIRTUAL_ID).unwrap();
    assert!(
        within(250, || colour_if_drawn(&rt, 0, 0) == Some(RED)).await,
        "shows {:?}",
        colour_if_drawn(&rt, 0, 0)
    );
    assert_eq!(provider_events(&mut bus), Vec::<String>::new());
}

/// Behaviour that is already right and must stay right: a surface that is not
/// on screen is correct the moment it is opened.
#[tokio::test(flavor = "multi_thread")]
async fn inactive_surfaces_stay_correct() {
    let rt = start(Options::default()).await;
    let (a, _) = new_surface(&rt, "A");
    let (b, pb) = new_surface(&rt, "B");
    rt.update_control(&b, &pb, demo_pad(0, 0, "alpha")).unwrap();
    rt.activate_surface(&a, None).unwrap();
    demo(&rt, "startTask", "alpha").await;
    wait("the event went by", || seen(&rt, "plugin.state.changed")).await;
    rt.activate_surface(&b, None).unwrap();
    assert!(within(250, || colour(&rt, 0, 0) == GREEN).await);
}

/// Agent Control: the shipped surface is correct after leaving it and coming
/// back, with no replay of its state by Agent Control. This is the
/// `agents.republish()` workaround this milestone removes from navigation.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "red until phase 1: the feedback model does not exist yet"]
async fn agent_control_surface_is_correct_after_a_switch_without_a_replay() {
    let rt = start(Options::default()).await;
    assert_eq!(rt.state().active_surface_id, "agent-control");
    // Atlas, Beacon, Cinder and Delta: four simulated agents, one per track.
    let idle = |slot: u8| {
        led(&rt, slot, 0)
            == Led {
                color: visual::status_visual(launchdeck_lib::agents::model::Status::Idle).color,
                animation: visual::status_visual(launchdeck_lib::agents::model::Status::Idle)
                    .animation,
            }
    };
    wait("the four tracks are idle", || (0..4).all(idle)).await;

    // Let the world go quiet, then leave and come back.
    tokio::time::sleep(std::time::Duration::from_millis(400)).await;
    let mut bus = rt.bus().subscribe();
    rt.activate_surface("development", None).unwrap();
    wait("development is drawn", || {
        rt.state().active_surface_id == "development"
    })
    .await;
    rt.activate_surface("agent-control", None).unwrap();

    assert!(
        within(250, || (0..4).all(idle)).await,
        "the tracks are idle again at once"
    );
    let mut replayed = Vec::new();
    while let Ok(e) = bus.try_recv() {
        if is_projection(&e) {
            replayed.push(format!("{}/{}", e.source, e.kind));
        }
    }
    assert_eq!(
        replayed,
        Vec::<String>::new(),
        "Agent Control was not asked to replay its state to repaint a surface"
    );
}

/// Precedence: one ordered list, first match wins, and the result is a
/// function of the current values, not of the order they arrived in.
///
/// Rule 1: alpha is running, show `a`. Rule 2: beta is running, show `b`.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "red until phase 1: the feedback model does not exist yet"]
async fn the_first_matching_rule_wins_whatever_arrived_last() {
    let rt = start(Options::default()).await;
    let (s, p) = new_surface(&rt, "Probe");
    rt.activate_surface(&s, None).unwrap();
    rt.update_control(
        &s,
        &p,
        demo_switch(0, 0, &[("alpha", "running", "a"), ("beta", "running", "b")]),
    )
    .unwrap();

    demo(&rt, "startTask", "beta").await;
    wait("only beta is running: rule 2", || {
        colour(&rt, 0, 0) == PURPLE
    })
    .await;
    demo(&rt, "startTask", "alpha").await;
    wait("both are running: rule 1 wins", || {
        colour(&rt, 0, 0) == AMBER
    })
    .await;

    // Alpha stops. It does not "send an event that undoes the look": the value
    // changed, so the rule no longer holds and rule 2 applies.
    demo(&rt, "stopTask", "alpha").await;
    assert!(
        within(300, || colour(&rt, 0, 0) == PURPLE).await,
        "rule 2 applies again, but the pad shows {:?}",
        colour(&rt, 0, 0)
    );

    // And in the other order of arrival the answer is the same.
    demo(&rt, "stopTask", "beta").await;
    assert!(within(300, || colour(&rt, 0, 0) == REST).await);
    demo(&rt, "startTask", "alpha").await;
    demo(&rt, "startTask", "beta").await;
    assert!(within(300, || colour(&rt, 0, 0) == AMBER).await);
}

/// A rule for a state nobody has set yet is not an error, and a pad whose
/// rules do not match rests (today the previous look would stay forever).
#[tokio::test(flavor = "multi_thread")]
#[ignore = "red until phase 1: the feedback model does not exist yet"]
async fn when_no_rule_is_true_the_pad_rests() {
    let rt = start(Options::default()).await;
    let (s, p) = new_surface(&rt, "Probe");
    rt.activate_surface(&s, None).unwrap();
    rt.update_control(&s, &p, demo_switch(0, 0, &[("alpha", "running", "a")]))
        .unwrap();
    demo(&rt, "startTask", "alpha").await;
    wait("rule 1", || colour(&rt, 0, 0) == AMBER).await;
    demo(&rt, "failTask", "alpha").await;
    assert!(
        within(300, || colour(&rt, 0, 0) == REST).await,
        "alpha failed, no rule is true, the pad rests; it shows {:?}",
        colour(&rt, 0, 0)
    );
}
