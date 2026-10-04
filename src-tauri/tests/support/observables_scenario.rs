//! A real Observable Catalog, its messages and three explanations, taken from
//! the real runtime with the example plugins loaded and doing things. Shared
//! by the `dump_observables` example and the contract test, so the TypeScript
//! schemas are checked against what Rust emits.
//!
//! The scenario covers every case the wire format has: a state that is known,
//! unknown and unavailable; a scoped state with instances; a plugin that is
//! off and one that failed to load; the three kinds of message the UI hears;
//! and an explanation with a temporary look on top, one with a look that does
//! not exist, and one whose states are unknown or gone.

use std::fs;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use launchdeck_lib::core::model::{
    Assignment, Control, ControlPos, Page, Preferences, Rgb, Surface, Visual, Workspace,
    SCHEMA_VERSION,
};
use launchdeck_lib::core::runtime::{ui, Runtime, RuntimeConfig, UiEmitter};
use launchdeck_lib::plugins::keyboard::{Combo, Keyboard};
use launchdeck_lib::plugins::ActionError;
use launchdeck_lib::state::feedback::{FeedbackRule, Hold, Test};
use launchdeck_lib::state::StateAddress;
use serde_json::{json, Value};

struct NoKeyboard;
impl Keyboard for NoKeyboard {
    fn press(&self, _combo: &Combo) -> Result<(), ActionError> {
        Ok(())
    }
}

#[derive(Default)]
struct Recorder(Mutex<Vec<(String, Value)>>);

impl UiEmitter for Recorder {
    fn emit(&self, event: &str, payload: Value) {
        self.0.lock().unwrap().push((event.to_string(), payload));
    }
}

impl Recorder {
    fn messages(&self) -> Vec<Value> {
        self.0
            .lock()
            .unwrap()
            .iter()
            .filter(|(e, _)| e == ui::OBSERVABLES)
            .map(|(_, v)| v.clone())
            .collect()
    }
    fn clear(&self) {
        self.0.lock().unwrap().clear();
    }
}

fn copy_dir(from: &Path, to: &Path) {
    fs::create_dir_all(to).unwrap();
    for entry in fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        if entry.file_name() == "node_modules" {
            continue;
        }
        if entry.file_type().unwrap().is_file() {
            fs::copy(entry.path(), to.join(entry.file_name())).unwrap();
        }
    }
}

fn workspace() -> Workspace {
    let rest = Rgb::new(12, 12, 12);
    let level = || StateAddress::new("example.counter", "level");

    // A state rule and an event rule on one control.
    let mut shown = Control::new(ControlPos::grid(0, 0), "Level", rest);
    shown
        .states
        .insert("blink".into(), Visual::solid(Rgb::new(255, 255, 255)));
    shown.feedback = vec![
        FeedbackRule::state(level(), Test::HasValue).show_from_value(),
        FeedbackRule::event("example.counter", "counter.changed")
            .show("blink")
            .hold(Hold::For(30_000)),
    ];
    // A rule that decides on a look the control does not have.
    let mut missing = Control::new(ControlPos::grid(1, 0), "Missing look", rest);
    missing.feedback = vec![FeedbackRule::state(level(), Test::HasValue).show("nope")];
    // States nobody offers, one that has not reported, and one whose plugin is off.
    let mut silent = Control::new(ControlPos::grid(2, 0), "Silent", rest);
    silent.feedback = vec![
        FeedbackRule::state(StateAddress::new("acme.missing", "thing"), Test::HasValue).show("x"),
        FeedbackRule::state(
            StateAddress::new("example.channels", "status").with("channel", "zeta"),
            Test::HasValue,
        )
        .show("x"),
        FeedbackRule::state(StateAddress::new("example.hello", "never"), Test::HasValue).show("x"),
    ];

    Workspace {
        schema_version: SCHEMA_VERSION,
        surfaces: vec![Surface {
            id: "scenario".into(),
            name: "Scenario".into(),
            description: String::new(),
            icon: None,
            category: "custom".into(),
            pages: vec![Page {
                id: "home".into(),
                name: "Home".into(),
                controls: vec![shown, missing, silent],
            }],
            context: None,
        }],
        last_active_surface: Some("scenario".into()),
        preferences: Preferences::default(),
    }
}

