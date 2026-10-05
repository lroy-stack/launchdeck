//! The surfaces a request chooses, in workspace order, and the id the file
//! gives each one.

use std::collections::{BTreeMap, HashSet};

use super::preview::{Collected, ExportCode};
use crate::core::model::{Surface, Workspace};

/// A chosen surface and the id the file gives it.
pub(super) struct Chosen<'w> {
    pub surface: &'w Surface,
    pub artifact_id: String,
}

/// Local surface id to the id the file gives it, for every exported surface.
pub(super) type IdTable<'a> = BTreeMap<&'a str, &'a str>;

pub(super) fn id_table<'a>(chosen: &'a [Chosen<'_>]) -> IdTable<'a> {
    chosen
        .iter()
        .map(|c| (c.surface.id.as_str(), c.artifact_id.as_str()))
        .collect()
}

/// The surfaces named by `ids`, in the workspace's order, whatever the
/// request's order; an id that names no surface is an error.
pub(super) fn choose<'w>(
    workspace: &'w Workspace,
    ids: &[String],
    found: &mut Collected,
) -> Vec<Chosen<'w>> {
    if ids.is_empty() {
        found.error(
            ExportCode::NoSurfaces,
            "Choose at least one surface to export.".into(),
            None,
        );
    }
    for id in ids {
        if workspace.surface(id).is_none() {
            found.error(
                ExportCode::UnknownSurface,
                format!("There is no surface \"{id}\" to export."),
                None,
            );
        }
    }
    let surfaces: Vec<&Surface> = workspace
        .surfaces
        .iter()
        .filter(|s| ids.contains(&s.id))
        .collect();
    let preferred: Vec<&str> = surfaces
        .iter()
        .map(|s| preferred_id(s, &surfaces))
        .collect();
    surfaces
        .into_iter()
        .zip(artifact_ids(&preferred))
        .map(|(surface, artifact_id)| Chosen {
            surface,
            artifact_id,
        })
        .collect()
}

/// The id its file gave a surface imported earlier, when no other chosen
/// surface came from the same id; otherwise the local id.
fn preferred_id<'w>(surface: &'w Surface, chosen: &[&'w Surface]) -> &'w str {
    let source = |s: &'w Surface| {
        s.imported_from
            .as_ref()
            .map(|origin| origin.source_surface_id.as_str())
    };
    match source(surface) {
        Some(id) if chosen.iter().filter(|s| source(s) == Some(id)).count() == 1 => id,
        _ => surface.id.as_str(),
    }
}

/// Each preferred id once: the first to prefer an id gets it, a later one
/// `<id>-2`, `-3`... (the rule of a new surface), never one another prefers.
pub(super) fn artifact_ids(preferred: &[&str]) -> Vec<String> {
    let wanted: HashSet<&str> = preferred.iter().copied().collect();
    let mut taken: HashSet<String> = HashSet::new();
    preferred
        .iter()
        .map(|&id| {
            let id = if taken.contains(id) {
                (2..)
                    .map(|n| format!("{id}-{n}"))
                    .find(|c| !taken.contains(c) && !wanted.contains(c.as_str()))
                    .unwrap_or_default()
            } else {
                id.to_string()
            };
            taken.insert(id.clone());
            id
        })
        .collect()
}
