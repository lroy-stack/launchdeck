//! A credential an earlier version kept in plain text in `plugins.json` is
//! never handed to a plugin, whatever its manifest's schema.
//!
//! Manifests of schemas 1 to 3 may mark a setting `"secret": true`. Its value
//! was saved with the plugin's settings, in the clear, and given to the
//! module at `init`. Now the module reads such a setting as not set; the plain
//! value stays exactly where it is until the person moves it into the
//! credential store or removes it.
//!
//! The module under test answers every action with the `init` request it was
//! handed, so what it received is the action's result. The credential store is
//! injected: an in-memory one, one that refuses every write, and one that
//! gives back something other than what was written.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use launchdeck_lib::events::EventBus;
use launchdeck_lib::plugins::external::capabilities::Grants;
use launchdeck_lib::plugins::external::host::LegacySource;
use launchdeck_lib::plugins::external::secrets::{
    MemoryStore, SecretError, SecretRef, SecretStore, StoreKind, UnavailableStore,
};
use launchdeck_lib::plugins::external::{HostError, Limits, PluginHost};
use launchdeck_lib::plugins::{PluginContext, PluginRegistry};
use launchdeck_lib::state::StateHub;
use serde_json::{json, Value};

#[path = "support/wat_plugins.rs"]
#[allow(dead_code)]
mod wat;

const ID: &str = "test.echo";
const ACTION: &str = "echo.read";
const PLAIN: &str = "plain-s3cret-from-plugins-json";
const NETWORK_HOST: &str = "https://api.example.com";

/// A manifest of `schema` with one ordinary setting, `greeting`, and one
/// credential, `token`: a setting marked secret before schema 4, a declared
/// secret from schema 4.
fn manifest(schema: u32) -> Value {
    let action = if schema == 1 {
        json!({ "id": ACTION, "label": "Read", "args": [] })
    } else {
        json!({ "id": ACTION, "label": "Read" })
    };
    let mut m = json!({
        "schemaVersion": schema, "id": ID, "name": "Echo", "version": "1.0.0",
        "author": "tests", "abi": 1, "main": "m.wasm", "actions": [action], "events": []
    });
    if schema < 4 {
        m["config"] = json!([
            { "key": "greeting", "type": "string" },
            { "key": "token", "type": "string", "secret": true }
        ]);
    } else {
        m["config"] = json!([{ "key": "greeting", "type": "string" }]);
        m["capabilities"] =
            json!({ "network": { "purpose": "Testing.", "hosts": [NETWORK_HOST] } });
        m["secrets"] = json!([{ "key": "token", "label": "Token", "host": NETWORK_HOST }]);
    }
    m
}

/// What an earlier version left in `plugins.json`: the greeting, and the token
/// in plain text.
fn saved_settings() -> Value {
    json!({ "enabled": true, "config": { "greeting": "Hi", "token": PLAIN } })
}

struct World {
    host: PluginHost,
    registry: PluginRegistry,
    state: PathBuf,
    plugins: PathBuf,
    data: PathBuf,
    _root: tempfile::TempDir,
}

fn write_plugin(plugins: &Path, manifest: &Value) {
    let dir = plugins.join("echo");
    fs::create_dir_all(&dir).unwrap();
    fs::write(
        dir.join("plugin.json"),
        serde_json::to_vec_pretty(manifest).unwrap(),
    )
    .unwrap();
    fs::write(dir.join("m.wasm"), wat::echo_init().build()).unwrap();
}

fn world(manifest: &Value, store: Arc<dyn SecretStore>) -> World {
    world_with(manifest, saved_settings(), store)
}

fn world_with(manifest: &Value, settings: Value, store: Arc<dyn SecretStore>) -> World {
    let root = tempfile::tempdir().unwrap();
    let (plugins, data) = (root.path().join("plugins"), root.path().join("data"));
    fs::create_dir_all(&data).unwrap();
    write_plugin(&plugins, manifest);
    let state = data.join("plugins.json");
    fs::write(
        &state,
        serde_json::to_vec_pretty(&json!({ "schemaVersion": 1, "plugins": { ID: settings } }))
            .unwrap(),
    )
    .unwrap();
    let registry = PluginRegistry::default();
    let host = PluginHost::with_parts(
        vec![plugins.clone()],
        Some(state.clone()),
        registry.clone(),
        Limits::default(),
        StateHub::new(),
        Arc::new(Grants::new(
            Some(data.join("permissions.json")),
            Some(data.join("audit.jsonl")),
        )),
        store,
    );
    host.scan();
    assert!(host.issues().is_empty(), "{:?}", host.issues());
    World {
        host,
        registry,
        state,
        plugins,
        data,
        _root: root,
    }
}

