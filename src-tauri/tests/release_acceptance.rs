//! The release acceptance scenario (PRODUCTIZATION_PLAN.md section 11),
//! headless, through the same `Runtime`, `PluginHost` and `ArtifactService`
//! calls the Tauri commands make: a clean data folder; the first run
//! (Development, Live, the onboarding flag); the virtual device; a pad from
//! the Action Library; a feedback rule; a Context Engine rule followed when
//! the application in front changes; the Counter example installed and its
//! pad pressed, its look following its level; a Pack exported with the rule;
//! the workspace reset and the plugin removed, as on a computer that never
//! had either; the Pack imported with its rule (the dependency missing, the
//! pad waiting for it, Auto Context left as it was); the plugin installed
//! again and the pad working; a restart from the same folders with
//! everything as it was. Then every hostile file is refused, or (the files
//! that are valid on purpose) only planned, and the settings files keep
//! their bytes.

#[path = "support/acceptance_scenario.rs"]
mod acceptance_scenario;
#[path = "support/artifact_world.rs"]
#[allow(dead_code)]
mod artifact_world;
#[path = "support/artifacts_scenario.rs"]
#[allow(dead_code)]
mod artifacts_scenario;

use std::fs;
use std::path::PathBuf;

use acceptance_scenario::{application, next_event, wait_until, Machine};
use artifacts_scenario::counter_pad;
use launchdeck_lib::actions::definition::Assignment;
use launchdeck_lib::artifacts::{
    ApplyOutcome, DependencyStatus, ExportRequest, ImportDecisions, InspectOutcome, PreviewOutcome,
    RuleOutcome,
};
use launchdeck_lib::context::{ContextRule, ContextSettings, Fallback, Mode as ContextMode};
use launchdeck_lib::core::model::{Control, ControlPos, Visual};
use launchdeck_lib::core::runtime::Mode;
use launchdeck_lib::devices::manager::DeviceStatus;
use launchdeck_lib::devices::virtual_device::VIRTUAL_ID;
use launchdeck_lib::devices::Led;
use launchdeck_lib::persistence::Store;
use launchdeck_lib::surfaces::default_workspace;
use serde_json::json;

const COUNTER: &str = "example.counter";
const CODE: &str = "Code";
const TERMINAL: &str = "Gnome-terminal";
const DEVELOPMENT: &str = "development";
const PRODUCTIVITY: &str = "productivity";
/// The Development surface the Pack adds next to the starter's.
const IMPORTED: &str = "development-2";
/// The pad given an action from the Action Library, free on Development.
const KEYS: ControlPos = ControlPos::Grid { col: 4, row: 0 };
/// Where the Counter's pad is (`examples/feedback-editor-counter.json`).
const PAD: ControlPos = ControlPos::Grid { col: 3, row: 3 };

fn led(v: Visual) -> Led {
    Led {
        color: v.color,
        animation: v.animation,
    }
}

/// The look a pad's rules give for `state` (`None`: its resting look).
fn look(pad: &Control, state: Option<&str>) -> Led {
    led(state.map_or(pad.visual, |s| pad.states[s]))
}

/// The Counter pad's look for a level.
fn counter_look(pad: &Control, level: &str) -> Led {
    let state = match level {
        "zero" => None,
        "low" => Some("active"),
        "high" => Some("warning"),
        _ => Some("error"),
    };
    look(pad, state)
}

fn installed(m: &Machine) -> bool {
    m.rt.catalog().providers.iter().any(|p| p.id == COUNTER)
}

fn control_on(m: &Machine, surface: &str, pos: ControlPos) -> Control {
    m.rt.snapshot().workspace.surface(surface).unwrap().pages[0]
        .control_at(pos)
        .unwrap_or_else(|| panic!("no pad at {pos:?} on {surface}"))
        .clone()
}

fn rule_for(surface: &str) -> ContextRule {
    ContextRule {
        application: application(CODE),
        surface_id: surface.into(),
        page_id: None,
    }
}

/// The Pack: Development and its rule.
fn pack_request() -> ExportRequest {
    serde_json::from_value(json!({
        "kind": "pack",
        "surfaceIds": [DEVELOPMENT],
        "metadata": {
            "id": "acceptance.development",
            "name": "Development",
            "version": "1.0.0",
            "author": { "name": "Release acceptance" },
            "license": "MIT"
        },
        "includeContextRules": true
    }))
    .unwrap()
}

