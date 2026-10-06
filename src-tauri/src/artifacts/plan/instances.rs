//! Earlier imports of the same file, the instance this import is recorded
//! under, and the table from the file's surface ids to local ids (plan
//! 7.13, 7.14).

use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet, HashSet};

use super::decisions::{ImportDecisions, Resolution};
use super::types::{ExistingInstance, ExistingSurface, PlannedSurface, Relation, SurfaceAction};
use crate::artifacts::env::Environment;
use crate::artifacts::format::{numbered_surface_id, SurfaceV1};
use crate::artifacts::inspect::{ParsedArtifact, KNOWN_CATEGORIES};
use crate::artifacts::json::{content_digest, to_canonical};
use crate::artifacts::review::visible;
use crate::core::model::{ImportOrigin, Surface, Workspace};
use crate::state::feedback::When;

/// What a surface of an unknown category is imported as.
const CUSTOM: &str = "custom";

/// `sha256:` and the digest of a surface in its canonical file form: what
/// `importedFrom.surfaceDigest` records when a surface is imported, and what
/// tells later whether it was changed since. None for a surface a file
/// cannot carry (a control on the logo).
pub(in crate::artifacts) fn surface_digest(surface: &Surface) -> Option<String> {
    let v1 = SurfaceV1::from_model(surface).ok()?;
    let value = serde_json::to_value(&v1).ok()?;
    Some(content_digest(&to_canonical(&value)))
}

/// Changed here since it was imported.
fn locally_edited(surface: &Surface, origin: &ImportOrigin) -> bool {
    surface_digest(surface).as_deref() != Some(origin.surface_digest.as_str())
}

/// Imported from a file of the same format and id.
fn same_artifact(origin: &ImportOrigin, parsed: &ParsedArtifact) -> bool {
    origin.format == parsed.kind.format() && origin.artifact_id == parsed.manifest.id
}

/// By SemVer precedence; as text when either is not a version.
fn compare_versions(a: &str, b: &str) -> Ordering {
    match (semver::Version::parse(a), semver::Version::parse(b)) {
        (Ok(a), Ok(b)) => a.cmp_precedence(&b),
        _ => a.cmp(b),
    }
}

/// The earlier imports of the file, by instance number.
pub(super) fn existing(parsed: &ParsedArtifact, workspace: &Workspace) -> Vec<ExistingInstance> {
    let mut instances: BTreeMap<u32, Vec<(&Surface, &ImportOrigin)>> = BTreeMap::new();
    for surface in &workspace.surfaces {
        if let Some(origin) = surface.imported_from.as_ref() {
            if same_artifact(origin, parsed) {
                instances
                    .entry(origin.instance)
                    .or_default()
                    .push((surface, origin));
            }
        }
    }
    instances
        .into_iter()
        .map(|(instance, surfaces)| {
            let version = surfaces
                .iter()
                .map(|(_, o)| o.artifact_version.as_str())
                .max_by(|a, b| compare_versions(a, b))
                .unwrap_or_default()
                .to_string();
            let relation = match compare_versions(&parsed.manifest.version, &version) {
                Ordering::Greater => Relation::Newer,
                Ordering::Equal => Relation::Same,
                Ordering::Less => Relation::Older,
            };
            ExistingInstance {
                instance,
                relation,
                identical_content: surfaces
                    .iter()
                    .all(|(_, o)| o.content_digest == parsed.content_digest),
                surfaces: surfaces
                    .iter()
                    .map(|(s, o)| ExistingSurface {
                        local_id: s.id.clone(),
                        name: visible(&s.name, false),
                        source_id: o.source_surface_id.clone(),
                        locally_edited: locally_edited(s, o),
                    })
                    .collect(),
                version,
            }
        })
        .collect()
}

/// The instance this import is recorded under, and what it replaces.
pub(super) struct Target<'w> {
    pub instance: u32,
    /// Surfaces of the instance being replaced, by the id the file gave them
    /// (the first, should two have the same).
    pub replaced: BTreeMap<String, &'w Surface>,
    /// Every surface of the instance being replaced.
    pub instance_surfaces: Vec<&'w Surface>,
    /// Why a resolution is still needed.
    pub unresolved: Option<String>,
}

