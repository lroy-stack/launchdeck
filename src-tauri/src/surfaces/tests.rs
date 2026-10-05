//! Tests of the starter surfaces and the starter workspace, including the
//! guard of the `examples/default-workspace.json` contract fixture.

use super::*;
use crate::core::model::{ControlPos, Span, Workspace, SCHEMA_VERSION};
use serde_json::json;
use std::collections::HashSet;

#[test]
fn starter_workspace_has_the_documented_surfaces() {
    let ws = default_workspace();
    let ids: Vec<&str> = ws.surfaces.iter().map(|s| s.id.as_str()).collect();
    for expected in [
        "agent-control",
        "development",
        "productivity",
        "smart-home",
        "accessibility",
    ] {
        assert!(ids.contains(&expected), "missing {expected}");
    }
    assert_eq!(ws.schema_version, SCHEMA_VERSION);
}

const SYSTEMS: [Platform; 3] = [Platform::Linux, Platform::MacOs, Platform::Windows];

/// The names people see, in the order the first run and the sidebar list
/// them. The smart-home starter is called Home; its id stays.
#[test]
fn the_starters_are_named_for_people() {
    for platform in SYSTEMS {
        let names: Vec<String> = starter_surfaces(platform)
            .into_iter()
            .map(|s| s.name)
            .collect();
        assert_eq!(
            names,
            [
                "Agent Control",
                "Development",
                "Productivity",
                "Home",
                "Accessibility"
            ],
            "{platform:?}"
        );
    }
}

/// Every starter says what it is for: the first run and the Overview show
/// this text and have none of their own.
#[test]
fn every_starter_describes_itself() {
    for s in starter_surfaces(Platform::current()) {
        assert!(!s.description.trim().is_empty(), "{}", s.id);
        assert!(s.icon.is_some(), "{}", s.id);
    }
}

#[test]
fn every_page_is_well_formed() {
    for s in SYSTEMS.into_iter().flat_map(starter_surfaces) {
        assert!(!s.pages.is_empty(), "{} has no pages", s.id);
        for p in &s.pages {
            let mut seen_cells = HashSet::new();
            let mut seen_ids = HashSet::new();
            for c in &p.controls {
                assert!(
                    seen_ids.insert(c.id.clone()),
                    "{}/{}: duplicate control id {}",
                    s.id,
                    p.id,
                    c.id
                );
                if let ControlPos::Grid { .. } = c.pos {
                    for cell in c.cells() {
                        assert!(
                            cell.0 < 8 && cell.1 < 8,
                            "{}/{}: {} outside 8x8",
                            s.id,
                            p.id,
                            c.label
                        );
                        assert!(
                            seen_cells.insert(cell),
                            "{}/{}: overlapping cell {:?}",
                            s.id,
                            p.id,
                            cell
                        );
                    }
                }
                assert!(
                    c.press.is_some(),
                    "{}/{}: {} has no action",
                    s.id,
                    p.id,
                    c.label
                );
            }
        }
    }
}

#[test]
fn navigation_targets_exist() {
    let ws = default_workspace();
    for s in &ws.surfaces {
        for p in &s.pages {
            for c in &p.controls {
                let a = c.press.as_ref().unwrap();
                if a.provider != "navigation" {
                    continue;
                }
                let arg = |k: &str| a.args.get(k).and_then(|v| v.as_str());
                match a.action.as_str() {
                    "openSurface" => {
                        let surface_id = arg("surfaceId").unwrap();
                        let target = ws
                            .surface(surface_id)
                            .unwrap_or_else(|| panic!("{surface_id} missing"));
                        if let Some(pid) = arg("pageId") {
                            assert!(target.page(pid).is_some());
                        }
                    }
                    "openPage" => {
                        let page_id = arg("pageId").unwrap();
                        assert!(
                            s.page(page_id).is_some(),
                            "{}: page {page_id} missing",
                            s.id
                        );
                    }
                    other => panic!("unknown navigation action {other}"),
                }
            }
        }
    }
}

#[test]
fn accessibility_surface_uses_large_targets_and_few_controls() {
    let s = default_workspace()
        .surfaces
        .into_iter()
        .find(|s| s.id == "accessibility")
        .unwrap();
    let home = s.page("home").unwrap();
    let grid: Vec<_> = home
        .controls
        .iter()
        .filter(|c| matches!(c.pos, ControlPos::Grid { .. }))
        .collect();
    assert_eq!(grid.len(), 12);
    assert!(grid.iter().all(|c| c.span == Span { w: 2, h: 2 }));
}

