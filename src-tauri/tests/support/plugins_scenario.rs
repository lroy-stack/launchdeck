//! A real Plugins screen payload, built from the real example plugins: one
//! healthy, one with a secret setting, one that failed to load, one folder
//! that is not a plugin. Shared by the `dump_plugins` example and the contract
//! test, so the TypeScript schemas are checked against what Rust emits.

use std::fs;
use std::path::Path;

use launchdeck_lib::plugins::external::{Limits, PluginHost};
use launchdeck_lib::plugins::PluginRegistry;
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

    let host = PluginHost::new(
        vec![plugins.clone()],
        Some(root.path().join("plugins.json")),
        PluginRegistry::default(),
        Limits::default(),
    );
    host.scan();
    host.set_config("example.hello", "token", json!("super-secret-value"))
        .unwrap();
    host.set_config("example.hello", "greeting", json!("Hola"))
        .unwrap();

    let mut out = serde_json::to_value(host.overview()).unwrap();
    // Keep the fixture free of machine paths.
    let prefix = format!("{}/", root.path().display());
    let text = serde_json::to_string(&out)
        .unwrap()
        .replace(&prefix, "<data>/");
    out = serde_json::from_str(&text).unwrap();
    out
}
