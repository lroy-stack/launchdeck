//! Apply on whole files and workspaces: what is written where, what is
//! left empty or as it was, and what the runtime's checks refuse.

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
