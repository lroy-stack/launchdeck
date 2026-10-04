//! Shared helpers for the unit tests in this module.

include!("../../../tests/support/wat_plugins.rs");

use std::path::PathBuf;
use std::sync::Arc;

use serde_json::{json, Map, Value};

use super::log::PluginLog;
use super::manifest::ExternalManifest;
use super::sandbox::Limits;
use super::wasm_plugin::WasmPlugin;
use crate::events::EventBus;
use crate::plugins::PluginContext;

/// A manifest with the given actions (ids are single letters in the kit) and events.
pub fn manifest_json(id: &str, actions: &[&str], events: &[&str]) -> Value {
    json!({
        "schemaVersion": 1, "id": id, "name": "Test", "version": "1.0.0", "author": "tests",
        "abi": 1, "main": "m.wasm",
        "actions": actions.iter().map(|a| json!({"id": a, "args": [{"name":"s","type":"string"}]})).collect::<Vec<_>>(),
        "events": events.iter().map(|e| json!({"type": e})).collect::<Vec<_>>(),
    })
}

pub fn load(wasm: Vec<u8>, manifest: Value, limits: Limits) -> Arc<WasmPlugin> {
    let m = ExternalManifest::parse(&manifest.to_string()).expect("test manifest");
    let id = m.id.clone();
    let plugin = WasmPlugin::new(
        m,
        PathBuf::from("."),
        0,
        Arc::new(PluginLog::new(&id)),
        limits,
    );
    plugin.activate(&wasm, &Map::new());
    plugin
}

/// Like [`load`], reporting to `hub`, with the plugin's states declared there
/// first (what the host does at install).
pub fn load_with_hub(
    wasm: Vec<u8>,
    manifest: Value,
    limits: Limits,
    hub: &crate::state::StateHub,
) -> Arc<WasmPlugin> {
    use crate::plugins::Plugin;
    let m = ExternalManifest::parse(&manifest.to_string()).expect("test manifest");
    let id = m.id.clone();
    let plugin = WasmPlugin::with_hub(
        m,
        PathBuf::from("."),
        0,
        Arc::new(PluginLog::new(&id)),
        limits,
        hub.clone(),
    );
    hub.declare(&id, plugin.states()).expect("states declare");
    plugin.activate(&wasm, &Map::new());
    plugin
}

pub fn ctx(id: &str, bus: &EventBus) -> PluginContext {
    PluginContext::new(id, bus.clone(), tokio::runtime::Handle::current())
}

/// Limits small enough that runaway tests finish quickly.
pub fn quick() -> Limits {
    Limits {
        fuel: 5_000_000,
        timeout: std::time::Duration::from_millis(400),
        memory_bytes: 2 * 1024 * 1024,
    }
}
