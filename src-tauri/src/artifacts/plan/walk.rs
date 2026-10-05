//! Every control and every assignment of a file's content, in the file's
//! order, with where it is: by the ids the file gives it, and as a JSON
//! pointer.

use crate::artifacts::format::{AssignmentV1, ContentV1, ControlV1, SurfaceV1};
use crate::artifacts::review::{Location, Slot};

/// A control and where it is.
pub(super) struct Placed<'a> {
    pub surface: &'a SurfaceV1,
    pub page_id: &'a str,
    pub control: &'a ControlV1,
    /// `/content/surfaces/<s>/pages/<p>/controls/<c>`.
    pub pointer: String,
}

/// A press or a long press and where it is.
pub(super) struct Pressed<'a> {
    pub at: Location,
    /// The pointer to the assignment (`…/press`).
    pub pointer: String,
    pub label: &'a str,
    pub assignment: &'a AssignmentV1,
}

pub(super) fn controls(content: &ContentV1) -> Vec<Placed<'_>> {
    let mut out = Vec::new();
    for (s, surface) in content.surfaces.iter().enumerate() {
        for (p, page) in surface.pages.iter().enumerate() {
            for (c, control) in page.controls.iter().enumerate() {
                out.push(Placed {
                    surface,
                    page_id: &page.id,
                    control,
                    pointer: format!("/content/surfaces/{s}/pages/{p}/controls/{c}"),
                });
            }
        }
    }
    out
}

pub(super) fn assignments(content: &ContentV1) -> Vec<Pressed<'_>> {
    let mut out = Vec::new();
    for placed in controls(content) {
        let control = placed.control;
        for (slot, assignment) in [
            (Slot::Press, &control.press),
            (Slot::LongPress, &control.long_press),
        ] {
            let Some(assignment) = assignment else {
                continue;
            };
            out.push(Pressed {
                at: Location {
                    surface_source_id: placed.surface.id.clone(),
                    page_id: placed.page_id.to_string(),
                    control_id: control.id.clone(),
                    slot,
                },
                pointer: format!("{}/{}", placed.pointer, slot.member()),
                label: &control.label,
                assignment,
            });
        }
    }
    out
}
