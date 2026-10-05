//! External plugins (WebAssembly). Thin: `PluginHost` owns the lifecycle. Loading
//! compiles a module, so the mutating commands run off the UI thread.

use serde_json::Value;
use tauri::State;

use super::CmdResult;
use crate::plugins::external::capabilities::Decision;
use crate::plugins::external::{
    LegacyMigration, LogEntry, PluginInfo, PluginsOverview, ScanReport,
};
use crate::AppState;

#[tauri::command]
pub(crate) fn plugins_list(state: State<'_, AppState>) -> PluginsOverview {
    state.runtime.plugin_host().overview()
}

#[tauri::command]
pub(crate) async fn plugins_set_enabled(
    state: State<'_, AppState>,
    id: String,
    enabled: bool,
) -> CmdResult<PluginInfo> {
    let host = state.runtime.plugin_host().clone();
    tauri::async_runtime::spawn_blocking(move || host.set_enabled(&id, enabled))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub(crate) async fn plugins_set_config(
    state: State<'_, AppState>,
    id: String,
    key: String,
    value: Value,
) -> CmdResult<PluginInfo> {
    let host = state.runtime.plugin_host().clone();
    tauri::async_runtime::spawn_blocking(move || host.set_config(&id, &key, value))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub(crate) async fn plugins_reload(
    state: State<'_, AppState>,
    id: String,
) -> CmdResult<PluginInfo> {
    let host = state.runtime.plugin_host().clone();
    tauri::async_runtime::spawn_blocking(move || host.reload(&id))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub(crate) async fn plugins_rescan(state: State<'_, AppState>) -> CmdResult<ScanReport> {
    let host = state.runtime.plugin_host().clone();
    tauri::async_runtime::spawn_blocking(move || host.scan())
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub(crate) fn plugins_logs(state: State<'_, AppState>, id: String) -> CmdResult<Vec<LogEntry>> {
    state
        .runtime
        .plugin_host()
        .logs(&id)
        .map_err(|e| e.to_string())
}

/// Grant (`"granted"`), deny (`"denied"`) or take back (`null`) one origin a
/// plugin asks for. Only the person does this: no plugin, event or remote
/// interface can reach this command.
#[tauri::command]
pub(crate) async fn plugins_set_permission(
    state: State<'_, AppState>,
    id: String,
    origin: String,
    decision: Option<Decision>,
) -> CmdResult<PluginInfo> {
    let host = state.runtime.plugin_host().clone();
    tauri::async_runtime::spawn_blocking(move || host.set_permission(&id, &origin, decision))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())
}

/// Store a secret. The value goes to the credential store and is never returned.
#[tauri::command]
pub(crate) async fn plugins_set_secret(
    state: State<'_, AppState>,
    id: String,
    key: String,
    value: String,
) -> CmdResult<PluginInfo> {
    let host = state.runtime.plugin_host().clone();
    tauri::async_runtime::spawn_blocking(move || host.set_secret(&id, &key, &value))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub(crate) async fn plugins_clear_secret(
    state: State<'_, AppState>,
    id: String,
    key: String,
) -> CmdResult<PluginInfo> {
    let host = state.runtime.plugin_host().clone();
    tauri::async_runtime::spawn_blocking(move || host.clear_secret(&id, &key))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())
}

/// Move the secrets an earlier version kept in plain text into the credential store.
#[tauri::command]
pub(crate) async fn plugins_migrate_legacy_secrets(
    state: State<'_, AppState>,
) -> CmdResult<LegacyMigration> {
    let host = state.runtime.plugin_host().clone();
    tauri::async_runtime::spawn_blocking(move || host.migrate_legacy_secrets())
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())
}

/// Delete the plain secrets without moving them. Returns how many there were.
#[tauri::command]
pub(crate) async fn plugins_discard_legacy_secrets(state: State<'_, AppState>) -> CmdResult<usize> {
    let host = state.runtime.plugin_host().clone();
    tauri::async_runtime::spawn_blocking(move || host.discard_legacy_secrets())
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())
}

/// Move one plugin's plain-text secrets into the credential store: each is
/// written, read back and compared, and only then removed from `plugins.json`.
/// If anything fails nothing is removed, and the error says why.
#[tauri::command]
pub(crate) async fn plugins_migrate_plugin_legacy_secrets(
    state: State<'_, AppState>,
    id: String,
) -> CmdResult<PluginInfo> {
    let host = state.runtime.plugin_host().clone();
    tauri::async_runtime::spawn_blocking(move || host.migrate_plugin_legacy_secrets(&id))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())
}

/// Delete one plugin's plain-text secrets from `plugins.json` without moving them.
#[tauri::command]
pub(crate) async fn plugins_remove_plugin_legacy_secrets(
    state: State<'_, AppState>,
    id: String,
) -> CmdResult<PluginInfo> {
    let host = state.runtime.plugin_host().clone();
    tauri::async_runtime::spawn_blocking(move || host.remove_plugin_legacy_secrets(&id))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())
}
