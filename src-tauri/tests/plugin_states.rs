//! Plugin states and the Observable Catalog, end to end on the real runtime.
//!
//! The plugins are the shipped examples, built outside the core with the public
//! SDK: Counter (`level`, an enum) and Channels (`status`, scoped by
//! `channel`). Nothing here is mocked at the plugin boundary. The assertions
//! read the virtual Launchpad's LEDs and the catalog Rust sends to the UI, so
//! they check the whole chain
//!
//!   pad press -> action -> plugin runs -> plugin sets a state -> sandbox gate
//!   -> StateHub -> feedback rule -> look -> renderer -> LED
//!                       \-> Observable Catalog -> UI
//!
//! and its lifecycle: unknown at start, known after a report, unavailable when
//! the plugin stops, unknown again (never the old value) when it returns.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use launchdeck_lib::actions::definition::Availability;
use launchdeck_lib::core::explain::OverlaySource;
use launchdeck_lib::core::model::{
    Assignment, Control, ControlPos, Page, Preferences, Rgb, Surface, Visual, Workspace,
    SCHEMA_VERSION,
};
use launchdeck_lib::core::runtime::{ui, Runtime, RuntimeConfig, UiEmitter};
use launchdeck_lib::devices::Led;
use launchdeck_lib::plugins::keyboard::{Combo, Keyboard};
use launchdeck_lib::plugins::ActionError;
use launchdeck_lib::state::catalog::{ObservableCatalog, ObservableEntry};
use launchdeck_lib::state::definition::ObservableKind;
use launchdeck_lib::state::eval::Diagnostic;
use launchdeck_lib::state::feedback::{FeedbackRule, Hold, Test};
use launchdeck_lib::state::hub::Status;
use launchdeck_lib::state::{Scalar, StateAddress};
use serde_json::{json, Value};

#[path = "support/wat_plugins.rs"]
#[allow(dead_code)]
mod wat;

struct NoKeyboard;
impl Keyboard for NoKeyboard {
    fn press(&self, _combo: &Combo) -> Result<(), ActionError> {
        Ok(())
    }
}

/// Records what the runtime tells the UI.
#[derive(Default)]
struct Recorder(Mutex<Vec<(String, Value)>>);

impl UiEmitter for Recorder {
    fn emit(&self, event: &str, payload: Value) {
        self.0.lock().unwrap().push((event.to_string(), payload));
    }
}

impl Recorder {
    fn of(&self, event: &str) -> Vec<Value> {
        self.0
            .lock()
            .unwrap()
            .iter()
            .filter(|(e, _)| e == event)
            .map(|(_, v)| v.clone())
            .collect()
    }
    fn clear(&self) {
        self.0.lock().unwrap().clear();
    }
}

fn examples() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../examples/plugins")
}

// ---------------------------------------------------------------------------
// The surface under test. It names providers and states only as data, exactly
// as a person's saved rules would.
// ---------------------------------------------------------------------------

const REST: Rgb = Rgb::new(12, 12, 12);
const WAITING: Rgb = Rgb::new(90, 40, 160);
const OFFLINE: Rgb = Rgb::new(120, 0, 0);
const GREY: Rgb = Rgb::new(60, 60, 60);
const BLUE: Rgb = Rgb::new(40, 110, 255);
const AMBER: Rgb = Rgb::new(255, 170, 0);
const RED: Rgb = Rgb::new(255, 40, 50);
const GREEN: Rgb = Rgb::new(30, 220, 90);
const FLASHY: Rgb = Rgb::new(255, 255, 255);

fn action(provider: &str, id: &str, args: Value) -> Assignment {
    Assignment::new(provider, id, args)
}

fn button(col: u8, row: u8, label: &str, press: Assignment) -> Control {
    let mut c = Control::new(ControlPos::grid(col, row), label, BLUE);
    c.press = Some(press);
    c
}

/// A pad that shows a state: the look named after its value when it has one,
/// `waiting` while nothing has reported, `offline` while its provider is gone.
fn display(
    col: u8,
    row: u8,
    label: &str,
    state: StateAddress,
    looks: &[(&str, Visual)],
) -> Control {
    let mut c = Control::new(ControlPos::grid(col, row), label, REST);
    c.visual = Visual::solid(REST);
    for (name, visual) in looks {
        c.states.insert((*name).into(), *visual);
    }
    c.states.insert("waiting".into(), Visual::solid(WAITING));
    c.states.insert("offline".into(), Visual::solid(OFFLINE));
    c.feedback = vec![
        FeedbackRule::state(state.clone(), Test::HasValue).show_from_value(),
        FeedbackRule::state(state.clone(), Test::NoValue).show("waiting"),
        FeedbackRule::state(state, Test::Unavailable).show("offline"),
    ];
    c
}

fn level() -> StateAddress {
    StateAddress::new("example.counter", "level")
}

fn channel(name: &str) -> StateAddress {
    StateAddress::new("example.channels", "status").with("channel", name)
}

