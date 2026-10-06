//! A control's feedback as Rust judges it: why it looks as it does now, and
//! what a draft would come to.

use tauri::State;

use super::{about, CmdResult};
use crate::core::explain::ControlExplanation;
use crate::core::model::Control;
use crate::core::review::FeedbackReview;
use crate::AppState;

/// Why a control looks the way it does right now, with the rules, what each
/// reads, which one decided and why a state may not answer.
#[tauri::command]
pub(crate) fn explain_control(
    state: State<'_, AppState>,
    surface_id: String,
    page_id: String,
    control_id: String,
) -> CmdResult<ControlExplanation> {
    state
        .runtime
        .explain_control(&surface_id, &page_id, &control_id)
        .map_err(about(&[
            ("surface", &surface_id),
            ("page", &page_id),
            ("control", &control_id),
        ]))
}

/// What a draft of a control's feedback comes to, without saving it: the
/// issues saving would refuse (with the field each is about), what Rust can
/// prove about the rules, the limits, and the draft's explanation. Read-only.
#[tauri::command]
pub(crate) fn review_feedback(
    state: State<'_, AppState>,
    surface_id: String,
    page_id: String,
    control: Control,
) -> CmdResult<FeedbackReview> {
    state
        .runtime
        .review_feedback(&surface_id, &page_id, &control)
        .map_err(about(&[
            ("surface", &surface_id),
            ("page", &page_id),
            ("control", &control.id),
        ]))
}
