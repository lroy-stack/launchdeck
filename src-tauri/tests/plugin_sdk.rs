//! The Plugin SDK, end to end on the real runtime.
//!
//! Real plugins built outside the core (`examples/plugins`: Hello and Counter
//! in AssemblyScript through the public SDK, Ping in plain C) are discovered on
//! disk, validated, loaded into the sandbox and driven from pads. The
//! assertions read the virtual Launchpad's LED frame, so they check the chain
//!
//!   pad press -> action -> plugin runs -> plugin publishes an event
//!   -> Event Bus -> surface binding -> control state -> renderer -> LED
//!
//! A few hostile plugins written in WAT prove the failure behaviour: the
//! runtime, the other plugins and Agent Control keep working. Modules frozen as
//! SDK 0.1 built them (`tests/fixtures/plugins/sdk-0.1`) prove that plugins
//! built before the network capability still load and run.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use futures_util::{SinkExt, StreamExt};
use launchdeck_lib::actions::ActionEngine;
use launchdeck_lib::core::model::{
    Assignment, Control, ControlPos, Page, Preferences, Rgb, Surface, Visual, Workspace,
    SCHEMA_VERSION,
};
use launchdeck_lib::core::runtime::{NullEmitter, Runtime, RuntimeConfig};
use launchdeck_lib::devices::Led;
use launchdeck_lib::events::{Event, EventBus};
use launchdeck_lib::persistence::Store;
use launchdeck_lib::plugins::external::{Limits, PluginHost, PluginStatus};
use launchdeck_lib::plugins::keyboard::{Combo, Keyboard};
use launchdeck_lib::plugins::{ActionError, PluginRegistry};
use launchdeck_lib::state::feedback::{FeedbackRule, Hold};
use launchdeck_lib::state::{Scalar, StateAddress, StateHub, Status};
use serde_json::{json, Value};
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::Message;

#[path = "support/wat_plugins.rs"]
#[allow(dead_code)]
mod wat;

struct NoKeyboard;
impl Keyboard for NoKeyboard {
    fn press(&self, _combo: &Combo) -> Result<(), ActionError> {
        Ok(())
    }
}

fn examples() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../examples/plugins")
}

// ---------------------------------------------------------------------------
// The surface under test. Nothing here knows how a plugin works inside: it
// calls actions and binds to events, like any other surface.
// ---------------------------------------------------------------------------

const BLUE: Rgb = Rgb::new(40, 110, 255);
const GREEN: Rgb = Rgb::new(30, 220, 90);
const AMBER: Rgb = Rgb::new(255, 170, 0);
const RED: Rgb = Rgb::new(255, 40, 50);
const GREY: Rgb = Rgb::new(60, 60, 60);

fn plugin(id: &str, action: &str, args: Value) -> Assignment {
    Assignment::new(id, action, args)
}

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

fn pad(col: u8, row: u8, label: &str, press: Option<Assignment>) -> Control {
    let mut c = Control::new(ControlPos::grid(col, row), label, BLUE);
    c.press = press;
    c.states.insert("error".into(), Visual::flash(RED));
    c
}

