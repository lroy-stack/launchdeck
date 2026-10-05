//! What the Plugins screen is told about the example plugins the app ships,
//! from the real examples laid out as `bundle.resources` in `tauri.conf.json`
//! places them, with Counter installed and the others not. Shared by the
//! `dump_plugin_examples` example and the contract test, so the TypeScript
//! schema is checked against what Rust emits.

use std::fs;
use std::path::Path;

use launchdeck_lib::plugins::external::{plugin_roots, Limits, PluginHost};
use launchdeck_lib::plugins::PluginRegistry;
use serde_json::Value;

pub fn capture() -> Value {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let conf: Value =
        serde_json::from_str(&fs::read_to_string(manifest_dir.join("tauri.conf.json")).unwrap())
            .unwrap();
    let tmp = tempfile::tempdir().unwrap();
    let resources = tmp.path().join("Resources");
    for (source, place) in conf["bundle"]["resources"].as_object().unwrap() {
        let to = resources.join(place.as_str().unwrap());
        fs::create_dir_all(to.parent().unwrap()).unwrap();
        fs::copy(manifest_dir.join(source), to).unwrap();
    }
    let examples = resources.join("examples/plugins");

    let data = tmp.path().join("data");
    let host = PluginHost::new(
        plugin_roots(&data, None),
        Some(data.join("plugins.json")),
        PluginRegistry::default(),
        Limits::default(),
    );
    host.scan();
    host.install_example(&examples, "example.counter").unwrap();
    serde_json::to_value(host.examples(&examples)).unwrap()
}
