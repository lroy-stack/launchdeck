//! Each chosen surface as the file carries it, in two passes. The format's
//! conversion (`SurfaceV1::from_model`) leaves out legacy bindings and the
//! context hint; `export_surface` then strips legacy actions and marks and
//! renames references to exported surfaces. Once the input bindings are in
//! (`bindings`), `look_over` reads each assignment as it will be written:
//! what a person should look at before sharing, credentials, and the text
//! arguments to offer as inputs; and each feedback rule, for credentials.

use serde_json::Value;

use super::credentials::{credential_in, credential_named};
use super::preview::{Collected, ExportCode, Parameter, SuggestedType, Suggestion};
use super::select::{Chosen, IdTable};
use crate::actions::definition::Assignment;
use crate::actions::params::{ParamKind, RefTarget};
use crate::artifacts::env::{Environment, KnownAction};
use crate::artifacts::format::{AssignmentV1, ControlV1, SurfaceV1};
use crate::artifacts::json::segment;
use crate::artifacts::review::{classify, Location, ReviewItem, ReviewKind, Slot};

/// The provider under which a schema 2 action that no later version knew is
/// kept (`persistence::legacy`). It runs nothing and is never exported.
const LEGACY_PROVIDER: &str = "legacy";

/// Where an assignment is: in the file, as a pointer, and for a person.
struct Place<'a> {
    at: Location,
    pointer: String,
    label: &'a str,
}

/// The surface at position `index` of the file, or `None` when it cannot be
/// carried (an error says why).
pub(super) fn export_surface(
    chosen: &Chosen<'_>,
    index: usize,
    ids: &IdTable<'_>,
    env: &Environment,
    found: &mut Collected,
) -> Option<SurfaceV1> {
    let surface = chosen.surface;
    let mut dto = match SurfaceV1::from_model(surface) {
        Ok(dto) => dto,
        Err(why) => {
            found.error(
                ExportCode::InvalidContent,
                why,
                Some(format!("/content/surfaces/{index}")),
            );
            return None;
        }
    };
    dto.id = chosen.artifact_id.clone();
    if surface.context.is_some() {
        found.stripped.context_hints += 1;
    }
    for (p, (page, page_dto)) in surface.pages.iter().zip(&mut dto.pages).enumerate() {
        for (c, (control, control_dto)) in
            page.controls.iter().zip(&mut page_dto.controls).enumerate()
        {
            found.stripped.legacy_bindings += control.legacy_bindings.len();
            for (slot, model) in [
                (Slot::Press, &control.press),
                (Slot::LongPress, &control.long_press),
            ] {
                let pointer = format!(
                    "/content/surfaces/{index}/pages/{p}/controls/{c}/{}",
                    slot.member()
                );
                *slot_of(control_dto, slot) = model
                    .as_ref()
                    .and_then(|a| export_assignment(a, &pointer, &control.label, ids, env, found));
            }
        }
    }
    Some(dto)
}

pub(super) fn slot_of(control: &mut ControlV1, slot: Slot) -> &mut Option<AssignmentV1> {
    match slot {
        Slot::Press => &mut control.press,
        Slot::LongPress => &mut control.long_press,
    }
}

fn export_assignment(
    a: &Assignment,
    pointer: &str,
    label: &str,
    ids: &IdTable<'_>,
    env: &Environment,
    found: &mut Collected,
) -> Option<AssignmentV1> {
    if a.provider == LEGACY_PROVIDER {
        found.stripped.legacy_actions += 1;
        return None;
    }
    let mut exported = AssignmentV1::from_model(a);
    if let Some(def) = env.action(&a.provider, &a.action) {
        mark_surface_refs(&mut exported, def, pointer, label, ids, found);
    }
    Some(exported)
}