/// Auto Context is ordered rules in the workspace; the early per-surface hint
/// is only read from old files, never written for a starter. A new
/// workspace has no rule and Auto Context off: nothing switches surfaces
/// until the person asks for it.
#[test]
fn no_starter_carries_context_rules_or_the_old_context_hint() {
    for platform in SYSTEMS {
        let ws = default_workspace_for(platform);
        assert!(ws.context.rules.is_empty(), "{platform:?}");
        assert!(!ws.context.auto, "{platform:?}");
        assert!(ws.context.is_default(), "{platform:?}");
        let json = serde_json::to_value(&ws).unwrap();
        assert!(json.get("context").is_none(), "{platform:?}");
        for (surface, written) in ws.surfaces.iter().zip(json["surfaces"].as_array().unwrap()) {
            assert_eq!(surface.context, None, "{platform:?}: {}", surface.id);
            assert!(
                written.get("context").is_none(),
                "{platform:?}: {}",
                surface.id
            );
        }
    }
}

/// The Home starter switches lights and switches through the Home Assistant
/// example plugin. Until that plugin is installed and configured its pads
/// read as needing it, and nothing is sent anywhere: no starter pad holds a
/// network address.
#[test]
fn the_home_starter_uses_the_home_assistant_example_plugin() {
    let ws = default_workspace();
    let home = ws.surface("smart-home").unwrap();
    assert_eq!(home.name, "Home");
    assert_eq!(home.category, "smart-home");
    assert!(home.description.contains("Home Assistant example plugin"));
    let pads: Vec<_> = home
        .pages
        .iter()
        .flat_map(|p| &p.controls)
        .filter(|c| matches!(c.pos, ControlPos::Grid { .. }))
        .collect();
    let entities: Vec<&str> = pads
        .iter()
        .map(|c| {
            let a = c.press.as_ref().unwrap();
            assert_eq!(
                (a.provider.as_str(), a.action.as_str()),
                ("example.home-assistant", "toggle"),
                "{}",
                c.label
            );
            assert_eq!(
                c.description, "Example entity: set your own in the inspector.",
                "{}",
                c.label
            );
            a.args["entity"].as_str().unwrap()
        })
        .collect();
    assert_eq!(
        entities,
        [
            "light.living_room",
            "light.kitchen",
            "light.desk",
            "light.porch",
            "switch.bedroom_fan",
            "switch.coffee_machine",
        ]
    );
    // The plugin switches these domains only (examples/plugins/home-assistant).
    assert!(entities
        .iter()
        .all(|e| e.starts_with("light.") || e.starts_with("switch.")));
}

#[test]
fn no_starter_pad_on_the_home_surface_talks_to_the_network() {
    for platform in SYSTEMS {
        let ws = default_workspace_for(platform);
        let home = ws.surface("smart-home").unwrap();
        let text = serde_json::to_string(home).unwrap();
        assert!(!text.contains("http"), "{platform:?}");
        assert!(home
            .pages
            .iter()
            .flat_map(|p| &p.controls)
            .filter_map(|c| c.press.as_ref())
            .all(|a| a.provider != "http"));
    }
}

#[test]
fn blank_surface_is_valid() {
    let s = blank_surface("mine", "Mine");
    assert_eq!(s.pages.len(), 1);
    assert_eq!(s.category, "custom");
}

/// `examples/default-workspace.json` is the contract fixture the TypeScript
/// schemas are tested against. It is the Linux workspace, whichever system
/// runs this. Refresh it with:
/// `cargo run --example dump_workspace > ../examples/default-workspace.json`
#[test]
fn contract_fixture_matches_the_default_workspace() {
    let fixture: serde_json::Value =
        serde_json::from_str(include_str!("../../../examples/default-workspace.json")).unwrap();
    assert_eq!(
        fixture,
        serde_json::to_value(default_workspace_for(Platform::Linux)).unwrap(),
        "examples/default-workspace.json is stale; regenerate it with the dump_workspace example"
    );
}

/// `surface/page/label` of every control that is not the same in both.
fn differing_pads(a: &Workspace, b: &Workspace) -> Vec<String> {
    assert_eq!(a.surfaces.len(), b.surfaces.len());
    let mut differ = Vec::new();
    for (sa, sb) in a.surfaces.iter().zip(&b.surfaces) {
        assert_eq!(sa.pages.len(), sb.pages.len(), "{}", sa.id);
        for (pa, pb) in sa.pages.iter().zip(&sb.pages) {
            assert_eq!(pa.controls.len(), pb.controls.len(), "{}/{}", sa.id, pa.id);
            for (ca, cb) in pa.controls.iter().zip(&pb.controls) {
                if ca != cb {
                    differ.push(format!("{}/{}/{}", sa.id, pa.id, ca.label));
                }
            }
        }
    }
    differ
}

