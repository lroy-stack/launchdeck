//! The Visual Feedback Editor on the real runtime, from the Action Library to
//! the pad.
//!
//! The plugins are the shipped examples (Counter and Channels, real WASM,
//! copied to a temporary folder), the pad is the virtual Launchpad, and the
//! controls are `examples/feedback-editor-counter.json` and
//! `examples/feedback-editor-event.json`: exactly what the editor's functions
//! produce. Nothing here names a colour; every expectation is read from the
//! fixture's own `visual` and `states`.
//!
//! * `review_feedback`: issues name the field, notes say what Rust can prove,
//!   the preview is the real explanation, and a review changes nothing;
//! * the whole workflow with the Counter: Library, review, save, Live, presses,
//!   surface away and back, plugin off, plugin on, invoke;
//! * the event acceptance: derived Active, a real `counter.changed`, an Error
//!   overlay that expires back to Active with the state untouched;
//! * a rule whose provider is not installed keeps its configuration.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use launchdeck_lib::actions::definition::Availability;
use launchdeck_lib::core::explain::{ControlExplanation, OverlaySource};
use launchdeck_lib::core::model::{
    Assignment, Control, ControlPos, Page, Preferences, Rgb, Surface, Visual, Workspace,
    SCHEMA_VERSION,
};
use launchdeck_lib::core::review::{FeedbackReview, RuleNote};
use launchdeck_lib::core::runtime::{ui, Mode, Runtime, RuntimeConfig, UiEmitter};
use launchdeck_lib::devices::Led;
use launchdeck_lib::persistence::Store;
use launchdeck_lib::plugins::keyboard::{Combo, Keyboard};
use launchdeck_lib::plugins::ActionError;
use launchdeck_lib::state::definition::ObservableKind;
use launchdeck_lib::state::feedback::{FeedbackRule, Hold, IssueField, Test, When, MAX_RULES};
use launchdeck_lib::state::hub::Status;
use launchdeck_lib::state::{Scalar, StateAddress};
use serde_json::{json, Value};

const SURFACE: &str = "editor";
const OTHER: &str = "other";
const PAGE: &str = "home";

struct NoKeyboard;
impl Keyboard for NoKeyboard {
    fn press(&self, _combo: &Combo) -> Result<(), ActionError> {
        Ok(())
    }
}

/// Records what the runtime tells the UI, for as long as the test runs.
#[derive(Default)]
struct Recorder(Mutex<Vec<(String, Value)>>);

impl UiEmitter for Recorder {
    fn emit(&self, event: &str, payload: Value) {
        self.0.lock().unwrap().push((event.to_string(), payload));
    }
}

impl Recorder {
    /// The payloads of the events of one source and type, oldest first.
    fn events(&self, source: &str, kind: &str) -> Vec<Value> {
        self.0
            .lock()
            .unwrap()
            .iter()
            .filter(|(e, p)| e == ui::EVENT && p["source"] == source && p["type"] == kind)
            .map(|(_, p)| p["payload"].clone())
            .collect()
    }

    fn counter_changes(&self) -> Vec<Value> {
        self.events("example.counter", "counter.changed")
    }

    fn count(&self, source: &str, kind: &str) -> usize {
        self.events(source, kind).len()
    }
}

// ---------------------------------------------------------------------------
// The world: a runtime with the real plugins and one empty page
// ---------------------------------------------------------------------------

struct Env {
    rt: Arc<Runtime>,
    rec: Arc<Recorder>,
    /// Where the workspace is saved.
    data: PathBuf,
    _root: tempfile::TempDir,
}

impl Env {
    fn saved_file(&self) -> PathBuf {
        self.data.join("workspace.json")
    }
}

fn copy_dir(from: &Path, to: &Path) {
    fs::create_dir_all(to).unwrap();
    for entry in fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        if entry.file_type().unwrap().is_file() {
            fs::copy(entry.path(), to.join(entry.file_name())).unwrap();
        }
    }
}

fn surface(id: &str, page: &str, controls: Vec<Control>) -> Surface {
    Surface {
        id: id.into(),
        name: id.into(),
        description: String::new(),
        icon: None,
        category: "custom".into(),
        pages: vec![Page {
            id: page.into(),
            name: page.into(),
            controls,
        }],
        context: None,
    }
}

/// Two surfaces. The first has one page holding `controls` (none by default:
/// an empty pad, as when a person starts), the second is somewhere to go.
fn workspace(controls: Vec<Control>) -> Workspace {
    Workspace {
        schema_version: SCHEMA_VERSION,
        surfaces: vec![
            surface(SURFACE, PAGE, controls),
            surface(OTHER, "start", vec![]),
        ],
        last_active_surface: Some(SURFACE.into()),
        preferences: Preferences::default(),
    }
}

async fn start(controls: Vec<Control>) -> Env {
    let root = tempfile::tempdir().unwrap();
    let examples = Path::new(env!("CARGO_MANIFEST_DIR")).join("../examples/plugins");
    let plugins = root.path().join("plugins");
    for name in ["counter", "channels"] {
        copy_dir(&examples.join(name), &plugins.join(name));
    }
    let data = root.path().join("data");
    let rec = Arc::new(Recorder::default());
    let rt = Runtime::start(RuntimeConfig {
        workspace: workspace(controls),
        store: Some(Store::new(&data)),
        emitter: rec.clone(),
        keyboard: Arc::new(NoKeyboard),
        scan_hardware: false,
        serve_external: false,
        demo_pace: 0.05,
        plugin_dirs: vec![plugins],
    })
    .await;
    wait_until("the first frame", || {
        !rt.virtual_handle().frame().leds.is_empty()
    })
    .await;
    Env {
        rt,
        rec,
        data,
        _root: root,
    }
}

