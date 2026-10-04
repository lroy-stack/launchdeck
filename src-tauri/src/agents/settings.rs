//! Agent Control settings (`agents.json`): access grants, notification
//! preference and the local hook endpoint configuration.
//!
//! Stored next to `workspace.json` with the same atomic-write approach. No
//! database, no prompts, no transcripts.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::model::AccessLevel;
use crate::persistence::write_private;

pub const SETTINGS_VERSION: u32 = 1;
pub const DEFAULT_HOOK_PORT: u16 = 47822;
pub const DEFAULT_HOLD_SECS: u64 = 90;

/// Configuration of the local HTTP endpoint that hook-based harnesses call.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct HooksSettings {
    /// The endpoint only listens when the user turns it on.
    pub enabled: bool,
    pub port: u16,
    /// Shared secret the harness must present as a bearer token. Generated
    /// the first time the endpoint is enabled.
    pub token: String,
    /// Per hook adapter id: whether its route is served. Missing means yes.
    pub harnesses: BTreeMap<String, bool>,
    /// How long a permission request is held waiting for a human before the
    /// harness is told "no decision" and shows its own prompt.
    pub hold_secs: u64,
}

impl HooksSettings {
    pub fn harness_enabled(&self, adapter_id: &str) -> bool {
        self.harnesses.get(adapter_id).copied().unwrap_or(true)
    }
}

impl Default for HooksSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            port: DEFAULT_HOOK_PORT,
            token: String::new(),
            harnesses: BTreeMap::new(),
            hold_secs: DEFAULT_HOLD_SECS,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    pub schema_version: u32,
    /// Agent id to the access the user granted. Missing means the adapter's
    /// default (`monitor` for everything that is not simulated).
    pub access: BTreeMap<String, AccessLevel>,
    pub notifications: bool,
    pub hooks: HooksSettings,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            schema_version: SETTINGS_VERSION,
            access: BTreeMap::new(),
            notifications: true,
            hooks: HooksSettings::default(),
        }
    }
}

/// A partial update from the desktop UI. Everything is optional; anything
/// present is validated before it is stored.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsPatch {
    pub notifications: Option<bool>,
    pub hooks_enabled: Option<bool>,
    pub hooks_port: Option<u16>,
    pub hooks_harnesses: Option<BTreeMap<String, bool>>,
    pub hooks_hold_secs: Option<u64>,
}

impl SettingsPatch {
    /// Whether the hook endpoint has to be restarted to honour this patch.
    pub fn touches_hooks(&self) -> bool {
        self.hooks_enabled.is_some()
            || self.hooks_port.is_some()
            || self.hooks_harnesses.is_some()
            || self.hooks_hold_secs.is_some()
    }

    pub fn validate(&self) -> Result<(), String> {
        if let Some(port) = self.hooks_port {
            if port < 1024 {
                return Err("Use a port from 1024 to 65535.".into());
            }
        }
        if let Some(secs) = self.hooks_hold_secs {
            if !(5..=600).contains(&secs) {
                return Err("The hold time must be between 5 and 600 seconds.".into());
            }
        }
        if let Some(map) = &self.hooks_harnesses {
            if map.len() > 16 || map.keys().any(|k| !super::model::valid_id(k)) {
                return Err("Unknown harness list.".into());
            }
        }
        Ok(())
    }

    pub fn apply(self, s: &mut Settings) {
        if let Some(v) = self.notifications {
            s.notifications = v;
        }
        if let Some(v) = self.hooks_enabled {
            s.hooks.enabled = v;
        }
        if let Some(v) = self.hooks_port {
            s.hooks.port = v;
        }
        if let Some(v) = self.hooks_harnesses {
            s.hooks.harnesses = v;
        }
        if let Some(v) = self.hooks_hold_secs {
            s.hooks.hold_secs = v;
        }
    }
}

/// Where Agent Control keeps its files.
#[derive(Debug, Clone)]
pub struct AgentsStorage {
    dir: PathBuf,
}

