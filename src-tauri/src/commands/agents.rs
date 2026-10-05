//! Agent Control. Also thin: `AgentControl` owns the state and the rules.

use tauri::State;

use super::CmdResult;
use crate::agents::adapters::HookSetup;
use crate::agents::model::{AccessLevel, Command};
use crate::agents::service::Target;
use crate::agents::settings::SettingsPatch;
use crate::AppState;

/// Which track a desktop command is aimed at.
#[derive(serde::Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum TargetArg {
    Focus,
    Session { id: String },
    Agent { id: String },
}

impl From<TargetArg> for Target {
    fn from(t: TargetArg) -> Self {
        match t {
            TargetArg::Focus => Target::Focus,
            TargetArg::Session { id } => Target::Session(id),
            TargetArg::Agent { id } => Target::Agent(id),
        }
    }
}

/// Returns the request id. The outcome arrives in the next agents snapshot.
#[tauri::command]
pub(crate) fn agents_command(
    state: State<'_, AppState>,
    target: TargetArg,
    command: Command,
) -> CmdResult<String> {
    state
        .runtime
        .agents()
        .command(target.into(), command)
        .map_err(|e| e.to_string())
}

/// Select a slot on the Agent Control surface (`null` clears the selection).
#[tauri::command]
pub(crate) fn agents_focus(state: State<'_, AppState>, slot: Option<usize>) -> CmdResult<()> {
    match slot {
        Some(slot) => state.runtime.agents().focus_slot(slot),
        None => {
            state.runtime.agents().clear_focus();
            Ok(())
        }
    }
}

/// Remove one finished session, or every finished session when `sessionId` is null.
#[tauri::command]
pub(crate) fn agents_dismiss(
    state: State<'_, AppState>,
    session_id: Option<String>,
) -> CmdResult<usize> {
    match session_id {
        Some(id) => state.runtime.agents().dismiss_session(&id).map(|()| 1),
        None => Ok(state.runtime.agents().dismiss_finished()),
    }
}

#[tauri::command]
pub(crate) fn agents_set_access(
    state: State<'_, AppState>,
    agent_id: String,
    access: AccessLevel,
) -> CmdResult<()> {
    state.runtime.agents().set_access(&agent_id, access)
}

#[tauri::command]
pub(crate) fn agents_demo(state: State<'_, AppState>, run: bool) -> CmdResult<()> {
    if run {
        state.runtime.demo_adapter().run_demo()
    } else {
        state.runtime.demo_adapter().reset_demo();
        Ok(())
    }
}

#[tauri::command]
pub(crate) async fn agents_update_settings(
    state: State<'_, AppState>,
    patch: SettingsPatch,
) -> CmdResult<()> {
    state.runtime.update_agent_settings(patch).await
}

/// Contains the hook token, so it is only returned when the user asks for it.
#[tauri::command]
pub(crate) fn agents_hook_setup(state: State<'_, AppState>) -> HookSetup {
    state.runtime.hooks().setup()
}

#[tauri::command]
pub(crate) async fn agents_rotate_hook_token(state: State<'_, AppState>) -> CmdResult<()> {
    state.runtime.hooks().rotate_token().await.map(|_| ())
}