/// What pressing a starter pad does: `provider.action` and its settings.
fn press(ws: &Workspace, surface: &str, page: &str, label: &str) -> (String, serde_json::Value) {
    let control = ws
        .surface(surface)
        .and_then(|s| s.page(page))
        .and_then(|p| p.controls.iter().find(|c| c.label == label))
        .unwrap_or_else(|| panic!("no pad {surface}/{page}/{label}"));
    let a = control.press.as_ref().unwrap();
    (format!("{}.{}", a.provider, a.action), a.args.clone())
}

#[test]
fn the_systems_differ_only_in_the_pads_that_must() {
    let linux = default_workspace_for(Platform::Linux);
    assert_eq!(
        differing_pads(&linux, &default_workspace_for(Platform::MacOs)),
        [
            "development/home/Terminal",
            "development/home/VS Code",
            "productivity/home/Files",
            "productivity/home/Screenshot",
        ]
    );
    // On Windows the demo commands wait with `ping` instead of `sleep`.
    assert_eq!(
        differing_pads(&linux, &default_workspace_for(Platform::Windows)),
        [
            "development/home/Terminal",
            "development/home/Docker",
            "development/home/Tests",
            "development/home/Build",
            "development/home/Dev Server",
            "development/home/Deploy",
            "development/git/Git Status",
            "development/git/Commit",
            "development/git/Pull",
            "development/git/Push",
            "productivity/home/Files",
            "productivity/home/Screenshot",
            "productivity/home/Do Not Disturb",
        ]
    );
}

#[test]
fn on_linux_the_starter_pads_use_common_desktop_commands() {
    let ws = default_workspace_for(Platform::Linux);
    let app = |name: &str| {
        (
            "system.launchApplication".to_string(),
            json!({ "app": name }),
        )
    };
    assert_eq!(
        press(&ws, "development", "home", "Terminal"),
        app("x-terminal-emulator")
    );
    assert_eq!(press(&ws, "development", "home", "VS Code"), app("code"));
    assert_eq!(
        press(&ws, "productivity", "home", "Files"),
        (
            "system.launchApplication".into(),
            json!({ "app": "xdg-open", "args": "." })
        )
    );
    assert_eq!(
        press(&ws, "productivity", "home", "Screenshot"),
        (
            "shell.execute".into(),
            json!({ "command": "gnome-screenshot --interactive" })
        )
    );
    assert_eq!(
        press(&ws, "development", "home", "Tests"),
        (
            "shell.execute".into(),
            json!({ "command": "sleep 2 && echo 'tests passed'" })
        )
    );
}

#[test]
fn on_macos_the_starter_pads_name_applications_and_use_cmd() {
    let ws = default_workspace_for(Platform::MacOs);
    let app = |name: &str| {
        (
            "system.launchApplication".to_string(),
            json!({ "app": name }),
        )
    };
    assert_eq!(
        press(&ws, "development", "home", "Terminal"),
        app("Terminal")
    );
    assert_eq!(
        press(&ws, "development", "home", "VS Code"),
        app("Visual Studio Code")
    );
    assert_eq!(press(&ws, "productivity", "home", "Files"), app("Finder"));
    assert_eq!(
        press(&ws, "productivity", "home", "Screenshot"),
        (
            "system.keyboardShortcut".into(),
            json!({ "keys": "cmd+shift+4" })
        )
    );
    assert_eq!(
        press(&ws, "development", "home", "Tests"),
        (
            "shell.execute".into(),
            json!({ "command": "sleep 2 && echo 'tests passed'" })
        )
    );
}

#[test]
fn on_windows_the_starter_pads_use_windows_programs_and_ping_to_wait() {
    let ws = default_workspace_for(Platform::Windows);
    let app = |name: &str| {
        (
            "system.launchApplication".to_string(),
            json!({ "app": name }),
        )
    };
    assert_eq!(press(&ws, "development", "home", "Terminal"), app("wt"));
    assert_eq!(press(&ws, "development", "home", "VS Code"), app("code"));
    assert_eq!(press(&ws, "productivity", "home", "Files"), app("explorer"));
    assert_eq!(
        press(&ws, "productivity", "home", "Screenshot"),
        (
            "system.keyboardShortcut".into(),
            json!({ "keys": "win+shift+s" })
        )
    );
    assert_eq!(
        press(&ws, "development", "home", "Tests"),
        (
            "shell.execute".into(),
            json!({ "command": "ping -n 3 127.0.0.1 >nul & echo tests passed" })
        )
    );
}

#[test]
fn the_default_workspace_is_the_one_for_this_system() {
    assert_eq!(
        default_workspace(),
        default_workspace_for(Platform::current())
    );
}
