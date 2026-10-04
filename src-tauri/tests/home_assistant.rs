//! The Home Assistant example plugin, end to end.
//!
//! * the real plugin (`examples/plugins/home-assistant`, WebAssembly built with
//!   the public SDK), copied to a temporary folder;
//! * the real runtime, plugin host, capability checks and network broker;
//! * a stand-in for Home Assistant on a loopback port, which answers the three
//!   REST endpoints the plugin uses and counts every request that reaches it;
//! * the virtual Launchpad.
//!
//! The pad's look is decided by its feedback rules from the state the plugin
//! reports, never by the outcome of the HTTP request.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use launchdeck_lib::core::model::{
    Animation, Assignment, Control, ControlPos, Page, Preferences, Rgb, Span, Surface, Visual,
    Workspace, SCHEMA_VERSION,
};
use launchdeck_lib::core::runtime::{ui, Mode, Runtime, RuntimeConfig, UiEmitter};
use launchdeck_lib::devices::Led;
use launchdeck_lib::persistence::Store;
use launchdeck_lib::plugins::external::capabilities::{Decision, GrantStatus, Reach};
use launchdeck_lib::plugins::external::secrets::{MemoryStore, SecretStore, StoreKind};
use launchdeck_lib::plugins::external::PluginStatus;
use launchdeck_lib::plugins::keyboard::{Combo, Keyboard};
use launchdeck_lib::plugins::ActionError;
use launchdeck_lib::state::feedback::FeedbackRule;
use launchdeck_lib::state::hub::Status;
use launchdeck_lib::state::Scalar;
use serde_json::{json, Value};

#[allow(dead_code)]
mod mock {
    include!("support/http_server.rs");
    include!("support/mock_home_assistant.rs");
}
use mock::MockHomeAssistant;

const PLUGIN: &str = "example.home-assistant";
const TOKEN: &str = "eyJ0eXAiOiJKV1QiLCJhbGciOiJIUzI1NiJ9.this-is-the-long-lived-token";
const KITCHEN: &str = "light.kitchen";
const DESK: &str = "switch.desk";
const SURFACE: &str = "home";
const PAGE: &str = "main";

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
    fn events(&self, kind: &str) -> Vec<Value> {
        self.0
            .lock()
            .unwrap()
            .iter()
            .filter(|(e, p)| e == ui::EVENT && p["source"] == "runtime" && p["type"] == kind)
            .map(|(_, p)| p["payload"].clone())
            .collect()
    }

    /// Every runtime state the UI was sent, oldest first.
    fn states(&self) -> Vec<Value> {
        self.0
            .lock()
            .unwrap()
            .iter()
            .filter(|(e, _)| e == ui::STATE)
            .map(|(_, p)| p.clone())
            .collect()
    }

    fn everything(&self) -> String {
        serde_json::to_string(&*self.0.lock().unwrap()).unwrap()
    }
}

const REST: Rgb = Rgb::new(40, 40, 60);
const ON: Rgb = Rgb::new(245, 197, 24);
const OFF: Rgb = Rgb::new(30, 30, 30);
const PENDING: Rgb = Rgb::new(120, 120, 255);
const FAILED: Rgb = Rgb::new(255, 40, 40);

fn solid(color: Rgb) -> Visual {
    Visual {
        color,
        animation: Animation::None,
    }
}

/// A pad that toggles an entity and shows it: the rules read the entity's
/// state, and two optional looks say what the pad's own action is doing.
fn pad(col: u8, entity: &str) -> Control {
    let rule = |value: &str, look: &str| -> FeedbackRule {
        serde_json::from_value(json!({
            "when": { "state": { "provider": PLUGIN, "id": "entity", "args": { "entity": entity } }, "is": { "op": "equals", "value": value } },
            "show": look
        }))
        .unwrap()
    };
    Control {
        id: format!("g.{col}.0"),
        pos: ControlPos::grid(col, 0),
        span: Span { w: 1, h: 1 },
        label: entity.to_string(),
        description: String::new(),
        icon: None,
        visual: solid(REST),
        states: [
            ("on".to_string(), solid(ON)),
            ("off".to_string(), solid(OFF)),
            (
                "pending".to_string(),
                Visual {
                    color: PENDING,
                    animation: Animation::Pulse,
                },
            ),
            (
                "failed".to_string(),
                Visual {
                    color: FAILED,
                    animation: Animation::Flash,
                },
            ),
        ]
        .into_iter()
        .collect(),
        press: Some(Assignment::new(
            PLUGIN,
            "toggle",
            json!({ "entity": entity }),
        )),
        long_press: None,
        feedback: vec![rule("on", "on"), rule("off", "off")],
        legacy_bindings: vec![],
    }
}