/// Pads: (0,0) Hello, (1,0) + , (2,0) -, (3,0) reset, (4,0) level display,
/// (0,1) Ping (C), (0,2) the crashing plugin, (0,3) a plugin that is disabled
/// later, with no binding so the runtime shows its automatic result.
fn workspace() -> Workspace {
    let mut hello = pad(
        0,
        0,
        "Hello",
        Some(plugin(
            "example.hello",
            "hello.say",
            json!({"name": "Leroy"}),
        )),
    );
    hello.states.insert("greeted".into(), Visual::solid(GREEN));
    hello.feedback = bind("example.hello", "hello.completed", "greeted");

    let plus = pad(
        1,
        0,
        "+",
        Some(plugin("example.counter", "counter.increment", json!({}))),
    );
    let minus = pad(
        2,
        0,
        "-",
        Some(plugin("example.counter", "counter.decrement", json!({}))),
    );
    let reset = pad(
        3,
        0,
        "0",
        Some(plugin("example.counter", "counter.reset", json!({}))),
    );
    let mut level = pad(4, 0, "Level", None);
    level.visual = Visual::solid(GREY);
    level.states.insert("zero".into(), Visual::solid(GREY));
    level.states.insert("low".into(), Visual::solid(BLUE));
    level.states.insert("high".into(), Visual::solid(AMBER));
    level.states.insert("max".into(), Visual::flash(RED));
    level.feedback = bind("example.counter", "counter.changed", "$payload.level");

    let mut ping = pad(
        0,
        1,
        "Ping",
        Some(plugin("example.ping", "ping.send", json!({}))),
    );
    ping.states.insert("sent".into(), Visual::pulse(GREEN));
    ping.feedback = bind("example.ping", "ping.sent", "sent");

    let crash = pad(0, 2, "Crash", Some(plugin("test.crash", "a", json!({}))));
    let mut off = pad(0, 3, "Off", Some(plugin("test.off", "a", json!({}))));
    off.states.insert("success".into(), Visual::solid(GREEN));

    Workspace {
        schema_version: SCHEMA_VERSION,
        surfaces: vec![Surface {
            id: "sdk".into(),
            name: "SDK".into(),
            description: String::new(),
            icon: None,
            category: "custom".into(),
            pages: vec![Page {
                id: "home".into(),
                name: "Home".into(),
                controls: vec![hello, plus, minus, reset, level, ping, crash, off],
            }],
            context: None,
            imported_from: None,
        }],
        last_active_surface: Some("sdk".into()),
        preferences: Preferences::default(),
        context: Default::default(),
    }
}

