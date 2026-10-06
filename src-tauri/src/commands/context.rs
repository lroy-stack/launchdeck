//! Auto Context: its status, its settings and resuming it after a manual override.

use tauri::State;

use super::CmdResult;
use crate::context::{ContextSettings, ContextStatus};
use crate::AppState;

/// Auto Context as it is now: settings, observer, application in front, decision.
#[tauri::command]
pub(crate) fn context_status(state: State<'_, AppState>) -> ContextStatus {
    state.runtime.context()
}

/// Replace the context settings: Auto on or off, the rules, the fallback.
#[tauri::command]
pub(crate) fn context_update(
    state: State<'_, AppState>,
    settings: ContextSettings,
) -> CmdResult<ContextStatus> {
    Ok(state.runtime.update_context(settings)?)
}

/// Resume Auto: end a manual override.
#[tauri::command]
pub(crate) fn context_resume(state: State<'_, AppState>) -> ContextStatus {
    state.runtime.resume_context()
}