struct Env {
    rt: Arc<Runtime>,
    rec: Arc<Recorder>,
    secrets: Arc<MemoryStore>,
    data: PathBuf,
    plugins: PathBuf,
    _root: tempfile::TempDir,
}

fn copy_plugin(to: &Path) {
    let from = Path::new(env!("CARGO_MANIFEST_DIR")).join("../examples/plugins/home-assistant");
    fs::create_dir_all(to).unwrap();
    for file in ["plugin.json", "home-assistant.wasm"] {
        fs::copy(from.join(file), to.join(file)).unwrap_or_else(|e| panic!("{file}: {e}"));
    }
}

async fn start(controls: Vec<Control>) -> Env {
    let root = tempfile::tempdir().unwrap();
    let plugins = root.path().join("plugins");
    copy_plugin(&plugins.join("home-assistant"));
    let data = root.path().join("data");
    let rec = Arc::new(Recorder::default());
    let secrets = Arc::new(MemoryStore::new());
    let rt = Runtime::start(RuntimeConfig {
        workspace: Workspace {
            schema_version: SCHEMA_VERSION,
            surfaces: vec![Surface {
                id: SURFACE.into(),
                name: "Home".into(),
                description: String::new(),
                icon: None,
                category: "custom".into(),
                pages: vec![Page {
                    id: PAGE.into(),
                    name: "Main".into(),
                    controls,
                }],
                context: None,
            }],
            last_active_surface: Some(SURFACE.into()),
            preferences: Preferences::default(),
        },
        store: Some(Store::new(&data)),
        emitter: rec.clone(),
        keyboard: Arc::new(NoKeyboard),
        scan_hardware: false,
        serve_external: false,
        demo_pace: 0.05,
        plugin_dirs: vec![plugins.clone()],
        secrets: Some(secrets.clone()),
    })
    .await;
    wait_until("the first frame", || {
        !rt.virtual_handle().frame().leds.is_empty()
    })
    .await;
    assert!(
        rt.plugin_host().issues().is_empty(),
        "the example plugin was refused: {:?}",
        rt.plugin_host().issues()
    );
    rt.set_mode(Mode::Live);
    Env {
        rt,
        rec,
        secrets,
        data,
        plugins,
        _root: root,
    }
}

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

fn led(env: &Env, col: u8) -> Led {
    env.rt
        .virtual_handle()
        .frame()
        .get(ControlPos::grid(col, 0))
        .expect("the pad exists")
}

