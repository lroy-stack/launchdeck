//! Where a plugin's secret may be kept, and what happens when it may not.
//!
//! * A secret lives in the system's credential store or nowhere. Without one,
//!   secret-backed functions are unavailable and say so; nothing is written to
//!   a plain file.
//! * `secrets.json`, which an earlier version wrote as a fallback, is never
//!   read to authenticate a request. It is listed, and moved or deleted when
//!   the person asks.
//! * A setting that a manifest older than schema 4 marks `secret` keeps
//!   working exactly as it did, and is flagged.
//!
//! The plugin is the real Home Assistant example; the other end of the wire is
//! a real local server that records what reaches it. The credential store is
//! the injectable part: a test never touches the keychain of the machine it
//! runs on. The last test does, on purpose, and is run by
//! `scripts/test-secret-service.sh` inside a private session.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use launchdeck_lib::events::EventBus;
use launchdeck_lib::plugins::external::capabilities::{Decision, Grants};
use launchdeck_lib::plugins::external::host::LegacySource;
use launchdeck_lib::plugins::external::secrets::{
    open_store, KeychainStore, LegacyFile, MemoryStore, SecretRef, SecretStore, StoreKind,
    UnavailableStore, LEGACY_FILE,
};
use launchdeck_lib::plugins::external::{Limits, PluginHost, PluginStatus};
use launchdeck_lib::plugins::{ActionError, PluginContext, PluginRegistry};
use launchdeck_lib::state::StateHub;
use serde_json::{json, Value};

#[allow(dead_code)]
mod mock {
    include!("support/http_server.rs");
    include!("support/mock_home_assistant.rs");
}
use mock::MockHomeAssistant;

const HA: &str = "example.home-assistant";
const HELLO: &str = "example.hello";
const TOKEN: &str = "eyJ0eXAiOiJKV1QiLCJhbGciOiJIUzI1NiJ9.plain-file-era-token";
const KITCHEN: &str = "light.kitchen";

struct World {
    host: PluginHost,
    registry: PluginRegistry,
    hub: StateHub,
    bus: EventBus,
    data: PathBuf,
    plugins: PathBuf,
    _root: tempfile::TempDir,
}

fn example(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../examples/plugins")
        .join(name)
}

fn install_example(plugins: &Path, name: &str) {
    let to = plugins.join(name);
    fs::create_dir_all(&to).unwrap();
    for file in ["plugin.json".to_string(), format!("{name}.wasm")] {
        fs::copy(example(name).join(&file), to.join(&file))
            .unwrap_or_else(|e| panic!("{file}: {e}"));
    }
}

/// A folder with a manifest of the test's own and the Hello module.
fn install_hello_as(plugins: &Path, manifest: &Value) {
    let to = plugins.join("hello");
    fs::create_dir_all(&to).unwrap();
    fs::write(
        to.join("plugin.json"),
        serde_json::to_vec_pretty(manifest).unwrap(),
    )
    .unwrap();
    fs::copy(example("hello").join("hello.wasm"), to.join("hello.wasm")).unwrap();
}

/// A host over `root` with the store it is given, as the app builds it.
fn host_in(root: &Path, store: Arc<dyn SecretStore>) -> (PluginHost, PluginRegistry, StateHub) {
    let (plugins, data) = (root.join("plugins"), root.join("data"));
    fs::create_dir_all(&data).unwrap();
    fs::create_dir_all(&plugins).unwrap();
    let grants = Arc::new(Grants::new(
        Some(data.join("permissions.json")),
        Some(data.join("audit.jsonl")),
    ));
    let registry = PluginRegistry::default();
    let hub = StateHub::new();
    let host = PluginHost::with_parts(
        vec![plugins],
        Some(data.join("plugins.json")),
        registry.clone(),
        Limits::default(),
        hub.clone(),
        grants,
        store,
    );
    host.scan();
    assert!(host.issues().is_empty(), "{:?}", host.issues());
    (host, registry, hub)
}

