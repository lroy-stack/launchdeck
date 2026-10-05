//! What stops an import until the person decides or answers (plan 7.12), in
//! a fixed order: the resolution, edits that would be overwritten, the pads
//! that run things, each input, the size of the workspace.

use super::decisions::ImportDecisions;
use super::instances::Target;
use super::types::{
    Blocker, BlockerCode, PlanEffects, PlannedInput, PlannedSurface, SurfaceAction,
};
use crate::artifacts::format::{echo, AppRefV1, InputDef};
use crate::artifacts::inspect::ParsedArtifact;
use crate::artifacts::limits::MAX_WORKSPACE_SURFACES;
use crate::core::model::Workspace;

/// What the blockers are computed from.
pub(super) struct Stops<'a> {
    pub target: &'a Target<'a>,
    pub surfaces: &'a [PlannedSurface],
    pub decisions: &'a ImportDecisions,
    pub runs_things: bool,
    pub inputs: &'a [PlannedInput],
    pub parsed: &'a ParsedArtifact,
    pub workspace: &'a Workspace,
    pub effects: &'a PlanEffects,
}

fn blocker(code: BlockerCode, message: String, input: Option<&str>) -> Blocker {
    Blocker {
        code,
        message,
        input: input.map(str::to_string),
    }
}

/// In a fixed order: the resolution, edits that would be overwritten, the
/// pads that run things, each input, the size of the workspace.
pub(super) fn blockers(s: &Stops<'_>) -> Vec<Blocker> {
    let mut out = Vec::new();
    if let Some(why) = &s.target.unresolved {
        out.push(blocker(BlockerCode::ResolutionNeeded, why.clone(), None));
    }
    out.extend(overwrite_blocker(s.surfaces, s.decisions));
    if s.runs_things && s.decisions.runs.is_none() {
        out.push(blocker(
            BlockerCode::RunsDecisionNeeded,
            "Some pads run a command, send a request or start a program: choose whether to keep \
             them or remove them."
                .into(),
            None,
        ));
    }
    for input in s.inputs {
        if let Some(b) = input_blocker(input, s.parsed, &s.decisions.context_rules) {
            out.push(b);
        }
    }
    let total = s.workspace.surfaces.len() + s.effects.add_surfaces.len();
    if total > MAX_WORKSPACE_SURFACES {
        out.push(blocker(
            BlockerCode::WorkspaceFull,
            format!(
                "A workspace holds at most {MAX_WORKSPACE_SURFACES} surfaces, and this import \
                 would make {total}. Remove some surfaces first."
            ),
            None,
        ));
    }
    out
}

/// An input stops the import when it is needed and has no answer, or its
/// answer is wrong. It is needed when a pad that stays or a selected
/// context rule takes it.
fn input_blocker(
    input: &PlannedInput,
    parsed: &ParsedArtifact,
    selected: &[usize],
) -> Option<Blocker> {
    let def: &InputDef = &input.def;
    let label = echo(&def.label);
    let taken_by_rule = parsed
        .content
        .context_rules
        .iter()
        .enumerate()
        .any(|(i, r)| {
            selected.contains(&i)
                && matches!(&r.application, AppRefV1::Input(x) if x.input == def.id)
        });
    let needed = def.required && (!input.used_by.is_empty() || taken_by_rule);
    let first = input.issues.first();
    match (&input.value, first) {
        (None, _) if needed || first.is_some() => Some(blocker(
            BlockerCode::InputRequired,
            match first {
                Some(issue) => format!("\"{label}\" needs an answer: {issue}"),
                None => format!("\"{label}\" needs an answer."),
            },
            Some(&def.id),
        )),
        (Some(_), Some(issue)) => Some(blocker(
            BlockerCode::InputInvalid,
            format!("\"{label}\": {issue}"),
            Some(&def.id),
        )),
        _ => None,
    }
}

/// Surfaces the import would replace that were changed here since they
/// were imported, unless the person confirmed.
fn overwrite_blocker(surfaces: &[PlannedSurface], decisions: &ImportDecisions) -> Option<Blocker> {
    let edited: Vec<String> = surfaces
        .iter()
        .filter(|p| {
            matches!(
                p.action,
                SurfaceAction::Replace {
                    locally_edited: true,
                    ..
                }
            )
        })
        .map(|p| format!("\"{}\"", echo(&p.name)))
        .collect();
    (!edited.is_empty() && !decisions.confirm_overwrite_edits).then(|| {
        blocker(
            BlockerCode::OverwriteEditsNotConfirmed,
            format!(
                "Confirm to replace the surfaces changed here since they were imported with the \
                 file's version: {}.",
                edited.join(", ")
            ),
            None,
        )
    })
}
