//! The Action Library on the real runtime.
//!
//! Real providers (the built-ins, Agent Control and the example plugins
//! compiled from `examples/plugins`) feed one registry. An action found in the
//! catalog is assigned to a pad the way the editor does it (`update_control`),
//! pressed on the virtual Launchpad, executed through the real engine and
//! sandbox, and the event it publishes lights another pad through a binding.
//! Nothing here mocks the Plugin SDK.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use launchdeck_lib::actions::definition::{
    ActionDefinition, Assignment, Availability, Catalog, Cause,
};
use launchdeck_lib::actions::definition::{ProviderKind, ProviderStatus};
use launchdeck_lib::actions::params::validate_defs;
use launchdeck_lib::core::model::{Control, ControlPos, Rgb, Visual, Workspace};
use launchdeck_lib::core::runtime::{NullEmitter, Runtime, RuntimeConfig};
use launchdeck_lib::devices::Led;
use launchdeck_lib::events::Event;
use launchdeck_lib::persistence::Store;
use launchdeck_lib::plugins::keyboard::{Combo, Keyboard};
use launchdeck_lib::plugins::ActionError;
use launchdeck_lib::state::feedback::{FeedbackRule, Hold};
use launchdeck_lib::surfaces::default_workspace;
use serde_json::{json, Value};

struct NoKeyboard;
impl Keyboard for NoKeyboard {
    fn press(&self, _combo: &Combo) -> Result<(), ActionError> {
        Ok(())
    }
}

fn examples() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../examples/plugins")
}

async fn start_with(workspace: Workspace, store: Option<Store>, plugins: bool) -> Arc<Runtime> {
    let rt = Runtime::start(RuntimeConfig {
        workspace,
        store,
        emitter: Arc::new(NullEmitter),
        keyboard: Arc::new(NoKeyboard),
        scan_hardware: false,
        serve_external: false,
        demo_pace: 0.05,
        plugin_dirs: if plugins { vec![examples()] } else { vec![] },
        secrets: None,
        observer: None,
    })
    .await;
    wait_until("first frame", || {
        !rt.virtual_handle().frame().leds.is_empty()
    })
    .await;
    rt
}

async fn start() -> Arc<Runtime> {
    start_with(default_workspace(), None, true).await
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

fn shows(rt: &Runtime, col: u8, row: u8, v: Visual) -> bool {
    led(rt, col, row)
        == Led {
            color: v.color,
            animation: v.animation,
        }
}

fn tap(rt: &Runtime, col: u8, row: u8) {
    let h = rt.virtual_handle();
    h.press(ControlPos::grid(col, row), 100).unwrap();
    h.release(ControlPos::grid(col, row)).unwrap();
}

fn events(rt: &Runtime) -> Vec<Event> {
    rt.snapshot().recent_events
}

fn find<'a>(c: &'a Catalog, id: &str) -> &'a ActionDefinition {
    c.actions
        .iter()
        .find(|a| a.id == id)
        .unwrap_or_else(|| panic!("`{id}` is not in the catalog"))
}

const BLUE: Rgb = Rgb::new(40, 110, 255);
const GREEN: Rgb = Rgb::new(30, 220, 90);
const AMBER: Rgb = Rgb::new(255, 170, 0);
const GREY: Rgb = Rgb::new(60, 60, 60);
const RED: Rgb = Rgb::new(255, 40, 50);

/// What a schema 3 binding became: an event rule that keeps its look until the
/// next event (`$payload.x` names the look from a payload field).
fn bind(source: &str, event: &str, set_state: &str) -> Vec<FeedbackRule> {
    let rule = FeedbackRule::event(source, event);
    let rule = match set_state.strip_prefix("$payload.") {
        Some(field) => rule.show_from_field(field),
        None => rule.show(set_state),
    };
    vec![rule.hold(Hold::Latch)]
}

/// A blank surface to work on, made the way the editor makes one, and made active.
fn blank(rt: &Runtime) -> (String, String) {
    let surface = rt.create_surface("Library").unwrap();
    let page = rt.snapshot().workspace.surface(&surface).unwrap().pages[0]
        .id
        .clone();
    rt.activate_surface(&surface, None).unwrap();
    (surface, page)
}

