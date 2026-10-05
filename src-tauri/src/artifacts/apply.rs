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
mod tests {
    use serde_json::{json, Value};

    use super::*;
    use crate::artifacts::env::Environment;
    use crate::artifacts::inspect::inspect;
    use crate::artifacts::json::content_digest;
    use crate::artifacts::plan::{
        plan, surface_digest, DependencyStatus, ImportDecisions, Resolution, Runs,
    };
    use crate::artifacts::review::{Location, Slot};
    use crate::artifacts::InputValue;
    use crate::context::identity::{ApplicationIdentity, X11};
    use crate::context::rules::ContextRule;
    use crate::core::model::{Control, ControlPos, Rgb};
    use crate::surfaces::{default_workspace_for, Platform};

    /// A pad of the file at `col` on the first row.
    fn pad(col: u8, label: &str, press: Value) -> Value {
        json!({
            "id": format!("g.{col}.0"),
            "pos": { "kind": "grid", "col": col, "row": 0 },
            "label": label,
            "visual": { "color": "#c82828" },
            "looks": { "done": { "color": "#28c828" } },
            "press": press,
        })
    }

    /// Studio 1.0.0, one surface `development` (an id the default workspace
    /// has) with a command, an address asked of the importer, a switch to
    /// itself, a pad of a plugin and a rule for an application.
    fn studio(version: &str) -> Value {
        json!({
            "format": "launchdeck.pack",
            "formatVersion": 1,
            "manifest": {
                "id": "acme.studio",
                "name": "Studio",
                "version": version,
                "createdWith": { "app": "launchdeck", "version": "9.9.9" },
                "requirements": {
                    "grid": { "minWidth": 8, "minHeight": 8 },
                    "aux": { "top": 8, "side": 8 },
                    "rgb": true
                },
                "dependencies": [
                    { "pluginId": "acme.notes", "version": "^1.0.0", "required": true }
                ],
                "inputs": [
                    { "id": "docs", "type": "url", "label": "Docs" }
                ],
                "integrity": { "content": "" }
            },
            "content": {
                "surfaces": [{
                    "id": "development",
                    "name": "Studio",
                    "category": "recording",
                    "pages": [{
                        "id": "home",
                        "name": "Home",
                        "controls": [
                            pad(0, "Build", json!({
                                "provider": "shell", "action": "execute",
                                "args": { "command": "make" }
                            })),
                            pad(1, "Docs", json!({
                                "provider": "system", "action": "openUrl",
                                "args": { "url": "https://example.com" },
                                "inputs": { "url": "docs" }
                            })),
                            pad(2, "Here", json!({
                                "provider": "navigation", "action": "openSurface",
                                "args": { "surfaceId": "development" },
                                "surfaceRefs": ["surfaceId"]
                            })),
                            pad(3, "Note", json!({
                                "provider": "acme.notes", "action": "mark", "args": {}
                            })),
                        ]
                    }]
                }],
                "contextRules": [{
                    "application": {
                        "name": "Code",
                        "identities": [{ "namespace": "x11", "id": "Code" }]
                    },
                    "surface": "development"
                }]
            }
        })
    }

    fn read(mut file: Value) -> ParsedArtifact {
        let content = serde_json::to_string(&file["content"]).unwrap();
        file["manifest"]["integrity"]["content"] = json!(content_digest(&content));
        let bytes = serde_json::to_vec(&file).unwrap();
        inspect(&bytes, "studio.ldpack").unwrap_or_else(|r| panic!("{r:?}"))
    }

    fn env() -> Environment {
        Environment {
            namespace: Some(X11.into()),
            ..Environment::default()
        }
    }

    fn workspace() -> Workspace {
        default_workspace_for(Platform::Linux)
    }

    fn docs(url: &str) -> ImportDecisions {
        ImportDecisions {
            runs: Some(Runs::Remove),
            inputs: [("docs".to_string(), InputValue::Text { value: url.into() })].into(),
            ..ImportDecisions::default()
        }
    }

    /// The workspace an import of `file` with `decisions` writes into `ws`.
    fn applied(file: &ParsedArtifact, ws: &Workspace, decisions: &ImportDecisions) -> Workspace {
        let plan = plan(file, ws, &env(), decisions);
        assert!(plan.ready, "{:?}", plan.blockers);
        let candidate = build(file, &plan, ws).unwrap();
        verify(&candidate, ws).unwrap();
        candidate
    }