impl World {
    /// The `init` request the running module was handed.
    fn received(&self) -> Value {
        let ctx = PluginContext::new(ID, EventBus::new(), tokio::runtime::Handle::current());
        self.registry
            .get(ID)
            .expect("the plugin is registered")
            .invoke(&ctx, ACTION, &json!({}))
            .expect("the module answers")
    }

    fn file(&self) -> String {
        fs::read_to_string(&self.state).unwrap()
    }

    /// The token as `plugins.json` holds it now.
    fn plain_token(&self) -> Option<Value> {
        let saved: Value = serde_json::from_str(&self.file()).unwrap();
        saved["plugins"][ID]["config"].get("token").cloned()
    }

    fn logs(&self) -> String {
        self.host
            .logs(ID)
            .unwrap()
            .into_iter()
            .map(|l| l.message)
            .collect::<Vec<_>>()
            .join("\n")
    }
}

fn memory() -> Arc<dyn SecretStore> {
    Arc::new(MemoryStore::new())
}

fn token() -> SecretRef {
    SecretRef::new(ID, "token")
}

/// A credential store that answers, and refuses every write.
struct RefusingStore;

impl SecretStore for RefusingStore {
    fn get(&self, _: &SecretRef) -> Result<Option<String>, SecretError> {
        Ok(None)
    }
    fn set(&self, _: &SecretRef, _: &str) -> Result<(), SecretError> {
        Err(SecretError::Store(
            "it is locked or cannot be reached".into(),
        ))
    }
    fn delete(&self, _: &SecretRef) -> Result<(), SecretError> {
        Ok(())
    }
    fn kind(&self) -> StoreKind {
        StoreKind::Keychain
    }
}

/// A credential store that keeps something other than what it was given.
#[derive(Default)]
struct GarblingStore(MemoryStore);

impl SecretStore for GarblingStore {
    fn get(&self, s: &SecretRef) -> Result<Option<String>, SecretError> {
        self.0.get(s)
    }
    fn set(&self, s: &SecretRef, value: &str) -> Result<(), SecretError> {
        self.0.set(s, &format!("{value}-garbled"))
    }
    fn delete(&self, s: &SecretRef) -> Result<(), SecretError> {
        self.0.delete(s)
    }
    fn kind(&self) -> StoreKind {
        StoreKind::Keychain
    }
}

