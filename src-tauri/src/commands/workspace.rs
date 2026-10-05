//! The workspace as the UI sees and edits it: the snapshot, the virtual
//! Launchpad X (its frame and its pads), Live/Edit, activation, editing
//! (controls, surfaces, pages), reset and preferences.

use tauri::State;

use super::CmdResult;
use crate::core::model::{Control, ControlPos, Preferences};
use crate::core::runtime::{Mode, Snapshot};
use crate::devices::SurfaceFrame;
use crate::AppState;

#[tauri::command]
pub(crate) fn get_snapshot(state: State<'_, AppState>) -> Snapshot {
    state.runtime.snapshot()
}

#[tauri::command]
pub(crate) fn virtual_frame(state: State<'_, AppState>) -> SurfaceFrame {
    state.runtime.virtual_handle().frame()
}

#[tauri::command]
pub(crate) fn set_mode(state: State<'_, AppState>, mode: Mode) {
    state.runtime.set_mode(mode);
}

#[tauri::command]
pub(crate) fn activate_surface(
    state: State<'_, AppState>,
    surface_id: String,
    page_id: Option<String>,
) -> CmdResult<()> {
    state
        .runtime
        .activate_surface(&surface_id, page_id.as_deref())
}

#[tauri::command]
pub(crate) fn activate_page(state: State<'_, AppState>, page_id: String) -> CmdResult<()> {
    state.runtime.activate_page(&page_id)
}

/// Press or release a pad on the virtual Launchpad X. This goes through the
/// same input path as hardware.
#[tauri::command]
pub(crate) fn virtual_input(
    state: State<'_, AppState>,
    pos: ControlPos,
    pressed: bool,
) -> CmdResult<()> {
    let handle = state.runtime.virtual_handle();
    let result = if pressed {
        handle.press(pos, 100)
    } else {
        handle.release(pos)
    };
    result.map_err(|e| e.to_string())
}

#[tauri::command]
pub(crate) fn update_control(
    state: State<'_, AppState>,
    surface_id: String,
    page_id: String,
    control: Control,
) -> CmdResult<()> {
    state.runtime.update_control(&surface_id, &page_id, control)
}

#[tauri::command]
pub(crate) fn remove_control(
    state: State<'_, AppState>,
    surface_id: String,
    page_id: String,
    control_id: String,
) -> CmdResult<()> {
    state
        .runtime
        .remove_control(&surface_id, &page_id, &control_id)
}

#[tauri::command]
pub(crate) fn create_surface(state: State<'_, AppState>, name: String) -> CmdResult<String> {
    state.runtime.create_surface(&name)
}

#[tauri::command]
pub(crate) fn add_page(
    state: State<'_, AppState>,
    surface_id: String,
    name: String,
) -> CmdResult<String> {
    state.runtime.add_page(&surface_id, &name)
}

/// The commands that rename or delete return nothing: the UI follows the
/// workspace event.
#[tauri::command]
pub(crate) fn remove_surface(state: State<'_, AppState>, surface_id: String) -> CmdResult<()> {
    state.runtime.remove_surface(&surface_id)
}

#[tauri::command]
pub(crate) fn update_surface(
    state: State<'_, AppState>,
    surface_id: String,
    name: String,
    description: String,
    icon: Option<String>,
) -> CmdResult<()> {
    state
        .runtime
        .update_surface(&surface_id, &name, &description, icon)
}

#[tauri::command]
pub(crate) fn rename_page(
    state: State<'_, AppState>,
    surface_id: String,
    page_id: String,
    name: String,
) -> CmdResult<()> {
    state.runtime.rename_page(&surface_id, &page_id, &name)
}

#[tauri::command]
pub(crate) fn remove_page(
    state: State<'_, AppState>,
    surface_id: String,
    page_id: String,
) -> CmdResult<()> {
    state.runtime.remove_page(&surface_id, &page_id)
}

/// Only the onboarding preference changes; the UI follows the workspace event.
#[tauri::command]
pub(crate) fn set_onboarding_done(state: State<'_, AppState>, done: bool) -> CmdResult<()> {
    state.runtime.set_onboarding_done(done)
}

#[tauri::command]
pub(crate) fn reset_workspace(state: State<'_, AppState>) {
    state.runtime.reset_workspace();
}

#[tauri::command]
pub(crate) fn update_preferences(state: State<'_, AppState>, preferences: Preferences) {
    state.runtime.update_preferences(preferences);
}
