//! A real Plugins screen payload, built from the real example plugins: one
//! healthy, one with a secret setting whose value an earlier version left in
//! plain text, one that failed to load, one folder
//! that is not a plugin, and two copies of the Home Assistant example: one
//! set up (an address, a grant, a token, and a decision left over from the
//! address it had before), one whose address is not usable, whose last
//! request was refused and whose token an earlier version left in a plain file. Shared by the `dump_plugins` example and the contract
//! test, so the TypeScript schemas are checked against what Rust emits.

use std::fs;
use std::path::Path;

use launchdeck_lib::events::EventBus;
use launchdeck_lib::plugins::external::capabilities::Decision;
use launchdeck_lib::plugins::external::{Limits, PluginHost};
use launchdeck_lib::plugins::{PluginContext, PluginRegistry};
use launchdeck_lib::state::StateAddress;
use serde_json::{json, Value};

fn copy_dir(from: &Path, to: &Path) {
    fs::create_dir_all(to).unwrap();
    for entry in fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let name = entry.file_name();
        if name == "node_modules" {
            continue;
        }
        if entry.file_type().unwrap().is_file() {
            fs::copy(entry.path(), to.join(name)).unwrap();
        }
    }
}

pub fn capture() -> Value {
    let examples = Path::new(env!("CARGO_MANIFEST_DIR")).join("../examples/plugins");
    let root = tempfile::tempdir().unwrap();
    let plugins = root.path().join("plugins");

    copy_dir(&examples.join("counter"), &plugins.join("counter"));

    // Hello with a secret setting added to its manifest.
    copy_dir(&examples.join("hello"), &plugins.join("hello"));
    let manifest_path = plugins.join("hello/plugin.json");
    let mut manifest: Value =
        serde_json::from_str(&fs::read_to_string(&manifest_path).unwrap()).unwrap();
    manifest["config"].as_array_mut().unwrap().push(json!({
        "key": "token", "type": "string", "secret": true, "description": "An API token. Stored locally, never shown."
    }));
    fs::write(
        &manifest_path,
        serde_json::to_string_pretty(&manifest).unwrap(),
    )
    .unwrap();

    // A plugin whose module asks for something the host never grants: it
    // fails to load and shows up with status "error".
    copy_dir(&examples.join("c-ping"), &plugins.join("broken"));
    let module = wat::parse_str(r#"(module (import "wasi_snapshot_preview1" "fd_write" (func (param i32 i32 i32 i32) (result i32))))"#).unwrap();
    fs::write(plugins.join("broken/ping.wasm"), module).unwrap();
    let broken_path = plugins.join("broken/plugin.json");
    let mut broken: Value =
        serde_json::from_str(&fs::read_to_string(&broken_path).unwrap()).unwrap();
    broken["id"] = json!("example.broken");
    fs::write(&broken_path, serde_json::to_string_pretty(&broken).unwrap()).unwrap();

    // A folder with a manifest the host refuses.
    fs::create_dir_all(plugins.join("junk")).unwrap();
    fs::write(plugins.join("junk/plugin.json"), b"{ \"id\": 7 }").unwrap();

    // Home Assistant, set up, and a second copy that is not.
    copy_dir(
        &examples.join("home-assistant"),
        &plugins.join("home-assistant"),
    );
    copy_dir(
        &examples.join("home-assistant"),
        &plugins.join("home-assistant-unset"),
    );
    let unset_path = plugins.join("home-assistant-unset/plugin.json");
    let mut unset: Value = serde_json::from_str(&fs::read_to_string(&unset_path).unwrap()).unwrap();
    unset["id"] = json!("example.home-assistant-unset");
    unset["name"] = json!("Home Assistant (not set up)");
    fs::write(&unset_path, serde_json::to_string_pretty(&unset).unwrap()).unwrap();

    // What an earlier version left behind when the system had no credential
    // store: a plain file. It is listed, by name only, and not used.
    fs::write(
        root.path().join("secrets.json"),
        r#"{"schemaVersion":1,"secrets":{"example.home-assistant-unset":{"token":"a-token-from-the-plain-file-era"}}}"#,
    )
    .unwrap();

    // An earlier version kept Hello's secret setting in plugins.json, in plain
    // text. It is reported by name, never used, and stays until the person acts.
    fs::write(
        root.path().join("plugins.json"),
        r#"{"schemaVersion":1,"plugins":{"example.hello":{"enabled":true,"config":{"token":"super-secret-value"}}}}"#,
    )
    .unwrap();

    let host = PluginHost::new(
        vec![plugins.clone()],
        Some(root.path().join("plugins.json")),
        PluginRegistry::default(),
        Limits::default(),
    );
    host.scan();
    host.set_config("example.hello", "greeting", json!("Hola"))
        .unwrap();

    // The first address was refused; a second one was typed, granted, and given its token.
    let ha = "example.home-assistant";
    host.set_permission(
        ha,
        "http://homeassistant.local:8123",
        Some(Decision::Denied),
    )
    .unwrap();
    host.set_config(ha, "baseUrl", json!("http://192.168.1.20:8123"))
        .unwrap();
    host.set_permission(ha, "http://192.168.1.20:8123", Some(Decision::Granted))
        .unwrap();
    host.set_secret(ha, "token", "a-long-lived-access-token")
        .unwrap();
    // The copy has an address that is not one, so its refresh is refused before
    // anything touches the network.
    let unset = "example.home-assistant-unset";
    host.set_config(unset, "baseUrl", json!("homeassistant"))
        .unwrap();
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let ctx = PluginContext::new(unset, EventBus::new(), runtime.handle().clone());
    let _ = host.refresh(
        unset,
        &ctx,
        &[StateAddress::new(unset, "entity").with("entity", "light.kitchen")],
    );

    let mut out = serde_json::to_value(host.overview()).unwrap();
    // Keep the fixture free of machine paths.
    let prefix = format!("{}/", root.path().display());
    let text = serde_json::to_string(&out)
        .unwrap()
        .replace(&prefix, "<data>/");
    out = serde_json::from_str(&text).unwrap();
    out
}
