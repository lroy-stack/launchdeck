//! Apply (plan 7.17): the workspace an import writes, built from the
//! current one and exactly the effects of its plan, and the checks it must
//! pass before it is saved. Pure: the artifact service runs both inside
//! `Runtime::commit_workspace`, under the runtime's lock, after making the
//! plan again against the workspace as it is then.
#![cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "the artifact service (C5) runs every import through it"
    )
)]

use std::collections::{BTreeMap, HashSet};
use std::fmt;

use serde::Serialize;

use super::format::{echo, plain_message, AssignmentV1, ControlV1, PageV1, SurfaceV1};
use super::inspect::ParsedArtifact;
use super::plan::{
    bound, surface_digest, DependencyCheck, DependencyStatus, ImportPlan, RuleOutcome,
};
use super::review::{Location, Slot};
use crate::actions::definition::Assignment;
use crate::context::rules::{checked, ContextRule};
use crate::core::model::{Control, ImportOrigin, Page, Surface, Workspace};
use crate::core::validate::validate_control;

/// Why an import could not be written. Nothing was changed. The message is
/// a sentence for a person; values from the file are echoed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ApplyError {
    /// The plan does not fit the file it was made from.
    Inconsistent(String),
    /// A control the import would write is one the editor would refuse.
    Control {
        surface: String,
        page: String,
        control: String,
        reason: String,
    },
    /// Two surfaces would have this id.
    DuplicateSurface(String),
    /// The context rules would not pass the rules' own check.
    Rules(String),
}

impl fmt::Display for ApplyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ApplyError::Inconsistent(why) => f.write_str(why),
            ApplyError::Control {
                surface,
                page,
                control,
                reason,
            } => write!(
                f,
                "The pad \"{}\" on the page \"{}\" of \"{}\" cannot be saved: {}.",
                echo(control),
                echo(page),
                echo(surface),
                plain_message(reason)
            ),
            ApplyError::DuplicateSurface(id) => {
                write!(f, "Two surfaces would have the id \"{}\".", echo(id))
            }
            ApplyError::Rules(why) => write!(
                f,
                "The context rules cannot be saved: {}.",
                plain_message(why)
            ),
        }
    }
}

/// What an import did, for the sentence the import view shows after it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportReport {
    /// Local ids of the surfaces added, in the file's order.
    pub surfaces_added: Vec<String>,
    /// Local ids of the surfaces replaced in place.
    pub surfaces_replaced: Vec<String>,
    pub rules_added: usize,
    /// Rules the person selected that were not added (a local rule names
    /// the application, no identity here, no room).
    pub rules_skipped: usize,
    /// Pads emptied because the person chose to remove what runs things.
    pub slots_removed: usize,
    /// Plugins imported pads or looks use that are not installed here.
    pub missing_dependencies: Vec<DependencyCheck>,
    /// Whether Auto Context is on; an import never changes it.
    pub auto_context: bool,
}

/// The workspace `current` becomes when `parsed` is imported as `plan`
/// says: each surface of the file with its local id, its answers, the local
/// ids of the surfaces it names and its origin, added after the others or
/// put in the place of the surface it replaces; the slots the person chose
/// to empty empty; the selected rules after the local ones. Everything else
/// is `current`'s. Only the plan's effects decide what is written.
pub(in crate::artifacts) fn build(
    parsed: &ParsedArtifact,
    plan: &ImportPlan,
    current: &Workspace,
) -> Result<Workspace, ApplyError> {
    let effects = &plan.effects;
    let local_ids: BTreeMap<String, String> = effects
        .add_surfaces
        .iter()
        .map(|s| (&s.source_id, &s.local_id))
        .chain(
            effects
                .replace_surfaces
                .iter()
                .map(|s| (&s.source_id, &s.local_id)),
        )
        .map(|(source, local)| (source.clone(), local.clone()))
        .collect();
    let replaced: HashSet<&str> = effects
        .replace_surfaces
        .iter()
        .map(|s| s.local_id.as_str())
        .collect();
    let mut surfaces = current.surfaces.clone();
    for incoming in &parsed.content.surfaces {
        let local_id = local_ids.get(&incoming.id).ok_or_else(|| {
            ApplyError::Inconsistent(format!(
                "The import plan has no place for the surface \"{}\" of the file; check the \
                 import again.",
                echo(&incoming.id)
            ))
        })?;
        let surface = written(parsed, plan, incoming, local_id, &local_ids)?;
        if !replaced.contains(local_id.as_str()) {
            surfaces.push(surface);
            continue;
        }
        let slot = surfaces
            .iter_mut()
            .find(|s| s.id == *local_id)
            .ok_or_else(|| {
                ApplyError::Inconsistent(format!(
                    "The surface \"{}\" the import would replace is gone; check the import \
                     again.",
                    echo(local_id)
                ))
            })?;
        *slot = surface;
    }
    let mut context = current.context.clone();
    context
        .rules
        .extend(effects.add_rules.iter().map(|r| ContextRule {
            application: r.application.clone(),
            surface_id: r.surface_local_id.clone(),
            page_id: r.page_id.clone(),
        }));
    Ok(Workspace {
        schema_version: current.schema_version,
        surfaces,
        last_active_surface: current.last_active_surface.clone(),
        preferences: current.preferences.clone(),
        context,
    })
}