fn world(store: Arc<dyn SecretStore>, prepare: impl FnOnce(&Path, &Path)) -> World {
    let root = tempfile::tempdir().unwrap();
    let (plugins, data) = (root.path().join("plugins"), root.path().join("data"));
    fs::create_dir_all(&data).unwrap();
    fs::create_dir_all(&plugins).unwrap();
    prepare(&plugins, &data);
    let (host, registry, hub) = host_in(root.path(), store);
    World {
        host,
        registry,
        hub,
        bus: EventBus::new(),
        data,
        plugins,
        _root: root,
    }
}

impl World {
    fn invoke(&self, id: &str, action: &str, args: Value) -> Result<Value, ActionError> {
        let ctx = PluginContext::new(id, self.bus.clone(), tokio::runtime::Handle::current())
            .with_hub(self.hub.clone());
        self.registry
            .get(id)
            .ok_or_else(|| ActionError::UnknownPlugin(id.into()))?
            .invoke(&ctx, action, &args)
    }

    /// Point the Home Assistant plugin at `ha` and allow that address.
    fn connect(&self, ha: &MockHomeAssistant) {
        let origin = ha.server.origin();
        self.host.set_config(HA, "baseUrl", json!(origin)).unwrap();
        self.host
            .set_permission(HA, &origin, Some(Decision::Granted))
            .unwrap();
    }

    fn toggle(&self) -> Result<Value, ActionError> {
        self.invoke(HA, "toggle", json!({ "entity": KITCHEN }))
    }

    /// Everything LaunchDeck wrote in its data folder, as one text.
    fn written(&self) -> String {
        let mut out = String::new();
        for entry in fs::read_dir(&self.data).unwrap().flatten() {
            if entry.path().is_file() {
                out.push_str(&String::from_utf8_lossy(&fs::read(entry.path()).unwrap()));
            }
        }
        out
    }

