//! The request's input bindings (plan 7.8): the answer to an input replaces
//! a whole text argument of an exported press or long press. Application
//! inputs bind only to context rules (`rules`), every other input only here.
//!
//! A bound argument is written as the input's default, or empty without
//! one: its value on this computer is what the person chose to ask the
//! importer for, and never leaves it (security finding SF-2). A default may
//! not carry a credential either.

use serde_json::Value;

use super::credentials::credential_in;
use super::preview::{Collected, ExportCode};
use super::request::{ExportRequest, InputBinding};
use super::surfaces::slot_of;
use crate::artifacts::format::{
    ArtifactKind, AssignmentV1, InputDef, InputType, InputValue, SurfaceV1,
};
use crate::artifacts::json::segment;

/// Record each binding of `request` on the assignment it names, with the
/// input's default in place of the argument's value, or say why it cannot
/// be. A Surface file has no inputs, so no bindings.
pub(super) fn bind(surfaces: &mut [SurfaceV1], request: &ExportRequest, found: &mut Collected) {
    check_defaults(&request.inputs, found);
    if request.bindings.is_empty() {
        return;
    }
    if request.kind == ArtifactKind::Surface {
        found.error(
            ExportCode::KindMismatch,
            "A Surface file asks the importer for nothing; export a Pack to use inputs.".into(),
            None,
        );
        return;
    }
    for binding in &request.bindings {
        match target(surfaces, binding) {
            Some((pointer, assignment)) => {
                let at = format!("{pointer}/inputs/{}", segment(&binding.arg));
                match problem(assignment, binding, request) {
                    Some(why) => found.error(ExportCode::InvalidBinding, why, Some(at)),
                    None => {
                        assignment
                            .inputs
                            .insert(binding.arg.clone(), binding.input.clone());
                        let written = default_text(request, &binding.input);
                        assignment
                            .args
                            .insert(binding.arg.clone(), Value::String(written));
                    }
                }
            }
            None => found.error(
                ExportCode::InvalidBinding,
                format!(
                    "An input is bound to a pad action that is not in the file (the control \"{}\" of the page \"{}\" of the surface \"{}\").",
                    binding.at.control_id, binding.at.page_id, binding.at.surface_source_id
                ),
                None,
            ),
        }
    }
}

/// What a bound argument is written as: the input's text default, or empty.
fn default_text(request: &ExportRequest, input: &str) -> String {
    let default = request
        .inputs
        .iter()
        .find(|i| i.id == input)
        .and_then(|i| i.default.as_ref());
    match default {
        Some(InputValue::Text { value }) => value.clone(),
        _ => String::new(),
    }
}

/// A default shaped like a credential is an error: it would be written in
/// every argument bound to its input.
fn check_defaults(inputs: &[InputDef], found: &mut Collected) {
    for (i, input) in inputs.iter().enumerate() {
        let Some(InputValue::Text { value }) = &input.default else {
            continue;
        };
        if let Some(credential) = credential_in(&Value::String(value.clone())) {
            found.error(
                ExportCode::Credential,
                format!(
                    "The default of the input \"{}\" {}. Remove it from the default.",
                    input.id,
                    credential.said("")
                ),
                Some(format!("/manifest/inputs/{i}/default")),
            );
        }
    }
}

/// The assignment a binding names, with the pointer to it.
fn target<'s>(
    surfaces: &'s mut [SurfaceV1],
    binding: &InputBinding,
) -> Option<(String, &'s mut AssignmentV1)> {
    let at = &binding.at;
    let (s, surface) = surfaces
        .iter_mut()
        .enumerate()
        .find(|(_, s)| s.id == at.surface_source_id)?;
    let (p, page) = surface
        .pages
        .iter_mut()
        .enumerate()
        .find(|(_, p)| p.id == at.page_id)?;
    let (c, control) = page
        .controls
        .iter_mut()
        .enumerate()
        .find(|(_, c)| c.id == at.control_id)?;
    let pointer = format!(
        "/content/surfaces/{s}/pages/{p}/controls/{c}/{}",
        at.slot.member()
    );
    slot_of(control, at.slot)
        .as_mut()
        .map(|assignment| (pointer, assignment))
}

/// Why `binding` cannot be recorded on `assignment`, if it cannot.
fn problem(
    assignment: &AssignmentV1,
    binding: &InputBinding,
    request: &ExportRequest,
) -> Option<String> {
    let (arg, input) = (&binding.arg, &binding.input);
    let Some(def) = request.inputs.iter().find(|i| &i.id == input) else {
        return Some(format!(
            "The argument \"{arg}\" is bound to the input \"{input}\", which is not declared."
        ));
    };
    if def.kind == InputType::Application {
        return Some(format!(
            "The input \"{input}\" is an application; it can only choose the application of a context rule."
        ));
    }
    if !matches!(assignment.args.get(arg), Some(Value::String(_))) {
        return Some(format!(
            "The action has no text argument \"{arg}\" for the input \"{input}\" to replace."
        ));
    }
    if assignment.surface_refs.contains(arg) {
        return Some(format!(
            "The argument \"{arg}\" names a surface of this file, which the import renames; it cannot be asked for."
        ));
    }
    if assignment.inputs.contains_key(arg) {
        return Some(format!(
            "The argument \"{arg}\" is bound to an input twice."
        ));
    }
    None
}