/// Generous: CI is slow, and a test that passes quickly pays nothing for it.
const PATIENCE: Duration = Duration::from_secs(20);

async fn wait_until(what: &str, mut cond: impl FnMut() -> bool) {
    let until = Instant::now() + PATIENCE;
    while Instant::now() < until {
        if cond() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    panic!("timed out waiting for: {what}");
}

// ---------------------------------------------------------------------------
// The pad
// ---------------------------------------------------------------------------

fn led(rt: &Runtime, pos: ControlPos) -> Option<Led> {
    rt.virtual_handle().frame().get(pos)
}

fn is(v: Visual) -> Led {
    Led {
        color: v.color,
        animation: v.animation,
    }
}

async fn shows_within(rt: &Runtime, pos: ControlPos, v: Visual, within: Duration) {
    let until = Instant::now() + within;
    while Instant::now() < until {
        if led(rt, pos) == Some(is(v)) {
            return;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    panic!("pad {pos:?} shows {:?}, expected {:?}", led(rt, pos), is(v));
}

async fn shows(rt: &Runtime, pos: ControlPos, v: Visual) {
    shows_within(rt, pos, v, PATIENCE).await;
}

fn tap(rt: &Runtime, pos: ControlPos) {
    let h = rt.virtual_handle();
    h.press(pos, 100).unwrap();
    h.release(pos).unwrap();
}

// ---------------------------------------------------------------------------
// The controls the editor produces
// ---------------------------------------------------------------------------

fn fixture(name: &str) -> Control {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../examples")
        .join(name);
    let text = fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    serde_json::from_str(&text).unwrap_or_else(|e| panic!("{name} is not a control: {e}"))
}

fn level() -> StateAddress {
    StateAddress::new("example.counter", "level")
}

fn alpha() -> StateAddress {
    StateAddress::new("example.channels", "status").with("channel", "alpha")
}

fn text(v: &str) -> Scalar {
    Scalar::text(v)
}

fn equals(v: &str) -> Test {
    Test::Equals { value: text(v) }
}

/// The look a control defines under `name`.
fn look(c: &Control, name: &str) -> Visual {
    *c.states
        .get(name)
        .unwrap_or_else(|| panic!("the fixture has no look called `{name}`"))
}

fn increment() -> Assignment {
    Assignment::new("example.counter", "counter.increment", json!({}))
}

fn decrement() -> Assignment {
    Assignment::new("example.counter", "counter.decrement", json!({}))
}

fn channel_up(name: &str) -> Assignment {
    Assignment::new("example.channels", "channel.up", json!({ "channel": name }))
}

fn review(env: &Env, c: &Control) -> FeedbackReview {
    env.rt.review_feedback(SURFACE, PAGE, c).unwrap()
}

fn explain(env: &Env, c: &Control) -> ControlExplanation {
    env.rt.explain_control(SURFACE, PAGE, &c.id).unwrap()
}

fn save(env: &Env, c: &Control) {
    env.rt.update_control(SURFACE, PAGE, c.clone()).unwrap();
}

fn stored(env: &Env, id: &str) -> Control {
    env.rt
        .snapshot()
        .workspace
        .surface(SURFACE)
        .unwrap()
        .page(PAGE)
        .unwrap()
        .control(id)
        .unwrap_or_else(|| panic!("`{id}` is not saved"))
        .clone()
}

fn kinds(r: &FeedbackReview) -> Vec<&'static str> {
    r.notes
        .iter()
        .map(|n| match n {
            RuleNote::MissingLook { .. } => "missingLook",
            RuleNote::Shadowed { .. } => "shadowed",
            RuleNote::MissingObservable { .. } => "missingObservable",
            RuleNote::ProviderUnavailable { .. } => "providerUnavailable",
        })
        .collect()
}

fn catalog_availability(env: &Env, id: &str) -> Availability {
    env.rt
        .observables()
        .observables
        .into_iter()
        .find(|o| o.definition.id == id)
        .unwrap_or_else(|| panic!("{id} is not in the catalog"))
        .live
        .availability
}

fn reading(env: &Env, a: &StateAddress) -> (Status, Option<Scalar>) {
    let o = env.rt.state_hub().read_address(a);
    (o.status, o.value)
}

/// A pad that was saved when a plugin existed that is not installed now.
fn orphan_pad() -> Control {
    let mut c = Control::new(ControlPos::grid(1, 1), "Orphan", Rgb::new(40, 40, 40));
    c.states
        .insert("on".into(), Visual::solid(Rgb::new(10, 200, 10)));
    c.feedback = vec![
        FeedbackRule::state(StateAddress::new("acme.missing", "thing"), equals("up")).show("on"),
        FeedbackRule::state(level(), equals("high")).show("on"),
    ];
    c
}

/// An unsaved pad with a look called `calm` and the given rules.
fn draft(rules: Vec<FeedbackRule>) -> Control {
    let mut c = Control::new(ControlPos::grid(6, 6), "Draft", Rgb::new(30, 30, 30));
    c.states
        .insert("calm".into(), Visual::solid(Rgb::new(0, 120, 255)));
    c.feedback = rules;
    c
}

// ---------------------------------------------------------------------------
// (a) The review
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread")]
async fn every_kind_of_mistake_points_at_the_field_it_is_about() {
    let env = start(vec![]).await;
    let mut rules = vec![
        // 0: nothing offers this state
        FeedbackRule::state(StateAddress::new("example.counter", "nope"), Test::HasValue)
            .show("calm"),
        // 1: a scoped state without its scope
        FeedbackRule::state(
            StateAddress::new("example.channels", "status"),
            Test::HasValue,
        )
        .show("calm"),
        // 2: an argument that is too long
        FeedbackRule::state(
            StateAddress::new("example.channels", "status")
                .with("channel", "x".repeat(40).as_str()),
            Test::HasValue,
        )
        .show("calm"),
        // 3: below, on an enum
        FeedbackRule::state(level(), Test::LessThan { value: 3.0 }).show("calm"),
        // 4: a value the enum does not list
        FeedbackRule::state(level(), equals("overflowing")).show("calm"),
        // 5: a value of the wrong type
        FeedbackRule::state(
            level(),
            Test::Equals {
                value: Scalar::Num(3.0),
            },
        )
        .show("calm"),
        // 6: no look chosen
        FeedbackRule::state(level(), Test::NoValue),
        // 7: an event that holds for 100 ms
        FeedbackRule::event("example.counter", "counter.changed")
            .show("calm")
            .hold(Hold::For(100)),
        // 8: an event without a source
        FeedbackRule::event("", "counter.changed").show("calm"),
        // 9: a hold on a state rule
        FeedbackRule::state(level(), equals("zero"))
            .show("calm")
            .hold(Hold::Latch),
        // 10: a good rule: no issue
        FeedbackRule::state(alpha(), equals("up")).show("calm"),
    ];
    // 11: nine conditions on an event
    let mut conditions = FeedbackRule::event("example.counter", "counter.changed").show("calm");
    for n in 0..9 {
        conditions = conditions.where_field(&format!("f{n}"), "x");
    }
    rules.push(conditions);
    let pad = draft(rules);

    let r = review(&env, &pad);
    let got: Vec<_> = r
        .issues
        .iter()
        .map(|i| (i.rule, i.field, i.param.as_deref()))
        .collect();
    assert_eq!(
        got,
        vec![
            (0, Some(IssueField::State), None),
            (1, Some(IssueField::Args), Some("channel")),
            (2, Some(IssueField::Args), Some("channel")),
            (3, Some(IssueField::Test), None),
            (4, Some(IssueField::Value), None),
            (5, Some(IssueField::Value), None),
            (6, Some(IssueField::Look), None),
            (7, Some(IssueField::Hold), None),
            (8, Some(IssueField::Event), None),
            (9, Some(IssueField::Hold), None),
            (11, Some(IssueField::Where), None),
        ],
        "{:#?}",
        r.issues
    );

    // These are exactly the problems saving refuses, in the same words.
    let refused = env
        .rt
        .update_control(SURFACE, PAGE, pad.clone())
        .unwrap_err();
    for i in &r.issues {
        let line = format!("Feedback rule {}: {}", i.rule + 1, i.message);
        assert!(refused.contains(&line), "`{line}` is not in `{refused}`");
    }
    assert!(
        env.rt
            .snapshot()
            .workspace
            .surface(SURFACE)
            .unwrap()
            .page(PAGE)
            .unwrap()
            .controls
            .is_empty(),
        "a refused save stores nothing"
    );

    // The wire carries the field and the argument by name.
    let json = serde_json::to_value(&r).unwrap();
    assert_eq!(json["issues"][1]["field"], "args");
    assert_eq!(json["issues"][1]["param"], "channel");
    assert_eq!(json["issues"][10]["field"], "where");
}

#[tokio::test(flavor = "multi_thread")]
async fn too_many_rules_is_an_issue_about_the_list() {
    let env = start(vec![]).await;
    let rule = FeedbackRule::state(level(), equals("zero")).show("calm");
    let pad = draft(vec![rule.clone(); MAX_RULES + 1]);
    let r = review(&env, &pad);
    let found = r.issues.iter().find(|i| i.rule == MAX_RULES).unwrap();
    assert_eq!(found.field, Some(IssueField::Rules));
    assert_eq!(r.limits.max_rules, MAX_RULES);
    assert!(env.rt.update_control(SURFACE, PAGE, pad).is_err());

    // At the limit it is fine.
    let at_limit = draft(vec![rule; MAX_RULES]);
    assert!(review(&env, &at_limit)
        .issues
        .iter()
        .all(|i| i.field != Some(IssueField::Rules)));
}

#[tokio::test(flavor = "multi_thread")]
async fn notes_say_what_rust_can_prove_about_the_rules() {
    let env = start(vec![]).await;
    let pad = draft(vec![
        // 0: a look the control does not have
        FeedbackRule::state(level(), equals("zero")).show("ghost"),
        // 1: the same test on the same state, after a rule that names a look
        FeedbackRule::state(level(), equals("zero")).show("calm"),
        // 2: nothing installed offers this state (and a new rule must, so it is an issue too)
        FeedbackRule::state(StateAddress::new("acme.missing", "thing"), Test::HasValue)
            .show("calm"),
        // 3: an event nothing installed announces
        FeedbackRule::event("ci", "build.finished").show("calm"),
        // 4, 5, 6: fine; `idle` is the resting look unless the control draws one
        FeedbackRule::event("example.counter", "counter.changed").show("calm"),
        FeedbackRule::state(level(), equals("high")).show("idle"),
        FeedbackRule::state(level(), Test::HasValue).show_from_value(),
    ]);
    let r = review(&env, &pad);
    assert_eq!(
        r.issues.iter().map(|i| i.rule).collect::<Vec<_>>(),
        vec![2],
        "only the rule on a state nothing offers is refused"
    );
    assert_eq!(
        r.notes,
        vec![
            RuleNote::MissingLook {
                rule: 0,
                look: "ghost".into()
            },
            RuleNote::Shadowed { rule: 1, by: 0 },
            RuleNote::MissingObservable {
                rule: 2,
                observable_kind: ObservableKind::State,
                provider: "acme.missing".into(),
                id: "thing".into()
            },
            RuleNote::MissingObservable {
                rule: 3,
                observable_kind: ObservableKind::Event,
                provider: "ci".into(),
                id: "build.finished".into()
            },
        ]
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_plugin_that_is_off_is_a_note_with_its_reason() {
    let env = start(vec![]).await;
    let pad = draft(vec![
        FeedbackRule::state(level(), equals("high")).show("calm"),
        FeedbackRule::event("example.counter", "counter.changed").show("calm"),
        FeedbackRule::state(alpha(), equals("up")).show("calm"),
    ]);
    assert!(review(&env, &pad).notes.is_empty(), "everything is running");

    env.rt
        .plugin_host()
        .set_enabled("example.counter", false)
        .unwrap();
    wait_until("the Counter to be unavailable", || {
        matches!(
            catalog_availability(&env, "example.counter.level"),
            Availability::Unavailable { .. }
        )
    })
    .await;

    let r = review(&env, &pad);
    assert!(r.issues.is_empty(), "a draft may read a plugin that is off");
    assert_eq!(
        r.notes,
        vec![
            RuleNote::ProviderUnavailable {
                rule: 0,
                provider: "example.counter".into(),
                provider_name: "Counter Plugin".into(),
                reason: "The plugin is turned off in Plugins.".into()
            },
            RuleNote::ProviderUnavailable {
                rule: 1,
                provider: "example.counter".into(),
                provider_name: "Counter Plugin".into(),
                reason: "The plugin is turned off in Plugins.".into()
            },
        ],
        "the Channels rule is still fine"
    );
    let json = serde_json::to_value(&r.notes[0]).unwrap();
    assert_eq!(json["kind"], "providerUnavailable");
    assert_eq!(json["providerName"], "Counter Plugin");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_saved_rule_whose_plugin_is_gone_is_a_note_and_never_an_issue() {
    let orphan = orphan_pad();
    let env = start(vec![orphan.clone()]).await;

    let r = review(&env, &orphan);
    assert!(r.issues.is_empty(), "{:?}", r.issues);
    assert_eq!(
        r.notes,
        vec![RuleNote::MissingObservable {
            rule: 0,
            observable_kind: ObservableKind::State,
            provider: "acme.missing".into(),
            id: "thing".into()
        }]
    );
    assert!(r.preview.is_none(), "rules and looks are the saved ones");

    // Another label changes neither the verdict nor the preview.
    let mut relabelled = orphan.clone();
    relabelled.label = "Other name".into();
    let r = review(&env, &relabelled);
    assert!(r.issues.is_empty() && r.preview.is_none());
    assert_eq!(kinds(&r), vec!["missingObservable"]);
}

#[tokio::test(flavor = "multi_thread")]
async fn the_preview_is_there_for_a_changed_or_unsaved_control_only() {
    let env = start(vec![]).await;
    let counter = fixture("feedback-editor-counter.json");

    // Not saved yet: the explanation of the draft, which nothing evaluates yet.
    let first = review(&env, &counter);
    assert!(first.issues.is_empty() && first.notes.is_empty());
    let preview = first.preview.expect("an unsaved control has a preview");
    assert_eq!(preview.control_id, counter.id);

    save(&env, &counter);
    let unchanged = review(&env, &counter);
    assert!(unchanged.preview.is_none(), "the UI asks explain_control");
    // The label, the resting look and the action are not rules or looks.
    let mut other = counter.clone();
    other.label = "Renamed".into();
    other.visual = Visual::solid(Rgb::new(1, 2, 3));
    other.press = None;
    assert!(review(&env, &other).preview.is_none());

    // A rule or a look that differs is previewed.
    let mut reordered = counter.clone();
    reordered.feedback.swap(1, 2);
    assert!(review(&env, &reordered).preview.is_some());
    let mut recoloured = counter.clone();
    recoloured
        .states
        .insert("active".into(), Visual::pulse(Rgb::new(9, 9, 9)));
    assert!(review(&env, &recoloured).preview.is_some());
    let mut fewer = counter.clone();
    fewer.feedback.pop();
    assert!(review(&env, &fewer).preview.is_some());
    let mut fewer_looks = counter.clone();
    fewer_looks.states.remove("error");
    let r = review(&env, &fewer_looks);
    assert!(r.preview.is_some());
    assert_eq!(kinds(&r), vec!["missingLook"], "rule 3 shows `error`");
}

#[tokio::test(flavor = "multi_thread")]
async fn the_preview_is_what_the_saved_control_explains() {
    let env = start(vec![]).await;
    let counter = fixture("feedback-editor-counter.json");
    // The Counter reports a value the draft will read.
    for _ in 0..5 {
        env.rt.run_action(increment()).await.unwrap();
    }
    wait_until("the Counter to report", || {
        reading(&env, &level()).1 == Some(text("high"))
    })
    .await;

    let mut changed = counter.clone();
    changed
        .states
        .insert("warning".into(), Visual::pulse(Rgb::new(250, 100, 0)));
    for pad in [counter, changed] {
        let preview = review(&env, &pad).preview.unwrap();
        assert_eq!(
            (preview.layer, preview.derived.as_deref()),
            ("state", Some("warning"))
        );
        assert_eq!(preview.winner, Some(2));
        assert!(preview.overlay.is_none());
        save(&env, &pad);
        // The same explanation, rule by rule, as the saved control gives.
        let saved = explain(&env, &pad);
        assert_eq!(
            serde_json::to_value(&preview).unwrap(),
            serde_json::to_value(&saved).unwrap()
        );
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn reviewing_changes_nothing() {
    let env = start(vec![]).await;
    let counter = fixture("feedback-editor-counter.json");
    save(&env, &counter);
    save(&env, &fixture("feedback-editor-event.json"));
    for _ in 0..3 {
        env.rt.run_action(increment()).await.unwrap();
    }
    let pad = counter.pos;
    let low = look(&counter, "active");
    shows(&env.rt, pad, low).await;
    // Let every message about those changes go out before looking.
    tokio::time::sleep(Duration::from_millis(300)).await;

    let observe = || {
        (
            serde_json::to_value(env.rt.snapshot().workspace).unwrap(),
            env.rt.state().control_states,
            env.rt.feedback_stats(),
            fs::read(env.saved_file()).unwrap(),
            serde_json::to_value(explain(&env, &counter)).unwrap(),
            env.rt.virtual_handle().frame(),
            reading(&env, &level()),
        )
    };
    let before = observe();

    let mut changed = counter.clone();
    changed.feedback.reverse();
    changed
        .states
        .insert("brand-new".into(), Visual::flash(Rgb::new(255, 0, 255)));
    let mut unsaved = draft(vec![
        FeedbackRule::state(level(), Test::HasValue).show_from_value(),
        FeedbackRule::state(level(), Test::LessThan { value: 1.0 }),
    ]);
    unsaved.id = "g.7.7".into();
    unsaved.pos = ControlPos::grid(7, 7);
    for pad in [&counter, &changed, &unsaved] {
        for _ in 0..3 {
            review(&env, pad);
        }
    }
    assert!(env
        .rt
        .review_feedback("nowhere", PAGE, &counter)
        .unwrap_err()
        .contains("unknown surface"));
    assert!(env
        .rt
        .review_feedback(SURFACE, "nowhere", &counter)
        .unwrap_err()
        .contains("unknown page"));
    tokio::time::sleep(Duration::from_millis(200)).await;

    let after = observe();
    assert_eq!(before.0, after.0, "the workspace");
    assert_eq!(before.1, after.1, "the derived looks");
    assert_eq!(before.2, after.2, "nothing was evaluated for a control");
    assert_eq!(before.3, after.3, "the saved file");
    assert_eq!(before.4, after.4, "the explanation of the saved control");
    assert_eq!(before.5, after.5, "the pads");
    assert_eq!(before.6, after.6, "the values");
    assert!(
        env.rt
            .snapshot()
            .workspace
            .surface(SURFACE)
            .unwrap()
            .page(PAGE)
            .unwrap()
            .control("g.7.7")
            .is_none(),
        "an unsaved control is still not saved"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn an_unknown_surface_or_page_is_refused_like_a_save_is() {
    let env = start(vec![]).await;
    let counter = fixture("feedback-editor-counter.json");
    for (surface, page) in [("nowhere", PAGE), (SURFACE, "nowhere")] {
        assert_eq!(
            env.rt.review_feedback(surface, page, &counter).unwrap_err(),
            env.rt
                .update_control(surface, page, counter.clone())
                .unwrap_err()
        );
    }
}

// ---------------------------------------------------------------------------
// (b) The Counter, from the Action Library to a disabled plugin and back
// ---------------------------------------------------------------------------

/// What each press of `+` should leave behind: the Counter's level, and the
/// name of the look the fixture gives that level.
fn after_presses(n: usize) -> (&'static str, &'static str) {
    match n {
        1..=4 => ("low", "active"),
        5..=8 => ("high", "warning"),
        _ => ("max", "error"),
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn the_counter_pad_works_from_the_library_to_a_disabled_plugin_and_back() {
    let env = start(vec![]).await;
    let (rt, rec) = (&env.rt, &env.rec);
    let control = fixture("feedback-editor-counter.json");
    let (pad, rest) = (control.pos, control.visual);

    // The Library offers Counter / Increment, and says what a pad for it shows.
    let library = rt.catalog();
    let increment_entry = library
        .actions
        .iter()
        .find(|a| a.category == "Counter" && a.name == "Increment")
        .expect("Counter / Increment is in the Action Library");
    assert!(increment_entry.availability.is_available());
    assert_eq!(
        control.press,
        Some(Assignment::new(
            &increment_entry.provider,
            &increment_entry.action,
            json!({})
        )),
        "the control presses exactly the Library's action"
    );
    let observables = rt.observables();
    let suggested: Vec<_> = increment_entry
        .suggests
        .iter()
        .map(|id| {
            observables
                .observables
                .iter()
                .find(|o| o.definition.id == *id)
                .unwrap_or_else(|| panic!("the suggestion {id} is not in the catalog"))
        })
        .collect();
    assert_eq!(suggested.len(), 1);
    assert_eq!(suggested[0].definition.kind, ObservableKind::State);
    assert_eq!(suggested[0].definition.id, "example.counter.level");
    // The rules of the control read exactly that state.
    for rule in &control.feedback {
        let When::State(c) = &rule.when else {
            panic!("the Counter pad has state rules only");
        };
        assert_eq!(
            (c.state.provider.as_str(), c.state.id.as_str()),
            (
                suggested[0].definition.provider.as_str(),
                suggested[0].definition.local.as_str()
            )
        );
    }

    // Review before saving: nothing wrong, nothing to remark, and the
    // explanation says the Counter has not reported yet and nothing wins.
    let r = review(&env, &control);
    assert!(r.issues.is_empty(), "{:?}", r.issues);
    assert!(r.notes.is_empty(), "{:?}", r.notes);
    let preview = r.preview.expect("a pad that is not saved has a preview");
    assert_eq!(preview.rules.len(), control.feedback.len());
    let first = &preview.rules[0];
    let observed = first.observed.as_ref().unwrap();
    assert_eq!((observed.status, &observed.value), (Status::Unknown, &None));
    assert!(
        first
            .reason
            .as_deref()
            .unwrap()
            .contains("has not reported"),
        "{:?}",
        first.reason
    );
    assert!(preview.rules.iter().all(|r| !r.holds && !r.decided));
    assert_eq!(
        (preview.winner, preview.derived.as_deref(), preview.shown),
        (None, None, None)
    );
    assert_eq!(preview.layer, "rest");
    // Reviewing saved nothing.
    assert!(rt
        .snapshot()
        .workspace
        .surface(SURFACE)
        .unwrap()
        .page(PAGE)
        .unwrap()
        .controls
        .is_empty());
    assert_ne!(led(rt, pad), Some(is(rest)));

    // Save: the pad shows its resting look, because nothing has reported.
    save(&env, &control);
    shows(rt, pad, rest).await;
    let why = explain(&env, &control);
    assert_eq!((why.layer, why.shown.as_deref()), ("rest", None));
    assert_eq!(
        why.rules[0].observed.as_ref().unwrap().status,
        Status::Unknown
    );
    assert!(review(&env, &control).preview.is_none());

    // Live: the pad does what the control says.
    rt.set_mode(Mode::Live);
    let mut presses = 0;
    while presses < 9 {
        presses += 1;
        let before = rec.counter_changes().len();
        tap(rt, pad);
        wait_until("the Counter to handle the press", || {
            rec.counter_changes().len() > before
        })
        .await;
        let (level_now, look_now) = after_presses(presses);
        assert_eq!(
            rec.counter_changes().last().unwrap()["level"],
            level_now,
            "after {presses} presses"
        );
        shows(rt, pad, look(&control, look_now)).await;
        let why = explain(&env, &control);
        assert_eq!(why.shown.as_deref(), Some(look_now), "{presses} presses");
        assert_eq!((why.layer, why.overlay.is_none()), ("state", true));
        assert_eq!(
            reading(&env, &level()),
            (Status::Known, Some(text(level_now)))
        );
    }

    // One more at the maximum: the action fails, the level and the look stay.
    let failures = rec.count("runtime", "action.failed");
    let changes = rec.counter_changes().len();
    tap(rt, pad);
    wait_until("the refused press to be reported", || {
        rec.count("runtime", "action.failed") > failures
    })
    .await;
    assert_eq!(rec.counter_changes().len(), changes, "nothing changed");
    assert_eq!(reading(&env, &level()).1, Some(text("max")));
    shows(rt, pad, look(&control, "error")).await;

    // Away and back: the right look is on the pad at once, and no event is needed.
    let (started, changed) = (
        rec.count("runtime", "action.started"),
        rec.counter_changes().len(),
    );
    rt.activate_surface(OTHER, None).unwrap();
    wait_until("the pad to leave the screen", || {
        led(rt, pad) != Some(is(look(&control, "error")))
    })
    .await;
    rt.activate_surface(SURFACE, None).unwrap();
    shows_within(rt, pad, look(&control, "error"), Duration::from_secs(3)).await;
    assert_eq!(
        (
            rec.count("runtime", "action.started"),
            rec.counter_changes().len()
        ),
        (started, changed),
        "nothing ran and nothing was announced to repaint the pad"
    );
    // And when the value changed while the pad was away.
    rt.activate_surface(OTHER, None).unwrap();
    rt.run_action(decrement()).await.unwrap();
    assert_eq!(reading(&env, &level()).1, Some(text("high")));
    rt.activate_surface(SURFACE, None).unwrap();
    shows_within(rt, pad, look(&control, "warning"), Duration::from_secs(3)).await;

    // The plugin is turned off: unavailable, with a reason, and the pad rests.
    rt.plugin_host()
        .set_enabled("example.counter", false)
        .unwrap();
    wait_until("the catalog to say unavailable", || {
        matches!(
            catalog_availability(&env, "example.counter.level"),
            Availability::Unavailable { .. }
        )
    })
    .await;
    shows(rt, pad, rest).await;
    let why = explain(&env, &control);
    let observed = why.rules[0].observed.as_ref().unwrap();
    assert_eq!(observed.status, Status::Unavailable);
    assert!(
        !why.rules[0].reason.as_deref().unwrap_or("").is_empty(),
        "an unavailable state says why"
    );
    assert_eq!((why.layer, why.shown, why.winner), ("rest", None, None));
    // Reviewing the saved pad now remarks on every rule, and flags nothing.
    let r = review(&env, &control);
    assert!(r.issues.is_empty() && r.preview.is_none());
    assert_eq!(
        kinds(&r),
        vec!["providerUnavailable"; control.feedback.len()]
    );

    // Turned on again: unknown, never the value it had, and the pad rests.
    rt.plugin_host()
        .set_enabled("example.counter", true)
        .unwrap();
    wait_until("the Counter to be available", || {
        catalog_availability(&env, "example.counter.level") == Availability::Available
    })
    .await;
    shows(rt, pad, rest).await;
    assert_eq!(reading(&env, &level()), (Status::Unknown, None));
    let why = explain(&env, &control);
    let observed = why.rules[0].observed.as_ref().unwrap();
    assert_eq!((observed.status, &observed.value), (Status::Unknown, &None));
    assert!(why.rules[0]
        .reason
        .as_deref()
        .unwrap()
        .contains("has not reported"));
    assert!(review(&env, &control).notes.is_empty());

    // It reports again when something is invoked: known, and the right look.
    rt.run_action(increment()).await.unwrap();
    wait_until("the Counter to report", || {
        reading(&env, &level()).1.is_some()
    })
    .await;
    assert_eq!(reading(&env, &level()).1, Some(text("low")));
    shows(rt, pad, look(&control, "active")).await;

    // What was saved is the rules and nothing else: no value, status, overlay
    // or renderer state ever reaches the file.
    let file: Value = serde_json::from_slice(&fs::read(env.saved_file()).unwrap()).unwrap();
    assert_persisted_is_only_rule_data(&file, &control);
    assert_persisted_is_only_rule_data(
        &serde_json::to_value(rt.snapshot().workspace).unwrap(),
        &control,
    );
}

fn keys(v: &Value, out: &mut Vec<String>) {
    match v {
        Value::Object(map) => {
            for (k, v) in map {
                out.push(k.clone());
                keys(v, out);
            }
        }
        Value::Array(items) => items.iter().for_each(|i| keys(i, out)),
        _ => {}
    }
}

fn assert_persisted_is_only_rule_data(workspace: &Value, control: &Control) {
    let controls = workspace["surfaces"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["id"] == SURFACE)
        .unwrap()["pages"][0]["controls"]
        .as_array()
        .unwrap();
    let saved = controls
        .iter()
        .find(|c| c["id"] == control.id.as_str())
        .expect("the control was saved");
    // Exactly what the editor produced...
    assert_eq!(
        &serde_json::from_value::<Control>(saved.clone()).unwrap(),
        control
    );
    // ...in the keys of a control and no others.
    let allowed = [
        "id",
        "pos",
        "span",
        "label",
        "description",
        "icon",
        "visual",
        "states",
        "press",
        "longPress",
        "feedback",
        "legacyBindings",
    ];
    for k in saved.as_object().unwrap().keys() {
        assert!(allowed.contains(&k.as_str()), "`{k}` is not control data");
    }
    let mut all = Vec::new();
    keys(saved, &mut all);
    for forbidden in [
        "observed",
        "status",
        "updatedAt",
        "instances",
        "availability",
        "derived",
        "overlay",
        "layer",
        "shown",
        "remainingMs",
        "winner",
        "led",
        "leds",
        "frame",
        "renderer",
        "controlStates",
    ] {
        assert!(
            !all.iter().any(|k| k == forbidden),
            "`{forbidden}` was saved with the control"
        );
    }
    // The only values are the ones the person wrote in the rules.
    let mut without_rules = saved.clone();
    without_rules.as_object_mut().unwrap().remove("feedback");
    let mut rest = Vec::new();
    keys(&without_rules, &mut rest);
    assert!(
        !rest.iter().any(|k| k == "value"),
        "a value outside the rules"
    );
    assert_eq!(
        saved["feedback"],
        serde_json::to_value(&control.feedback).unwrap()
    );
}

// ---------------------------------------------------------------------------
// (c) Event acceptance
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread")]
async fn an_event_overlay_covers_the_state_look_and_leaves_the_state_alone() {
    let env = start(vec![]).await;
    let (rt, rec) = (&env.rt, &env.rec);
    let control = fixture("feedback-editor-event.json");
    let (pad, rest) = (control.pos, control.visual);
    let hold_ms = match control.feedback[1].hold {
        Some(Hold::For(ms)) => u64::from(ms),
        other => panic!("the fixture's event rule holds for a time, not {other:?}"),
    };

    let r = review(&env, &control);
    assert!(r.issues.is_empty() && r.notes.is_empty(), "{r:?}");
    assert!(r.preview.is_some());
    save(&env, &control);
    rt.set_mode(Mode::Live);
    shows(rt, pad, rest).await;

    // Channel alpha goes up: the pad's derived look is Active.
    rt.run_action(channel_up("alpha")).await.unwrap();
    wait_until("alpha to be up", || {
        reading(&env, &alpha()).1 == Some(text("up"))
    })
    .await;
    shows(rt, pad, look(&control, "active")).await;
    let why = explain(&env, &control);
    assert_eq!(
        (why.layer, why.shown.as_deref(), why.winner),
        ("state", Some("active"), Some(0))
    );
    assert!(why.overlay.is_none());
    let alpha_before = rt.state_hub().read_address(&alpha());
    assert_eq!(alpha_before.status, Status::Known);

    // The first four steps are low: the event does not match, nothing covers the pad.
    for n in 1..=4 {
        rt.run_action(increment()).await.unwrap();
        wait_until("the Counter's announcement", || {
            rec.counter_changes().len() == n
        })
        .await;
        assert_eq!(rec.counter_changes()[n - 1]["level"], "low");
        let why = explain(&env, &control);
        assert!(why.overlay.is_none(), "step {n}");
        assert_eq!(why.shown.as_deref(), Some("active"));
        assert_eq!(led(rt, pad), Some(is(look(&control, "active"))));
    }

    // The fifth is high: the event rule covers the pad with Error, for a while.
    rt.run_action(increment()).await.unwrap();
    wait_until("the Counter's fifth announcement", || {
        rec.counter_changes().len() == 5
    })
    .await;
    assert_eq!(rec.counter_changes()[4]["level"], "high");
    let mut covered = None;
    wait_until("the overlay", || {
        covered = explain(&env, &control).overlay;
        covered.is_some()
    })
    .await;
    let overlay = covered.unwrap();
    assert_eq!(overlay.source, OverlaySource::Event);
    assert_eq!(overlay.look, "error");
    assert_eq!(overlay.rule, Some(1));
    assert!(!overlay.latched);
    let left = overlay.remaining_ms.expect("a held look ends by itself");
    assert!(left > 0 && left <= hold_ms, "{left} of {hold_ms} ms");
    let error = look(&control, "error");
    shows(rt, pad, error).await;
    let why = explain(&env, &control);
    assert_eq!(
        (why.layer, why.shown.as_deref()),
        ("overlay", Some("error"))
    );
    assert_eq!(
        why.derived.as_deref(),
        Some("active"),
        "underneath, the state still says Active"
    );
    assert!(why.rules[1].decided && why.rules[0].decided);
    assert_eq!(
        reading(&env, &alpha()).1,
        Some(text("up")),
        "the event never wrote the state"
    );

    // It ends by itself, and the pad is Active again.
    wait_until("the overlay to end", || {
        explain(&env, &control).overlay.is_none()
    })
    .await;
    shows(rt, pad, look(&control, "active")).await;
    let why = explain(&env, &control);
    assert_eq!((why.layer, why.shown.as_deref()), ("state", Some("active")));
    assert!(why.overlay.is_none());
    assert_eq!(why.derived.as_deref(), Some("active"));
    // The state underneath was never touched: same value, same moment.
    assert_eq!(rt.state_hub().read_address(&alpha()), alpha_before);
}

// ---------------------------------------------------------------------------
// (d) A provider that is not installed keeps the configuration
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread")]
async fn a_rule_on_a_provider_that_is_not_installed_survives_every_edit_that_leaves_it_alone() {
    let orphan = orphan_pad();
    let env = start(vec![orphan.clone()]).await;
    let kept_rules = orphan.feedback.clone();

    // The explanation says what is wrong, and keeps the rule.
    let why = explain(&env, &orphan);
    assert_eq!(
        why.rules[0].reason.as_deref(),
        Some("Nothing named `acme.missing` is installed.")
    );

    // A new name for the pad, the same rules: saved.
    let mut relabelled = orphan.clone();
    relabelled.label = "Still here".into();
    let r = review(&env, &relabelled);
    assert!(r.issues.is_empty(), "{:?}", r.issues);
    assert_eq!(kinds(&r), vec!["missingObservable"]);
    env.rt
        .update_control(SURFACE, PAGE, relabelled.clone())
        .unwrap();
    let now = stored(&env, &orphan.id);
    assert_eq!(now.label, "Still here");
    assert_eq!(now.feedback, kept_rules, "the rule is exactly as it was");
    let file: Value = serde_json::from_slice(&fs::read(env.saved_file()).unwrap()).unwrap();
    let on_disk = file["surfaces"][0]["pages"][0]["controls"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["id"] == orphan.id.as_str())
        .unwrap();
    assert_eq!(
        on_disk["feedback"],
        serde_json::to_value(&kept_rules).unwrap()
    );

    // A valid rule next to it is saved, and the orphan is still first and untouched.
    let mut more = relabelled.clone();
    more.feedback
        .push(FeedbackRule::state(level(), equals("zero")).show("on"));
    assert!(review(&env, &more).issues.is_empty());
    env.rt.update_control(SURFACE, PAGE, more).unwrap();
    let now = stored(&env, &orphan.id);
    assert_eq!(now.feedback.len(), 3);
    assert_eq!(now.feedback[..2], kept_rules[..]);

    // A new rule on the missing state is refused, with the field to mark.
    let mut worse = now.clone();
    worse.feedback.push(
        FeedbackRule::state(StateAddress::new("acme.missing", "other"), Test::HasValue).show("on"),
    );
    let r = review(&env, &worse);
    assert_eq!(r.issues.len(), 1, "{:?}", r.issues);
    assert_eq!(
        (r.issues[0].rule, r.issues[0].field),
        (3, Some(IssueField::State))
    );
    assert!(r.issues[0].message.contains("not a state anything offers"));
    assert_eq!(kinds(&r), vec!["missingObservable", "missingObservable"]);
    let refused = env.rt.update_control(SURFACE, PAGE, worse).unwrap_err();
    assert!(refused.contains("Feedback rule 4"), "{refused}");
    assert_eq!(
        stored(&env, &orphan.id),
        now,
        "a refused save changes nothing"
    );

    // Changing the orphan rule itself makes it a new rule, and it is refused too.
    let mut edited = now.clone();
    edited.feedback[0] = edited.feedback[0].clone().show("elsewhere");
    let r = review(&env, &edited);
    assert_eq!(r.issues.len(), 1);
    assert_eq!(
        (r.issues[0].rule, r.issues[0].field),
        (0, Some(IssueField::State))
    );
    assert!(env.rt.update_control(SURFACE, PAGE, edited).is_err());
    assert_eq!(stored(&env, &orphan.id), now);
}