/// A new instance unless the person chose to replace one that exists. While
/// no valid choice is made for a file imported before, the plan shows a new
/// instance and says a resolution is needed.
pub(super) fn target<'w>(
    parsed: &ParsedArtifact,
    workspace: &'w Workspace,
    existing: &[ExistingInstance],
    decisions: &ImportDecisions,
) -> Target<'w> {
    let next = existing
        .iter()
        .map(|e| e.instance)
        .max()
        .unwrap_or(0)
        .saturating_add(1);
    let as_new = |unresolved: Option<String>| Target {
        instance: next,
        replaced: BTreeMap::new(),
        instance_surfaces: Vec::new(),
        unresolved,
    };
    if existing.is_empty() {
        return as_new(None);
    }
    match decisions.resolution {
        Some(Resolution::ImportAsNew) => as_new(None),
        Some(Resolution::Replace { instance })
            if existing.iter().any(|e| e.instance == instance) =>
        {
            let instance_surfaces: Vec<&Surface> = workspace
                .surfaces
                .iter()
                .filter(|s| {
                    s.imported_from
                        .as_ref()
                        .is_some_and(|o| same_artifact(o, parsed) && o.instance == instance)
                })
                .collect();
            let mut replaced = BTreeMap::new();
            for s in &instance_surfaces {
                if let Some(origin) = &s.imported_from {
                    replaced
                        .entry(origin.source_surface_id.clone())
                        .or_insert(*s);
                }
            }
            Target {
                instance,
                replaced,
                instance_surfaces,
                unresolved: None,
            }
        }
        Some(Resolution::Replace { instance }) => as_new(Some(format!(
            "There is no import {instance} of this file to replace: import it as new, or \
             replace one of the imports listed."
        ))),
        None => as_new(Some(
            "This file was imported before: import it again as new surfaces, or replace an \
             earlier import."
                .into(),
        )),
    }
}

/// A surface of the file and its id here.
pub(super) struct Mapping<'w> {
    pub source_id: String,
    pub local_id: String,
    /// The surface of an earlier import it replaces, in place.
    pub replaces: Option<&'w Surface>,
}

/// In the file's order: a replaced surface keeps its local id; a new one
/// gets the file's id when free, else `<id>-2`, `-3`... (the rule of a new
/// surface), never an id taken here or earlier in the table.
pub(super) fn id_table<'w>(
    parsed: &ParsedArtifact,
    workspace: &Workspace,
    target: &Target<'w>,
) -> Vec<Mapping<'w>> {
    let mut taken: HashSet<String> = workspace.surfaces.iter().map(|s| s.id.clone()).collect();
    parsed
        .content
        .surfaces
        .iter()
        .map(|s| {
            if let Some(local) = target.replaced.get(&s.id) {
                return Mapping {
                    source_id: s.id.clone(),
                    local_id: local.id.clone(),
                    replaces: Some(*local),
                };
            }
            let local_id = free_id(&s.id, &taken);
            taken.insert(local_id.clone());
            Mapping {
                source_id: s.id.clone(),
                local_id,
                replaces: None,
            }
        })
        .collect()
}

fn free_id(id: &str, taken: &HashSet<String>) -> String {
    if !taken.contains(id) {
        return id.to_string();
    }
    (2u32..)
        .map(|n| numbered_surface_id(id, n))
        .find(|candidate| !taken.contains(candidate))
        .unwrap_or_default()
}

/// The category a surface is imported with.
pub(super) fn imported_category(category: &str) -> &str {
    if KNOWN_CATEGORIES.contains(&category) {
        category
    } else {
        CUSTOM
    }
}

pub(super) fn planned_surfaces(
    parsed: &ParsedArtifact,
    workspace: &Workspace,
    env: &Environment,
    table: &[Mapping<'_>],
) -> Vec<PlannedSurface> {
    parsed
        .content
        .surfaces
        .iter()
        .zip(table)
        .map(|(s, m)| {
            let action = match m.replaces {
                Some(local) => SurfaceAction::Replace {
                    previous_version: local
                        .imported_from
                        .as_ref()
                        .map(|o| o.artifact_version.clone())
                        .unwrap_or_default(),
                    locally_edited: local
                        .imported_from
                        .as_ref()
                        .is_some_and(|o| locally_edited(local, o)),
                },
                None => SurfaceAction::Add,
            };
            let same_name_as = workspace
                .surfaces
                .iter()
                .find(|w| w.name == s.name && w.id != m.local_id)
                .map(|w| w.id.clone());
            PlannedSurface {
                source_id: s.id.clone(),
                local_id: m.local_id.clone(),
                name: visible(&s.name, false),
                category: imported_category(&s.category).to_string(),
                pages: s.pages.len(),
                controls: s.pages.iter().map(|p| p.controls.len()).sum(),
                action,
                id_changed: m.local_id != s.id,
                same_name_as,
                missing_providers: missing_providers(s, env),
            }
        })
        .collect()
}

/// Providers the surface's pads name that have no actions here, and
/// providers its state rules name that offer nothing here.
fn missing_providers(surface: &SurfaceV1, env: &Environment) -> Vec<String> {
    let has_actions = |p: &&str| env.providers.contains_key(*p);
    let offers = |p: &&str| has_actions(p) || env.observable_sources.contains(*p);
    let controls = surface.pages.iter().flat_map(|p| &p.controls);
    let actions = controls
        .clone()
        .flat_map(|c| [&c.press, &c.long_press])
        .flatten()
        .map(|a| a.provider.as_str())
        .filter(|p| !has_actions(p));
    let states = controls
        .flat_map(|c| &c.feedback)
        .filter_map(|rule| match &rule.when {
            When::State(s) => Some(s.state.provider.as_str()),
            When::Event(_) => None,
        })
        .filter(|p| !offers(p));
    actions
        .chain(states)
        .map(str::to_string)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}