    fn control<'w>(ws: &'w Workspace, surface: &str, id: &str) -> &'w Control {
        ws.surface(surface).unwrap().pages[0].control(id).unwrap()
    }

    #[test]
    fn a_new_surface_is_added_after_the_others_with_where_it_came_from() {
        let file = read(studio("1.0.0"));
        let ws = workspace();
        let after = applied(&file, &ws, &docs("https://docs.example"));

        assert_eq!(after.surfaces.len(), ws.surfaces.len() + 1);
        assert_eq!(after.surfaces[..ws.surfaces.len()], ws.surfaces[..]);
        let added = after.surfaces.last().unwrap();
        assert_eq!(added.id, "development-2");
        assert_eq!(added.name, "Studio");
        assert_eq!(added.category, "custom", "an unknown category");
        let origin = added.imported_from.as_ref().unwrap();
        assert_eq!(origin.format, "launchdeck.pack");
        assert_eq!(
            (&origin.artifact_id[..], &origin.artifact_name[..]),
            ("acme.studio", "Studio")
        );
        assert_eq!(origin.artifact_version, "1.0.0");
        assert_eq!(origin.instance, 1);
        assert_eq!(origin.source_surface_id, "development");
        assert_eq!(origin.content_digest, file.content_digest);
        assert_eq!(
            Some(&origin.surface_digest),
            surface_digest(added).as_ref(),
            "the digest of the surface as it was written here"
        );
        assert_eq!(
            (after.preferences.clone(), after.schema_version),
            (ws.preferences.clone(), ws.schema_version)
        );
    }

    #[test]
    fn answers_and_local_ids_are_written_and_everything_else_as_the_file_has_it() {
        let file = read(studio("1.0.0"));
        let after = applied(&file, &workspace(), &docs("https://docs.example"));
        let press = |id| control(&after, "development-2", id).press.clone().unwrap();
        assert_eq!(
            press("g.1.0").args,
            json!({ "url": "https://docs.example" })
        );
        assert_eq!(press("g.2.0").args, json!({ "surfaceId": "development-2" }));
        assert_eq!(
            (press("g.3.0").provider, press("g.3.0").action),
            ("acme.notes".to_string(), "mark".to_string()),
            "a plugin that is not installed is kept verbatim"
        );
        let note = control(&after, "development-2", "g.3.0");
        assert_eq!(note.states.len(), 1);
        assert_eq!(note.pos, ControlPos::grid(3, 0));
    }

    #[test]
    fn slots_the_person_removed_are_empty_and_the_control_stays() {
        let file = read(studio("1.0.0"));
        let ws = workspace();
        let removed = applied(&file, &ws, &docs("https://docs.example"));
        let build = control(&removed, "development-2", "g.0.0");
        assert_eq!(build.press, None);
        assert_eq!(build.label, "Build");
        assert!(build.states.contains_key("done"));

        let kept = ImportDecisions {
            runs: Some(Runs::Keep),
            ..docs("https://docs.example")
        };
        let kept = applied(&file, &ws, &kept);
        let press = control(&kept, "development-2", "g.0.0").press.clone();
        assert_eq!(press.unwrap().args, json!({ "command": "make" }));
    }

    #[test]
    fn selected_rules_are_appended_and_auto_is_left_alone() {
        let file = read(studio("1.0.0"));
        let mut ws = workspace();
        ws.context.auto = true;
        let local = ContextRule {
            application: ApplicationIdentity::new(X11, "firefox", "Firefox"),
            surface_id: "productivity".into(),
            page_id: None,
        };
        ws.context.rules.push(local.clone());
        let none = applied(&file, &ws, &docs("https://docs.example"));
        assert_eq!(none.context, ws.context, "rules are opt-in");

        let chosen = ImportDecisions {
            context_rules: vec![0],
            ..docs("https://docs.example")
        };
        let after = applied(&file, &ws, &chosen);
        assert!(after.context.auto);
        assert_eq!(after.context.fallback, ws.context.fallback);
        assert_eq!(after.context.rules.len(), 2);
        assert_eq!(after.context.rules[0], local);
        let added = &after.context.rules[1];
        assert_eq!(
            (
                added.application.namespace.as_str(),
                added.application.id.as_str()
            ),
            (X11, "Code")
        );
        assert_eq!(added.surface_id, "development-2");
    }

    #[test]
    fn a_replace_keeps_the_place_and_the_id_and_records_the_new_version() {
        let first = read(studio("1.0.0"));
        let mut ws = applied(&first, &workspace(), &docs("https://docs.example"));
        // Something after it, which must stay after it.
        let last = ws.surfaces[0].clone();
        ws.surfaces.push(crate::core::model::Surface {
            id: "later".into(),
            ..last
        });
        let place = ws.surfaces.len() - 2;
        assert_eq!(ws.surfaces[place].id, "development-2");

        let mut next = studio("1.1.0");
        next["content"]["surfaces"][0]["name"] = json!("Studio 2");
        let next = read(next);
        let decisions = ImportDecisions {
            resolution: Some(Resolution::Replace { instance: 1 }),
            ..docs("https://docs.example")
        };
        let after = applied(&next, &ws, &decisions);
        assert_eq!(after.surfaces.len(), ws.surfaces.len());
        let replaced = &after.surfaces[place];
        assert_eq!(
            (replaced.id.as_str(), replaced.name.as_str()),
            ("development-2", "Studio 2")
        );
        let origin = replaced.imported_from.as_ref().unwrap();
        assert_eq!(
            (origin.instance, origin.artifact_version.as_str()),
            (1, "1.1.0")
        );
        assert_eq!(after.surfaces.last().unwrap().id, "later");
    }

    #[test]
    fn verify_refuses_what_the_runtime_would_not_save() {
        let file = read(studio("1.0.0"));
        let ws = workspace();
        let decisions = docs("https://docs.example");
        let plan = plan(&file, &ws, &env(), &decisions);
        let good = build(&file, &plan, &ws).unwrap();

        let mut overlap = good.clone();
        let page = &mut overlap.surfaces.last_mut().unwrap().pages[0];
        page.controls[1].pos = ControlPos::grid(0, 0);
        let err = verify(&overlap, &ws).unwrap_err();
        assert!(matches!(err, ApplyError::Control { .. }), "{err:?}");
        assert!(err.to_string().ends_with('.'), "{err}");

        let mut twice = good.clone();
        twice.surfaces.push(twice.surfaces[1].clone());
        assert!(matches!(
            verify(&twice, &ws),
            Err(ApplyError::DuplicateSurface(id)) if id == "development"
        ));

        let mut nowhere = good.clone();
        nowhere.context.rules.push(ContextRule {
            application: ApplicationIdentity::new(X11, "Code", "Code"),
            surface_id: "gone".into(),
            page_id: None,
        });
        assert!(matches!(verify(&nowhere, &ws), Err(ApplyError::Rules(_))));
    }

    #[test]
    fn verify_checks_only_the_pages_the_import_touches() {
        let file = read(studio("1.0.0"));
        let mut ws = workspace();
        // A control an earlier version saved that today's checks refuse.
        let odd = Control::new(ControlPos::grid(0, 0), "Odd", Rgb::new(1, 1, 1));
        ws.surfaces[1].pages[0].controls.push(odd);
        let decisions = docs("https://docs.example");
        let plan = plan(&file, &ws, &env(), &decisions);
        let candidate = build(&file, &plan, &ws).unwrap();
        verify(&candidate, &ws).unwrap();
    }

    #[test]
    fn a_plan_that_does_not_fit_the_file_writes_nothing() {
        let file = read(studio("1.0.0"));
        let ws = workspace();
        let mut plan = plan(&file, &ws, &env(), &docs("https://docs.example"));
        plan.effects.add_surfaces.clear();
        assert!(matches!(
            build(&file, &plan, &ws),
            Err(ApplyError::Inconsistent(_))
        ));
    }

    #[test]
    fn the_report_says_what_was_written() {
        let file = read(studio("1.0.0"));
        let ws = workspace();
        let decisions = ImportDecisions {
            context_rules: vec![0],
            ..docs("https://docs.example")
        };
        let plan = plan(&file, &ws, &env(), &decisions);
        let report = report(&plan, true);
        assert_eq!(report.surfaces_added, ["development-2"]);
        assert!(report.surfaces_replaced.is_empty());
        assert_eq!((report.rules_added, report.rules_skipped), (1, 0));
        assert_eq!(report.slots_removed, 1);
        assert!(report.auto_context);
        let missing: Vec<_> = report
            .missing_dependencies
            .iter()
            .map(|d| (d.plugin_id.as_str(), &d.status))
            .collect();
        assert_eq!(missing, [("acme.notes", &DependencyStatus::Missing)]);
        assert_eq!(
            plan.effects.removed_slots,
            [Location {
                surface_source_id: "development".into(),
                page_id: "home".into(),
                control_id: "g.0.0".into(),
                slot: Slot::Press,
            }]
        );
    }
}