fn pos(col: u8, row: u8) -> ControlPos {
    ControlPos::grid(col, row)
}

/// Pads: row 0 the counter (+, -, 0, and its level), row 1 the channels
/// (alpha up, alpha down, beta up, forget alpha, then alpha and beta), row 2 a
/// pad with a state rule and an event rule at once.
fn workspace() -> Workspace {
    let counter = |id: &str| action("example.counter", id, json!({}));
    let chan = |id: &str, name: &str| action("example.channels", id, json!({ "channel": name }));

    let looks_level = [
        ("zero", Visual::solid(GREY)),
        ("low", Visual::solid(BLUE)),
        ("high", Visual::solid(AMBER)),
        ("max", Visual::flash(RED)),
    ];
    let looks_channel = [("up", Visual::solid(GREEN)), ("down", Visual::solid(RED))];

    let mut mixed = display(0, 2, "Mixed", level(), &looks_level);
    mixed.states.insert("blink".into(), Visual::solid(FLASHY));
    mixed.feedback.push(
        FeedbackRule::event("example.counter", "counter.changed")
            .show("blink")
            .hold(Hold::For(5_000)),
    );

    // A rule that decides on a look the control does not have.
    let mut broken = display(1, 2, "Broken", level(), &[]);
    broken.feedback = vec![FeedbackRule::state(level(), Test::HasValue).show("not-a-look")];

    // A rule whose provider is not installed, and one on a state nothing offers.
    let mut gone = Control::new(pos(2, 2), "Gone", REST);
    gone.feedback = vec![
        FeedbackRule::state(StateAddress::new("acme.missing", "thing"), Test::HasValue).show("x"),
        FeedbackRule::state(StateAddress::new("example.counter", "nope"), Test::HasValue).show("x"),
    ];

    Workspace {
        schema_version: SCHEMA_VERSION,
        surfaces: vec![
            Surface {
                id: "states".into(),
                name: "States".into(),
                description: String::new(),
                icon: None,
                category: "custom".into(),
                pages: vec![Page {
                    id: "home".into(),
                    name: "Home".into(),
                    controls: vec![
                        button(1, 0, "+", counter("counter.increment")),
                        button(2, 0, "-", counter("counter.decrement")),
                        button(3, 0, "0", counter("counter.reset")),
                        display(4, 0, "Level", level(), &looks_level),
                        button(0, 1, "Up a", chan("channel.up", "alpha")),
                        button(1, 1, "Down a", chan("channel.down", "alpha")),
                        button(2, 1, "Up b", chan("channel.up", "beta")),
                        button(3, 1, "Forget a", chan("channel.forget", "alpha")),
                        display(4, 1, "Alpha", channel("alpha"), &looks_channel),
                        display(5, 1, "Beta", channel("beta"), &looks_channel),
                        mixed,
                        broken,
                        gone,
                    ],
                }],
                context: None,
            },
            Surface {
                id: "other".into(),
                name: "Other".into(),
                description: String::new(),
                icon: None,
                category: "custom".into(),
                pages: vec![Page {
                    id: "start".into(),
                    name: "Start".into(),
                    controls: vec![],
                }],
                context: None,
            },
        ],
        last_active_surface: Some("states".into()),
        preferences: Preferences::default(),
        context: Default::default(),
    }
}

async fn start_with(dirs: Vec<PathBuf>) -> (Arc<Runtime>, Arc<Recorder>) {
    let rec = Arc::new(Recorder::default());
    let rt = Runtime::start(RuntimeConfig {
        workspace: workspace(),
        store: None,
        emitter: rec.clone(),
        keyboard: Arc::new(NoKeyboard),
        scan_hardware: false,
        serve_external: false,
        demo_pace: 0.05,
        plugin_dirs: dirs,
        secrets: None,
        observer: None,
    })
    .await;
    wait_until("first frame", || {
        !rt.virtual_handle().frame().leds.is_empty()
    })
    .await;
    (rt, rec)
}

async fn start() -> (Arc<Runtime>, Arc<Recorder>) {
    start_with(vec![examples()]).await
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
        .get(pos(col, row))
        .expect("pad exists")
}

fn is(v: Visual) -> Led {
    Led {
        color: v.color,
        animation: v.animation,
    }
}

