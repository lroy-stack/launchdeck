//! Hardware devices: connect, disconnect, rescan, and what a device can be
//! given beyond frames (brightness, sleep, scrolling text).

use tauri::State;

use super::{CmdResult, CommandError};
use crate::devices::TextScroll;
use crate::AppState;

#[tauri::command]
pub(crate) fn connect_device(state: State<'_, AppState>, id: String) -> CmdResult<()> {
    Ok(state.runtime.connect_device(&id)?)
}

#[tauri::command]
pub(crate) fn disconnect_device(state: State<'_, AppState>, id: String) -> CmdResult<()> {
    Ok(state.runtime.disconnect_device(&id)?)
}

#[tauri::command]
pub(crate) async fn rescan_devices(state: State<'_, AppState>) -> CmdResult<()> {
    let rt = state.runtime.clone();
    tauri::async_runtime::spawn_blocking(move || rt.look_again())
        .await
        .map_err(|e| CommandError::from(e.to_string()))
}

/// The LED brightness in percent for every device that has it, or each
/// device's own level (`None`). The UI uses this, never
/// `update_preferences`, to change it.
#[tauri::command]
pub(crate) fn set_device_brightness(
    state: State<'_, AppState>,
    percent: Option<u8>,
) -> CmdResult<()> {
    Ok(state.runtime.set_device_brightness(percent)?)
}

#[tauri::command]
pub(crate) fn set_device_sleep(
    state: State<'_, AppState>,
    id: String,
    asleep: bool,
) -> CmdResult<()> {
    Ok(state.runtime.set_device_sleep(&id, asleep)?)
}

/// Scrolls text across a device, or stops the scroll (`None`).
#[tauri::command]
pub(crate) fn scroll_device_text(
    state: State<'_, AppState>,
    id: String,
    text: Option<TextScroll>,
) -> CmdResult<()> {
    Ok(state.runtime.scroll_device_text(&id, text)?)
}