/// (1) A folder that never held LaunchDeck: the starters, the first run to do.
fn clean_start(m: &Machine) {
    let snapshot = m.rt.snapshot();
    assert!(snapshot.notices.is_empty(), "{:?}", snapshot.notices);
    assert_eq!(snapshot.workspace.surfaces, default_workspace().surfaces);
    assert!(!snapshot.workspace.preferences.onboarding_done);
    assert_eq!(snapshot.context.settings, ContextSettings::default());
    assert!(!installed(m));
}

/// (2) The first run: Development chosen, Live, the flag set and saved.
fn first_run(m: &Machine) {
    m.rt.activate_surface(DEVELOPMENT, None).unwrap();
    m.rt.set_mode(Mode::Live);
    m.rt.set_onboarding_done(true).unwrap();
    let state = m.rt.state();
    assert_eq!(state.active_surface_id, DEVELOPMENT);
    assert_eq!(state.mode, Mode::Live);
    let saved = Store::new(&m.data).load(|| panic!("the workspace is saved"));
    assert!(saved.workspace.preferences.onboarding_done);
}

/// (3) The virtual device is connected and shows Development.
async fn device(m: &Machine) {
    let devices = m.rt.state().devices;
    let simulator = devices
        .iter()
        .find(|d| d.identity.id == VIRTUAL_ID)
        .unwrap();
    assert_eq!(simulator.status, DeviceStatus::Connected);
    let terminal = control_on(m, DEVELOPMENT, ControlPos::Grid { col: 0, row: 0 });
    m.until_lit(
        "Development on the device",
        terminal.pos,
        led(terminal.visual),
    )
    .await;
}

/// (4) A pad given a keyboard shortcut from the Action Library, checked as
/// the inspector checks it, saved, and pressed.
async fn library_pad(m: &Machine) {
    let catalog = m.rt.catalog();
    assert!(catalog
        .actions
        .iter()
        .any(|a| a.id == "system.keyboardShortcut"));
    let chosen = Assignment {
        provider: "system".into(),
        action: "keyboardShortcut".into(),
        args: json!({ "keys": "mod+shift+p" }),
    };
    let press = m.rt.validate_assignment(&chosen).unwrap();
    let pad: Control = serde_json::from_value(json!({
        "id": "g.4.0",
        "pos": { "kind": "grid", "col": 4, "row": 0 },
        "label": "Palette",
        "visual": { "color": "#6e7681", "animation": "none" },
        "states": { "active": { "color": "#22c55e", "animation": "none" } },
        "press": press,
    }))
    .unwrap();
    m.rt.update_control(DEVELOPMENT, "home", pad).unwrap();
    m.press(KEYS);
    wait_until("the shortcut to be pressed", || m.keyboard.presses() == 1).await;
}

/// (5) A feedback rule on that pad, reviewed as the editor reviews it: it
/// shows `active` while Auto Context is on.
async fn feedback_rule(m: &Machine) {
    let mut draft = control_on(m, DEVELOPMENT, KEYS);
    draft.feedback = serde_json::from_value(json!([{
        "when": {
            "state": { "provider": "runtime", "id": "context.mode" },
            "is": { "op": "equals", "value": "auto" }
        },
        "show": "active"
    }]))
    .unwrap();
    let review = m.rt.review_feedback(DEVELOPMENT, "home", &draft).unwrap();
    assert!(review.issues.is_empty(), "{:?}", review.issues);
    m.rt.update_control(DEVELOPMENT, "home", draft.clone())
        .unwrap();
    m.until_lit("the resting look, Auto off", KEYS, look(&draft, None))
        .await;
}

/// (6) A rule shows Development for Visual Studio Code; Auto on; another
/// surface by hand, then the editor in front: Development comes back.
async fn context_rule(m: &Machine) {
    m.front(TERMINAL).await;
    m.rt.activate_surface(PRODUCTIVITY, None).unwrap();
    m.rt.update_context(ContextSettings {
        auto: true,
        rules: vec![rule_for(DEVELOPMENT)],
        fallback: Fallback::Keep,
    })
    .unwrap();
    m.front(CODE).await;
    m.until_shown(DEVELOPMENT).await;
    assert_eq!(m.rt.context().mode, ContextMode::Auto);
    let pad = control_on(m, DEVELOPMENT, KEYS);
    m.until_lit("the Auto look", KEYS, look(&pad, Some("active")))
        .await;
}