fn pad(col: u8, row: u8, label: &str) -> Control {
    let mut c = Control::new(ControlPos::grid(col, row), label, BLUE);
    c.states.insert("error".into(), Visual::flash(RED));
    c
}

/// Put a definition on a pad exactly as the editor does after the person
/// picks it: its provider, its action and the arguments of the form.
fn assign(
    rt: &Runtime,
    at: &(String, String),
    mut control: Control,
    def: &ActionDefinition,
    args: Value,
) -> Control {
    control.press = Some(Assignment::new(&def.provider, &def.action, args));
    rt.update_control(&at.0, &at.1, control.clone()).unwrap();
    rt.snapshot()
        .workspace
        .surface(&at.0)
        .unwrap()
        .page(&at.1)
        .unwrap()
        .control(&control.id)
        .unwrap()
        .clone()
}

// ---------------------------------------------------------------------------
// Composition
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread")]
async fn one_catalog_holds_built_in_agent_control_and_installed_plugin_actions() {
    let rt = start().await;
    let c = rt.catalog();

    let provider = |id: &str| {
        c.providers
            .iter()
            .find(|p| p.id == id)
            .unwrap_or_else(|| panic!("provider {id}"))
    };
    for builtin in ["navigation", "system", "shell", "http", "demo"] {
        assert_eq!(provider(builtin).kind, ProviderKind::Builtin, "{builtin}");
    }
    assert_eq!(provider("agent-control").kind, ProviderKind::Module);
    for plugin in ["example.hello", "example.counter", "example.ping"] {
        assert_eq!(provider(plugin).kind, ProviderKind::Plugin, "{plugin}");
        assert_eq!(provider(plugin).status, ProviderStatus::Ready, "{plugin}");
    }

    // Plugin actions come from `plugin.json` alone: no second registration.
    let hello = find(&c, "example.hello.hello.say");
    assert_eq!(hello.name, "Say hello");
    assert_eq!(hello.category, "Greetings");
    assert!(hello.keywords.contains(&"greet".to_string()));
    assert_eq!(hello.params[0].name, "name");
    // A schema 1 manifest (the C plugin) is listed too, with the plugin as its category.
    let ping = find(&c, "example.ping.ping.send");
    assert_eq!(ping.category, "Ping Plugin (C)");

    // Agent Control goes through the same registry, with real parameters.
    let task = find(&c, "agent-control.newTask");
    assert!(task.params.iter().any(|p| p.name == "task" && p.required));
    assert_eq!(
        find(&c, "agent-control.approve")
            .appearance
            .as_ref()
            .unwrap()
            .label
            .as_deref(),
        Some("Approve")
    );

    // Ids are unique, every schema is valid, only shell and HTTP are privileged.
    let mut ids: Vec<&str> = c.actions.iter().map(|a| a.id.as_str()).collect();
    let total = ids.len();
    ids.sort_unstable();
    ids.dedup();
    assert_eq!(ids.len(), total, "duplicate ids");
    assert!(c.issues.is_empty(), "{:?}", c.issues);
    for a in &c.actions {
        validate_defs(&a.params).unwrap_or_else(|e| panic!("{}: {e}", a.id));
        assert!(
            !a.name.is_empty() && !a.description.is_empty() && !a.category.is_empty(),
            "{}",
            a.id
        );
    }
    let mut privileged: Vec<&str> = c
        .actions
        .iter()
        .filter(|a| a.privileged)
        .map(|a| a.id.as_str())
        .collect();
    privileged.sort_unstable();
    assert_eq!(privileged, ["http.request", "shell.execute"]);

    // The same catalog travels in the snapshot, so the UI has it from the first paint.
    assert_eq!(rt.snapshot().catalog.actions.len(), c.actions.len());
}

