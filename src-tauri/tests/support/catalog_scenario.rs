//! A real Action Library catalog, built from the real runtime: the built-in
//! providers, Agent Control, and the example plugins. One plugin is switched
//! off and one fails to load, so the fixture covers every provider status and
//! both reasons an action can be unavailable. Shared by the `dump_catalog`
//! example and the contract test, so the TypeScript schemas are checked
//! against what Rust emits.

use std::fs;
use std::path::Path;
use std::sync::Arc;

use launchdeck_lib::core::runtime::{NullEmitter, Runtime, RuntimeConfig};
use launchdeck_lib::plugins::keyboard::{Combo, Keyboard};
use launchdeck_lib::plugins::ActionError;
use launchdeck_lib::surfaces::default_workspace;
use serde_json::{json, Value};

struct NoKeyboard;
impl Keyboard for NoKeyboard {
    fn press(&self, _combo: &Combo) -> Result<(), ActionError> {
        Ok(())
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

pub async fn capture() -> Value {
    let examples = Path::new(env!("CARGO_MANIFEST_DIR")).join("../examples/plugins");
    let root = tempfile::tempdir().unwrap();
    let plugins = root.path().join("plugins");
    for name in ["hello", "counter", "c-ping"] {
        copy_dir(&examples.join(name), &plugins.join(name));
    }
    // A plugin whose module asks for something the host never grants: it fails
    // to load, and its actions stay listed as unavailable.
    copy_dir(&examples.join("c-ping"), &plugins.join("broken"));
    let module = wat::parse_str(r#"(module (import "wasi_snapshot_preview1" "fd_write" (func (param i32 i32 i32 i32) (result i32))))"#).unwrap();
    fs::write(plugins.join("broken/ping.wasm"), module).unwrap();
    let broken_path = plugins.join("broken/plugin.json");
    let mut broken: Value =
        serde_json::from_str(&fs::read_to_string(&broken_path).unwrap()).unwrap();
    broken["id"] = json!("example.broken");
    fs::write(&broken_path, serde_json::to_string_pretty(&broken).unwrap()).unwrap();

    let rt = Runtime::start(RuntimeConfig {
        workspace: default_workspace(),
        store: None,
        emitter: Arc::new(NullEmitter),
        keyboard: Arc::new(NoKeyboard),
        scan_hardware: false,
        serve_external: false,
        demo_pace: 1.0,
        plugin_dirs: vec![plugins],
    })
    .await;
    rt.plugin_host()
        .set_enabled("example.counter", false)
        .unwrap();

    let out = serde_json::to_value(rt.catalog()).unwrap();
    // Keep the fixture free of machine paths.
    let prefix = format!("{}/", root.path().display());
    let text = serde_json::to_string_pretty(&out)
        .unwrap()
        .replace(&prefix, "<data>/");
    serde_json::from_str(&text).unwrap()
}