/// Press the Counter's pad: it counts, and the pad shows the new level.
async fn count(m: &Machine, surface: &str) {
    let pad = control_on(m, surface, PAD);
    let mut events = m.rt.bus().subscribe();
    m.press(PAD);
    let changed = next_event(&mut events, COUNTER, "counter.changed").await;
    let level = changed.payload["level"].as_str().unwrap().to_string();
    assert_ne!(level, "zero", "{changed:?}");
    m.until_lit("the Counter's level", PAD, counter_look(&pad, &level))
        .await;
}

/// (7) The Counter example installed as the Plugins screen installs it, its
/// pad put on Development and pressed.
async fn counter(m: &Machine) {
    m.install_example(COUNTER).await.unwrap();
    m.rescan().await;
    assert!(m.rt.plugin_host().get(COUNTER).unwrap().enabled);
    assert!(installed(m));
    let pad = counter_pad();
    m.rt.validate_assignment(pad.press.as_ref().unwrap())
        .unwrap();
    let review = m.rt.review_feedback(DEVELOPMENT, "home", &pad).unwrap();
    assert!(review.issues.is_empty(), "{:?}", review.issues);
    m.rt.update_control(DEVELOPMENT, "home", pad).unwrap();
    count(m, DEVELOPMENT).await;
}

/// (8) Development exported as a Pack with its rule.
fn export(m: &Machine) -> (Vec<u8>, String) {
    let preview = m.service.export_preview(&pack_request());
    assert_eq!(preview.dependencies.len(), 1);
    assert_eq!(preview.dependencies[0].plugin_id, COUNTER);
    assert!(preview.dependencies[0].required);
    let saved = m.service.export_save(&pack_request()).unwrap();
    let bytes = fs::read(m.exports.join(&saved.file_name)).unwrap();
    (bytes, saved.file_name)
}

/// (9) and (10) The workspace reset, Auto on again with no rules, and the
/// plugin's folder removed: a computer that has neither.
async fn fresh_computer(m: &Machine) {
    m.rt.reset_workspace();
    let ws = m.rt.snapshot().workspace;
    assert_eq!(ws.surfaces, default_workspace().surfaces);
    assert!(ws.preferences.onboarding_done, "a reset keeps preferences");
    assert_eq!(m.rt.context().settings, ContextSettings::default());
    m.rt.update_context(ContextSettings {
        auto: true,
        ..ContextSettings::default()
    })
    .unwrap();
    fs::remove_dir_all(m.plugins().join(COUNTER)).unwrap();
    m.rescan().await;
    assert!(!installed(m));
}

/// (11) The Pack inspected, previewed with its rule ticked, and applied.
fn import(m: &Machine, pack: &[u8], file_name: &str) {
    let auto_before = m.rt.context().settings.auto;
    let presses = m.keyboard.presses();
    let InspectOutcome::Plan { plan } = m.service.inspect(pack, file_name) else {
        panic!("the exported Pack is refused");
    };
    assert_eq!(plan.dependencies[0].status, DependencyStatus::Missing);
    assert_eq!(plan.surfaces[0].local_id, IMPORTED);
    assert_eq!(plan.surfaces[0].missing_providers, [COUNTER]);
    assert_eq!(plan.context_rules[0].outcome, RuleOutcome::NotSelected);
    assert!(plan.ready, "{:?}", plan.blockers);
    let decisions = ImportDecisions {
        context_rules: vec![0],
        ..plan.decisions.clone()
    };
    let PreviewOutcome::Plan { plan } = m.service.preview(&plan.plan_id, &decisions) else {
        panic!("the import is no longer pending");
    };
    let outcome = m
        .service
        .apply(&plan.plan_id, &decisions, &plan.plan_digest);
    let ApplyOutcome::Applied { report } = outcome else {
        panic!("{outcome:?}");
    };
    assert_eq!(report.surfaces_added, [IMPORTED]);
    assert_eq!(report.rules_added, 1);
    assert_eq!(report.missing_dependencies[0].plugin_id, COUNTER);
    assert_eq!(report.auto_context, auto_before);
    assert_eq!(m.keyboard.presses(), presses, "an import pressed a key");
    assert!(m.rt.state().pending.is_empty(), "an import ran an action");
}