/// Every assignment of the exported surfaces, as it will be written, read
/// once: a pad that runs things is listed for review (with what the file
/// carries, a bound argument's answer never), a credential in an argument
/// not asked of the importer is an error, and every text argument is
/// offered as an input.
pub(super) fn look_over(surfaces: &[SurfaceV1], env: &Environment, found: &mut Collected) {
    for (index, surface) in surfaces.iter().enumerate() {
        for (p, page) in surface.pages.iter().enumerate() {
            for (c, control) in page.controls.iter().enumerate() {
                let pointer = format!("/content/surfaces/{index}/pages/{p}/controls/{c}");
                for slot in [Slot::Press, Slot::LongPress] {
                    let place = Place {
                        at: Location {
                            surface_source_id: surface.id.clone(),
                            page_id: page.id.clone(),
                            control_id: control.id.clone(),
                            slot,
                        },
                        pointer: format!("{pointer}/{}", slot.member()),
                        label: &control.label,
                    };
                    if let Some(a) = assignment_of(control, slot) {
                        look_at(a, &place, env, found);
                    }
                }
                refuse_feedback_credentials(control, &pointer, found);
            }
        }
    }
}

/// A feedback rule is written as it is, its state's arguments and its
/// event's conditions included: a credential there blocks the file too.
/// No input can stand in for it, so the person removes it.
fn refuse_feedback_credentials(control: &ControlV1, pointer: &str, found: &mut Collected) {
    for (k, rule) in control.feedback.iter().enumerate() {
        let value = serde_json::to_value(rule).unwrap_or(Value::Null);
        if let Some(what) = credential_in(&value) {
            found.error(
                ExportCode::Credential,
                format!(
                    "The pad \"{}\" carries a password or token in a feedback rule: {what}. \
                     Remove it from the rule.",
                    control.label
                ),
                Some(format!("{pointer}/feedback/{k}")),
            );
        }
    }
}

fn assignment_of(control: &ControlV1, slot: Slot) -> Option<&AssignmentV1> {
    match slot {
        Slot::Press => control.press.as_ref(),
        Slot::LongPress => control.long_press.as_ref(),
    }
}

fn look_at(exported: &AssignmentV1, place: &Place<'_>, env: &Environment, found: &mut Collected) {
    let model = exported.to_model();
    if let Some(kind) = classify(&model, env) {
        found.warn(
            ExportCode::ReviewBeforeSharing,
            review_warning(kind, place.label),
            Some(place.pointer.clone()),
        );
        found
            .review
            .push(ReviewItem::new(place.at.clone(), place.label, kind, &model));
    }
    refuse_credentials(exported, place, found);
    offer_arguments(exported, place, found);
}

/// An argument whose definition is a reference to a surface and that names
/// an exported surface is renamed to the surface's id in the file and
/// listed in `surfaceRefs`; one naming another surface keeps its value.
fn mark_surface_refs(
    exported: &mut AssignmentV1,
    def: &KnownAction,
    pointer: &str,
    label: &str,
    ids: &IdTable<'_>,
    found: &mut Collected,
) {
    let surface_params = def
        .params
        .iter()
        .filter(|p| p.kind == ParamKind::Reference && p.target == Some(RefTarget::Surface));
    for param in surface_params {
        let Some(Value::String(local)) = exported.args.get(&param.name) else {
            continue;
        };
        match ids.get(local.as_str()) {
            Some(&artifact_id) => {
                let renamed = Value::String(artifact_id.to_string());
                exported.args.insert(param.name.clone(), renamed);
                exported.surface_refs.push(param.name.clone());
            }
            None if !local.is_empty() => found.warn(
                ExportCode::ExternalSurfaceReference,
                format!(
                    "The pad \"{}\" opens the surface \"{local}\", which is not in this file. \
                     After import it opens that surface only where one has this id.",
                    label
                ),
                Some(format!("{pointer}/args/{}", segment(&param.name))),
            ),
            None => {}
        }
    }
}

fn review_warning(kind: ReviewKind, label: &str) -> String {
    let what = match kind {
        ReviewKind::RunsCommand => "runs a command",
        ReviewKind::SendsRequest => "sends a request",
        ReviewKind::LaunchesProgram => "starts a program",
        ReviewKind::InstructsAgent => "gives an agent an instruction",
    };
    format!("The pad \"{label}\" {what}: check it holds no password or token before sharing.")
}

