//! The app itself, apart from any one feature: its data folder.

use tauri::{AppHandle, Manager};

use super::CmdResult;

/// Show the app data folder (the workspace, its backups, plugins and
/// settings) in the system's file manager.
#[tauri::command]
pub(crate) fn app_reveal_data_folder(app: AppHandle) -> CmdResult<()> {
    let folder = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("LaunchDeck could not find its data folder ({e})."))?;
    open::that_detached(&folder)
        .map_err(|e| format!("LaunchDeck could not open {} ({e}).", folder.display()))
}