#[tokio::test(flavor = "multi_thread")]
async fn every_action_of_every_starter_surface_is_a_registered_definition() {
    let rt = start().await;
    let c = rt.catalog();
    let ws = rt.snapshot().workspace;
    let mut seen = 0;
    for s in &ws.surfaces {
        for p in &s.pages {
            for ctl in &p.controls {
                for a in [&ctl.press, &ctl.long_press].into_iter().flatten() {
                    find(&c, &a.definition_id());
                    // And it validates exactly as saving would validate it.
                    rt.validate_assignment(a)
                        .unwrap_or_else(|e| panic!("{}/{}: {e:?}", s.id, ctl.label));
                    seen += 1;
                }
            }
        }
    }
    assert!(seen > 100, "{seen}");
}

// ---------------------------------------------------------------------------
// The full chain: catalog -> pad -> press -> sandbox -> event -> binding -> LED
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread")]
async fn a_plugin_action_found_in_the_catalog_runs_from_a_pad_and_its_event_lights_another() {
    let rt = start().await;
    let at = blank(&rt);
    let catalog = rt.catalog();

    // The person finds "Increment" in the library and gives it to a pad.
    let increment = find(&catalog, "example.counter.counter.increment");
    assert!(increment.availability.is_available());
    assert_eq!(
        increment.appearance.as_ref().unwrap().icon.as_deref(),
        Some("+")
    );
    let plus = assign(&rt, &at, pad(0, 0, "Plus"), increment, json!({}));
    assert_eq!(
        plus.press.as_ref().unwrap(),
        &Assignment::new("example.counter", "counter.increment", json!({}))
    );

    // A second pad only displays the level the plugin reports.
    let mut level = pad(1, 0, "Level");
    level.visual = Visual::solid(GREY);
    level.states.insert("low".into(), Visual::solid(BLUE));
    level.states.insert("high".into(), Visual::solid(AMBER));
    level.feedback = bind("example.counter", "counter.changed", "$payload.level");
    rt.update_control(&at.0, &at.1, level).unwrap();

    // And a third runs Hello with a setting from the generated form.
    let say = find(&catalog, "example.hello.hello.say");
    let mut hello = pad(2, 0, "Hello");
    hello.states.insert("greeted".into(), Visual::solid(GREEN));
    hello.feedback = bind("example.hello", "hello.completed", "greeted");
    assign(&rt, &at, hello, say, json!({ "name": "Leroy" }));

    wait_until("the level pad starts idle", || {
        shows(&rt, 1, 0, Visual::solid(GREY))
    })
    .await;
    tap(&rt, 0, 0);
    wait_until("the plugin ran and the level pad followed", || {
        shows(&rt, 1, 0, Visual::solid(BLUE))
    })
    .await;
    // The action went through the usual events, named by definition id.
    let log = events(&rt);
    assert!(log.iter().any(|e| e.kind == "action.started"
        && e.payload["action"] == "example.counter.counter.increment"));
    assert!(log.iter().any(|e| e.kind == "action.completed"
        && e.payload["action"] == "example.counter.counter.increment"));
    assert!(log
        .iter()
        .any(|e| e.source == "example.counter" && e.kind == "counter.changed"));

    tap(&rt, 2, 0);
    wait_until("hello lit its pad", || {
        shows(&rt, 2, 0, Visual::solid(GREEN))
    })
    .await;
    let greeted = events(&rt)
        .into_iter()
        .find(|e| e.kind == "hello.completed")
        .expect("hello.completed");
    assert_eq!(greeted.payload["name"], "Leroy");
}

#[tokio::test(flavor = "multi_thread")]
async fn the_inspector_test_run_reports_the_outcome_through_the_same_events() {
    let rt = start().await;
    // The demo agents publish a lot; listen from the start rather than read the tail.
    let mut rx = rt.bus().subscribe();
    let ok = rt
        .run_action(Assignment::new(
            "demo",
            "startTask",
            json!({ "agent": "alpha" }),
        ))
        .await
        .unwrap();
    assert_eq!(ok["state"], "running");
    let err = rt
        .run_action(Assignment::new("demo", "startTask", json!({})))
        .await
        .unwrap_err();
    assert!(err.contains("Agent is required"), "{err}");
    let mut log = Vec::new();
    while let Ok(e) = rx.try_recv() {
        log.push(e);
    }
    let of = |kind: &str| {
        log.iter()
            .filter(|e| e.kind == kind && e.payload["action"] == "demo.startTask")
            .count()
    };
    // Both runs start; the valid one completes and the invalid one fails with its message.
    assert_eq!(of("action.started"), 2);
    assert_eq!(of("action.completed"), 1);
    assert_eq!(of("action.failed"), 1);
}