/// One surface of the file as the import writes it, with where it came from.
fn written(
    parsed: &ParsedArtifact,
    plan: &ImportPlan,
    incoming: &SurfaceV1,
    local_id: &str,
    local_ids: &BTreeMap<String, String>,
) -> Result<Surface, ApplyError> {
    let model = incoming.to_model();
    let category = plan
        .surfaces
        .iter()
        .find(|s| s.source_id == incoming.id)
        .map_or_else(|| model.category.clone(), |s| s.category.clone());
    let pages = incoming
        .pages
        .iter()
        .zip(model.pages)
        .map(|(v1, page)| written_page(parsed, plan, incoming, v1, page, local_ids))
        .collect();
    let surface = Surface {
        id: local_id.to_string(),
        category,
        pages,
        ..model
    };
    let surface_digest = surface_digest(&surface).ok_or_else(|| {
        ApplyError::Inconsistent(format!(
            "The surface \"{}\" cannot be recorded as imported.",
            echo(&incoming.name)
        ))
    })?;
    let m = &parsed.manifest;
    let origin = ImportOrigin {
        format: parsed.kind.format().to_string(),
        artifact_id: m.id.clone(),
        artifact_name: m.name.clone(),
        artifact_version: m.version.clone(),
        instance: plan.effects.instance,
        source_surface_id: incoming.id.clone(),
        content_digest: parsed.content_digest.clone(),
        surface_digest,
    };
    Ok(Surface {
        imported_from: Some(origin),
        ..surface
    })
}

fn written_page(
    parsed: &ParsedArtifact,
    plan: &ImportPlan,
    surface: &SurfaceV1,
    v1: &PageV1,
    page: Page,
    local_ids: &BTreeMap<String, String>,
) -> Page {
    let effects = &plan.effects;
    let slot = |control: &ControlV1, slot: Slot, a: Option<&AssignmentV1>| -> Option<Assignment> {
        let at = Location {
            surface_source_id: surface.id.clone(),
            page_id: v1.id.clone(),
            control_id: control.id.clone(),
            slot,
        };
        let defs = &parsed.manifest.inputs;
        a.filter(|_| !effects.removed_slots.contains(&at))
            .map(|a| bound(a, defs, &effects.input_values, local_ids))
    };
    let controls = v1
        .controls
        .iter()
        .zip(page.controls)
        .map(|(c, control)| Control {
            press: slot(c, Slot::Press, c.press.as_ref()),
            long_press: slot(c, Slot::LongPress, c.long_press.as_ref()),
            ..control
        })
        .collect();
    Page { controls, ..page }
}

/// What the runtime checks before it saves an edit, on everything the
/// import changes: surface ids are unique, every control of a page of a
/// surface it adds or replaces passes `validate_control`, and the context
/// rules pass `context::rules::checked` against the new workspace (rules
/// that were there before as they were).
pub(in crate::artifacts) fn verify(
    candidate: &Workspace,
    current: &Workspace,
) -> Result<(), ApplyError> {
    let mut ids = HashSet::new();
    if let Some(twice) = candidate.surfaces.iter().find(|s| !ids.insert(&s.id)) {
        return Err(ApplyError::DuplicateSurface(twice.id.clone()));
    }
    let touched = candidate
        .surfaces
        .iter()
        .filter(|s| current.surface(&s.id) != Some(*s));
    for surface in touched {
        for page in &surface.pages {
            for control in &page.controls {
                validate_control(page, control).map_err(|reason| ApplyError::Control {
                    surface: surface.name.clone(),
                    page: page.name.clone(),
                    control: control.label.clone(),
                    reason,
                })?;
            }
        }
    }
    checked(&candidate.context, &current.context, candidate)
        .map_err(|e| ApplyError::Rules(e.to_string()))?;
    Ok(())
}

/// What `plan` writes, in counts and ids. `auto_context`: whether Auto
/// Context is on.
pub(in crate::artifacts) fn report(plan: &ImportPlan, auto_context: bool) -> ImportReport {
    let effects = &plan.effects;
    let skipped =
        |o: &RuleOutcome| !matches!(o, RuleOutcome::Add { .. } | RuleOutcome::NotSelected);
    ImportReport {
        surfaces_added: effects
            .add_surfaces
            .iter()
            .map(|s| s.local_id.clone())
            .collect(),
        surfaces_replaced: effects
            .replace_surfaces
            .iter()
            .map(|s| s.local_id.clone())
            .collect(),
        rules_added: effects.add_rules.len(),
        rules_skipped: plan
            .context_rules
            .iter()
            .filter(|r| skipped(&r.outcome))
            .count(),
        slots_removed: effects.removed_slots.len(),
        missing_dependencies: plan
            .dependencies
            .iter()
            .filter(|d| d.status == DependencyStatus::Missing)
            .cloned()
            .collect(),
        auto_context,
    }
}

#[cfg(test)]
#[path = "apply_tests.rs"]
mod tests;