/// A credential store that keeps every key but one.
struct RefusesOneKey(MemoryStore, &'static str);

impl SecretStore for RefusesOneKey {
    fn get(&self, s: &SecretRef) -> Result<Option<String>, SecretError> {
        self.0.get(s)
    }
    fn set(&self, s: &SecretRef, value: &str) -> Result<(), SecretError> {
        if s.key == self.1 {
            return Err(SecretError::Store("the value is too long for it".into()));
        }
        self.0.set(s, value)
    }
    fn delete(&self, s: &SecretRef) -> Result<(), SecretError> {
        self.0.delete(s)
    }
    fn kind(&self) -> StoreKind {
        StoreKind::Keychain
    }
}

// ---------------------------------------------------------------------------
// Never used
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread")]
async fn a_schema_3_secret_setting_from_plugins_json_never_reaches_the_module() {
    let w = world(&manifest(3), memory());
    let got = w.received();
    assert_eq!(got["op"], "init");
    assert_eq!(
        got["config"]["greeting"], "Hi",
        "an ordinary setting still reaches the module"
    );
    assert!(
        got["config"].get("token").is_none(),
        "the module reads the secret setting as not set: {got}"
    );
    assert!(!got.to_string().contains(PLAIN));
    // The plugin loads and runs; the value is still on disk, untouched.
    assert_eq!(w.plain_token(), Some(json!(PLAIN)));
    // Said in the log, without the value.
    let log = w.logs();
    assert!(
        log.contains("marked secret by a manifest older than schema 4"),
        "{log}"
    );
    assert!(log.contains("does not hand"), "{log}");
    assert!(!log.contains(PLAIN), "{log}");
}

#[tokio::test(flavor = "multi_thread")]
async fn no_secret_value_from_plugins_json_reaches_a_module_whatever_the_schema() {
    for schema in [1, 2, 3, 4] {
        let w = world(&manifest(schema), memory());
        let got = w.received();
        assert_eq!(got["config"]["greeting"], "Hi", "schema {schema}");
        assert!(
            !got.to_string().contains(PLAIN),
            "schema {schema}: the module received {got}"
        );
        // Not after a restart of the plugin, a rescan or a change of another setting either.
        w.host.reload(ID).unwrap();
        w.host.scan();
        w.host.set_config(ID, "greeting", json!("Hey")).unwrap();
        let got = w.received();
        assert_eq!(got["config"]["greeting"], "Hey", "schema {schema}");
        assert!(
            !got.to_string().contains(PLAIN),
            "schema {schema}: the module received {got}"
        );
        assert_eq!(w.plain_token(), Some(json!(PLAIN)), "schema {schema}");
        // Reported the same way, by name.
        let info = w.host.get(ID).unwrap();
        let migration = info.secret_migration.expect("migration required");
        assert_eq!(migration.migration_required, ["token"], "schema {schema}");
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn nothing_but_the_person_changes_the_plain_value_in_plugins_json() {
    let w = world(&manifest(3), memory());
    let before = w.file();
    let entry = format!("\"token\": \"{PLAIN}\"");
    assert!(before.contains(&entry), "{before}");

    // Loading, reloading and rescanning do not even write the file.
    w.received();
    w.host.reload(ID).unwrap();
    w.host.scan();
    write_plugin(&w.plugins, &{
        let mut m = manifest(3);
        m["version"] = json!("1.0.1");
        m
    });
    assert_eq!(w.host.scan().reloaded, 1);
    assert_eq!(w.file(), before, "plugins.json is byte for byte as it was");

    // Saving another setting and disabling or enabling the plugin rewrite the
    // file and keep the plain value exactly as it was.
    w.host.set_config(ID, "greeting", json!("Hello")).unwrap();
    assert!(w.file().contains(&entry));
    w.host.set_enabled(ID, false).unwrap();
    assert!(w.file().contains(&entry));
    w.host.set_enabled(ID, true).unwrap();
    assert!(w.file().contains(&entry));
    assert_eq!(w.plain_token(), Some(json!(PLAIN)));
    assert!(!w.received().to_string().contains(PLAIN));

    // A new process finds it where it was, and does not use it either.
    let again = PluginHost::new(
        vec![w.plugins.clone()],
        Some(w.state.clone()),
        PluginRegistry::default(),
        Limits::default(),
    );
    again.scan();
    assert!(w.file().contains(&entry));
}

#[tokio::test(flavor = "multi_thread")]
async fn a_secret_setting_cannot_be_given_a_new_plain_value() {
    let w = world(&manifest(3), memory());
    let before = w.file();
    for value in [json!("typed-now"), Value::Null] {
        let err = w.host.set_config(ID, "token", value).unwrap_err();
        assert!(matches!(err, HostError::Invalid(_)), "{err:?}");
        assert!(err.to_string().contains("`secrets`"), "{err}");
    }
    assert_eq!(w.file(), before, "nothing was written");
    let info = w.host.get(ID).unwrap();
    let token = info.config.iter().find(|c| c.key == "token").unwrap();
    assert!(token.secret && token.is_set && token.value.is_none());
}

// ---------------------------------------------------------------------------
// Reported, by name
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread")]
async fn the_plugin_says_migration_is_required_naming_the_key_and_never_the_value() {
    let w = world(&manifest(3), memory());
    let info = w.host.get(ID).unwrap();
    let migration = info.secret_migration.clone().expect("migration required");
    assert_eq!(migration.migration_required, ["token"]);
    assert!(migration.needs_update.is_empty());
    let view = serde_json::to_value(&info).unwrap();
    assert_eq!(
        view["secretMigration"],
        json!({ "migrationRequired": ["token"], "needsUpdate": [] })
    );
    // Also in the list of plain secrets, as a setting.
    let legacy = w.host.overview().legacy_secrets;
    assert!(legacy
        .entries
        .iter()
        .any(|e| e.plugin == ID && e.key == "token" && e.source == LegacySource::Settings));
    // The value is nowhere in what the interface receives, nor in the log.
    let everything = serde_json::to_string(&w.host.overview()).unwrap();
    assert!(!everything.contains(PLAIN));
    assert!(w.logs().contains("not used"), "{}", w.logs());
    assert!(!w.logs().contains(PLAIN));

    // A plugin with nothing left in plain text says nothing.
    let clean = world(&manifest(3), memory());
    clean.host.remove_plugin_legacy_secrets(ID).unwrap();
    let after = serde_json::to_value(clean.host.get(ID).unwrap()).unwrap();
    assert!(after.get("secretMigration").is_none(), "{after}");
}

// ---------------------------------------------------------------------------
// Migrate: write, read back, then remove
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread")]
async fn migrate_moves_the_value_into_the_store_and_only_then_removes_the_plain_copy() {
    let store = Arc::new(MemoryStore::new());
    let w = world(&manifest(3), store.clone());
    let info = w.host.migrate_plugin_legacy_secrets(ID).unwrap();

    assert_eq!(store.get(&token()).unwrap().as_deref(), Some(PLAIN));
    assert_eq!(w.plain_token(), None, "the plain copy is gone");
    assert!(!w.file().contains(PLAIN), "and persisted");
    let saved: Value = serde_json::from_str(&w.file()).unwrap();
    assert_eq!(saved["plugins"][ID]["config"]["greeting"], "Hi");
    // This version cannot use it: it declares the credential the old way.
    let migration = info.secret_migration.expect("still something to say");
    assert!(migration.migration_required.is_empty());
    assert_eq!(migration.needs_update, ["token"]);
    // The module still never receives it, now or after a restart.
    assert!(!w.received().to_string().contains(PLAIN));
    w.host.reload(ID).unwrap();
    assert!(!w.received().to_string().contains(PLAIN));
    assert!(w.logs().contains("under `secrets`"), "{}", w.logs());
    // The audit trail says what happened, never the value.
    let trail = fs::read_to_string(w.data.join("audit.jsonl")).unwrap();
    assert!(trail.contains("secrets.migrated"), "{trail}");
    assert!(!trail.contains(PLAIN));
    assert!(!w.logs().contains(PLAIN));

    // Known after a restart too.
    let again = PluginHost::with_parts(
        vec![w.plugins.clone()],
        Some(w.state.clone()),
        PluginRegistry::default(),
        Limits::default(),
        StateHub::new(),
        Arc::new(Grants::new(None, None)),
        store.clone(),
    );
    again.scan();
    let migration = again.get(ID).unwrap().secret_migration.unwrap();
    assert_eq!(migration.needs_update, ["token"]);
    assert!(migration.migration_required.is_empty());
    // Migrating again has nothing to do.
    assert!(again.migrate_plugin_legacy_secrets(ID).is_ok());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_schema_4_plugin_uses_a_migrated_value_through_the_broker_and_never_receives_it() {
    let store = Arc::new(MemoryStore::new());
    let w = world(&manifest(4), store.clone());
    assert!(!w.host.get(ID).unwrap().secrets[0].is_set);
    let info = w.host.migrate_plugin_legacy_secrets(ID).unwrap();
    assert_eq!(store.get(&token()).unwrap().as_deref(), Some(PLAIN));
    assert!(
        info.secrets[0].is_set,
        "the declared secret now has its value"
    );
    assert!(info.secret_migration.is_none(), "nothing left to say");
    assert_eq!(w.plain_token(), None);
    assert!(!w.received().to_string().contains(PLAIN));
}

#[tokio::test(flavor = "multi_thread")]
async fn a_store_that_refuses_the_write_leaves_the_plain_value_untouched() {
    let w = world(&manifest(3), Arc::new(RefusingStore));
    let before = w.file();
    let err = w.host.migrate_plugin_legacy_secrets(ID).unwrap_err();
    let said = err.to_string();
    assert!(
        said.contains("`token`") && said.contains("locked"),
        "{said}"
    );
    assert!(said.contains("still where it was"), "{said}");
    assert!(!said.contains(PLAIN));
    assert_eq!(w.file(), before, "nothing persisted");
    let migration = w.host.get(ID).unwrap().secret_migration.unwrap();
    assert_eq!(migration.migration_required, ["token"]);
    assert!(!w.received().to_string().contains(PLAIN));
}

#[tokio::test(flavor = "multi_thread")]
async fn a_store_that_gives_back_something_else_leaves_the_plain_value_untouched() {
    let store = Arc::new(GarblingStore::default());
    let w = world(&manifest(3), store.clone());
    let before = w.file();
    let said = w
        .host
        .migrate_plugin_legacy_secrets(ID)
        .unwrap_err()
        .to_string();
    assert!(said.contains("did not return what was written"), "{said}");
    assert!(!said.contains(PLAIN));
    assert_eq!(w.file(), before, "nothing persisted");
    // The wrong value is not left in the store, so trying again neither
    // mistakes it for a good one nor is blocked by it.
    assert_eq!(store.get(&token()).unwrap(), None);
    let again = w
        .host
        .migrate_plugin_legacy_secrets(ID)
        .unwrap_err()
        .to_string();
    assert_eq!(again, said);
    assert_eq!(store.get(&token()).unwrap(), None);
    assert_eq!(w.file(), before);
    assert_eq!(
        w.host
            .get(ID)
            .unwrap()
            .secret_migration
            .unwrap()
            .migration_required,
        ["token"]
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_migration_that_fails_for_one_key_leaves_the_store_as_it_was() {
    let mut m = manifest(3);
    m["config"]
        .as_array_mut()
        .unwrap()
        .push(json!({ "key": "pin", "type": "string", "secret": true }));
    let mut settings = saved_settings();
    settings["config"]["pin"] = json!("plain-pin");
    let store = Arc::new(RefusesOneKey(MemoryStore::new(), "pin"));
    let w = world_with(&m, settings, store.clone());
    let before = w.file();
    let said = w
        .host
        .migrate_plugin_legacy_secrets(ID)
        .unwrap_err()
        .to_string();
    assert!(
        said.contains("`pin`") && said.contains("too long"),
        "{said}"
    );
    assert_eq!(w.file(), before, "nothing persisted");
    // `token` was written and read back, then taken out again: all or nothing.
    assert_eq!(store.get(&token()).unwrap(), None);
    let migration = w.host.get(ID).unwrap().secret_migration.unwrap();
    assert_eq!(migration.migration_required, ["pin", "token"]);
    assert!(migration.needs_update.is_empty(), "{migration:?}");
}

#[tokio::test(flavor = "multi_thread")]
async fn without_a_credential_store_migrate_is_refused_with_the_reason() {
    let w = world(
        &manifest(3),
        Arc::new(UnavailableStore::new("No Secret Service answered.")),
    );
    let before = w.file();
    let said = w
        .host
        .migrate_plugin_legacy_secrets(ID)
        .unwrap_err()
        .to_string();
    assert!(said.contains("No Secret Service answered."), "{said}");
    assert_eq!(w.file(), before);
}

#[tokio::test(flavor = "multi_thread")]
async fn moving_every_plain_secret_at_once_keeps_a_value_the_store_holds_differently() {
    let store = Arc::new(MemoryStore::new());
    store.set(&token(), "entered-later").unwrap();
    let w = world(&manifest(3), store.clone());
    let report = w.host.migrate_legacy_secrets().unwrap();
    assert_eq!(report.failed.len(), 1, "{report:?}");
    assert!(report.failed[0].reason.contains("different value"));
    assert_eq!(w.plain_token(), Some(json!(PLAIN)), "the plain one stays");
    assert_eq!(
        store.get(&token()).unwrap().as_deref(),
        Some("entered-later"),
        "and the stored one is not overwritten"
    );
}

// ---------------------------------------------------------------------------
// Remove
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread")]
async fn remove_deletes_the_plain_value_and_nothing_else() {
    let store = Arc::new(MemoryStore::new());
    let w = world(&manifest(3), store.clone());
    w.host.set_enabled(ID, false).unwrap();
    let info = w.host.remove_plugin_legacy_secrets(ID).unwrap();
    assert_eq!(w.plain_token(), None);
    assert!(!w.file().contains(PLAIN));
    let saved: Value = serde_json::from_str(&w.file()).unwrap();
    assert_eq!(saved["plugins"][ID]["config"]["greeting"], "Hi");
    assert_eq!(saved["plugins"][ID]["enabled"], false);
    assert!(!info.enabled);
    assert!(info.secret_migration.is_none());
    assert_eq!(
        store.get(&token()).unwrap(),
        None,
        "the store is not touched"
    );
    let trail = fs::read_to_string(w.data.join("audit.jsonl")).unwrap();
    assert!(trail.contains("secrets.legacy_discarded"), "{trail}");
}

#[tokio::test(flavor = "multi_thread")]
async fn the_commands_refuse_an_unknown_plugin() {
    let w = world(&manifest(3), memory());
    assert!(matches!(
        w.host.migrate_plugin_legacy_secrets("t.nope"),
        Err(HostError::Unknown(_))
    ));
    assert!(matches!(
        w.host.remove_plugin_legacy_secrets("t.nope"),
        Err(HostError::Unknown(_))
    ));
}
