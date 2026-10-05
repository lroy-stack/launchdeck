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
use launchdeck_lib::plugins::external::secrets::{MemoryStore, SecretStore};
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
    let root = tempfile::tempdir().unwrap();
    let (plugins, data) = (root.path().join("plugins"), root.path().join("data"));
    fs::create_dir_all(&data).unwrap();
    write_plugin(&plugins, manifest);
    let state = data.join("plugins.json");
    fs::write(
        &state,
        serde_json::to_vec_pretty(
            &json!({ "schemaVersion": 1, "plugins": { ID: saved_settings() } }),
        )
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