// ---------------------------------------------------------------------------
// Saving: validation, defaults, replacement
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread")]
async fn saving_validates_applies_defaults_and_stores_only_the_semantic_assignment() {
    let dir = tempfile::tempdir().unwrap();
    let rt = start_with(default_workspace(), Some(Store::new(dir.path())), true).await;
    let at = blank(&rt);
    let c = rt.catalog();

    // Defaults are written into the saved assignment.
    let http = find(&c, "http.request");
    let saved = assign(
        &rt,
        &at,
        pad(0, 0, "Deploy"),
        http,
        json!({ "url": "https://example.com/deploy" }),
    );
    assert_eq!(
        saved.press.unwrap().args,
        json!({ "method": "GET", "url": "https://example.com/deploy" })
    );
    let cont = find(&c, "agent-control.continue");
    let saved = assign(&rt, &at, pad(1, 0, "Go"), cont, json!({}));
    assert_eq!(saved.press.unwrap().args, json!({ "text": "continue" }));

    // Malformed assignments are refused with a message that names the setting.
    let refuse = |args: Value, provider: &str, action: &str| {
        let mut c = pad(2, 0, "Bad");
        c.press = Some(Assignment::new(provider, action, args));
        rt.update_control(&at.0, &at.1, c).unwrap_err()
    };
    assert!(refuse(json!({}), "agent-control", "newTask").contains("Task is required"));
    assert!(refuse(
        json!({ "task": "x", "extra": 1 }),
        "agent-control",
        "newTask"
    )
    .contains("not a setting"));
    assert!(refuse(
        json!({ "method": "TRACE", "url": "https://a.b" }),
        "http",
        "request"
    )
    .contains("listed choices"));
    assert!(
        refuse(json!({ "url": "file:///etc/passwd" }), "system", "openUrl").contains("not allowed")
    );
    assert!(
        refuse(json!({ "keys": "wat+c" }), "system", "keyboardShortcut").contains("not a modifier")
    );
    assert!(refuse(json!({ "slot": 9 }), "agent-control", "focusSlot").contains("between 0 and 7"));
    assert!(refuse(json!({}), "acme.gone", "do").contains("acme.gone"));
    assert!(refuse(json!({}), "system", "nope").contains("no action"));

    // What reaches the disk is the semantic assignment and nothing else.
    drop(rt);
    let raw: Value =
        serde_json::from_str(&std::fs::read_to_string(dir.path().join("workspace.json")).unwrap())
            .unwrap();
    let surface = raw["surfaces"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["id"] == "library")
        .unwrap();
    let deploy = surface["pages"][0]["controls"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["label"] == "Deploy")
        .unwrap();
    assert_eq!(
        deploy["press"],
        json!({ "provider": "http", "action": "request",
                "args": { "method": "GET", "url": "https://example.com/deploy" } })
    );
    assert!(
        !raw.to_string().contains("\"availability\""),
        "runtime state never reaches the file"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn replacing_an_assignment_changes_only_the_assignment() {
    let rt = start().await;
    let at = blank(&rt);
    let c = rt.catalog();

    let mut control = pad(0, 0, "Mine");
    control.description = "My pad".into();
    control.icon = Some("★".into());
    control.visual = Visual::solid(Rgb::new(1, 2, 3));
    control
        .states
        .insert("success".into(), Visual::solid(GREEN));
    control.feedback = bind("demo", "plugin.state.changed", "success");
    control.long_press = Some(Assignment::new("demo", "resetAll", json!({})));
    let first = assign(
        &rt,
        &at,
        control,
        find(&c, "system.openUrl"),
        json!({ "url": "https://github.com" }),
    );

    let second = assign(
        &rt,
        &at,
        first.clone(),
        find(&c, "http.request"),
        json!({ "url": "https://example.com" }),
    );
    assert_eq!(second.press.as_ref().unwrap().provider, "http");
    assert_ne!(second.press, first.press);
    // Look, text, bindings and the other slot are exactly as they were.
    let mut expected = first.clone();
    expected.press = second.press.clone();
    assert_eq!(second, expected);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_long_press_assignment_is_validated_and_runs_like_a_press() {
    let rt = start().await;
    let at = blank(&rt);
    let c = rt.catalog();
    let mut control = pad(0, 0, "Hold");
    control.press = Some(Assignment::new(
        "demo",
        "startTask",
        json!({ "agent": "alpha" }),
    ));
    control.long_press = Some(Assignment::new("demo", "stopTask", json!({})));
    assert!(rt
        .update_control(&at.0, &at.1, control.clone())
        .unwrap_err()
        .starts_with("Long press action"));
    let stop = find(&c, "demo.stopTask");
    control.long_press = Some(Assignment::new(
        &stop.provider,
        &stop.action,
        json!({ "agent": "alpha" }),
    ));
    rt.update_control(&at.0, &at.1, control).unwrap();
}

// ---------------------------------------------------------------------------
// Availability, disabled and missing providers
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread")]
async fn a_disabled_plugin_stays_listed_unavailable_and_its_pads_keep_working_when_it_returns() {
    let rt = start().await;
    let at = blank(&rt);
    let inc = find(&rt.catalog(), "example.counter.counter.increment").clone();
    let plus = assign(&rt, &at, pad(0, 0, "Plus"), &inc, json!({}));

    rt.plugin_host()
        .set_enabled("example.counter", false)
        .unwrap();
    let c = rt.catalog();
    let listed = find(&c, "example.counter.counter.increment");
    match &listed.availability {
        Availability::Unavailable { cause, reason } => {
            assert_eq!(*cause, Cause::Provider);
            assert!(reason.contains("turned off"), "{reason}");
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(
        c.providers
            .iter()
            .find(|p| p.id == "example.counter")
            .unwrap()
            .status,
        ProviderStatus::Disabled
    );

    // Nothing was deleted, and the pad is still editable: relabel it.
    let mut renamed = plus.clone();
    renamed.label = "Plus (off)".into();
    rt.update_control(&at.0, &at.1, renamed.clone()).unwrap();
    let stored = rt
        .snapshot()
        .workspace
        .surface(&at.0)
        .unwrap()
        .page(&at.1)
        .unwrap()
        .control(&plus.id)
        .unwrap()
        .clone();
    assert_eq!(stored.press, plus.press);
    assert_eq!(stored.label, "Plus (off)");

    // Pressing fails loudly, with the plugin named.
    let before = events(&rt).len();
    tap(&rt, 0, 0);
    wait_until("the press failed", || {
        events(&rt).iter().skip(before).any(|e| {
            e.kind == "action.failed"
                && e.payload["error"]
                    .as_str()
                    .is_some_and(|m| m.contains("disabled"))
        })
    })
    .await;

    // Back on, the same pad works again with no re-assignment.
    rt.plugin_host()
        .set_enabled("example.counter", true)
        .unwrap();
    assert!(find(&rt.catalog(), "example.counter.counter.increment")
        .availability
        .is_available());
    let before = events(&rt).len();
    tap(&rt, 0, 0);
    wait_until("the press completed", || {
        events(&rt).iter().skip(before).any(|e| {
            e.kind == "action.completed"
                && e.payload["action"] == "example.counter.counter.increment"
        })
    })
    .await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_surface_with_a_missing_provider_loads_stays_editable_and_fails_only_on_press() {
    // A workspace saved while a plugin that is no longer installed was there.
    let mut ws = default_workspace();
    let mut gone = pad(0, 0, "Gone");
    gone.press = Some(Assignment::new("acme.gone", "do.it", json!({ "level": 3 })));
    gone.long_press = Some(Assignment::new("demo", "resetAll", json!({})));
    let id = "legacy";
    ws.surfaces
        .push(launchdeck_lib::surfaces::blank_surface(id, "Legacy"));
    ws.surface_mut(id).unwrap().pages[0]
        .controls
        .push(gone.clone());
    let rt = start_with(ws, None, false).await;

    // Loaded, intact, and not part of the catalog (there is nothing to show for it).
    let kept = rt
        .snapshot()
        .workspace
        .surface(id)
        .unwrap()
        .page("home")
        .unwrap()
        .control(&gone.id)
        .unwrap()
        .clone();
    assert_eq!(kept, gone);
    assert!(rt.catalog().providers.iter().all(|p| p.id != "acme.gone"));

    // Editing anything else about the pad works; the assignment is untouched.
    let mut relabelled = kept.clone();
    relabelled.label = "Still here".into();
    relabelled.visual = Visual::solid(Rgb::new(9, 9, 9));
    rt.update_control(id, "home", relabelled).unwrap();
    let after = rt
        .snapshot()
        .workspace
        .surface(id)
        .unwrap()
        .page("home")
        .unwrap()
        .control(&gone.id)
        .unwrap()
        .clone();
    assert_eq!(after.press, gone.press);
    assert_eq!(after.label, "Still here");

    // But a brand new assignment to a missing provider is refused.
    let mut fresh = pad(1, 0, "Fresh");
    fresh.press = Some(Assignment::new("acme.gone", "other", json!({})));
    assert!(rt.update_control(id, "home", fresh).is_err());

    // Pressing the pad fails visibly and nothing else is affected.
    rt.activate_surface(id, None).unwrap();
    tap(&rt, 0, 0);
    wait_until("the press failed", || {
        events(&rt).iter().any(|e| {
            e.kind == "action.failed"
                && e.payload["error"]
                    .as_str()
                    .is_some_and(|m| m.contains("acme.gone"))
        })
    })
    .await;
}

#[tokio::test(flavor = "multi_thread")]
async fn agent_control_availability_in_the_catalog_is_the_one_the_pads_use() {
    let rt = start_with(default_workspace(), None, false).await;
    let reason = |c: &Catalog, id: &str| match &find(c, id).availability {
        Availability::Available => None,
        Availability::Unavailable { cause, reason } => {
            assert_eq!(*cause, Cause::State, "{id}");
            Some(reason.clone())
        }
    };

    // Nothing is selected: commands cannot run, but they are listed with the reason.
    let idle = rt.catalog();
    assert_eq!(
        reason(&idle, "agent-control.approve").as_deref(),
        Some("No session selected.")
    );
    assert!(reason(&idle, "agent-control.pause").is_some());
    // Selection and demo actions never depend on a session.
    assert!(reason(&idle, "agent-control.focusSlot").is_none());
    assert!(reason(&idle, "agent-control.runDemo").is_none());

    // Run the simulated agents; when one waits for approval, Approve becomes available.
    rt.run_action(Assignment::new("agent-control", "runDemo", json!({})))
        .await
        .unwrap();
    wait_until("approval available", || {
        reason(&rt.catalog(), "agent-control.approve").is_none()
    })
    .await;
    assert!(reason(&rt.catalog(), "agent-control.reject").is_none());
}

#[tokio::test(flavor = "multi_thread")]
async fn assignments_survive_a_restart_unchanged() {
    let dir = tempfile::tempdir().unwrap();
    let rt = start_with(default_workspace(), Some(Store::new(dir.path())), true).await;
    let at = blank(&rt);
    let c = rt.catalog();
    let saved = assign(
        &rt,
        &at,
        pad(0, 0, "Hello"),
        find(&c, "example.hello.hello.say"),
        json!({ "name": "Leroy" }),
    );
    drop(rt);

    let loaded = Store::new(dir.path()).load_or_default(|| panic!("workspace file must exist"));
    let rt2 = start_with(loaded, Some(Store::new(dir.path())), true).await;
    let again = rt2
        .snapshot()
        .workspace
        .surface(&at.0)
        .unwrap()
        .page(&at.1)
        .unwrap()
        .control(&saved.id)
        .unwrap()
        .clone();
    assert_eq!(again.press, saved.press);
    rt2.validate_assignment(again.press.as_ref().unwrap())
        .unwrap();
}