/// Test-only plugins written in WAT, installed next to the real examples.
fn hostile_plugins(root: &Path) {
    let write = |dir: &str, id: &str, wasm: Vec<u8>| {
        let d = root.join(dir);
        std::fs::create_dir_all(&d).unwrap();
        let manifest = json!({
            "schemaVersion": 1, "id": id, "name": id, "version": "1.0.0", "author": "tests",
            "abi": 1, "main": "m.wasm",
            "actions": [{"id": "a"}], "events": []
        });
        std::fs::write(d.join("plugin.json"), manifest.to_string()).unwrap();
        std::fs::write(d.join("m.wasm"), wasm).unwrap();
    };
    // Traps on the first invocation.
    write(
        "crash",
        "test.crash",
        wat::Wat::new().body("unreachable").build(),
    );
    // Fine, until it is disabled; then it must never run.
    write(
        "off",
        "test.off",
        wat::Wat::new()
            .reply(r#"{"status":"ok","value":"on"}"#)
            .build(),
    );
}

async fn start_with(dirs: Vec<PathBuf>, store: Option<Store>) -> Arc<Runtime> {
    let rt = Runtime::start(RuntimeConfig {
        workspace: workspace(),
        store,
        emitter: Arc::new(NullEmitter),
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
    rt
}

/// Real examples plus the hostile ones. The temp dir lives as long as the test.
async fn start() -> (Arc<Runtime>, tempfile::TempDir) {
    let tmp = tempfile::tempdir().unwrap();
    hostile_plugins(tmp.path());
    let rt = start_with(vec![examples(), tmp.path().to_path_buf()], None).await;
    (rt, tmp)
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

fn status(rt: &Runtime, id: &str) -> PluginStatus {
    rt.plugin_host()
        .get(id)
        .unwrap()
        .plugin
        .external
        .unwrap()
        .status
}

// ---------------------------------------------------------------------------
// Discovery and contract
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread")]
async fn external_plugins_are_discovered_and_described_next_to_the_built_in_ones() {
    let (rt, _tmp) = start().await;
    let plugins = rt.snapshot().plugins;
    for builtin in ["system", "demo", "agent-control"] {
        let p = plugins.iter().find(|p| p.id == builtin).expect(builtin);
        assert!(p.external.is_none(), "{builtin} is built in");
    }
    let hello = plugins
        .iter()
        .find(|p| p.id == "example.hello")
        .expect("hello");
    let ext = hello.external.as_ref().unwrap();
    assert_eq!(ext.status, PluginStatus::Enabled);
    assert_eq!(ext.author, "LaunchDeck examples");
    assert_eq!(hello.version, "0.1.0");
    assert_eq!(hello.actions[0].id, "hello.say");
    assert_eq!(hello.actions[0].params[0].name, "name");
    assert_eq!(ext.events[0].kind, "hello.completed");
    assert!(
        hello.permissions.is_empty(),
        "no capabilities exist in protocol 1"
    );
    for id in ["example.counter", "example.ping", "test.crash", "test.off"] {
        assert!(plugins.iter().any(|p| p.id == id), "{id}");
    }
    assert!(
        rt.plugin_host().issues().is_empty(),
        "{:?}",
        rt.plugin_host().issues()
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn the_snapshot_wire_format_keeps_built_ins_unchanged_and_adds_external_info() {
    let (rt, _tmp) = start().await;
    let json = serde_json::to_value(rt.snapshot().plugins).unwrap();
    let find = |id: &str| {
        json.as_array()
            .unwrap()
            .iter()
            .find(|p| p["id"] == id)
            .unwrap()
            .clone()
    };
    assert!(find("demo").get("external").is_none());
    assert_eq!(find("example.hello")["external"]["status"], "enabled");
    assert_eq!(
        find("example.hello")["actions"][0]["params"][0]["type"],
        "string"
    );
    // Built-in plugins declare their settings the same way.
    assert_eq!(find("demo")["actions"][0]["params"][0]["type"], "enum");
    assert_eq!(find("demo")["kind"], "builtin");
    assert_eq!(find("example.hello")["kind"], "plugin");
    assert_eq!(find("agent-control")["kind"], "module");
}

// ---------------------------------------------------------------------------
// The full chain
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread")]
async fn a_pad_runs_an_external_plugin_action_and_the_event_it_publishes_lights_the_pad() {
    let (rt, _tmp) = start().await;
    assert!(
        shows(&rt, 0, 0, Visual::solid(BLUE)),
        "idle look before the press"
    );

    let pressed = Instant::now();
    tap(&rt, 0, 0);
    wait_until("the pad to follow hello.completed", || {
        shows(&rt, 0, 0, Visual::solid(GREEN))
    })
    .await;
    let latency = pressed.elapsed();
    eprintln!("press to LED through an external plugin: {latency:?} (debug build)");
    assert!(latency < Duration::from_millis(500), "{latency:?}");

    // Every link in the chain left its trace on the one event bus.
    let log = events(&rt);
    let completed = log
        .iter()
        .find(|e| e.kind == "hello.completed")
        .expect("the plugin's event reached the bus");
    assert_eq!(completed.source, "example.hello");
    assert_eq!(
        completed.payload,
        json!({"name": "Leroy", "message": "Hello, Leroy!"})
    );
    let started = log.iter().position(|e| e.kind == "action.started").unwrap();
    let done = log
        .iter()
        .position(|e| e.kind == "action.completed")
        .unwrap();
    assert!(started < done);
    let result = &log[done].payload;
    assert_eq!(result["action"], "example.hello.hello.say");
}

#[tokio::test(flavor = "multi_thread")]
async fn the_action_result_is_normalized_and_the_args_are_typed() {
    let (rt, _tmp) = start().await;
    // The same engine and registry a pad press goes through.
    let engine = ActionEngine::new(rt.plugin_registry(), EventBus::new());
    let value = engine
        .execute(&plugin(
            "example.hello",
            "hello.say",
            json!({"name": "Ada"}),
        ))
        .await
        .unwrap();
    assert_eq!(value, json!({"message": "Hello, Ada!"}));
    // Wrong type: refused before the module runs.
    let err = engine
        .execute(&plugin("example.hello", "hello.say", json!({"name": 5})))
        .await
        .unwrap_err();
    assert!(matches!(err, ActionError::InvalidArgs(_)), "{err:?}");
    let err = engine
        .execute(&plugin(
            "example.hello",
            "hello.say",
            json!({"name": "x", "extra": 1}),
        ))
        .await
        .unwrap_err();
    assert!(matches!(err, ActionError::InvalidArgs(_)), "{err:?}");
    // Unknown action and unknown plugin.
    let err = engine
        .execute(&plugin("example.hello", "hello.nope", json!({})))
        .await
        .unwrap_err();
    assert!(matches!(err, ActionError::UnknownAction { .. }));
    let err = engine
        .execute(&plugin("example.nope", "x", json!({})))
        .await
        .unwrap_err();
    assert!(matches!(err, ActionError::UnknownPlugin(_)));
    assert_eq!(status(&rt, "example.hello"), PluginStatus::Enabled);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_stateful_plugin_remembers_between_presses_and_reports_levels_not_pads() {
    let (rt, _tmp) = start().await;
    // Three steps of three reach the maximum of nine.
    rt.plugin_host()
        .set_config("example.counter", "step", json!(3))
        .unwrap();
    assert!(shows(&rt, 4, 0, Visual::solid(GREY)));

    tap(&rt, 1, 0);
    wait_until("low", || shows(&rt, 4, 0, Visual::solid(BLUE))).await;
    tap(&rt, 1, 0);
    wait_until("high", || shows(&rt, 4, 0, Visual::solid(AMBER))).await;
    tap(&rt, 1, 0);
    wait_until("max", || shows(&rt, 4, 0, Visual::flash(RED))).await;

    // At the maximum the plugin refuses; that is a normal failure, not a crash.
    let before = events(&rt).len();
    tap(&rt, 1, 0);
    wait_until("action.failed", || {
        events(&rt).iter().skip(before).any(|e| {
            e.kind == "action.failed"
                && e.payload["error"]
                    .as_str()
                    .is_some_and(|m| m.contains("maximum"))
        })
    })
    .await;
    assert_eq!(status(&rt, "example.counter"), PluginStatus::Enabled);
    assert!(
        shows(&rt, 4, 0, Visual::flash(RED)),
        "the level is unchanged"
    );

    tap(&rt, 2, 0);
    wait_until("back to high", || shows(&rt, 4, 0, Visual::solid(AMBER))).await;
    tap(&rt, 3, 0);
    wait_until("zero", || shows(&rt, 4, 0, Visual::solid(GREY))).await;

    let values: Vec<i64> = events(&rt)
        .iter()
        .filter(|e| e.kind == "counter.changed")
        .map(|e| e.payload["value"].as_i64().unwrap())
        .collect();
    assert_eq!(values, vec![3, 6, 9, 6, 0]);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_c_plugin_with_no_sdk_works_through_the_same_chain() {
    let (rt, _tmp) = start().await;
    tap(&rt, 0, 1);
    wait_until("ping pad", || shows(&rt, 0, 1, Visual::pulse(GREEN))).await;
    let e = events(&rt)
        .into_iter()
        .find(|e| e.kind == "ping.sent")
        .unwrap();
    assert_eq!(e.source, "example.ping");
    assert_eq!(e.payload, json!({"from": "c"}));
}

// ---------------------------------------------------------------------------
// Configuration, enable and disable
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread")]
async fn plugin_configuration_reaches_the_plugin_and_stays_out_of_the_workspace() {
    let tmp_state = tempfile::tempdir().unwrap();
    let store = Store::new(tmp_state.path());
    let rt = start_with(vec![examples()], Some(store)).await;
    rt.plugin_host()
        .set_config("example.hello", "greeting", json!("Hola"))
        .unwrap();
    tap(&rt, 0, 0);
    wait_until("greeting", || {
        events(&rt)
            .iter()
            .any(|e| e.kind == "hello.completed" && e.payload["message"] == "Hola, Leroy!")
    })
    .await;
    // Not in the workspace the surfaces live in, in memory or on disk.
    let workspace = serde_json::to_string(&rt.snapshot().workspace).unwrap();
    assert!(!workspace.contains("Hola"));
    let settings = std::fs::read_to_string(tmp_state.path().join("plugins.json")).unwrap();
    assert!(settings.contains("Hola"));
    for entry in std::fs::read_dir(tmp_state.path()).unwrap().flatten() {
        if entry.file_name() != "plugins.json" {
            let text = std::fs::read_to_string(entry.path()).unwrap_or_default();
            assert!(
                !text.contains("Hola"),
                "{:?} leaked the setting",
                entry.file_name()
            );
        }
    }
    // A bad value is refused and nothing changes.
    assert!(rt
        .plugin_host()
        .set_config("example.hello", "greeting", json!(5))
        .is_err());
    assert!(rt
        .plugin_host()
        .set_config("example.hello", "nope", json!("x"))
        .is_err());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_disabled_plugin_never_receives_invocations_and_can_be_enabled_again() {
    let (rt, _tmp) = start().await;
    rt.plugin_host().set_enabled("test.off", false).unwrap();
    assert_eq!(status(&rt, "test.off"), PluginStatus::Disabled);

    let before = events(&rt).len();
    tap(&rt, 0, 3);
    wait_until("action.failed for the disabled plugin", || {
        events(&rt).iter().skip(before).any(|e| {
            e.kind == "action.failed"
                && e.payload["error"]
                    .as_str()
                    .is_some_and(|m| m.contains("disabled"))
        })
    })
    .await;
    assert_eq!(
        status(&rt, "test.off"),
        PluginStatus::Disabled,
        "refusing is not failing"
    );

    rt.plugin_host().set_enabled("test.off", true).unwrap();
    let before = events(&rt).len();
    tap(&rt, 0, 3);
    wait_until("it works again", || {
        events(&rt)
            .iter()
            .skip(before)
            .any(|e| e.kind == "action.completed" && e.payload["controlId"] == "g.0.3")
    })
    .await;
}

// ---------------------------------------------------------------------------
// Failure isolation
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread")]
async fn a_crashing_plugin_fails_alone_and_everything_else_keeps_working() {
    let (rt, _tmp) = start().await;

    tap(&rt, 0, 2);
    wait_until("the crash pad to show an error", || {
        shows(&rt, 0, 2, Visual::flash(RED))
    })
    .await;
    let failure = events(&rt)
        .into_iter()
        .find(|e| e.kind == "action.failed" && e.payload["controlId"] == "g.0.2")
        .expect("a failure event");
    let message = failure.payload["error"].as_str().unwrap().to_string();
    assert!(message.contains("plugin `test.crash` failed"), "{message}");
    assert!(!message.to_lowercase().contains("launchdeck crashed"));

    // Marked unhealthy, with the reason, and a log line a person can read.
    let info = rt.plugin_host().get("test.crash").unwrap();
    let ext = info.plugin.external.unwrap();
    assert_eq!(ext.status, PluginStatus::Error);
    assert!(ext.error.unwrap().contains("aborted"));
    let logs = rt.plugin_host().logs("test.crash").unwrap();
    assert!(logs.iter().any(|l| l.message.contains("stopped")));

    // Pressing again fails fast and clearly; it does not run the dead module.
    let before = events(&rt).len();
    tap(&rt, 0, 2);
    wait_until("second failure", || {
        events(&rt).iter().skip(before).any(|e| {
            e.kind == "action.failed"
                && e.payload["error"]
                    .as_str()
                    .is_some_and(|m| m.contains("unavailable"))
        })
    })
    .await;

    // Other plugins, the device runtime and Agent Control carry on.
    tap(&rt, 0, 0);
    wait_until("hello still works", || {
        shows(&rt, 0, 0, Visual::solid(GREEN))
    })
    .await;
    tap(&rt, 0, 1);
    wait_until("ping still works", || {
        shows(&rt, 0, 1, Visual::pulse(GREEN))
    })
    .await;
    assert_eq!(status(&rt, "example.hello"), PluginStatus::Enabled);
    rt.run_action(plugin("agent-control", "runDemo", json!({})))
        .await
        .unwrap();
    wait_until("Agent Control still runs its demo", || {
        !rt.snapshot().agents.sessions.is_empty()
    })
    .await;
    assert!(rt.state().devices.iter().any(|d| d.connected));

    // A restart of the plugin is explicit, and fails the same way, cleanly.
    let again = rt.plugin_host().reload("test.crash").unwrap();
    assert_eq!(again.plugin.external.unwrap().status, PluginStatus::Enabled);
}

#[tokio::test(flavor = "multi_thread")]
async fn broken_plugin_folders_never_stop_the_runtime_from_starting() {
    let tmp = tempfile::tempdir().unwrap();
    let mk = |name: &str, manifest: &str, wasm: &[u8]| {
        let d = tmp.path().join(name);
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("plugin.json"), manifest).unwrap();
        std::fs::write(d.join("m.wasm"), wasm).unwrap();
    };
    mk("garbage-json", "{{{{", b"\0asm\x01\0\0\0");
    mk(
        "future-schema",
        r#"{"schemaVersion": 7}"#,
        b"\0asm\x01\0\0\0",
    );
    mk("not-wasm", &json!({"schemaVersion":1,"id":"bad.one","name":"x","version":"1.0.0","author":"x","abi":1,"main":"m.wasm"}).to_string(), b"hello");
    mk("future-abi", &json!({"schemaVersion":1,"id":"bad.two","name":"x","version":"1.0.0","author":"x","abi":2,"main":"m.wasm"}).to_string(), b"\0asm\x01\0\0\0");
    mk("wants-network", &json!({"schemaVersion":1,"id":"bad.three","name":"x","version":"1.0.0","author":"x","abi":1,"main":"m.wasm","capabilities":["network"]}).to_string(), b"\0asm\x01\0\0\0");
    mk("wasi", &json!({"schemaVersion":1,"id":"bad.four","name":"x","version":"1.0.0","author":"x","abi":1,"main":"m.wasm"}).to_string(),
        &wat::Wat::new().import(r#"(import "wasi_snapshot_preview1" "fd_write" (func (param i32 i32 i32 i32) (result i32)))"#).build());
    let rt = start_with(vec![examples(), tmp.path().to_path_buf()], None).await;
    // Five folders are refused outright, one loads as a plugin in error.
    assert_eq!(
        rt.plugin_host().issues().len(),
        5,
        "{:?}",
        rt.plugin_host().issues()
    );
    assert_eq!(status(&rt, "bad.four"), PluginStatus::Error);
    tap(&rt, 0, 0);
    wait_until("hello works", || shows(&rt, 0, 0, Visual::solid(GREEN))).await;
}

// ---------------------------------------------------------------------------
// Spoofing and scale
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread")]
async fn network_clients_cannot_impersonate_an_installed_plugin() {
    let mut ws = workspace();
    ws.preferences.external_events_enabled = true;
    ws.preferences.external_events_port = 0;
    let rt = Runtime::start(RuntimeConfig {
        workspace: ws,
        store: None,
        emitter: Arc::new(NullEmitter),
        keyboard: Arc::new(NoKeyboard),
        scan_hardware: false,
        serve_external: true,
        demo_pace: 0.05,
        plugin_dirs: vec![examples()],
        secrets: None,
        observer: None,
    })
    .await;
    let port = rt.state().external.port;
    let (mut sock, _) = connect_async(format!("ws://127.0.0.1:{port}"))
        .await
        .unwrap();
    for (source, allowed) in [
        ("example.hello", false),
        ("example.counter", false),
        ("runtime", false),
        ("some.other.tool", true),
    ] {
        sock.send(Message::text(
            json!({"type":"event","source":source,"event":"hello.completed","payload":{}})
                .to_string(),
        ))
        .await
        .unwrap();
        let reply = tokio::time::timeout(Duration::from_secs(3), sock.next())
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        let reply: Value = serde_json::from_str(reply.to_text().unwrap()).unwrap();
        assert_eq!(reply["type"] == "ack", allowed, "{source}: {reply}");
    }
    // The spoofed events never reached the bus; the allowed one did.
    wait_until("the allowed event", || {
        events(&rt).iter().any(|e| e.source == "some.other.tool")
    })
    .await;
    assert!(!events(&rt).iter().any(|e| e.source == "example.hello"));
}

#[tokio::test(flavor = "multi_thread")]
async fn many_plugins_load_and_run_without_a_per_press_cost() {
    let tmp = tempfile::tempdir().unwrap();
    let n = 40;
    for i in 0..n {
        let d = tmp.path().join(format!("p{i:02}"));
        std::fs::create_dir_all(&d).unwrap();
        let manifest = json!({
            "schemaVersion": 1, "id": format!("bulk.p{i:02}"), "name": "Bulk", "version": "1.0.0",
            "author": "tests", "abi": 1, "main": "m.wasm", "actions": [{"id": "a"}]
        });
        std::fs::write(d.join("plugin.json"), manifest.to_string()).unwrap();
        std::fs::write(
            d.join("m.wasm"),
            wat::Wat::new().reply(r#"{"status":"ok"}"#).build(),
        )
        .unwrap();
    }
    let started = Instant::now();
    let rt = start_with(vec![tmp.path().to_path_buf()], None).await;
    let startup = started.elapsed();
    assert_eq!(rt.plugin_host().list().len(), n);
    assert!(rt
        .plugin_host()
        .list()
        .iter()
        .all(|p| p.plugin.external.as_ref().unwrap().status == PluginStatus::Enabled));

    // Unchanged plugins are not restarted by a rescan.
    let rescan = Instant::now();
    let report = rt.plugin_host().scan();
    let rescan = rescan.elapsed();
    assert_eq!(
        (report.unchanged, report.loaded, report.reloaded),
        (n, 0, 0)
    );

    // Invocation cost with all of them loaded, straight through the engine.
    let engine = ActionEngine::new(rt.plugin_registry(), EventBus::new());
    let calls = 400;
    let started = Instant::now();
    for i in 0..calls {
        engine
            .execute(&plugin(&format!("bulk.p{:02}", i % n), "a", json!({})))
            .await
            .unwrap();
    }
    let per_call = started.elapsed() / calls as u32;
    eprintln!("{n} plugins: startup {startup:?}, unchanged rescan {rescan:?}, {per_call:?} per invocation (debug build)");
    assert!(startup < Duration::from_secs(20), "{startup:?}");
    assert!(rescan < Duration::from_secs(2), "{rescan:?}");
    assert!(per_call < Duration::from_millis(5), "{per_call:?}");
}

// ---------------------------------------------------------------------------
// Compatibility
// ---------------------------------------------------------------------------

/// Hello, Counter and Channels exactly as SDK 0.1.x built them, with their
/// manifests (`tests/fixtures/plugins/sdk-0.1/PROVENANCE.md`). The examples are
/// rebuilt with every SDK and these files never are, so this fails when a
/// change to the host breaks plugins that people already have.
#[tokio::test(flavor = "multi_thread")]
async fn modules_built_with_sdk_0_1_still_load_and_run() {
    let frozen = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/plugins/sdk-0.1");
    let registry = PluginRegistry::default();
    let hub = StateHub::new();
    let host = PluginHost::with_hub(
        vec![frozen],
        None,
        registry.clone(),
        Limits::default(),
        hub.clone(),
    );
    host.scan();
    assert!(host.issues().is_empty(), "{:?}", host.issues());
    for (id, sdk) in [
        ("example.hello", "0.1.0"),
        ("example.counter", "0.1.1"),
        ("example.channels", "0.1.1"),
    ] {
        let ext = host.get(id).unwrap().plugin.external.unwrap();
        assert_eq!(
            (ext.status, ext.sdk.as_deref()),
            (PluginStatus::Enabled, Some(sdk)),
            "{id}"
        );
    }

    let bus = EventBus::new();
    let mut published = bus.subscribe();
    let engine = ActionEngine::with_hub(registry, bus, hub.clone());
    let hello = plugin("example.hello", "hello.say", json!({"name": "Ada"}));
    let counter = plugin("example.counter", "counter.increment", json!({}));
    let channel = plugin(
        "example.channels",
        "channel.up",
        json!({"channel": "alpha"}),
    );
    assert_eq!(
        engine.execute(&hello).await.unwrap(),
        json!({"message": "Hello, Ada!"})
    );
    assert_eq!(
        engine.execute(&counter).await.unwrap(),
        json!({"value": 1, "level": "low"})
    );
    engine.execute(&channel).await.unwrap();

    let mut kinds = Vec::new();
    while let Ok(event) = published.try_recv() {
        kinds.push(event.kind);
    }
    for kind in ["hello.completed", "counter.changed", "channel.changed"] {
        assert!(kinds.iter().any(|k| k == kind), "{kind} in {kinds:?}");
    }
    let state = |addr: StateAddress| {
        let seen = hub.read_address(&addr);
        (seen.status, seen.value)
    };
    assert_eq!(
        state(StateAddress::new("example.counter", "level")),
        (Status::Known, Some(Scalar::from("low")))
    );
    assert_eq!(
        state(StateAddress::new("example.channels", "status").with("channel", "alpha")),
        (Status::Known, Some(Scalar::from("up")))
    );
}

#[test]
fn the_shipped_example_manifests_are_valid_and_their_modules_are_in_the_artifact() {
    for name in ["hello", "counter", "c-ping"] {
        let dir = examples().join(name);
        let c = launchdeck_lib::plugins::external::discovery::load_dir(&dir)
            .unwrap_or_else(|e| panic!("{name}: {e}"));
        assert!(c.wasm.len() > 100, "{name} ships its built module");
        assert!(c.wasm.len() < 64 * 1024, "{name} stays small");
    }
}
