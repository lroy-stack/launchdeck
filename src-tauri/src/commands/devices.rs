//! Hardware devices: connect, disconnect, rescan.

use tauri::State;

use super::CmdResult;
use crate::AppState;

#[tauri::command]
pub(crate) fn connect_device(state: State<'_, AppState>, id: String) -> CmdResult<()> {
    state.runtime.connect_device(&id)
}

#[tauri::command]
pub(crate) fn disconnect_device(state: State<'_, AppState>, id: String) -> CmdResult<()> {
    state.runtime.disconnect_device(&id)
}

#[tauri::command]
pub(crate) async fn rescan_devices(state: State<'_, AppState>) -> CmdResult<()> {
    let rt = state.runtime.clone();
    tauri::async_runtime::spawn_blocking(move || rt.rescan())
        .await
        .map_err(|e| e.to_string())
}