/// What the import left: the pad waits for its plugin, the rule is there,
/// Auto Context as it was.
fn imported(m: &Machine) {
    let pad = control_on(m, IMPORTED, PAD);
    let issues =
        m.rt.validate_assignment(pad.press.as_ref().unwrap())
            .unwrap_err();
    assert!(issues[0].message.contains("not installed"), "{issues:?}");
    let context = m.rt.context().settings;
    assert!(context.auto);
    assert_eq!(context.rules, [rule_for(IMPORTED)]);
    assert!(control_on(m, IMPORTED, KEYS).press.is_some());
}

/// (12) The Counter installed again and found by a rescan: the editor in
/// front shows the imported surface, both pads work.
async fn recover(m: &Machine) {
    m.install_example(COUNTER).await.unwrap();
    m.rescan().await;
    assert!(installed(m));
    m.front(TERMINAL).await;
    m.rt.activate_surface(PRODUCTIVITY, None).unwrap();
    m.front(CODE).await;
    m.until_shown(IMPORTED).await;
    count(m, IMPORTED).await;
    let keys = control_on(m, IMPORTED, KEYS);
    m.until_lit("the Auto look", KEYS, look(&keys, Some("active")))
        .await;
    let presses = m.keyboard.presses();
    m.press(KEYS);
    wait_until("the shortcut again", || m.keyboard.presses() == presses + 1).await;
}

/// (13) A restart from the same folders: the same workspace, rules, Auto
/// Context, onboarding flag and plugin. The surface it opens on is the one
/// a person chose last: what Auto showed is not saved.
async fn restart(m: Machine) -> Machine {
    let mut workspace = m.rt.snapshot().workspace;
    workspace.last_active_surface = Some(PRODUCTIVITY.into());
    let context = m.rt.context().settings;
    let m = m.restart().await;
    let snapshot = m.rt.snapshot();
    assert!(snapshot.notices.is_empty(), "{:?}", snapshot.notices);
    assert_eq!(snapshot.workspace, workspace);
    assert_eq!(m.rt.context().settings, context);
    assert!(snapshot.workspace.preferences.onboarding_done);
    assert!(m.rt.plugin_host().get(COUNTER).unwrap().enabled);
    assert!(installed(&m));
    let pad = control_on(&m, IMPORTED, PAD);
    m.rt.validate_assignment(pad.press.as_ref().unwrap())
        .unwrap();
    m
}

/// Valid on purpose (`tests/artifacts_security.rs`): what they launch is
/// reviewed, or can never run.
fn valid_on_purpose(name: &str) -> bool {
    name.starts_with("review-") || name.starts_with("never-runs-")
}

/// Every hostile file: refused, or only planned; the settings files keep
/// their bytes (one that did not exist is not created) and nothing runs.
fn hostile_files(m: &Machine) {
    let digests = m.settings_digests();
    assert_eq!(digests[0].0, "workspace.json");
    let workspace = m.rt.snapshot().workspace;
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/artifacts/hostile");
    let mut seen = 0;
    for entry in fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        let outcome = m.service.inspect(&fs::read(&path).unwrap(), &name);
        match outcome {
            InspectOutcome::Rejected { .. } => assert!(!valid_on_purpose(&name), "{name}"),
            InspectOutcome::Plan { .. } => assert!(valid_on_purpose(&name), "{name} accepted"),
        }
        seen += 1;
    }
    assert!(seen > 30, "only {seen} hostile files");
    assert_eq!(m.settings_digests(), digests);
    assert_eq!(m.rt.snapshot().workspace, workspace);
    assert_eq!(m.keyboard.presses(), 0, "a key was pressed");
    assert!(m.rt.state().pending.is_empty(), "an action is running");
}

#[tokio::test(flavor = "multi_thread")]
async fn from_a_clean_folder_to_a_restart() {
    let m = Machine::first_start().await;
    clean_start(&m);
    first_run(&m);
    device(&m).await;
    library_pad(&m).await;
    feedback_rule(&m).await;
    context_rule(&m).await;
    counter(&m).await;
    let (pack, file_name) = export(&m);
    fresh_computer(&m).await;
    import(&m, &pack, &file_name);
    imported(&m);
    recover(&m).await;
    let m = restart(m).await;
    hostile_files(&m);
}