    fn logs(&self, id: &str) -> String {
        self.host
            .logs(id)
            .unwrap()
            .into_iter()
            .map(|l| l.message)
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn secret_is_set(&self) -> bool {
        self.host.get(HA).unwrap().secrets[0].is_set
    }
}

fn plant_legacy_file(data: &Path, plugin: &str, key: &str, value: &str) -> PathBuf {
    let path = data.join(LEGACY_FILE);
    fs::write(
        &path,
        serde_json::to_vec(&json!({ "schemaVersion": 1, "secrets": { plugin: { key: value } } }))
            .unwrap(),
    )
    .unwrap();
    path
}

fn ha() -> MockHomeAssistant {
    MockHomeAssistant::start(TOKEN, &[(KITCHEN, "off")])
}

// ---------------------------------------------------------------------------
// No credential store
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread")]
async fn without_a_credential_store_a_secret_cannot_be_saved_and_nothing_is_written_instead() {
    let ha = ha();
    let w = world(
        Arc::new(UnavailableStore::new(
            "No Secret Service answered in this session.",
        )),
        |plugins, _| install_example(plugins, "home-assistant"),
    );
    w.connect(&ha);

    // The state is explicit, with the reason, before anyone tries anything.
    let overview = w.host.overview();
    assert_eq!(overview.secret_store.kind, StoreKind::Unavailable);
    assert!(!overview.secret_store.available);
    assert_eq!(
        overview.secret_store.reason.as_deref(),
        Some("No Secret Service answered in this session.")
    );
    let info = w.host.get(HA).unwrap();
    assert_eq!(info.secret_store, StoreKind::Unavailable);
    assert!(!info.secrets[0].is_set);
    // The plugin itself is fine. What is unavailable is what needs the secret.
    assert_eq!(
        info.plugin.external.as_ref().unwrap().status,
        PluginStatus::Enabled
    );

    // Saving is refused, in words.
    let e = w
        .host
        .set_secret(HA, "token", TOKEN)
        .unwrap_err()
        .to_string();
    assert!(e.contains("no secure credential store"), "{e}");
    assert!(!w.secret_is_set());

    // And nothing was written anywhere instead.
    assert!(!w.data.join(LEGACY_FILE).exists(), "no plain file appears");
    assert!(!w.written().contains(TOKEN));
    assert!(!w.logs(HA).contains(TOKEN));

    // The action that needs the secret fails with the reason, and nothing is sent.
    let e = w.toggle().unwrap_err().to_string();
    assert!(e.contains("no secure credential store"), "{e}");
    assert!(e.contains("secret_unavailable"), "{e}");
    assert_eq!(
        ha.server.hits(),
        0,
        "no request went out without its credential"
    );
    let failure = w.host.get(HA).unwrap().network_failure.unwrap();
    assert_eq!(failure.code, "secret_unavailable");
}

// ---------------------------------------------------------------------------
// The plain file of an earlier version
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread")]
async fn a_plain_file_from_an_earlier_version_is_never_used_until_it_is_moved() {
    let ha = ha();
    let store = Arc::new(MemoryStore::new());
    let mut file = PathBuf::new();
    let w = world(store.clone(), |plugins, data| {
        install_example(plugins, "home-assistant");
        file = plant_legacy_file(data, HA, "token", TOKEN);
    });
    w.connect(&ha);

    // It is reported by name, never by value, and it is not a set secret.
    let legacy = w.host.overview().legacy_secrets;
    assert_eq!(legacy.entries.len(), 1);
    let entry = &legacy.entries[0];
    assert_eq!(
        (entry.plugin.as_str(), entry.key.as_str(), entry.source),
        (HA, "token", LegacySource::File)
    );
    assert_eq!(entry.plugin_name.as_deref(), Some("Home Assistant"));
    assert_eq!(legacy.file.as_deref(), Some(file.to_str().unwrap()));
    assert!(!serde_json::to_string(&w.host.overview())
        .unwrap()
        .contains(TOKEN));
    assert!(!w.secret_is_set());

    // The plain value authenticates nothing.
    let e = w.toggle().unwrap_err().to_string();
    assert!(e.contains("secret_missing"), "{e}");
    assert_eq!(ha.server.hits(), 0);
    assert_eq!(store.get(&SecretRef::new(HA, "token")).unwrap(), None);
    assert!(file.exists(), "and nobody touched the file");

    // Moved on request: written, read back, and only then is the file removed.
    let report = w.host.migrate_legacy_secrets().unwrap();
    assert_eq!((report.moved, report.kept), (1, 0));
    assert!(report.failed.is_empty() && report.file_removed);
    assert!(!file.exists());
    assert_eq!(
        store.get(&SecretRef::new(HA, "token")).unwrap().as_deref(),
        Some(TOKEN)
    );
    assert!(w.secret_is_set());
    assert!(w.host.overview().legacy_secrets.is_empty());
    assert!(
        !w.written().contains(TOKEN),
        "no file of LaunchDeck's holds it now"
    );

    // And the plugin works with it.
    w.toggle().unwrap();
    assert_eq!(ha.state(KITCHEN).as_deref(), Some("on"));
    assert!(ha
        .server
        .received()
        .iter()
        .all(|r| r.header("authorization") == Some(&format!("Bearer {TOKEN}"))));

    // The trail says what happened and holds no value.
    let trail = fs::read_to_string(w.data.join("audit.jsonl")).unwrap();
    assert!(
        trail.contains("secrets.migrated") && trail.contains("moved 1"),
        "{trail}"
    );
    assert!(!trail.contains(TOKEN));
    assert!(!w.logs(HA).contains(TOKEN));

    // Moving again finds nothing to do.
    let again = w.host.migrate_legacy_secrets().unwrap();
    assert_eq!((again.moved, again.kept, again.file_removed), (0, 0, false));
}

#[tokio::test(flavor = "multi_thread")]
async fn without_a_credential_store_the_plain_file_is_neither_used_nor_touched() {
    let ha = ha();
    let mut file = PathBuf::new();
    let w = world(
        Arc::new(UnavailableStore::new("none here")),
        |plugins, data| {
            install_example(plugins, "home-assistant");
            file = plant_legacy_file(data, HA, "token", TOKEN);
        },
    );
    w.connect(&ha);
    let before = fs::read(&file).unwrap();

    assert_eq!(
        w.host.overview().legacy_secrets.entries.len(),
        1,
        "it is reported"
    );
    assert!(!w.secret_is_set());
    assert!(w.toggle().is_err());
    assert_eq!(
        ha.server.hits(),
        0,
        "the plain value is not loaded for a schema 4 plugin"
    );

    // It cannot be moved, and trying changes nothing.
    let e = w.host.migrate_legacy_secrets().unwrap_err().to_string();
    assert!(e.contains("no secure credential store"), "{e}");
    assert_eq!(fs::read(&file).unwrap(), before);

    // Deleting it is the person's choice, and is the only thing that removes it.
    assert_eq!(w.host.discard_legacy_secrets().unwrap(), 1);
    assert!(!file.exists());
    assert!(w.host.overview().legacy_secrets.is_empty());
    let trail = fs::read_to_string(w.data.join("audit.jsonl")).unwrap();
    assert!(trail.contains("secrets.legacy_discarded"), "{trail}");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_value_already_in_the_credential_store_wins_and_a_refused_one_keeps_the_file() {
    let store = Arc::new(MemoryStore::new());
    let w = world(store.clone(), |plugins, data| {
        install_example(plugins, "home-assistant");
        fs::write(
            data.join(LEGACY_FILE),
            serde_json::to_vec(&json!({ "schemaVersion": 1, "secrets": {
                HA: { "token": "older-plain-value" },
                "gone.plugin": { "key": "x".repeat(5000) }
            }}))
            .unwrap(),
        )
        .unwrap();
    });
    // Entered after the plain file stopped being used.
    w.host.set_secret(HA, "token", "newer-value").unwrap();

    let legacy = w.host.overview().legacy_secrets;
    assert_eq!(legacy.entries.len(), 2);
    assert_eq!(
        legacy
            .entries
            .iter()
            .map(|e| e.plugin_name.is_some())
            .collect::<Vec<_>>(),
        vec![true, false],
        "a plugin that is not installed is listed by its id only"
    );

    let report = w.host.migrate_legacy_secrets().unwrap();
    assert_eq!((report.moved, report.kept), (0, 1));
    assert_eq!(report.failed.len(), 1);
    assert_eq!(report.failed[0].plugin, "gone.plugin");
    assert!(
        report.failed[0].reason.contains("longer than"),
        "{:?}",
        report.failed
    );
    assert!(
        !report.file_removed,
        "one value could not be moved, so the file stays whole"
    );
    assert!(w.data.join(LEGACY_FILE).exists());
    assert_eq!(
        store.get(&SecretRef::new(HA, "token")).unwrap().as_deref(),
        Some("newer-value")
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_plain_file_this_version_cannot_read_is_reported_and_left_alone() {
    let mut file = PathBuf::new();
    let w = world(Arc::new(MemoryStore::new()), |plugins, data| {
        install_example(plugins, "home-assistant");
        file = data.join(LEGACY_FILE);
        fs::write(
            &file,
            r#"{"schemaVersion": 99, "secrets": {"a.b": {"k": "v"}}}"#,
        )
        .unwrap();
    });
    let legacy = w.host.overview().legacy_secrets;
    assert!(legacy.unreadable && legacy.entries.is_empty() && legacy.file.is_some());
    let report = w.host.migrate_legacy_secrets().unwrap();
    assert_eq!(report.moved, 0);
    assert_eq!(report.failed.len(), 1);
    assert!(!report.file_removed && file.exists());
}

// ---------------------------------------------------------------------------
// Manifests older than schema 4
// ---------------------------------------------------------------------------

fn hello_manifest(schema: u32, version: &str, extra: Value) -> Value {
    let mut m = json!({
        "schemaVersion": schema,
        "id": HELLO,
        "name": "Hello Plugin",
        "version": version,
        "author": "LaunchDeck tests",
        "abi": 1,
        "main": "hello.wasm",
        "actions": [{ "id": "hello.say", "label": "Say hello",
            "params": [{ "name": "name", "label": "Who", "type": "string", "maxLength": 60 }] }],
        "events": [{ "type": "hello.completed" }]
    });
    for (k, v) in extra.as_object().unwrap() {
        m[k] = v.clone();
    }
    m
}

#[tokio::test(flavor = "multi_thread")]
async fn a_secret_setting_of_an_older_manifest_keeps_working_as_it_did_and_is_flagged() {
    // Schema 3: `greeting` is a setting marked secret. The module reads it.
    let legacy = hello_manifest(
        3,
        "1.0.0",
        json!({ "config": [{ "key": "greeting", "type": "string", "secret": true }] }),
    );
    let w = world(
        Arc::new(UnavailableStore::new("none here")),
        |plugins, _| install_hello_as(plugins, &legacy),
    );
    w.host
        .set_config(HELLO, "greeting", json!("Sesame"))
        .unwrap();

    // Unchanged: it is stored with the plugin's settings and handed to the
    // module, even on a system without a credential store.
    let said = w
        .invoke(HELLO, "hello.say", json!({ "name": "Ada" }))
        .unwrap();
    assert_eq!(said["message"], "Sesame, Ada!");
    assert!(fs::read_to_string(w.data.join("plugins.json"))
        .unwrap()
        .contains("Sesame"));
    // Unchanged: the interface never gets the value back.
    let info = w.host.get(HELLO).unwrap();
    assert!(info.config[0].secret && info.config[0].is_set && info.config[0].value.is_none());
    assert!(!serde_json::to_string(&w.host.overview())
        .unwrap()
        .contains("Sesame"));
    // New: it is said plainly what this is.
    let log = w.logs(HELLO);
    assert!(
        log.contains("marked secret by a manifest older than schema 4"),
        "{log}"
    );
    assert!(log.contains("without encryption"), "{log}");
    assert!(!log.contains("Sesame"));
    // It is not a secret of the new kind, so it is not something to move.
    assert!(info.secrets.is_empty());
    assert!(w.host.overview().legacy_secrets.is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_plugin_that_moves_a_setting_to_a_secret_has_the_plain_value_moved_on_request() {
    let server = mock::TestServer::start(|_| mock::Reply::text(200, "ok"));
    let old = hello_manifest(
        3,
        "1.0.0",
        json!({ "config": [{ "key": "token", "type": "string", "secret": true }] }),
    );
    let store = Arc::new(MemoryStore::new());
    let w = world(store.clone(), |plugins, _| install_hello_as(plugins, &old));
    w.host
        .set_config(HELLO, "token", json!("typed-under-schema-3"))
        .unwrap();

    // The author moves to schema 4: `token` is now a secret, bound to a host.
    let new = hello_manifest(
        4,
        "2.0.0",
        json!({
            "capabilities": { "network": { "purpose": "Testing.", "hosts": [server.origin()] } },
            "secrets": [{ "key": "token", "label": "Token", "host": server.origin() }]
        }),
    );
    install_hello_as(&w.plugins, &new);
    w.host.scan();
    assert!(w.host.issues().is_empty(), "{:?}", w.host.issues());

    // The value typed before is not adopted silently, and not lost: it is listed.
    let info = w.host.get(HELLO).unwrap();
    assert_eq!(info.plugin.version, "2.0.0");
    assert!(!info.secrets[0].is_set);
    let legacy = w.host.overview().legacy_secrets;
    assert_eq!(legacy.entries.len(), 1);
    assert_eq!(
        (legacy.entries[0].key.as_str(), legacy.entries[0].source),
        ("token", LegacySource::Settings)
    );
    assert!(legacy.file.is_none());
    // The module of the new version never receives it.
    let said = w
        .invoke(HELLO, "hello.say", json!({ "name": "Ada" }))
        .unwrap();
    assert!(!said.to_string().contains("typed-under-schema-3"));

    let report = w.host.migrate_legacy_secrets().unwrap();
    assert_eq!((report.moved, report.failed.len()), (1, 0));
    assert_eq!(
        store
            .get(&SecretRef::new(HELLO, "token"))
            .unwrap()
            .as_deref(),
        Some("typed-under-schema-3")
    );
    assert!(w.host.get(HELLO).unwrap().secrets[0].is_set);
    assert!(
        !fs::read_to_string(w.data.join("plugins.json"))
            .unwrap()
            .contains("typed-under-schema-3"),
        "the plain copy is gone from plugins.json"
    );
    assert!(w.host.overview().legacy_secrets.is_empty());
}

// ---------------------------------------------------------------------------
// Which store the app opens
// ---------------------------------------------------------------------------

#[test]
fn nothing_in_the_runtime_can_open_a_plain_file_as_a_store() {
    // The only stores are the system's, none, and memory. `LegacyFile` is not
    // one: it has no way to answer the broker.
    let dir = tempfile::tempdir().unwrap();
    plant_legacy_file(dir.path(), HA, "token", TOKEN);
    let legacy = LegacyFile::in_dir(dir.path());
    // (compile-time) `LegacyFile` does not implement `SecretStore`:
    //   let _: &dyn SecretStore = &legacy;
    assert!(legacy.path().exists());
    assert_eq!(open_store(None).kind(), StoreKind::Memory);
    let source = fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("src/plugins/external/secrets.rs"),
    )
    .unwrap();
    let stores: Vec<&str> = source
        .lines()
        .filter(|l| l.starts_with("impl SecretStore for "))
        .collect();
    assert_eq!(
        stores,
        vec![
            "impl SecretStore for MemoryStore {",
            "impl SecretStore for UnavailableStore {",
            "impl SecretStore for KeychainStore {",
        ],
        "a new store must be a secure one; a file is not"
    );
    assert!(
        !source.contains("LAUNCHDECK_SECRET_STORE"),
        "no switch brings the file back"
    );
}

/// The system's real credential store. Run inside a private D-Bus session with
/// an unlocked keyring (`scripts/test-secret-service.sh`), never by a plain
/// `cargo test`: it would write to the keychain of whoever runs it.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs a real credential store; run scripts/test-secret-service.sh"]
async fn the_real_credential_store_keeps_a_secret_out_of_every_plain_file() {
    let store = KeychainStore::open().expect("a credential store answers in this session");
    assert_eq!(store.kind(), StoreKind::Keychain);
    assert!(store.status().available);
    let dir = tempfile::tempdir().unwrap();
    let opened = open_store(Some(dir.path()));
    assert_eq!(
        opened.kind(),
        StoreKind::Keychain,
        "the app would use it too"
    );

    let ha = ha();
    let mut file = PathBuf::new();
    let w = world(Arc::new(store), |plugins, data| {
        install_example(plugins, "home-assistant");
        file = plant_legacy_file(data, HA, "token", TOKEN);
    });
    w.connect(&ha);
    let secret = SecretRef::new(HA, "token");
    w.host.secret_store().delete(&secret).unwrap();

    // The plain file of an earlier version moves into the real store.
    let report = w.host.migrate_legacy_secrets().unwrap();
    assert_eq!(
        (report.moved, report.failed.len(), report.file_removed),
        (1, 0, true)
    );
    assert!(!file.exists());
    assert!(w.secret_is_set());

    // A new process finds it there: the store, not this process, holds it.
    let again = KeychainStore::open().unwrap();
    assert_eq!(again.get(&secret).unwrap().as_deref(), Some(TOKEN));

    // The broker reads it from there, and it reaches its one address.
    w.toggle().unwrap();
    assert_eq!(ha.state(KITCHEN).as_deref(), Some("on"));

    // Replaced, then removed.
    w.host.set_secret(HA, "token", "a-second-value").unwrap();
    assert_eq!(
        again.get(&secret).unwrap().as_deref(),
        Some("a-second-value")
    );
    assert!(!w.written().contains(TOKEN) && !w.written().contains("a-second-value"));
    w.host.clear_secret(HA, "token").unwrap();
    assert_eq!(again.get(&secret).unwrap(), None);
    assert!(!w.secret_is_set());
}