async fn settle() {
    tokio::time::sleep(Duration::from_millis(250)).await;
}

fn id_at(rt: &Runtime, col: u8) -> String {
    rt.snapshot().workspace.surfaces[0].pages[0]
        .control_at(ControlPos::grid(col, 0))
        .unwrap()
        .id
        .clone()
}

pub async fn capture() -> Value {
    let examples = Path::new(env!("CARGO_MANIFEST_DIR")).join("../examples/plugins");
    let root = tempfile::tempdir().unwrap();
    let plugins = root.path().join("plugins");
    for name in ["hello", "counter", "channels", "c-ping"] {
        copy_dir(&examples.join(name), &plugins.join(name));
    }
    // A plugin that fails to load: its events stay listed, unavailable, with the reason.
    copy_dir(&examples.join("c-ping"), &plugins.join("broken"));
    let module = wat::parse_str(r#"(module (import "wasi_snapshot_preview1" "fd_write" (func (param i32 i32 i32 i32) (result i32))))"#).unwrap();
    fs::write(plugins.join("broken/ping.wasm"), module).unwrap();
    let broken_path = plugins.join("broken/plugin.json");
    let mut broken: Value =
        serde_json::from_str(&fs::read_to_string(&broken_path).unwrap()).unwrap();
    broken["id"] = json!("example.broken");
    fs::write(&broken_path, serde_json::to_string_pretty(&broken).unwrap()).unwrap();

    let recorder = Arc::new(Recorder::default());
    let rt = Runtime::start(RuntimeConfig {
        workspace: workspace(),
        store: None,
        emitter: recorder.clone(),
        keyboard: Arc::new(NoKeyboard),
        scan_hardware: false,
        serve_external: false,
        demo_pace: 1.0,
        plugin_dirs: vec![plugins],
    })
    .await;
    settle().await;
    recorder.clear();

    // Values: the Counter reports, two channels report, one channel is unknown.
    let run = |provider: &str, action: &str, args: Value| {
        let rt = rt.clone();
        let a = Assignment::new(provider, action, args);
        async move { rt.run_action(a).await.unwrap() }
    };
    run("example.counter", "counter.increment", json!({})).await;
    run(
        "example.channels",
        "channel.up",
        json!({ "channel": "alpha" }),
    )
    .await;
    run(
        "example.channels",
        "channel.down",
        json!({ "channel": "beta" }),
    )
    .await;
    settle().await;
    let values = recorder
        .messages()
        .into_iter()
        .find(|m| {
            m["observables"]
                .as_array()
                .is_some_and(|o| o.iter().any(|u| u["id"] == "example.counter.level"))
        })
        .expect("a message about the counter");

    // The explanations are taken while the event's look is still on.
    let explain = |col: u8| {
        let id = id_at(&rt, col);
        serde_json::to_value(rt.explain_control("scenario", "home", &id).unwrap()).unwrap()
    };
    let overlay = explain(0);
    let missing_look = explain(1);

    // A plugin is turned off: its status and everything it offers.
    recorder.clear();
    rt.plugin_host()
        .set_enabled("example.hello", false)
        .unwrap();
    settle().await;
    let lifecycle = recorder
        .messages()
        .into_iter()
        .find(|m| m["providers"].as_array().is_some_and(|p| !p.is_empty()))
        .expect("a lifecycle message");
    let silent = explain(2);

    // The catalog as it is now: one plugin off, one failed, the rest running.
    let catalog = serde_json::to_value(rt.observables()).unwrap();

    // And back on: it is loaded again, so the definitions are read again.
    recorder.clear();
    rt.plugin_host().set_enabled("example.hello", true).unwrap();
    settle().await;
    let catalog_message = recorder
        .messages()
        .into_iter()
        .find(|m| m["catalog"] == true)
        .expect("a catalog message");

    let out = json!({
        "catalog": catalog,
        "messages": {
            "values": values,
            "lifecycle": lifecycle,
            "catalog": catalog_message,
        },
        "explain": {
            "overlay": overlay,
            "missingLook": missing_look,
            "unknown": silent,
        },
    });
    // Keep the fixture free of machine paths.
    let prefix = format!("{}/", root.path().display());
    let text = serde_json::to_string_pretty(&out)
        .unwrap()
        .replace(&prefix, "<data>/");
    serde_json::from_str(&text).unwrap()
}
