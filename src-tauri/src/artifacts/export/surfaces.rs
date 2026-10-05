//! Each chosen surface as the file carries it. The format's conversion
//! (`SurfaceV1::from_model`) leaves out legacy bindings and the context
//! hint; here each assignment is then read once: a legacy action is
//! stripped, references to exported surfaces are marked and renamed, and
//! what a person should look at before sharing is collected.

use serde_json::{Map, Value};

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
                let place = Place {
                    at: Location {
                        surface_source_id: chosen.artifact_id.clone(),
                        page_id: page.id.clone(),
                        control_id: control.id.clone(),
                        slot,
                    },
                    pointer: format!(
                        "/content/surfaces/{index}/pages/{p}/controls/{c}/{}",
                        slot.member()
                    ),
                    label: &control.label,
                };
                *slot_of(control_dto, slot) = model
                    .as_ref()
                    .and_then(|a| export_assignment(a, &place, ids, env, found));
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
    place: &Place<'_>,
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
        mark_surface_refs(&mut exported, def, place, ids, found);
    }
    if let Some(kind) = classify(a, env) {
        found.warn(
            ExportCode::ReviewBeforeSharing,
            review_warning(kind, place.label),
            Some(place.pointer.clone()),
        );
        found
            .review
            .push(ReviewItem::new(place.at.clone(), place.label, kind, a));
    }
    refuse_credentials(&exported.args, place, found);
    offer_arguments(&exported, place, found);
    Some(exported)
}

/// An argument whose definition is a reference to a surface and that names
/// an exported surface is renamed to the surface's id in the file and
/// listed in `surfaceRefs`; one naming another surface keeps its value.
fn mark_surface_refs(
    exported: &mut AssignmentV1,
    def: &KnownAction,
    place: &Place<'_>,
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
                    place.label
                ),
                Some(format!("{}/args/{}", place.pointer, segment(&param.name))),
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

/// A text argument that is an address with a user name or password blocks
/// the file: whoever receives it would receive the credentials.
fn refuse_credentials(args: &Map<String, Value>, place: &Place<'_>, found: &mut Collected) {
    for (name, value) in args {
        if texts(value).any(has_credentials) {
            found.error(
                ExportCode::CredentialInUrl,
                format!(
                    "The pad \"{}\" has an address with a user name or password in \"{name}\". \
                     Remove them before sharing.",
                    place.label
                ),
                Some(format!("{}/args/{}", place.pointer, segment(name))),
            );
        }
    }
}

/// Every text in a value, at any depth.
fn texts(value: &Value) -> Box<dyn Iterator<Item = &str> + '_> {
    match value {
        Value::String(s) => Box::new(std::iter::once(s.as_str())),
        Value::Array(items) => Box::new(items.iter().flat_map(texts)),
        Value::Object(map) => Box::new(map.values().flat_map(texts)),
        _ => Box::new(std::iter::empty()),
    }
}

fn has_credentials(text: &str) -> bool {
    url::Url::parse(text.trim()).is_ok_and(|u| !u.username().is_empty() || u.password().is_some())
}

/// Every text argument is a parameter the importer could be asked for; one
/// that looks like a path on this computer is also suggested as one. An
/// argument naming an exported surface is neither: the import renames it.
fn offer_arguments(exported: &AssignmentV1, place: &Place<'_>, found: &mut Collected) {
    for (arg, value) in &exported.args {
        let Value::String(text) = value else {
            continue;
        };
        if exported.surface_refs.contains(arg) {
            continue;
        }
        found.parameters.push(Parameter {
            at: place.at.clone(),
            arg: arg.clone(),
            value: text.clone(),
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
        }
    }
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
