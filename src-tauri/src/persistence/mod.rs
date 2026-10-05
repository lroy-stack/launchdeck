//! Versioned JSON persistence. No database.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::core::model::{Workspace, SCHEMA_VERSION};

pub mod bindings;
pub mod legacy;

#[derive(Debug, thiserror::Error)]
pub enum PersistError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("invalid workspace file: {0}")]
    Invalid(String),
    #[error("workspace schema version {found} is newer than this build supports ({supported})")]
    TooNew { found: u32, supported: u32 },
}

/// Upgrade an on-disk document to the current schema.
///
/// * v1 -> v2: adds the Agent Control surface and a button to it on the
///   existing surfaces. Nothing the user made is changed or removed.
/// * v2 -> v3: every control action becomes an assignment
///   (`{provider, action, args}`); see [`legacy`]. Nothing is dropped.
/// * v3 -> v4: every control's `bindings` become `feedback` rules; what cannot
///   be expressed is kept in `legacyBindings`; see [`bindings`]. The values
///   states hold are never part of the file.
pub fn migrate(value: Value) -> Result<Workspace, PersistError> {
    let version = value
        .get("schemaVersion")
        .and_then(Value::as_u64)
        .ok_or_else(|| PersistError::Invalid("missing `schemaVersion`".into()))?
        as u32;
    let parse = |v: Value| -> Result<Workspace, PersistError> {
        serde_json::from_value(v).map_err(|e| PersistError::Invalid(e.to_string()))
    };
    let upgraded = |mut v: Value, from: u32| {
        if from < 3 {
            legacy::upgrade_actions(&mut v);
        }
        if from < 4 {
            let report = bindings::upgrade_bindings(&mut v);
            if report != bindings::Report::default() {
                log::info!("workspace v{from} -> v{SCHEMA_VERSION}: {report:?}");
            }
        }
        v["schemaVersion"] = Value::from(SCHEMA_VERSION);
        v
    };
    match version {
        SCHEMA_VERSION => parse(value),
        2 | 3 => parse(upgraded(value, version)),
        1 => {
            let mut ws = parse(upgraded(value, 1))?;
            crate::surfaces::add_agent_control(&mut ws);
            Ok(ws)
        }
        v if v > SCHEMA_VERSION => Err(PersistError::TooNew {
            found: v,
            supported: SCHEMA_VERSION,
        }),
        v => Err(PersistError::Invalid(format!(
            "unsupported schema version {v}"
        ))),
    }
}

pub struct Store {
    path: PathBuf,
}