async fn shows(rt: &Runtime, col: u8, row: u8, v: Visual) {
    for _ in 0..600 {
        if led(rt, col, row) == is(v) {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!(
        "pad ({col},{row}) shows {:?}, expected {:?}",
        led(rt, col, row),
        is(v)
    );
}

fn tap(rt: &Runtime, col: u8, row: u8) {
    let h = rt.virtual_handle();
    h.press(pos(col, row), 100).unwrap();
    h.release(pos(col, row)).unwrap();
}

/// How many times the Counter announced a change. Each successful press is one.
fn changes(rt: &Runtime) -> usize {
    rt.snapshot()
        .recent_events
        .iter()
        .filter(|e| e.source == "example.counter" && e.kind == "counter.changed")
        .count()
}

/// Press `+` and wait until the Counter has handled it, so presses never
/// overlap and each one counts.
async fn increment(rt: &Runtime, times: usize) {
    for _ in 0..times {
        let before = changes(rt);
        tap(rt, 1, 0);
        wait_until("the counter handled the press", || changes(rt) > before).await;
    }
}

fn entry(rt: &Runtime, id: &str) -> ObservableEntry {
    catalog(rt)
        .observables
        .into_iter()
        .find(|o| o.definition.id == id)
        .unwrap_or_else(|| panic!("{id} is not in the catalog"))
}

fn catalog(rt: &Runtime) -> ObservableCatalog {
    rt.observables()
}

fn read(rt: &Runtime, a: &StateAddress) -> (Status, Option<Scalar>) {
    let o = rt.state_hub().read_address(a);
    (o.status, o.value)
}

// ---------------------------------------------------------------------------
// The catalog
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread")]
async fn the_catalog_lists_plugin_states_and_events_with_everything_a_rule_needs() {
    let (rt, _) = start().await;
    let c = catalog(&rt);
    assert!(c.issues.is_empty(), "{:?}", c.issues);

    let level = entry(&rt, "example.counter.level");
    assert_eq!(level.definition.kind, ObservableKind::State);
    assert_eq!(level.provider_name, "Counter Plugin");
    assert_eq!(level.definition.provider, "example.counter");
    let spec = level.definition.value.as_ref().unwrap();
    let values: Vec<_> = spec.values.iter().map(|v| v.value.clone()).collect();
    assert_eq!(
        values,
        ["zero", "low", "high", "max"].map(Scalar::text),
        "the enum values of the manifest"
    );
    assert!(spec
        .values
        .iter()
        .all(|v| v.role.is_some() && !v.label.is_empty()));
    assert!(level.definition.params.is_empty());
    assert_eq!(level.live.availability, Availability::Available);
    assert_eq!(
        level.live.instances.len(),
        1,
        "an unscoped state has one instance"
    );
    assert_eq!(level.live.instances[0].observed.status, Status::Unknown);

    let status = entry(&rt, "example.channels.status");
    assert_eq!(status.definition.params.len(), 1, "scoped by one argument");
    assert_eq!(status.definition.params[0].name, "channel");
    assert!(
        status.live.instances.is_empty(),
        "no channel has reported yet"
    );

    let changed = entry(&rt, "example.counter.counter.changed");
    assert_eq!(
        changed.definition.kind,
        ObservableKind::Event,
        "events stay events"
    );
    let fields: Vec<_> = changed
        .definition
        .fields
        .iter()
        .map(|f| f.name.as_str())
        .collect();
    assert_eq!(fields, ["value", "level"]);
    assert!(changed.live.instances.is_empty(), "an event has no value");

    // The built-ins are in the same list.
    for id in [
        "agent-control.slot.status",
        "agent-control.slot.focused",
        "agent-control.command.state",
        "agent-control.session.running",
        "agent-control.approval.created",
        "agent-control.command.completed",
        "runtime.surface.active",
        "runtime.page.active",
        "runtime.action.completed",
        "device.control.pressed",
        "demo.alpha",
    ] {
        entry(&rt, id);
    }
    // The projection events exist for older bus subscribers and are not offered.
    assert!(c
        .observables
        .iter()
        .all(|o| o.definition.id != "agent-control.slot.changed"));
}

#[tokio::test(flavor = "multi_thread")]
async fn agent_control_states_come_through_the_same_catalog_with_their_scope() {
    let (rt, _) = start().await;
    let slot = entry(&rt, "agent-control.slot.status");
    assert_eq!(slot.provider_name, "Agent Control");
    assert_eq!(slot.definition.params[0].name, "slot");
    assert_eq!(slot.live.availability, Availability::Available);
    // The track statuses are values the hub holds, one instance per track.
    assert_eq!(slot.live.instance_count, 8, "{:?}", slot.live.instances);
    let slots: Vec<f64> = slot
        .live
        .instances
        .iter()
        .map(|i| i.args["slot"].as_number().unwrap())
        .collect();
    assert_eq!(slots, [0.0, 1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0]);
    assert!(slot
        .live
        .instances
        .iter()
        .all(|i| i.observed.status == Status::Known));

    let command = entry(&rt, "agent-control.command.state");
    assert_eq!(command.definition.params[0].name, "command");
    assert!(command.live.instance_count > 0);
}

#[tokio::test(flavor = "multi_thread")]
async fn the_surface_and_page_on_screen_are_states_the_runtime_keeps_current() {
    let (rt, _) = start().await;
    let surface = StateAddress::new("runtime", "surface.active");
    let page = StateAddress::new("runtime", "page.active");
    assert_eq!(
        read(&rt, &surface),
        (Status::Known, Some(Scalar::text("states")))
    );
    assert_eq!(
        read(&rt, &page),
        (Status::Known, Some(Scalar::text("home")))
    );

    rt.activate_surface("other", None).unwrap();
    wait_until("the hub follows navigation", || {
        read(&rt, &surface).1 == Some(Scalar::text("other"))
    })
    .await;
    assert_eq!(read(&rt, &page).1, Some(Scalar::text("start")));
    let e = entry(&rt, "runtime.surface.active");
    assert_eq!(
        e.live.instances[0].observed.value,
        Some(Scalar::text("other"))
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn actions_suggest_the_observables_a_pad_for_them_would_show() {
    let (rt, _) = start().await;
    let actions = rt.catalog().actions;
    let inc = actions
        .iter()
        .find(|a| a.id == "example.counter.counter.increment")
        .unwrap();
    assert_eq!(inc.suggests, ["example.counter.level"]);
    let up = actions
        .iter()
        .find(|a| a.id == "example.channels.channel.up")
        .unwrap();
    assert_eq!(up.suggests, ["example.channels.status"]);
    // Every suggestion names something the catalog lists.
    let ids: Vec<String> = catalog(&rt)
        .observables
        .into_iter()
        .map(|o| o.definition.id)
        .collect();
    for a in &actions {
        for s in &a.suggests {
            assert!(ids.contains(s), "{} suggests {s}", a.id);
        }
    }
}

// ---------------------------------------------------------------------------
// The acceptance flow with the real Counter
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread")]
async fn the_counter_drives_its_pad_through_start_report_disable_and_return() {
    let (rt, _) = start().await;
    let level_pad = (4, 0);

    // Started: declared, nothing reported. The pad is not guessing.
    assert_eq!(read(&rt, &level()), (Status::Unknown, None));
    shows(&rt, level_pad.0, level_pad.1, Visual::solid(WAITING)).await;

    // The plugin reports through its action; no event is involved in the pad.
    tap(&rt, 1, 0);
    shows(&rt, 4, 0, Visual::solid(BLUE)).await;
    assert_eq!(
        read(&rt, &level()),
        (Status::Known, Some(Scalar::text("low")))
    );
    let e = entry(&rt, "example.counter.level");
    assert_eq!(
        e.live.instances[0].observed.value,
        Some(Scalar::text("low"))
    );
    assert_eq!(e.live.instance_count, 1);

    increment(&rt, 4).await;
    shows(&rt, 4, 0, Visual::solid(AMBER)).await;
    increment(&rt, 4).await;
    shows(&rt, 4, 0, Visual::flash(RED)).await;

    // A refused increment still tells the truth.
    tap(&rt, 1, 0);
    tokio::time::sleep(Duration::from_millis(150)).await;
    assert_eq!(read(&rt, &level()).1, Some(Scalar::text("max")));
    assert!(led(&rt, 4, 0) == is(Visual::flash(RED)));

    // Disabled: unavailable, and the pad says so.
    rt.plugin_host()
        .set_enabled("example.counter", false)
        .unwrap();
    shows(&rt, 4, 0, Visual::solid(OFFLINE)).await;
    assert_eq!(read(&rt, &level()), (Status::Unavailable, None));
    let e = entry(&rt, "example.counter.level");
    assert!(matches!(
        e.live.availability,
        Availability::Unavailable { .. }
    ));
    assert_eq!(e.live.instances[0].observed.status, Status::Unavailable);
    assert_eq!(e.live.instance_count, 0, "nothing it said is kept");

    // Back: unknown, never the value it had.
    rt.plugin_host()
        .set_enabled("example.counter", true)
        .unwrap();
    shows(&rt, 4, 0, Visual::solid(WAITING)).await;
    assert_eq!(read(&rt, &level()), (Status::Unknown, None));
    assert_eq!(
        entry(&rt, "example.counter.level").live.availability,
        Availability::Available
    );

    // Reports again, from zero: the counter restarted, and so does what it says.
    tap(&rt, 1, 0);
    shows(&rt, 4, 0, Visual::solid(BLUE)).await;
    assert_eq!(read(&rt, &level()).1, Some(Scalar::text("low")));
}

#[tokio::test(flavor = "multi_thread")]
async fn a_failed_action_still_reports_what_is_true() {
    let (rt, _) = start().await;
    // Decrement at zero fails, and says the counter is at zero.
    tap(&rt, 2, 0);
    shows(&rt, 4, 0, Visual::solid(GREY)).await;
    assert_eq!(read(&rt, &level()).1, Some(Scalar::text("zero")));
}

#[tokio::test(flavor = "multi_thread")]
async fn events_stay_events_the_hub_never_holds_them() {
    let (rt, _) = start().await;
    tap(&rt, 1, 0);
    shows(&rt, 4, 0, Visual::solid(BLUE)).await;
    let held: Vec<String> = rt
        .state_hub()
        .known()
        .into_iter()
        .map(|(k, _)| format!("{}.{}", k.provider(), k.id()))
        .collect();
    assert!(held.iter().any(|k| k == "example.counter.level"));
    assert!(
        held.iter().all(|k| !k.contains("counter.changed")),
        "{held:?}"
    );
    wait_until("the event reached the bus log", || {
        rt.snapshot()
            .recent_events
            .iter()
            .any(|e| e.source == "example.counter" && e.kind == "counter.changed")
    })
    .await;
}

// ---------------------------------------------------------------------------
// Scope: one state, many instances
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread")]
async fn a_scoped_state_has_an_instance_per_argument_and_each_pad_reads_its_own() {
    let (rt, _) = start().await;
    shows(&rt, 4, 1, Visual::solid(WAITING)).await;
    shows(&rt, 5, 1, Visual::solid(WAITING)).await;

    tap(&rt, 0, 1); // alpha up
    shows(&rt, 4, 1, Visual::solid(GREEN)).await;
    assert_eq!(
        led(&rt, 5, 1),
        is(Visual::solid(WAITING)),
        "beta has not reported"
    );

    tap(&rt, 2, 1); // beta up
    shows(&rt, 5, 1, Visual::solid(GREEN)).await;
    tap(&rt, 1, 1); // alpha down
    shows(&rt, 4, 1, Visual::solid(RED)).await;
    assert_eq!(
        led(&rt, 5, 1),
        is(Visual::solid(GREEN)),
        "one channel never moves another"
    );

    let e = entry(&rt, "example.channels.status");
    assert_eq!(e.live.instance_count, 2);
    let by_channel: Vec<(String, Option<Scalar>)> = e
        .live
        .instances
        .iter()
        .map(|i| (i.args["channel"].to_string(), i.observed.value.clone()))
        .collect();
    assert_eq!(by_channel.len(), 2);
    assert!(by_channel.contains(&("alpha".into(), Some(Scalar::text("down")))));
    assert!(by_channel.contains(&("beta".into(), Some(Scalar::text("up")))));

    // An instance can lose its value without touching the others.
    tap(&rt, 3, 1); // forget alpha
    shows(&rt, 4, 1, Visual::solid(WAITING)).await;
    assert_eq!(led(&rt, 5, 1), is(Visual::solid(GREEN)));
    assert_eq!(entry(&rt, "example.channels.status").live.instance_count, 1);

    // What an address reads comes from Rust, checked against the definition.
    let read_state =
        |channel: &str| rt.read_state("example.channels", "status", &json!({ "channel": channel }));
    assert_eq!(read_state("beta").unwrap().value, Some(Scalar::text("up")));
    assert_eq!(read_state("gamma").unwrap().status, Status::Unknown);
    assert!(rt
        .read_state("example.channels", "status", &json!({ "chan": "beta" }))
        .is_err());
    assert!(rt
        .read_state("example.channels", "status", &json!({}))
        .is_err());
    assert!(rt
        .read_state("example.channels", "nope", &json!({}))
        .is_err());
}

#[tokio::test(flavor = "multi_thread")]
async fn stopping_a_plugin_makes_every_instance_unavailable_and_returning_makes_them_unknown() {
    let (rt, _) = start().await;
    tap(&rt, 0, 1);
    tap(&rt, 2, 1);
    shows(&rt, 4, 1, Visual::solid(GREEN)).await;
    shows(&rt, 5, 1, Visual::solid(GREEN)).await;

    rt.plugin_host()
        .set_enabled("example.channels", false)
        .unwrap();
    shows(&rt, 4, 1, Visual::solid(OFFLINE)).await;
    shows(&rt, 5, 1, Visual::solid(OFFLINE)).await;
    let e = entry(&rt, "example.channels.status");
    assert!(e.live.instances.is_empty());
    assert!(matches!(
        e.live.availability,
        Availability::Unavailable { .. }
    ));

    rt.plugin_host()
        .set_enabled("example.channels", true)
        .unwrap();
    shows(&rt, 4, 1, Visual::solid(WAITING)).await;
    shows(&rt, 5, 1, Visual::solid(WAITING)).await;
    assert_eq!(entry(&rt, "example.channels.status").live.instance_count, 0);
    // The other plugin never noticed.
    tap(&rt, 1, 0);
    shows(&rt, 4, 0, Visual::solid(BLUE)).await;
}

// ---------------------------------------------------------------------------
// explain_control
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread")]
async fn explain_names_the_rule_the_state_its_value_and_the_look() {
    let (rt, _) = start().await;
    tap(&rt, 1, 0);
    shows(&rt, 4, 0, Visual::solid(BLUE)).await;
    let id = control_id(&rt, 4, 0);
    let why = rt.explain_control("states", "home", &id).unwrap();
    assert_eq!(why.layer, "state");
    assert_eq!(why.shown.as_deref(), Some("low"));
    assert_eq!(why.derived.as_deref(), Some("low"));
    assert_eq!(why.winner, Some(0));
    assert!(why.overlay.is_none() && why.diagnostics.is_empty());
    let rule = &why.rules[0];
    assert!(rule.holds && rule.decided);
    assert_eq!(
        rule.observed.as_ref().unwrap().value,
        Some(Scalar::text("low"))
    );
    assert_eq!(
        rule.value_label.as_deref(),
        Some("Low"),
        "in the words of the definition"
    );
    assert_eq!(rule.look.as_deref(), Some("low"));
    assert_eq!(rule.look_exists, Some(true));
    let observable = rule.observable.as_ref().unwrap();
    assert_eq!(observable.id, "example.counter.level");
    assert_eq!(observable.name.as_deref(), Some("Counter level"));
    assert_eq!(observable.provider_name.as_deref(), Some("Counter Plugin"));
    assert!(rule.reason.is_none());
    // The rules after the winner did not hold, and say what the state reads.
    assert!(!why.rules[1].holds && !why.rules[1].decided);
    assert_eq!(
        why.rules[1].observed.as_ref().unwrap().status,
        Status::Known
    );

    // It travels as JSON with the names the UI reads.
    let json = serde_json::to_value(&why).unwrap();
    assert_eq!(json["controlId"], id.as_str());
    assert_eq!(
        json["rules"][0]["observable"]["address"],
        "example.counter.level"
    );
    assert!(json.get("stateRules").is_none());
}

#[tokio::test(flavor = "multi_thread")]
async fn explain_says_why_a_state_is_unknown_and_why_it_is_unavailable() {
    let (rt, _) = start().await;
    let id = control_id(&rt, 4, 0);

    // Running, nothing reported.
    let why = rt.explain_control("states", "home", &id).unwrap();
    assert_eq!(why.layer, "state");
    assert_eq!(why.shown.as_deref(), Some("waiting"));
    assert_eq!(why.winner, Some(1));
    let first = &why.rules[0];
    assert!(!first.holds);
    assert_eq!(first.observed.as_ref().unwrap().status, Status::Unknown);
    assert!(
        first
            .reason
            .as_deref()
            .unwrap()
            .contains("has not reported"),
        "{:?}",
        first.reason
    );

    // Stopped.
    rt.plugin_host()
        .set_enabled("example.counter", false)
        .unwrap();
    shows(&rt, 4, 0, Visual::solid(OFFLINE)).await;
    let why = rt.explain_control("states", "home", &id).unwrap();
    assert_eq!(why.shown.as_deref(), Some("offline"));
    assert_eq!(why.winner, Some(2));
    assert_eq!(
        why.rules[0].observed.as_ref().unwrap().status,
        Status::Unavailable
    );
    assert_eq!(
        why.rules[0].reason.as_deref(),
        Some("The plugin is turned off in Plugins.")
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn explain_covers_a_missing_provider_a_missing_state_and_a_missing_look() {
    let (rt, _) = start().await;
    let gone = control_id(&rt, 2, 2);
    let why = rt.explain_control("states", "home", &gone).unwrap();
    assert_eq!(why.layer, "rest");
    assert_eq!(why.shown, None);
    assert_eq!(why.winner, None, "nothing holds");
    assert_eq!(
        why.rules[0].reason.as_deref(),
        Some("Nothing named `acme.missing` is installed.")
    );
    assert_eq!(
        why.rules[1].reason.as_deref(),
        Some("Counter Plugin does not offer a state called `nope`.")
    );
    assert!(why.rules.iter().all(|r| r.observable.is_some()));
    assert_eq!(
        why.rules[0].observable.as_ref().unwrap().provider_name,
        None
    );

    // The rule decided, and the control has no such look: the pad rests.
    tap(&rt, 1, 0);
    wait_until("the counter reported", || read(&rt, &level()).1.is_some()).await;
    let broken = control_id(&rt, 1, 2);
    let why = rt.explain_control("states", "home", &broken).unwrap();
    assert_eq!(why.winner, Some(0));
    assert_eq!(why.derived.as_deref(), Some("not-a-look"));
    assert_eq!(
        why.diagnostics,
        [Diagnostic::MissingLook {
            rule: 0,
            look: "not-a-look".into()
        }]
    );
    assert_eq!(why.rules[0].look_exists, Some(false));
    assert_eq!(
        led(&rt, 1, 2),
        is(Visual::solid(REST)),
        "the resting look is shown"
    );

    assert!(rt.explain_control("states", "home", "nope").is_err());
}

#[tokio::test(flavor = "multi_thread")]
async fn explain_shows_an_event_overlay_on_top_of_the_state_look() {
    let (rt, _) = start().await;
    let mixed = control_id(&rt, 0, 2);

    tap(&rt, 1, 0);
    shows(&rt, 0, 2, Visual::solid(FLASHY)).await; // the event's look, over the state's
    let why = rt.explain_control("states", "home", &mixed).unwrap();
    assert_eq!(why.layer, "overlay");
    assert_eq!(why.shown.as_deref(), Some("blink"));
    assert_eq!(
        why.derived.as_deref(),
        Some("low"),
        "underneath, the state still says low"
    );
    let overlay = why.overlay.as_ref().unwrap();
    assert_eq!(overlay.source, OverlaySource::Event);
    assert_eq!(overlay.look, "blink");
    assert_eq!(overlay.rule, Some(3));
    assert!(!overlay.latched);
    let left = overlay.remaining_ms.expect("a held look ends by itself");
    assert!(left > 0 && left <= 5_000, "{left}");
    assert!(why.rules[3].decided, "the event rule put the overlay there");
    assert!(
        why.rules[0].decided,
        "and the state rule still decides underneath"
    );
    assert_eq!(why.rules[3].kind, "event");
}

fn control_id(rt: &Runtime, col: u8, row: u8) -> String {
    rt.snapshot()
        .workspace
        .surface("states")
        .unwrap()
        .page("home")
        .unwrap()
        .control_at(pos(col, row))
        .unwrap()
        .id
        .clone()
}

// ---------------------------------------------------------------------------
// What the UI hears
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread")]
async fn the_ui_hears_the_state_that_changed_and_nothing_else() {
    let (rt, rec) = start().await;
    tokio::time::sleep(Duration::from_millis(200)).await;
    rec.clear();

    tap(&rt, 1, 0);
    wait_until("an observables message", || {
        rec.of(ui::OBSERVABLES).iter().any(|m| {
            m["observables"]
                .as_array()
                .is_some_and(|o| o.iter().any(|u| u["id"] == "example.counter.level"))
        })
    })
    .await;
    let messages = rec.of(ui::OBSERVABLES);
    let update = messages
        .iter()
        .flat_map(|m| m["observables"].as_array().cloned().unwrap_or_default())
        .find(|u| u["id"] == "example.counter.level")
        .unwrap();
    assert_eq!(update["instances"][0]["status"], "known");
    assert_eq!(update["instances"][0]["value"], "low");
    assert_eq!(update["instanceCount"], 1);
    assert_eq!(update["availability"]["status"], "available");
    // The catalog itself did not travel, nor any other plugin's states.
    for m in &messages {
        assert_eq!(m["catalog"], false, "{m}");
        for u in m["observables"].as_array().unwrap() {
            assert!(
                !u["id"].as_str().unwrap().starts_with("example.channels"),
                "{u}"
            );
        }
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_burst_of_writes_is_one_message_to_the_ui() {
    let (rt, rec) = start().await;
    tokio::time::sleep(Duration::from_millis(200)).await;
    rec.clear();
    increment(&rt, 8).await;
    tokio::time::sleep(Duration::from_millis(200)).await;
    let messages = rec.of(ui::OBSERVABLES);
    let about_level = messages
        .iter()
        .filter(|m| {
            m["observables"]
                .as_array()
                .is_some_and(|o| o.iter().any(|u| u["id"] == "example.counter.level"))
        })
        .count();
    assert!(
        (1..=4).contains(&about_level),
        "8 presses reached the UI as {about_level} messages"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn disabling_a_plugin_tells_the_ui_its_status_and_everything_it_offered() {
    let (rt, rec) = start().await;
    tokio::time::sleep(Duration::from_millis(200)).await;
    rec.clear();
    rt.plugin_host()
        .set_enabled("example.counter", false)
        .unwrap();
    wait_until("a lifecycle message", || {
        rec.of(ui::OBSERVABLES)
            .iter()
            .any(|m| m["providers"].as_array().is_some_and(|p| !p.is_empty()))
    })
    .await;
    let message = rec
        .of(ui::OBSERVABLES)
        .into_iter()
        .find(|m| m["providers"].as_array().is_some_and(|p| !p.is_empty()))
        .unwrap();
    assert_eq!(message["providers"][0]["id"], "example.counter");
    assert_eq!(message["providers"][0]["status"], "disabled");
    let ids: Vec<&str> = message["observables"]
        .as_array()
        .unwrap()
        .iter()
        .map(|u| u["id"].as_str().unwrap())
        .collect();
    assert!(ids.contains(&"example.counter.level"), "{ids:?}");
    assert!(
        ids.contains(&"example.counter.counter.changed"),
        "the event too: {ids:?}"
    );
    for u in message["observables"].as_array().unwrap() {
        assert_eq!(u["availability"]["status"], "unavailable", "{u}");
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_new_plugin_makes_the_ui_read_the_catalog_again() {
    let tmp = tempfile::tempdir().unwrap();
    let (rt, rec) = start_with(vec![examples(), tmp.path().to_path_buf()]).await;
    tokio::time::sleep(Duration::from_millis(200)).await;
    rec.clear();
    assert!(catalog(&rt)
        .observables
        .iter()
        .all(|o| o.definition.provider != "test.late"));

    let dir = tmp.path().join("late");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("plugin.json"),
        json!({
            "schemaVersion": 3, "id": "test.late", "name": "Late", "version": "1.0.0",
            "author": "tests", "abi": 1, "main": "m.wasm",
            "actions": [{"id": "a"}], "events": [],
            "states": [{ "id": "x", "name": "X", "type": "number" }]
        })
        .to_string(),
    )
    .unwrap();
    std::fs::write(
        dir.join("m.wasm"),
        wat::Wat::new().reply(r#"{"status":"ok"}"#).build(),
    )
    .unwrap();
    let host = rt.plugin_host().clone();
    tokio::task::spawn_blocking(move || host.scan())
        .await
        .unwrap();

    wait_until("a catalog message", || {
        rec.of(ui::OBSERVABLES).iter().any(|m| m["catalog"] == true)
    })
    .await;
    let late = entry(&rt, "test.late.x");
    assert_eq!(late.live.instances[0].observed.status, Status::Unknown);
}

// ---------------------------------------------------------------------------
// Isolation
// ---------------------------------------------------------------------------

/// A plugin that declares one state and, when invoked, publishes `message`.
fn hostile(root: &Path, dir: &str, id: &str, local: &str, message: &str) {
    let d = root.join(dir);
    std::fs::create_dir_all(&d).unwrap();
    let ok = r#"{"status":"ok"}"#;
    let wasm = wat::Wat::new()
        .data(wat::EVENT_AT, message)
        .data(0, ok)
        .body(&format!(
            "(drop (call $publish (i32.const {at}) (i32.const {len})))
             (call $respond (i32.const 0) (i32.const {ok}))
             (i32.const 0)",
            at = wat::EVENT_AT,
            len = message.len(),
            ok = ok.len()
        ))
        .build();
    std::fs::write(
        d.join("plugin.json"),
        json!({
            "schemaVersion": 3, "id": id, "name": id, "version": "1.0.0", "author": "tests",
            "abi": 1, "main": "m.wasm",
            "actions": [{"id": "a"}], "events": [],
            "states": [{ "id": local, "name": "Mine", "type": "enum",
                         "values": [{"value": "a", "label": "A"}] }]
        })
        .to_string(),
    )
    .unwrap();
    std::fs::write(d.join("m.wasm"), wasm).unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_plugin_cannot_write_a_state_it_did_not_declare_whoever_owns_it() {
    let tmp = tempfile::tempdir().unwrap();
    // It names Agent Control's state by its local id, and the Counter's.
    hostile(
        tmp.path(),
        "one",
        "test.one",
        "mine",
        r#"{"state":"slot.status","args":{"slot":0},"value":"failed"}"#,
    );
    hostile(
        tmp.path(),
        "two",
        "test.two",
        "mine",
        r#"{"state":"level","value":"max"}"#,
    );
    let (rt, _) = start_with(vec![examples(), tmp.path().to_path_buf()]).await;

    let before = entry(&rt, "agent-control.slot.status").live.instances;
    for plugin in ["test.one", "test.two"] {
        rt.run_action(action(plugin, "a", json!({}))).await.ok();
    }
    tokio::time::sleep(Duration::from_millis(150)).await;
    assert_eq!(
        entry(&rt, "agent-control.slot.status").live.instances,
        before,
        "Agent Control's states are its own"
    );
    assert_eq!(
        read(&rt, &level()),
        (Status::Unknown, None),
        "so are the Counter's"
    );
    // Neither wrote its own declared state either: it never asked to.
    assert_eq!(
        read(&rt, &StateAddress::new("test.two", "mine")),
        (Status::Unknown, None)
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_plugin_id_cannot_take_over_the_id_of_a_built_in_observable() {
    let tmp = tempfile::tempdir().unwrap();
    // `agent-control.slot` + `status` spells the id of Agent Control's `slot.status`.
    hostile(
        tmp.path(),
        "squat",
        "agent-control.slot",
        "status",
        r#"{"state":"status","value":"a"}"#,
    );
    let (rt, _) = start_with(vec![examples(), tmp.path().to_path_buf()]).await;
    let c = catalog(&rt);
    // The built-in keeps its id; the squatter's state is left out and reported.
    assert!(
        c.issues
            .iter()
            .any(|i| i.contains("agent-control.slot.status") && i.contains("already uses")),
        "{:?}",
        c.issues
    );
    let ours = c
        .observables
        .iter()
        .filter(|o| o.definition.id == "agent-control.slot.status")
        .collect::<Vec<_>>();
    assert_eq!(ours.len(), 1);
    assert_eq!(ours[0].definition.provider, "agent-control");
    // Addresses keep provider and id apart, so the hub never confuses the two.
    rt.run_action(action("agent-control.slot", "a", json!({})))
        .await
        .ok();
    tokio::time::sleep(Duration::from_millis(150)).await;
    assert_eq!(
        read(&rt, &StateAddress::new("agent-control.slot", "status")).1,
        Some(Scalar::text("a"))
    );
    let real = StateAddress::new("agent-control", "slot.status").with("slot", 0);
    assert_ne!(read(&rt, &real).1, Some(Scalar::text("a")));
}