/// A text shaped like a credential, or a value under a name that says it
/// is one, blocks the file, unless the person asks the importer for the
/// argument (a bound argument carries the input's
/// default, checked on its own): whoever receives the file would receive it.
fn refuse_credentials(exported: &AssignmentV1, place: &Place<'_>, found: &mut Collected) {
    for (name, value) in &exported.args {
        if exported.inputs.contains_key(name) {
            continue;
        }
        if let Some(what) = credential_named(name, value).or_else(|| credential_in(value)) {
            found.error(
                ExportCode::Credential,
                format!(
                    "The pad \"{}\" carries a password or token in \"{name}\": {what}. \
                     Bind \"{name}\" to an input so the importer gives their own, or remove it.",
                    place.label
                ),
                Some(format!("{}/args/{}", place.pointer, segment(name))),
            );
        }
    }
}

/// Every text argument is a parameter the importer could be asked for; one
/// that looks like a path on this computer is also suggested as one, and
/// one that names a home folder inside a longer text is pointed out. An
/// argument naming an exported surface is neither: the import renames it.
/// A bound argument is listed as asked of the importer, without a value.
fn offer_arguments(exported: &AssignmentV1, place: &Place<'_>, found: &mut Collected) {
    for (arg, value) in &exported.args {
        let Value::String(text) = value else {
            continue;
        };
        if exported.surface_refs.contains(arg) {
            continue;
        }
        if let Some(input) = exported.inputs.get(arg) {
            found.parameters.push(Parameter {
                at: place.at.clone(),
                arg: arg.clone(),
                value: None,
                input: Some(input.clone()),
            });
            continue;
        }
        found.parameters.push(Parameter {
            at: place.at.clone(),
            arg: arg.clone(),
            value: Some(text.clone()),
            input: None,
        });
        if looks_like_path(text) {
            found.warn(
                ExportCode::LocalPath,
                format!(
                    "The pad \"{}\" names a path on this computer in \"{arg}\". \
                     Consider asking the importer for it.",
                    place.label
                ),
                Some(format!("{}/args/{}", place.pointer, segment(arg))),
            );
            found.suggestions.push(Suggestion {
                at: place.at.clone(),
                arg: arg.clone(),
                value: text.clone(),
                suggested_type: SuggestedType::Path,
            });
        } else if names_home_folder(text) {
            found.warn(
                ExportCode::LocalPath,
                format!(
                    "The pad \"{}\" names a folder in a home folder in \"{arg}\", \
                     which shows a user name. Check it before sharing.",
                    place.label
                ),
                Some(format!("{}/args/{}", place.pointer, segment(arg))),
            );
        }
    }
}

/// A folder in a home folder anywhere in a text, which holds a user name:
/// `/Users/<name>/`, `/home/<name>/` or `<letter>:\Users\<name>\`.
fn names_home_folder(text: &str) -> bool {
    let user_then = |rest: &str, separator: char| {
        rest.split_once(separator)
            .is_some_and(|(user, _)| !user.is_empty() && !user.contains(char::is_whitespace))
    };
    let unix = ["/Users/", "/home/"].iter().any(|root| {
        text.match_indices(root)
            .any(|(i, _)| user_then(&text[i + root.len()..], '/'))
    });
    const WINDOWS: &str = ":\\users\\";
    let lower = text.to_ascii_lowercase();
    let windows = lower.match_indices(WINDOWS).any(|(i, _)| {
        lower[..i].ends_with(|c: char| c.is_ascii_alphabetic())
            && user_then(&lower[i + WINDOWS.len()..], '\\')
    });
    unix || windows
}

/// `/…`, `~/…` or `<letter>:\…`.
fn looks_like_path(text: &str) -> bool {
    let mut chars = text.chars();
    let drive = matches!(
        (chars.next(), chars.next(), chars.next()),
        (Some(letter), Some(':'), Some('\\')) if letter.is_ascii_alphabetic()
    );
    drive || text.starts_with('/') || text.starts_with("~/")
}