impl Store {
    pub fn new(dir: impl AsRef<Path>) -> Self {
        Self {
            path: dir.as_ref().join("workspace.json"),
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Load the workspace, or build the default one. A file that cannot be
    /// read is copied to `workspace.json.bak` (never silently destroyed) and
    /// the defaults are used.
    pub fn load_or_default(&self, default: impl FnOnce() -> Workspace) -> Workspace {
        let Ok(text) = fs::read_to_string(&self.path) else {
            return default();
        };
        let parsed = serde_json::from_str::<Value>(&text)
            .map_err(|e| PersistError::Invalid(e.to_string()))
            .and_then(|doc| {
                let found = doc
                    .get("schemaVersion")
                    .and_then(Value::as_u64)
                    .unwrap_or(0) as u32;
                migrate(doc).map(|ws| (found, ws))
            });
        match parsed {
            Ok((found, ws)) => {
                if found < SCHEMA_VERSION {
                    // Keep the file as it was the first time it is upgraded.
                    let backup = self.path.with_extension(format!("json.v{found}.bak"));
                    if !backup.exists() {
                        let _ = fs::copy(&self.path, backup);
                    }
                }
                ws
            }
            Err(e) => {
                log::warn!(
                    "could not load {}: {e}; keeping a backup and using defaults",
                    self.path.display()
                );
                let _ = fs::copy(&self.path, self.path.with_extension("json.bak"));
                default()
            }
        }
    }

    /// Atomic write: temp file in the same directory, then rename.
    pub fn save(&self, workspace: &Workspace) -> Result<(), PersistError> {
        let json = serde_json::to_string_pretty(workspace)
            .map_err(|e| PersistError::Invalid(e.to_string()))?;
        write_atomic(&self.path, json.as_bytes())?;
        Ok(())
    }
}

/// Write `bytes` to `path` atomically: temp file in the same directory, then
/// rename. Shared by every JSON file LaunchDeck keeps.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    write_file(path, bytes, false)
}

/// Like [`write_atomic`], for a file that holds a secret (the hook token): on
/// Unix it is created readable by the current user only, from the first byte,
/// so there is no window in which it is world-readable. On Windows the per-user
/// app data folder is already restricted to the user by its default ACL.
pub fn write_private(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    write_file(path, bytes, true)
}

fn write_file(path: &Path, bytes: &[u8], private: bool) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".tmp");
    let tmp = PathBuf::from(tmp);
    {
        let mut options = fs::OpenOptions::new();
        options.write(true).create(true).truncate(true);
        #[cfg(unix)]
        if private {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        #[cfg(not(unix))]
        let _ = private;
        let mut f = options.open(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
    }
    fs::rename(&tmp, path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::model::{ImportOrigin, Surface};
    use crate::surfaces::{default_workspace, default_workspace_for, Platform};

    #[test]
    fn workspace_roundtrips_through_disk() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::new(dir.path());
        let mut ws = default_workspace();
        ws.last_active_surface = Some("development".into());
        ws.preferences.onboarding_done = true;
        store.save(&ws).unwrap();
        let loaded = store.load_or_default(|| panic!("should load from disk"));
        assert_eq!(loaded, ws);
        let raw: Value = serde_json::from_str(&fs::read_to_string(store.path()).unwrap()).unwrap();
        assert_eq!(raw["schemaVersion"], 4);
    }

    #[test]
    fn missing_file_yields_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let ws = Store::new(dir.path()).load_or_default(default_workspace);
        assert!(!ws.surfaces.is_empty());
    }

    #[test]
    fn corrupt_file_is_backed_up_not_destroyed() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::new(dir.path());
        fs::write(store.path(), "{ this is not json").unwrap();
        let ws = store.load_or_default(default_workspace);
        assert!(!ws.surfaces.is_empty());
        assert_eq!(
            fs::read_to_string(dir.path().join("workspace.json.bak")).unwrap(),
            "{ this is not json"
        );
    }

    #[test]
    fn newer_schema_is_refused_with_a_clear_error() {
        let v = serde_json::json!({ "schemaVersion": 99, "surfaces": [] });
        assert!(matches!(
            migrate(v),
            Err(PersistError::TooNew {
                found: 99,
                supported: 4
            })
        ));
        assert!(matches!(
            migrate(serde_json::json!({})),
            Err(PersistError::Invalid(_))
        ));
    }

    #[test]
    fn v1_workspaces_gain_agent_control_without_losing_anything() {
        // A v1 file: today's default workspace minus the Agent Control surface.
        let mut ws = default_workspace();
        ws.surfaces.retain(|s| s.id != "agent-control");
        let mut doc = serde_json::to_value(&ws).unwrap();
        doc["schemaVersion"] = serde_json::json!(1);

        let migrated = migrate(doc.clone()).unwrap();
        assert_eq!(migrated.schema_version, SCHEMA_VERSION);
        assert_eq!(
            migrated.surfaces[0].id, "agent-control",
            "the flagship comes first"
        );
        // Every original surface is still there, with all its original controls.
        for original in &ws.surfaces {
            let kept = migrated.surface(&original.id).expect("surface kept");
            for c in &original.pages[0].controls {
                assert!(
                    kept.pages[0].controls.iter().any(|k| k == c),
                    "{} lost",
                    c.label
                );
            }
            // ...plus one switcher to Agent Control in the free top slot.
            let added: Vec<_> = kept.pages[0]
                .controls
                .iter()
                .filter(|k| !original.pages[0].controls.contains(k))
                .collect();
            assert_eq!(added.len(), 1, "{}", original.id);
            assert_eq!(
                added[0].pos,
                crate::core::model::ControlPos::Top { index: 5 }
            );
        }
        // Idempotent: migrating the migrated document changes nothing.
        let mut again = serde_json::to_value(&migrated).unwrap();
        again["schemaVersion"] = serde_json::json!(1);
        assert_eq!(migrate(again).unwrap(), migrated);
    }

    /// A surface's early `context` hint (`foregroundApps`) is written only
    /// where a file already had one: such a file loads and keeps it, and a
    /// surface without one gets no `context` member.
    #[test]
    fn a_surface_with_the_old_context_hint_still_loads_and_keeps_it() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::new(dir.path());
        let hint = serde_json::json!({ "foregroundApps": ["com.apple.Safari", "firefox"] });
        let mut doc = serde_json::to_value(default_workspace()).unwrap();
        for surface in doc["surfaces"].as_array_mut().unwrap() {
            let object = surface.as_object_mut().unwrap();
            object.remove("context");
            if object["id"] == "productivity" {
                object.insert("context".into(), hint.clone());
            }
        }
        fs::write(store.path(), doc.to_string()).unwrap();

        let ws = store.load_or_default(|| panic!("a file with a hint must load"));
        let kept = ws.surface("productivity").unwrap().context.as_ref();
        assert_eq!(
            kept.map(|h| h.foreground_apps.clone()),
            Some(vec!["com.apple.Safari".to_string(), "firefox".to_string()])
        );

        store.save(&ws).unwrap();
        let saved: Value =
            serde_json::from_str(&fs::read_to_string(store.path()).unwrap()).unwrap();
        for surface in saved["surfaces"].as_array().unwrap() {
            match surface["id"].as_str().unwrap() {
                "productivity" => assert_eq!(surface["context"], hint),
                other => assert!(surface.get("context").is_none(), "{other}"),
            }
        }
    }

    fn origin() -> ImportOrigin {
        ImportOrigin {
            format: "launchdeck.pack".into(),
            artifact_id: "acme.studio".into(),
            artifact_name: "Studio".into(),
            artifact_version: "1.2.0".into(),
            instance: 2,
            source_surface_id: "mixer".into(),
            content_digest: format!("sha256:{}", "a".repeat(64)),
            surface_digest: format!("sha256:{}", "b".repeat(64)),
        }
    }

    /// Where an imported surface came from is kept with it, in camelCase
    /// under `importedFrom`, and read back as it was written.
    #[test]
    fn an_imported_surface_keeps_its_origin_through_a_save_and_a_load() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::new(dir.path());
        let mut ws = default_workspace();
        ws.surfaces[1].imported_from = Some(origin());
        store.save(&ws).unwrap();

        let loaded = store.load_or_default(|| panic!("should load from disk"));
        assert_eq!(loaded, ws);
        let raw: Value = serde_json::from_str(&fs::read_to_string(store.path()).unwrap()).unwrap();
        assert_eq!(
            raw["surfaces"][1]["importedFrom"],
            serde_json::json!({
                "format": "launchdeck.pack",
                "artifactId": "acme.studio",
                "artifactName": "Studio",
                "artifactVersion": "1.2.0",
                "instance": 2,
                "sourceSurfaceId": "mixer",
                "contentDigest": format!("sha256:{}", "a".repeat(64)),
                "surfaceDigest": format!("sha256:{}", "b".repeat(64)),
            })
        );
        for (i, surface) in raw["surfaces"].as_array().unwrap().iter().enumerate() {
            assert_eq!(surface.get("importedFrom").is_some(), i == 1, "{i}");
        }
    }

    /// A surface that was not imported has no `importedFrom` member: a
    /// workspace without one is written byte for byte as before (the default
    /// workspace's fixture test holds the bytes), and no schema step is needed.
    #[test]
    fn a_workspace_without_imports_is_written_as_before() {
        let ws = default_workspace();
        assert!(ws.surfaces.iter().all(|s| s.imported_from.is_none()));
        let text = serde_json::to_string_pretty(&ws).unwrap();
        assert!(!text.contains("importedFrom"));
        let mut doc: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(migrate(doc.clone()).unwrap(), ws);
        doc["surfaces"][0]["importedFrom"] = serde_json::to_value(origin()).unwrap();
        let read = migrate(doc).unwrap();
        assert_eq!(read.schema_version, SCHEMA_VERSION);
        assert_eq!(read.surfaces[0].imported_from, Some(origin()));
    }

    #[cfg(unix)]
    #[test]
    fn private_files_are_readable_by_the_owner_only() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("secret.json");
        write_private(&path, b"{}").unwrap();
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        // Replacing it keeps it private.
        write_private(&path, b"{\"a\":1}").unwrap();
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert_eq!(fs::read_to_string(&path).unwrap(), "{\"a\":1}");
    }

    const V2_FIXTURE: &str = include_str!("../../tests/fixtures/workspace-v2.json");

    /// Every control of a document that carries an action, as (surface, control, slot, action).
    fn actions_of(doc: &Value) -> Vec<(String, String, String, Value)> {
        let mut out = Vec::new();
        for s in doc["surfaces"].as_array().unwrap() {
            for p in s["pages"].as_array().unwrap() {
                for c in p["controls"].as_array().unwrap() {
                    for slot in ["press", "longPress"] {
                        if !c[slot].is_null() {
                            out.push((
                                s["id"].as_str().unwrap().to_string(),
                                c["id"].as_str().unwrap().to_string(),
                                slot.to_string(),
                                c[slot].clone(),
                            ));
                        }
                    }
                }
            }
        }
        out
    }

    #[test]
    fn a_real_v2_workspace_opens_in_the_current_schema_with_every_action_kept() {
        let old: Value = serde_json::from_str(V2_FIXTURE).unwrap();
        assert_eq!(old["schemaVersion"], 2);
        let before = actions_of(&old);
        assert!(before.len() > 100, "the fixture exercises many actions");

        let ws = migrate(old.clone()).unwrap();
        assert_eq!(ws.schema_version, SCHEMA_VERSION);
        let new = serde_json::to_value(&ws).unwrap();
        let after = actions_of(&new);
        assert_eq!(before.len(), after.len(), "no action was dropped");
        for ((s, c, slot, was), (s2, c2, slot2, now)) in before.iter().zip(&after) {
            assert_eq!((s, c, slot), (s2, c2, slot2), "same controls, same order");
            let kind = was["type"].as_str().unwrap();
            let expected = match kind {
                "keyboardShortcut" | "openUrl" | "launchApplication" => "system",
                "shellCommand" => "shell",
                "httpRequest" => "http",
                "openSurface" | "openPage" => "navigation",
                "plugin" => was["plugin"].as_str().unwrap(),
                other => panic!("unexpected legacy type {other}"),
            };
            assert_eq!(now["provider"], expected, "{s}/{c}: {was}");
            assert!(now["args"].is_object());
        }
        // Everything else about the controls is untouched.
        let strip = |mut v: Value| {
            for s in v["surfaces"].as_array_mut().unwrap() {
                // A surface without the old context hint is no longer written
                // with `"context": null`; a hint the file had is kept.
                if s["context"].is_null() {
                    s.as_object_mut().unwrap().remove("context");
                }
                for p in s["pages"].as_array_mut().unwrap() {
                    for c in p["controls"].as_array_mut().unwrap() {
                        c.as_object_mut().unwrap().remove("press");
                        c.as_object_mut().unwrap().remove("longPress");
                        // Bindings became feedback rules (checked in `bindings.rs`).
                        c.as_object_mut().unwrap().remove("bindings");
                        c.as_object_mut().unwrap().remove("feedback");
                        c.as_object_mut().unwrap().remove("legacyBindings");
                    }
                }
            }
            v["schemaVersion"] = Value::Null;
            v
        };
        assert_eq!(strip(old), strip(new));
    }

    const V3_FIXTURE: &str = include_str!("../../tests/fixtures/workspace-v3.json");

    /// The strongest check of the conversion: the schema 3 workspace the last
    /// release shipped, migrated by the rules in [`bindings`], is the same
    /// workspace the code builds by hand today, control by control. The file
    /// was saved on Linux, so it is compared with the Linux starter pads.
    #[test]
    fn a_real_v3_workspace_migrates_to_exactly_what_ships_now() {
        let old: Value = serde_json::from_str(V3_FIXTURE).unwrap();
        assert_eq!(old["schemaVersion"], 3);
        let ws = migrate(old).unwrap();
        assert_eq!(ws.schema_version, SCHEMA_VERSION);
        let shipped = default_workspace_for(Platform::Linux);
        assert_eq!(ws.surfaces.len(), shipped.surfaces.len());
        for (a, b) in ws.surfaces.iter().zip(&shipped.surfaces) {
            // The file keeps the old context hint it was saved with; the
            // starters no longer write one. It is compared on its own below.
            let a = Surface {
                context: None,
                ..a.clone()
            };
            assert_eq!(&a, b, "surface `{}` differs after migration", b.id);
        }
        let hint = ws.surface("development").and_then(|s| s.context.as_ref());
        assert_eq!(
            hint.map(|h| h.foreground_apps.clone()),
            Some(vec!["com.microsoft.VSCode".to_string(), "Code".to_string()]),
            "the hint the file had is kept"
        );
        assert!(ws
            .surfaces
            .iter()
            .flat_map(|s| &s.pages)
            .flat_map(|p| &p.controls)
            .all(|c| c.legacy_bindings.is_empty()));
    }

    #[test]
    fn a_v3_file_is_backed_up_once_and_loads_as_v4() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::new(dir.path());
        fs::write(store.path(), V3_FIXTURE).unwrap();
        let ws = store.load_or_default(|| panic!("should migrate"));
        assert_eq!(ws.schema_version, 4);
        let backup = dir.path().join("workspace.json.v3.bak");
        assert_eq!(fs::read_to_string(&backup).unwrap(), V3_FIXTURE);
        store.save(&ws).unwrap();
        let on_disk: Value =
            serde_json::from_str(&fs::read_to_string(store.path()).unwrap()).unwrap();
        assert_eq!(on_disk["schemaVersion"], 4);
        // The file holds rules, never a binding, a value or a frame.
        let text = on_disk.to_string();
        assert!(!text.contains("\"bindings\""));
        assert!(text.contains("\"feedback\""));
        assert!(!text.contains("\"updatedAt\""));
        // A second load keeps the first backup and reads the new file as it is.
        let again = store.load_or_default(|| panic!("should load"));
        assert_eq!(again, ws);
        assert_eq!(fs::read_to_string(&backup).unwrap(), V3_FIXTURE);
    }

    #[test]
    fn a_binding_that_cannot_be_converted_survives_a_save_and_a_load() {
        let mut doc: Value = serde_json::from_str(V3_FIXTURE).unwrap();
        let odd = serde_json::json!({ "event": "x", "when": [{ "field": "a", "equals": [1, 2] }],
                                       "setState": "y" });
        let binding = serde_json::json!({ "source": "ci", "rules": [odd] });
        doc["surfaces"][1]["pages"][0]["controls"][0]["bindings"] =
            serde_json::json!([binding.clone()]);
        let ws = migrate(doc).unwrap();
        let control = &ws.surfaces[1].pages[0].controls[0];
        assert!(control.feedback.is_empty());
        assert_eq!(control.legacy_bindings, vec![binding.clone()]);

        let dir = tempfile::tempdir().unwrap();
        let store = Store::new(dir.path());
        store.save(&ws).unwrap();
        let loaded = store.load_or_default(|| panic!("should load"));
        assert_eq!(
            loaded.surfaces[1].pages[0].controls[0].legacy_bindings,
            vec![binding]
        );
        assert_eq!(loaded, ws);
    }

    #[test]
    fn every_binding_of_the_v2_fixture_is_converted_or_kept() {
        let old: Value = serde_json::from_str(V2_FIXTURE).unwrap();
        let count = |doc: &Value, key: &str| -> usize {
            doc["surfaces"]
                .as_array()
                .unwrap()
                .iter()
                .flat_map(|s| s["pages"].as_array().unwrap())
                .flat_map(|p| p["controls"].as_array().unwrap())
                .map(|c| c[key].as_array().map_or(0, Vec::len))
                .sum()
        };
        assert_eq!(count(&old, "feedback"), 0);
        let before = count(&old, "bindings");
        assert!(before > 20, "the fixture has bindings");
        let ws = migrate(old).unwrap();
        let new = serde_json::to_value(&ws).unwrap();
        assert_eq!(count(&new, "bindings"), 0);
        assert!(count(&new, "feedback") > 0);
        assert_eq!(count(&new, "legacyBindings"), 0);
    }

    #[test]
    fn migrated_actions_run_against_the_real_providers() {
        use crate::actions::registry::ActionRegistry;
        use crate::plugins::demo::DemoPlugin;
        use crate::plugins::keyboard::{Combo, Keyboard};
        use crate::plugins::system::SystemPlugin;
        use crate::plugins::{ActionError, PluginRegistry};
        use std::sync::Arc;

        struct Silent;
        impl Keyboard for Silent {
            fn press(&self, _: &Combo) -> Result<(), ActionError> {
                Ok(())
            }
        }
        let plugins = PluginRegistry::default();
        plugins.register(Arc::new(crate::actions::builtin::NavigationProvider));
        plugins.register(Arc::new(crate::actions::builtin::ShellProvider));
        plugins.register(Arc::new(crate::actions::builtin::HttpProvider));
        plugins.register(Arc::new(SystemPlugin::new(Arc::new(Silent))));
        plugins.register(Arc::new(DemoPlugin::new()));
        let registry = ActionRegistry::new(plugins);

        let ws = migrate(serde_json::from_str(V2_FIXTURE).unwrap()).unwrap();
        let mut checked = 0;
        for s in &ws.surfaces {
            for p in &s.pages {
                for c in &p.controls {
                    for a in [&c.press, &c.long_press].into_iter().flatten() {
                        // Agent Control is not registered in this test; everything else must validate.
                        if a.provider == "agent-control" {
                            continue;
                        }
                        registry
                            .prepare(a)
                            .unwrap_or_else(|e| panic!("{}/{}: {a:?}: {e}", s.id, c.label));
                        checked += 1;
                    }
                }
            }
        }
        assert!(checked > 60, "checked {checked}");
    }

    #[test]
    fn the_first_upgrade_keeps_the_old_file_next_to_the_new_one() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::new(dir.path());
        fs::write(store.path(), V2_FIXTURE).unwrap();
        let ws = store.load_or_default(|| panic!("should migrate"));
        assert_eq!(ws.schema_version, SCHEMA_VERSION);
        let backup = dir.path().join("workspace.json.v2.bak");
        assert_eq!(fs::read_to_string(&backup).unwrap(), V2_FIXTURE);
        // Saving writes the current version and does not touch the backup.
        store.save(&ws).unwrap();
        let again = store.load_or_default(|| panic!("should load"));
        assert_eq!(again, ws);
        assert_eq!(fs::read_to_string(&backup).unwrap(), V2_FIXTURE);
    }

    #[test]
    fn a_workspace_with_a_removed_provider_still_loads_and_keeps_its_assignment() {
        let mut doc = serde_json::to_value(default_workspace()).unwrap();
        doc["surfaces"][1]["pages"][0]["controls"][0]["press"] =
            serde_json::json!({ "provider": "acme.gone", "action": "do", "args": { "x": 1 } });
        let ws = migrate(doc).unwrap();
        let a = ws.surfaces[1].pages[0].controls[0].press.as_ref().unwrap();
        assert_eq!(
            (a.provider.as_str(), a.action.as_str()),
            ("acme.gone", "do")
        );
        assert_eq!(a.args["x"], 1);
    }
}