impl AgentsStorage {
    pub fn new(dir: impl AsRef<Path>) -> Self {
        Self {
            dir: dir.as_ref().to_path_buf(),
        }
    }

    pub fn settings_path(&self) -> PathBuf {
        self.dir.join("agents.json")
    }

    pub fn audit_path(&self) -> PathBuf {
        self.dir.join("audit.jsonl")
    }

    /// Missing or unreadable files yield defaults. A damaged file is copied to
    /// `agents.json.bak` first, never silently destroyed.
    pub fn load(&self) -> Settings {
        let path = self.settings_path();
        let Ok(text) = fs::read_to_string(&path) else {
            return Settings::default();
        };
        match serde_json::from_str::<Settings>(&text) {
            Ok(s) if s.schema_version <= SETTINGS_VERSION => s,
            Ok(s) => {
                log::warn!(
                    "agents.json schema {} is newer than this build supports; using defaults",
                    s.schema_version
                );
                Settings::default()
            }
            Err(e) => {
                log::warn!("could not read agents.json: {e}; keeping a backup");
                let _ = fs::copy(&path, path.with_extension("json.bak"));
                Settings::default()
            }
        }
    }

    pub fn save(&self, settings: &Settings) -> std::io::Result<()> {
        let json = serde_json::to_string_pretty(settings).map_err(std::io::Error::other)?;
        write_private(&self.settings_path(), json.as_bytes())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn patches_are_validated_before_they_are_applied() {
        let bad_port = SettingsPatch {
            hooks_port: Some(80),
            ..Default::default()
        };
        assert!(bad_port.validate().is_err());
        let bad_hold = SettingsPatch {
            hooks_hold_secs: Some(1),
            ..Default::default()
        };
        assert!(bad_hold.validate().is_err());
        let bad_harness = SettingsPatch {
            hooks_harnesses: Some([("has space".to_string(), true)].into()),
            ..Default::default()
        };
        assert!(bad_harness.validate().is_err());

        let ok = SettingsPatch {
            notifications: Some(false),
            hooks_enabled: Some(true),
            hooks_port: Some(50000),
            hooks_harnesses: Some([("codex".to_string(), false)].into()),
            hooks_hold_secs: Some(30),
        };
        assert!(ok.validate().is_ok() && ok.touches_hooks());
        let mut s = Settings::default();
        ok.apply(&mut s);
        assert!(!s.notifications && s.hooks.enabled && s.hooks.port == 50000);
        assert!(!s.hooks.harness_enabled("codex") && s.hooks.harness_enabled("claude-code"));
        assert!(!SettingsPatch {
            notifications: Some(true),
            ..Default::default()
        }
        .touches_hooks());
    }

    #[test]
    fn defaults_are_safe() {
        let s = Settings::default();
        assert!(!s.hooks.enabled, "the hook endpoint is opt-in");
        assert!(s.hooks.token.is_empty());
        assert!(s.access.is_empty(), "nobody has control by default");
        assert!(s.notifications);
    }

    #[test]
    fn settings_roundtrip_and_tolerate_missing_fields() {
        let dir = tempfile::tempdir().unwrap();
        let storage = AgentsStorage::new(dir.path());
        let mut s = Settings::default();
        s.access.insert("a".into(), AccessLevel::Control);
        storage.save(&s).unwrap();
        assert_eq!(storage.load(), s);

        fs::write(storage.settings_path(), r#"{"notifications":false}"#).unwrap();
        let loaded = storage.load();
        assert!(!loaded.notifications);
        assert_eq!(loaded.hooks.port, DEFAULT_HOOK_PORT);
    }

    #[test]
    fn a_damaged_file_is_backed_up_and_defaults_are_used() {
        let dir = tempfile::tempdir().unwrap();
        let storage = AgentsStorage::new(dir.path());
        fs::write(storage.settings_path(), "{ nope").unwrap();
        assert_eq!(storage.load(), Settings::default());
        assert_eq!(
            fs::read_to_string(dir.path().join("agents.json.bak")).unwrap(),
            "{ nope"
        );
    }
}
