//! Actions and what controls observe: running and checking an assignment, the
//! Action Library's catalog, the Observable Catalog, reading one state, and a
//! test event.

use serde_json::Value;
use tauri::State;

use super::{CmdResult, CommandError};
use crate::actions::definition::{Assignment, Catalog, Issue};
use crate::events::external::{parse_inbound, Parsed};
use crate::state::catalog::ObservableCatalog;
use crate::state::Observed;
use crate::AppState;

/// Run one assignment immediately (the inspector's "Test" button) and report
/// how it went. Shell commands only ever come from controls the user
/// configured in this UI.
#[tauri::command]
pub(crate) async fn run_action(state: State<'_, AppState>, action: Assignment) -> CmdResult<Value> {
    let runtime = state.runtime.clone();
    runtime.run_action(action).await.map_err(CommandError::from)
}

/// The Action Library's catalog: every provider and every action, with
/// availability evaluated now. React searches it locally.
#[tauri::command]
pub(crate) fn actions_catalog(state: State<'_, AppState>) -> Catalog {
    state.runtime.catalog()
}

/// Everything a control can observe, states and events, with what each state
/// reads now. Updates arrive as `observables-changed`; this is the whole list.
#[tauri::command]
pub(crate) fn observables_catalog(state: State<'_, AppState>) -> ObservableCatalog {
    state.runtime.observables()
}

/// What one state reads now: `known`, `unknown` or `unavailable`. The address
/// is checked against the state's definition, so a wrong argument is an error.
#[tauri::command]
pub(crate) fn observable_state(
    state: State<'_, AppState>,
    provider: String,
    id: String,
    args: Option<Value>,
) -> CmdResult<Observed> {
    state
        .runtime
        .read_state(&provider, &id, &args.unwrap_or(Value::Null))
        .map_err(CommandError::from)
}

#[derive(serde::Serialize)]
pub(crate) struct ValidationOutcome {
    ok: bool,
    /// The assignment as it would be saved: defaults applied.
    #[serde(skip_serializing_if = "Option::is_none")]
    assignment: Option<Assignment>,
    issues: Vec<Issue>,
}

/// Check an assignment the way saving does, without saving.
#[tauri::command]
pub(crate) fn actions_validate(
    state: State<'_, AppState>,
    assignment: Assignment,
) -> ValidationOutcome {
    match state.runtime.validate_assignment(&assignment) {
        Ok(a) => ValidationOutcome {
            ok: true,
            assignment: Some(a),
            issues: vec![],
        },
        Err(issues) => ValidationOutcome {
            ok: false,
            assignment: None,
            issues,
        },
    }
}

/// Send a test event through the same validation as the WebSocket interface.
#[tauri::command]
pub(crate) fn send_event(
    state: State<'_, AppState>,
    source: String,
    event: String,
    payload: Option<Value>,
) -> CmdResult<()> {
    let text = serde_json::json!({ "type": "event", "source": source, "event": event, "payload": payload }).to_string();
    match parse_inbound(&text)? {
        Parsed::Event(e) => {
            state.runtime.publish_external(e);
            Ok(())
        }
        Parsed::Ping => Ok(()),
    }
}