async fn shows(env: &Env, col: u8, color: Rgb, what: &str) {
    let until = Instant::now() + PATIENCE;
    while Instant::now() < until {
        if led(env, col).color == color {
            return;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    panic!(
        "{what}: the pad shows {:?}, expected {color:?}",
        led(env, col)
    );
}

fn tap(env: &Env, col: u8) {
    let h = env.rt.virtual_handle();
    h.press(ControlPos::grid(col, 0), 100).unwrap();
    h.release(ControlPos::grid(col, 0)).unwrap();
}

fn reading(env: &Env, id: &str) -> (Status, Option<Scalar>) {
    let o = env
        .rt
        .read_state(PLUGIN, "entity", &json!({ "entity": id }))
        .unwrap();
    (o.status, o.value)
}

fn known(value: &str) -> (Status, Option<Scalar>) {
    (Status::Known, Some(Scalar::text(value)))
}

/// What the Plugins screen does: the address, the grant, the token.
fn connect(env: &Env, ha: &MockHomeAssistant) {
    let host = env.rt.plugin_host();
    host.set_config(PLUGIN, "baseUrl", json!(ha.origin()))
        .unwrap();
    host.set_permission(PLUGIN, &ha.origin(), Some(Decision::Granted))
        .unwrap();
    host.set_secret(PLUGIN, "token", &ha.token).unwrap();
}

// ---------------------------------------------------------------------------
// The required flow
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread")]
async fn a_light_is_toggled_and_followed_and_permission_decides_whether_anything_is_sent() {
    let ha = MockHomeAssistant::start(TOKEN, &[(KITCHEN, "off"), (DESK, "on")]);
    let env = start(vec![pad(0, KITCHEN), pad(1, DESK)]).await;
    let host = env.rt.plugin_host();

    // Installed and enabled: it runs, it asks, it has nothing.
    let info = host.get(PLUGIN).unwrap();
    assert_eq!(
        info.plugin.external.as_ref().unwrap().status,
        PluginStatus::Enabled
    );
    let net = info.network.unwrap();
    assert_eq!(net.purpose, "Connect to your Home Assistant instance.");
    assert_eq!(net.status, GrantStatus::NeedsApproval);
    assert_eq!(
        net.hosts[0].origin, "http://homeassistant.local:8123",
        "the documented default address"
    );
    assert_eq!(net.hosts[0].reach, Reach::Local);
    assert_eq!(info.secret_store, StoreKind::Memory);
    assert!(!info.secrets[0].is_set);
    assert_eq!(
        reading(&env, KITCHEN),
        (Status::Unknown, None),
        "a plugin that starts knows nothing"
    );
    shows(&env, 0, REST, "nothing is known yet").await;

    // The address is typed: the request is now for exactly that origin, and still not granted.
    host.set_config(PLUGIN, "baseUrl", json!(ha.origin()))
        .unwrap();
    let net = host.get(PLUGIN).unwrap().network.unwrap();
    assert_eq!(
        (
            net.hosts[0].origin.as_str(),
            net.hosts[0].status,
            net.hosts[0].reach
        ),
        (
            ha.origin().as_str(),
            GrantStatus::NeedsApproval,
            Reach::Loopback
        )
    );
    tokio::time::sleep(Duration::from_millis(700)).await;
    assert_eq!(
        ha.hits(),
        0,
        "without a grant the plugin's refresh reaches nothing"
    );
    assert_eq!(reading(&env, KITCHEN), (Status::Unknown, None));

    // Granted, and the token entered: the plugin fetches what the pads show.
    host.set_permission(PLUGIN, &ha.origin(), Some(Decision::Granted))
        .unwrap();
    host.set_secret(PLUGIN, "token", TOKEN).unwrap();
    assert_eq!(
        host.get(PLUGIN).unwrap().network.unwrap().status,
        GrantStatus::Granted
    );
    wait_until("the kitchen light to be known", || {
        reading(&env, KITCHEN) == known("off")
    })
    .await;
    wait_until("the desk switch to be known", || {
        reading(&env, DESK) == known("on")
    })
    .await;
    shows(&env, 0, OFF, "the light is off").await;
    shows(&env, 1, ON, "the switch is on").await;
    assert!(
        ha.service_calls().is_empty(),
        "reading is all that happened"
    );

    // Toggle. Home Assistant is slow enough to see the action pending.
    ha.slow(Duration::from_millis(400));
    let (started, completed) = (
        env.rec.events("action.started").len(),
        env.rec.events("action.completed").len(),
    );
    tap(&env, 0);
    wait_until("the action to be pending", || {
        env.rt.state().pending.iter().any(|p| {
            p.action == format!("{PLUGIN}.toggle")
                && p.control.as_deref() == Some("home/main/g.0.0")
        })
    })
    .await;
    assert_eq!(env.rec.events("action.started").len(), started + 1);
    assert_eq!(
        env.rec.events("action.completed").len(),
        completed,
        "pending is not completed"
    );
    shows(&env, 0, PENDING, "the pad's own pending look").await;
    wait_until("the action to finish", || env.rt.state().pending.is_empty()).await;
    ha.slow(Duration::ZERO);

    assert_eq!(
        ha.service_calls(),
        vec![format!("light/toggle {KITCHEN}")],
        "the call reached Home Assistant"
    );
    assert_eq!(ha.state(KITCHEN).as_deref(), Some("on"));
    wait_until("the state to be on", || {
        reading(&env, KITCHEN) == known("on")
    })
    .await;
    shows(&env, 0, ON, "the look comes from the entity's state").await;
    assert_eq!(env.rec.events("action.completed").len(), completed + 1);
    // The UI was told about pending by the runtime, and told again when it ended.
    let told: Vec<usize> = env
        .rec
        .states()
        .iter()
        .map(|s| s["pending"].as_array().map_or(0, Vec::len))
        .collect();
    assert!(told.contains(&1) && told.last() == Some(&0), "{told:?}");

    // A change made somewhere else arrives with the next refresh, with no press.
    ha.set(KITCHEN, "off");
    shows(&env, 0, OFF, "a change made in Home Assistant").await;
    ha.set(DESK, "unavailable");
    wait_until("the switch to be unavailable", || {
        reading(&env, DESK) == known("unavailable")
    })
    .await;
    shows(
        &env,
        1,
        REST,
        "no rule is about `unavailable`, so the pad rests",
    )
    .await;
    ha.set(DESK, "on");
    shows(&env, 1, ON, "and back").await;

    // Revoke: the action fails, says why, and nothing reaches the server.
    host.set_permission(PLUGIN, &ha.origin(), None).unwrap();
    assert_eq!(
        host.get(PLUGIN).unwrap().network.unwrap().status,
        GrantStatus::NeedsApproval
    );
    wait_until("the states to be dropped", || {
        reading(&env, KITCHEN).0 == Status::Unknown
    })
    .await;
    shows(
        &env,
        0,
        REST,
        "what cannot be read is not shown as if it were known",
    )
    .await;
    let (before, failures) = (ha.hits(), env.rec.events("action.failed").len());
    tap(&env, 0);
    wait_until("the refused press to be reported", || {
        env.rec.events("action.failed").len() > failures
    })
    .await;
    let failure = env.rec.events("action.failed").last().unwrap().clone();
    let why = failure["error"].as_str().unwrap();
    assert!(
        why.contains("waiting for approval in Plugins") && why.contains(&ha.origin()),
        "{why}"
    );
    shows(&env, 0, FAILED, "the pad's own failed look").await;
    shows(&env, 0, REST, "which ends by itself").await;
    tokio::time::sleep(Duration::from_millis(600)).await;
    assert_eq!(
        ha.hits(),
        before,
        "no request reached Home Assistant without the grant"
    );
    assert_eq!(
        ha.state(KITCHEN).as_deref(),
        Some("off"),
        "and nothing was switched"
    );
    let failure = host.get(PLUGIN).unwrap().network_failure.unwrap();
    assert_eq!(failure.code, "permission_denied");
    assert!(
        env.rt.state().pending.is_empty(),
        "a refused action is not left pending"
    );

    // Grant again: it recovers by itself.
    host.set_permission(PLUGIN, &ha.origin(), Some(Decision::Granted))
        .unwrap();
    shows(&env, 0, OFF, "known again after the grant").await;
    tap(&env, 0);
    shows(&env, 0, ON, "and the pad works again").await;
    assert_eq!(ha.state(KITCHEN).as_deref(), Some("on"));
    assert!(host.get(PLUGIN).unwrap().network_failure.is_none());
}

// ---------------------------------------------------------------------------
// The other actions, and what Home Assistant can answer
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread")]
async fn turn_on_and_turn_off_call_the_matching_service_and_bad_entities_are_refused_by_the_plugin()
{
    let ha = MockHomeAssistant::start(TOKEN, &[(KITCHEN, "off"), (DESK, "on")]);
    let env = start(vec![pad(0, KITCHEN)]).await;
    connect(&env, &ha);
    let run = |action: &'static str, entity: &'static str| {
        let rt = env.rt.clone();
        async move {
            rt.run_action(Assignment::new(PLUGIN, action, json!({ "entity": entity })))
                .await
        }
    };
    run("turn_on", KITCHEN).await.unwrap();
    assert_eq!(reading(&env, KITCHEN), known("on"));
    run("turn_on", KITCHEN).await.unwrap();
    assert_eq!(
        ha.state(KITCHEN).as_deref(),
        Some("on"),
        "turning on what is on leaves it on"
    );
    run("turn_off", DESK).await.unwrap();
    assert_eq!(reading(&env, DESK), known("off"));
    run("toggle", DESK).await.unwrap();
    assert_eq!(
        ha.service_calls(),
        vec![
            format!("light/turn_on {KITCHEN}"),
            format!("light/turn_on {KITCHEN}"),
            format!("switch/turn_off {DESK}"),
            format!("switch/toggle {DESK}")
        ]
    );

    // Not an entity, and not a kind this plugin switches: refused before any request.
    let before = ha.hits();
    for (bad, says) in [
        ("kitchen", "not an entity id"),
        ("light.", "not an entity id"),
        ("Light.Kitchen", "not an entity id"),
        ("light.a/../../b", "not an entity id"),
        ("lock.front_door", "lights and switches"),
    ] {
        let e = env
            .rt
            .run_action(Assignment::new(PLUGIN, "toggle", json!({ "entity": bad })))
            .await
            .unwrap_err();
        assert!(e.contains(says), "{bad}: {e}");
    }
    assert_eq!(ha.hits(), before);

    // An entity Home Assistant does not have: the command is accepted and changes
    // nothing, the state cannot be read, and the pad would show unknown.
    let e = run("toggle", "light.attic").await.unwrap_err();
    assert!(e.contains("does not know that entity"), "{e}");
    assert_eq!(reading(&env, "light.attic").0, Status::Unknown);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_missing_wrong_or_removed_token_fails_clearly_and_the_token_is_written_nowhere() {
    let ha = MockHomeAssistant::start(TOKEN, &[(KITCHEN, "off")]);
    let env = start(vec![pad(0, KITCHEN)]).await;
    let host = env.rt.plugin_host();
    host.set_config(PLUGIN, "baseUrl", json!(ha.origin()))
        .unwrap();
    host.set_permission(PLUGIN, &ha.origin(), Some(Decision::Granted))
        .unwrap();
    let toggle = || {
        env.rt.run_action(Assignment::new(
            PLUGIN,
            "toggle",
            json!({ "entity": KITCHEN }),
        ))
    };

    // No token yet.
    let e = toggle().await.unwrap_err();
    assert!(
        e.contains("the secret `token` is not set; enter it in Plugins"),
        "{e}"
    );
    assert_eq!(ha.hits(), 0, "an unauthenticated request is not even sent");
    assert_eq!(
        host.get(PLUGIN).unwrap().network_failure.unwrap().code,
        "secret_missing"
    );

    // A wrong one: Home Assistant says no, and the plugin says so in words.
    host.set_secret(PLUGIN, "token", "not-the-token").unwrap();
    let e = toggle().await.unwrap_err();
    assert!(e.contains("refused the access token"), "{e}");
    assert_eq!(reading(&env, KITCHEN).0, Status::Unknown);
    assert_eq!(ha.state(KITCHEN).as_deref(), Some("off"));

    // The right one.
    host.set_secret(PLUGIN, "token", TOKEN).unwrap();
    toggle().await.unwrap();
    assert_eq!(reading(&env, KITCHEN), known("on"));
    assert_eq!(
        ha.server.received().last().unwrap().header("authorization"),
        Some(format!("Bearer {TOKEN}").as_str())
    );

    // Where the token is: the store, and the requests. Where it is not: everywhere else.
    assert_eq!(
        env.secrets
            .get(&launchdeck_lib::plugins::external::secrets::SecretRef::new(
                PLUGIN, "token"
            ))
            .unwrap()
            .as_deref(),
        Some(TOKEN)
    );
    let mut places: Vec<(String, String)> = vec![
        (
            "the snapshot".into(),
            serde_json::to_string(&env.rt.snapshot()).unwrap(),
        ),
        (
            "the plugin list".into(),
            serde_json::to_string(&host.overview()).unwrap(),
        ),
        (
            "the plugin's log".into(),
            serde_json::to_string(&host.logs(PLUGIN).unwrap()).unwrap(),
        ),
        ("what the UI was sent".into(), env.rec.everything()),
        (
            "the manifest".into(),
            fs::read_to_string(env.plugins.join("home-assistant/plugin.json")).unwrap(),
        ),
        (
            "the workspace in memory".into(),
            serde_json::to_string(&env.rt.snapshot().workspace).unwrap(),
        ),
    ];
    for entry in fs::read_dir(&env.data).unwrap() {
        let path = entry.unwrap().path();
        if path.is_file() {
            places.push((
                format!("{}", path.file_name().unwrap().to_string_lossy()),
                fs::read_to_string(&path).unwrap_or_default(),
            ));
        }
    }
    assert!(
        places.iter().any(|(name, _)| name == "plugins.json")
            && places.iter().any(|(name, _)| name == "permissions.json"),
        "{:?}",
        places.iter().map(|p| &p.0).collect::<Vec<_>>()
    );
    for (place, text) in &places {
        assert!(
            !text.contains(TOKEN)
                && !text.contains("this-is-the-long-lived-token")
                && !text.contains("not-the-token"),
            "the token is in {place}"
        );
        assert!(
            !text.contains("Bearer "),
            "an Authorization header is in {place}"
        );
    }
    // The address is a setting, kept with the plugin's settings and not in the surface.
    let plugins_json = fs::read_to_string(env.data.join("plugins.json")).unwrap();
    assert!(plugins_json.contains(&ha.origin()));
    let workspace = serde_json::to_string(&env.rt.snapshot().workspace).unwrap();
    assert!(
        workspace.contains(KITCHEN) && !workspace.contains(&ha.origin()),
        "the entity is the surface's; the address is not"
    );
    // Grants are not in the workspace either: they do not travel with a surface.
    assert!(!workspace.contains("granted"));
    assert!(fs::read_to_string(env.data.join("permissions.json"))
        .unwrap()
        .contains(&ha.origin()));

    // Removed: authentication stops, cleanly.
    let before = ha.hits();
    host.clear_secret(PLUGIN, "token").unwrap();
    let e = toggle().await.unwrap_err();
    assert!(e.contains("is not set"), "{e}");
    wait_until("the state to be dropped", || {
        reading(&env, KITCHEN).0 == Status::Unknown
    })
    .await;
    assert_eq!(ha.hits(), before);
}

#[tokio::test(flavor = "multi_thread")]
async fn when_home_assistant_is_gone_nothing_is_left_pending_and_no_stale_value_is_shown() {
    let ha = MockHomeAssistant::start(TOKEN, &[(KITCHEN, "on")]);
    let env = start(vec![pad(0, KITCHEN)]).await;
    connect(&env, &ha);
    shows(&env, 0, ON, "the light is on").await;
    let origin = ha.origin();
    drop(ha);

    // The next refresh cannot reach it: the state is cleared, not kept.
    wait_until("the state to be dropped", || {
        reading(&env, KITCHEN).0 == Status::Unknown
    })
    .await;
    shows(&env, 0, REST, "unknown, not a stale `on`").await;
    let failures = env.rec.events("action.failed").len();
    tap(&env, 0);
    wait_until("the press to fail", || {
        env.rec.events("action.failed").len() > failures
    })
    .await;
    let why = env.rec.events("action.failed").last().unwrap()["error"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(why.contains(&origin), "{why}");
    wait_until("nothing to be pending", || {
        env.rt.state().pending.is_empty()
    })
    .await;
    // The plugin is still running, and so is everything else.
    assert_eq!(
        env.rt
            .plugin_host()
            .get(PLUGIN)
            .unwrap()
            .plugin
            .external
            .unwrap()
            .status,
        PluginStatus::Enabled
    );
    assert_eq!(
        env.rt
            .plugin_host()
            .get(PLUGIN)
            .unwrap()
            .network_failure
            .unwrap()
            .code,
        "network_error"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn many_pads_are_refreshed_in_turns() {
    // More entities than one refresh asks about: every one is still reported.
    let names: Vec<String> = (0..20).map(|i| format!("light.l{i}")).collect();
    let entities: Vec<(&str, &str)> = names.iter().map(|n| (n.as_str(), "on")).collect();
    let ha = MockHomeAssistant::start(TOKEN, &entities);
    let mut controls: Vec<Control> = Vec::new();
    for (i, name) in names.iter().enumerate() {
        let mut c = pad((i % 8) as u8, name);
        c.pos = ControlPos::grid((i % 8) as u8, (i / 8) as u8);
        c.id = format!("g.{}.{}", i % 8, i / 8);
        controls.push(c);
    }
    let env = start(controls).await;
    connect(&env, &ha);
    wait_until("every light to be known", || {
        names.iter().all(|n| reading(&env, n) == known("on"))
    })
    .await;
    let per_refresh = ha
        .server
        .received()
        .iter()
        .filter(|r| r.method == "GET")
        .count();
    assert!(per_refresh >= 20, "{per_refresh}");
}

// ---------------------------------------------------------------------------
// The plugin is a plugin
// ---------------------------------------------------------------------------

#[test]
fn launchdeck_knows_nothing_about_home_assistant() {
    // Nothing in the core may know this plugin: not its id, not the API it speaks.
    // (The product's name does appear in prose, as an example of a local server.)
    fn walk(dir: &Path, hits: &mut Vec<String>) {
        for entry in fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                walk(&path, hits);
            } else if path.extension().is_some_and(|e| e == "rs") {
                let text = fs::read_to_string(&path).unwrap();
                for needle in [
                    PLUGIN,
                    "home-assistant.wasm",
                    "/api/states",
                    "/api/services",
                    "entity_id",
                ] {
                    if text.contains(needle) {
                        hits.push(format!("{} mentions {needle}", path.display()));
                    }
                }
            }
        }
    }
    let mut hits = Vec::new();
    walk(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("src"),
        &mut hits,
    );
    assert!(
        hits.is_empty(),
        "the core special-cases the plugin: {hits:?}"
    );

    // And the plugin uses only what any plugin may: the SDK, and the two
    // network functions its manifest asks for.
    let manifest: Value = serde_json::from_str(include_str!(
        "../../examples/plugins/home-assistant/plugin.json"
    ))
    .unwrap();
    assert_eq!(manifest["schemaVersion"], 4);
    assert_eq!(
        manifest["capabilities"]["network"]["hosts"],
        json!([{ "setting": "baseUrl" }])
    );
    assert_eq!(
        manifest["secrets"][0]["host"],
        json!({ "setting": "baseUrl" })
    );
    let actions: Vec<&str> = manifest["actions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|a| a["id"].as_str().unwrap())
        .collect();
    assert_eq!(actions, vec!["toggle", "turn_on", "turn_off"]);
    let wasm = fs::read(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../examples/plugins/home-assistant/home-assistant.wasm"),
    )
    .unwrap();
    let module = wasmparser_imports(&wasm);
    assert!(module.iter().all(|(m, _)| m == "launchdeck"), "{module:?}");
    let mut names: Vec<&str> = module.iter().map(|(_, n)| n.as_str()).collect();
    names.sort_unstable();
    assert_eq!(
        names,
        vec!["http_request", "http_response", "log", "publish", "respond"]
    );
}

/// The import section of a module, read by hand: (module, name) pairs.
fn wasmparser_imports(wasm: &[u8]) -> Vec<(String, String)> {
    fn leb(bytes: &[u8], at: &mut usize) -> usize {
        let (mut value, mut shift) = (0usize, 0);
        loop {
            let b = bytes[*at];
            *at += 1;
            value |= ((b & 0x7f) as usize) << shift;
            shift += 7;
            if b < 0x80 {
                return value;
            }
        }
    }
    let mut out = Vec::new();
    let mut at = 8;
    while at < wasm.len() {
        let id = wasm[at];
        at += 1;
        let size = leb(wasm, &mut at);
        if id == 2 {
            let mut p = at;
            let count = leb(wasm, &mut p);
            for _ in 0..count {
                let name = |p: &mut usize| {
                    let len = leb(wasm, p);
                    let s = String::from_utf8_lossy(&wasm[*p..*p + len]).to_string();
                    *p += len;
                    s
                };
                let (module, field) = (name(&mut p), name(&mut p));
                let kind = wasm[p];
                p += 1;
                assert_eq!(kind, 0, "only functions are imported");
                leb(wasm, &mut p);
                out.push((module, field));
            }
        }
        at += size;
    }
    out
}
