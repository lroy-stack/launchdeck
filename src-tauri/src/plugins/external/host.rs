//! `PluginHost`: the lifecycle of installed external plugins.
//!
//! It scans plugin folders, remembers per plugin whether it is enabled and what
//! its configuration is (`plugins.json`, owner-only), loads plugins in the
//! sandbox and registers them in the shared [`PluginRegistry`]. Registering is
//! all it takes for the action engine to see a plugin; nothing else in
//! LaunchDeck knows this module exists.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use super::discovery::{self, Candidate, Issue};
use super::log::{Level, LogEntry, PluginLog};
use super::manifest::ConfigType;
use super::sandbox::Limits;
use super::wasm_plugin::{effective_config, WasmPlugin};
use super::PluginStatus;
use crate::persistence::write_private;
use crate::plugins::{Plugin, PluginManifest, PluginRegistry};

const STATE_SCHEMA: u32 = 1;

#[derive(Debug, thiserror::Error, Clone, PartialEq, Eq)]
pub enum HostError {
    #[error("no external plugin `{0}`")]
    Unknown(String),
    #[error("{0}")]
    Invalid(String),
    #[error("could not save plugin settings: {0}")]
    Storage(String),
}

/// What the person chose for one plugin.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
struct Settings {
    #[serde(default = "yes")]
    enabled: bool,
    #[serde(default)]
    config: Map<String, Value>,
}

fn yes() -> bool {
    true
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            enabled: true,
            config: Map::new(),
        }
    }
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SavedState {
    #[serde(default)]
    schema_version: u32,
    #[serde(default)]
    plugins: BTreeMap<String, Settings>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ConfigView {
    pub key: String,
    #[serde(rename = "type")]
    pub kind: ConfigType,
    pub description: String,
    pub secret: bool,
    /// The current value. Always absent for secrets.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<Value>,
    /// Whether a value is stored (the only thing shown about a secret).
    pub is_set: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default: Option<Value>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PluginInfo {
    pub plugin: PluginManifest,
    pub dir: String,
    /// What the person chose; `plugin.external.status` is what actually happened.
    pub enabled: bool,
    pub config: Vec<ConfigView>,
}

/// Everything the Plugins screen needs in one read.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PluginsOverview {
    pub plugins: Vec<PluginInfo>,
    /// Folders that held something that is not a usable plugin, and why.
    pub issues: Vec<Issue>,
    /// Where plugins are searched, in priority order.
    pub folders: Vec<String>,
}

#[derive(Debug, Default, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ScanReport {
    pub loaded: usize,
    pub reloaded: usize,
    pub unchanged: usize,
    pub removed: usize,
    pub issues: usize,
}

struct Data {
    plugins: BTreeMap<String, Arc<WasmPlugin>>,
    logs: HashMap<String, Arc<PluginLog>>,
    issues: Vec<Issue>,
    saved: SavedState,
}

struct Inner {
    roots: Vec<PathBuf>,
    state_path: Option<PathBuf>,
    registry: PluginRegistry,
    limits: Limits,
    data: Mutex<Data>,
}

#[derive(Clone)]
pub struct PluginHost {
    inner: Arc<Inner>,
}

fn load_state(path: &Path) -> SavedState {
    let Ok(text) = std::fs::read_to_string(path) else {
        return SavedState::default();
    };
    match serde_json::from_str::<SavedState>(&text) {
        Ok(s) if s.schema_version <= STATE_SCHEMA => s,
        _ => {
            // Damaged or written by a newer LaunchDeck: keep it aside, start clean.
            let mut bak = path.as_os_str().to_owned();
            bak.push(".bak");
            let _ = std::fs::copy(path, PathBuf::from(bak));
            log::warn!("plugins.json could not be used; starting with defaults");
            SavedState::default()
        }
    }
}

impl PluginHost {
    pub fn new(
        roots: Vec<PathBuf>,
        state_path: Option<PathBuf>,
        registry: PluginRegistry,
        limits: Limits,
    ) -> PluginHost {
        let saved = state_path.as_deref().map(load_state).unwrap_or_default();
        PluginHost {
            inner: Arc::new(Inner {
                roots,
                state_path,
                registry,
                limits,
                data: Mutex::new(Data {
                    plugins: BTreeMap::new(),
                    logs: HashMap::new(),
                    issues: Vec::new(),
                    saved,
                }),
            }),
        }
    }

    pub fn roots(&self) -> &[PathBuf] {
        &self.inner.roots
    }

    fn data(&self) -> MutexGuard<'_, Data> {
        self.inner.data.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn persist(&self, saved: &SavedState) -> Result<(), HostError> {
        let Some(path) = &self.inner.state_path else {
            return Ok(());
        };
        let mut out = saved.clone();
        out.schema_version = STATE_SCHEMA;
        let bytes =
            serde_json::to_vec_pretty(&out).map_err(|e| HostError::Storage(e.to_string()))?;
        write_private(path, &bytes).map_err(|e| HostError::Storage(e.to_string()))
    }

    /// Build the plugin for a candidate, start it if it is enabled, and put it
    /// in the registry (replacing any earlier version of it).
    fn install(&self, data: &mut Data, cand: Candidate) {
        let id = cand.manifest.id.clone();
        let log = data
            .logs
            .entry(id.clone())
            .or_insert_with(|| Arc::new(PluginLog::new(&id)))
            .clone();
        let settings = data.saved.plugins.get(&id).cloned().unwrap_or_default();
        let fields = cand.manifest.config.clone();
        let plugin = WasmPlugin::new(
            cand.manifest,
            cand.dir,
            cand.fingerprint,
            log.clone(),
            self.inner.limits.clone(),
        );
        if settings.enabled {
            plugin.activate(&cand.wasm, &effective_config(&fields, &settings.config));
        } else {
            log.host(Level::Info, "installed but disabled");
        }
        let arc: Arc<dyn Plugin> = plugin.clone();
        self.inner.registry.register(arc);
        data.plugins.insert(id, plugin);
    }

    /// Re-read one plugin's folder and start it again with current settings.
    fn reload_locked(&self, data: &mut Data, id: &str) -> Result<(), HostError> {
        let existing = data
            .plugins
            .get(id)
            .cloned()
            .ok_or_else(|| HostError::Unknown(id.to_string()))?;
        match discovery::load_dir(existing.dir()) {
            Ok(cand) if cand.manifest.id == id => self.install(data, cand),
            Ok(cand) => {
                let msg = format!(
                    "the folder now holds `{}`; run a rescan to pick up the change",
                    cand.manifest.id
                );
                existing.mark_error(&msg);
            }
            Err(msg) => existing.mark_error(&msg),
        }
        Ok(())
    }

    /// Find plugins on disk and reconcile them with what is loaded. Unchanged
    /// plugins keep running (and keep their state); changed ones restart;
    /// vanished ones are removed. Plugins stopped by a failure get another try.
    pub fn scan(&self) -> ScanReport {
        let found = discovery::scan(&self.inner.roots);
        let mut data = self.data();
        let mut report = ScanReport {
            issues: found.issues.len(),
            ..ScanReport::default()
        };
        data.issues = found.issues;
        let mut seen = HashSet::new();
        for cand in found.candidates {
            let id = cand.manifest.id.clone();
            seen.insert(id.clone());
            let existing = data.plugins.get(&id).cloned();
            let wanted = data
                .saved
                .plugins
                .get(&id)
                .map(|s| s.enabled)
                .unwrap_or(true);
            match existing {
                Some(p)
                    if p.fingerprint() == cand.fingerprint
                        && p.dir() == &cand.dir
                        && !(wanted && p.status().0 == PluginStatus::Error) =>
                {
                    report.unchanged += 1;
                }
                Some(_) => {
                    self.install(&mut data, cand);
                    report.reloaded += 1;
                }
                None => {
                    self.install(&mut data, cand);
                    report.loaded += 1;
                }
            }
        }
        let gone: Vec<String> = data
            .plugins
            .keys()
            .filter(|id| !seen.contains(*id))
            .cloned()
            .collect();
        for id in gone {
            self.inner.registry.unregister(&id);
            data.plugins.remove(&id);
            report.removed += 1;
        }
        report
    }

    fn info(&self, data: &Data, plugin: &Arc<WasmPlugin>) -> PluginInfo {
        let settings = data
            .saved
            .plugins
            .get(plugin.id())
            .cloned()
            .unwrap_or_default();
        let config = plugin
            .spec()
            .config
            .iter()
            .map(|f| {
                let stored = settings.config.get(&f.key).filter(|v| f.kind.accepts(v));
                ConfigView {
                    key: f.key.clone(),
                    kind: f.kind,
                    description: f.description.clone(),
                    secret: f.secret,
                    value: if f.secret { None } else { stored.cloned() },
                    is_set: stored.is_some(),
                    default: f.default.clone(),
                }
            })
            .collect();
        PluginInfo {
            plugin: plugin.manifest(),
            dir: plugin.dir().display().to_string(),
            enabled: settings.enabled,
            config,
        }
    }

    pub fn overview(&self) -> PluginsOverview {
        PluginsOverview {
            plugins: self.list(),
            issues: self.issues(),
            folders: self
                .inner
                .roots
                .iter()
                .map(|p| p.display().to_string())
                .collect(),
        }
    }

    pub fn list(&self) -> Vec<PluginInfo> {
        let data = self.data();
        data.plugins.values().map(|p| self.info(&data, p)).collect()
    }

    pub fn get(&self, id: &str) -> Result<PluginInfo, HostError> {
        let data = self.data();
        let plugin = data
            .plugins
            .get(id)
            .ok_or_else(|| HostError::Unknown(id.to_string()))?;
        Ok(self.info(&data, plugin))
    }

    /// Folders that held something that is not a usable plugin, and why.
    pub fn issues(&self) -> Vec<Issue> {
        self.data().issues.clone()
    }

    pub fn logs(&self, id: &str) -> Result<Vec<LogEntry>, HostError> {
        let data = self.data();
        data.plugins
            .get(id)
            .map(|p| p.log().entries())
            .ok_or_else(|| HostError::Unknown(id.to_string()))
    }

    pub fn set_enabled(&self, id: &str, enabled: bool) -> Result<PluginInfo, HostError> {
        let mut data = self.data();
        let plugin = data
            .plugins
            .get(id)
            .cloned()
            .ok_or_else(|| HostError::Unknown(id.to_string()))?;
        let mut saved = data.saved.clone();
        saved.plugins.entry(id.to_string()).or_default().enabled = enabled;
        self.persist(&saved)?;
        data.saved = saved;
        if enabled {
            self.reload_locked(&mut data, id)?;
        } else {
            plugin.deactivate();
        }
        let plugin = data.plugins.get(id).cloned().unwrap_or(plugin);
        Ok(self.info(&data, &plugin))
    }

    /// Start a plugin again from disk, for example after it stopped on an error.
    pub fn reload(&self, id: &str) -> Result<PluginInfo, HostError> {
        let mut data = self.data();
        let enabled = data
            .saved
            .plugins
            .get(id)
            .map(|s| s.enabled)
            .unwrap_or(true);
        if !enabled {
            return Err(HostError::Invalid(format!(
                "plugin `{id}` is disabled; enable it first"
            )));
        }
        self.reload_locked(&mut data, id)?;
        let plugin = data
            .plugins
            .get(id)
            .cloned()
            .ok_or_else(|| HostError::Unknown(id.to_string()))?;
        Ok(self.info(&data, &plugin))
    }

    /// Set one configuration value (`null` clears it). Takes effect by
    /// restarting the plugin, which resets whatever state it kept in memory.
    pub fn set_config(&self, id: &str, key: &str, value: Value) -> Result<PluginInfo, HostError> {
        let mut data = self.data();
        let plugin = data
            .plugins
            .get(id)
            .cloned()
            .ok_or_else(|| HostError::Unknown(id.to_string()))?;
        let Some(field) = plugin.spec().config.iter().find(|f| f.key == key) else {
            return Err(HostError::Invalid(format!(
                "plugin `{id}` has no setting `{key}`"
            )));
        };
        if !value.is_null() && !field.kind.accepts(&value) {
            return Err(HostError::Invalid(format!(
                "setting `{key}` must be a {}",
                match field.kind {
                    ConfigType::String => "string",
                    ConfigType::Number => "number",
                    ConfigType::Boolean => "boolean",
                }
            )));
        }
        if matches!(&value, Value::String(s) if s.len() > 4096) {
            return Err(HostError::Invalid(format!("setting `{key}` is too long")));
        }
        let mut saved = data.saved.clone();
        let entry = saved.plugins.entry(id.to_string()).or_default();
        if value.is_null() {
            entry.config.remove(key);
        } else {
            entry.config.insert(key.to_string(), value);
        }
        let enabled = entry.enabled;
        self.persist(&saved)?;
        data.saved = saved;
        if enabled && plugin.status().0 == PluginStatus::Enabled {
            self.reload_locked(&mut data, id)?;
        }
        let plugin = data.plugins.get(id).cloned().unwrap_or(plugin);
        Ok(self.info(&data, &plugin))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::events::EventBus;
    use crate::plugins::external::testkit::*;
    use crate::plugins::{ActionError, PluginContext};
    use serde_json::json;

    const OK: &str = r#"{"status":"ok","value":{"x":1}}"#;

    fn write(root: &Path, dir: &str, manifest: Value, wasm: &[u8]) -> PathBuf {
        let d = root.join(dir);
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("plugin.json"), manifest.to_string()).unwrap();
        std::fs::write(d.join("m.wasm"), wasm).unwrap();
        d
    }

    fn host(root: &Path, state: Option<PathBuf>) -> (PluginHost, PluginRegistry) {
        let registry = PluginRegistry::default();
        let host = PluginHost::new(
            vec![root.to_path_buf()],
            state,
            registry.clone(),
            Limits {
                fuel: 5_000_000,
                timeout: std::time::Duration::from_millis(400),
                memory_bytes: 2 * 1024 * 1024,
            },
        );
        (host, registry)
    }

    fn invoke(registry: &PluginRegistry, id: &str) -> Result<Value, ActionError> {
        let bus = EventBus::new();
        let ctx = PluginContext::new(id, bus, tokio::runtime::Handle::current());
        registry
            .get(id)
            .ok_or_else(|| ActionError::UnknownPlugin(id.into()))?
            .invoke(&ctx, "a", &json!({}))
    }

    fn good() -> Vec<u8> {
        Wat::new().reply(OK).build()
    }

    #[tokio::test]
    async fn scan_loads_plugins_into_the_shared_registry() {
        let root = tempfile::tempdir().unwrap();
        write(
            root.path(),
            "one",
            manifest_json("t.one", &["a"], &[]),
            &good(),
        );
        write(
            root.path(),
            "two",
            manifest_json("t.two", &["a"], &[]),
            &good(),
        );
        let (host, registry) = host(root.path(), None);
        assert!(!registry.contains("t.one"), "nothing before the scan");
        let report = host.scan();
        assert_eq!((report.loaded, report.issues), (2, 0));
        assert!(registry.contains("t.one") && registry.contains("t.two"));
        assert_eq!(invoke(&registry, "t.one").unwrap(), json!({"x": 1}));
        let list = host.list();
        assert_eq!(list.len(), 2);
        assert!(list.iter().all(|p| p.enabled));
        assert!(registry.manifests().iter().all(|m| m
            .external
            .as_ref()
            .is_some_and(|e| e.status == PluginStatus::Enabled)));
    }

    #[tokio::test]
    async fn bad_plugins_are_reported_and_good_ones_still_load() {
        let root = tempfile::tempdir().unwrap();
        write(
            root.path(),
            "a-good",
            manifest_json("t.good", &["a"], &[]),
            &good(),
        );
        write(
            root.path(),
            "b-dup",
            manifest_json("t.good", &["a"], &[]),
            &good(),
        );
        write(
            root.path(),
            "c-abi",
            {
                let mut m = manifest_json("t.abi", &["a"], &[]);
                m["abi"] = json!(9);
                m
            },
            &good(),
        );
        write(
            root.path(),
            "d-garbage",
            manifest_json("t.garbage", &["a"], &[]),
            b"\0asm\x01\0\0\0nonsense",
        );
        write(
            root.path(),
            "e-trap",
            manifest_json("t.trap", &["a"], &[]),
            &Wat::new().init("unreachable").build(),
        );
        let (host, registry) = host(root.path(), None);
        let report = host.scan();
        assert_eq!(
            report.loaded, 3,
            "good, garbage module and trapping init are known plugins"
        );
        assert_eq!(report.issues, 2, "duplicate id and incompatible protocol");
        let by_id = |id: &str| host.get(id).unwrap();
        assert_eq!(
            by_id("t.good").plugin.external.unwrap().status,
            PluginStatus::Enabled
        );
        assert_eq!(
            by_id("t.garbage").plugin.external.unwrap().status,
            PluginStatus::Error
        );
        let trap = by_id("t.trap").plugin.external.unwrap();
        assert_eq!(trap.status, PluginStatus::Error);
        assert!(trap.error.unwrap().contains("init"));
        // Errored plugins are registered (so the failure is explicit) and refuse to run.
        let msg = invoke(&registry, "t.trap").unwrap_err().to_string();
        assert!(msg.contains("unavailable"), "{msg}");
        assert!(invoke(&registry, "t.good").is_ok());
        let issues = host.issues();
        assert!(issues
            .iter()
            .any(|i| i.error.contains("duplicate plugin id")));
        assert!(issues.iter().any(|i| i.error.contains("protocol (ABI)")));
    }

    #[tokio::test]
    async fn the_enabled_flag_persists_and_a_disabled_plugin_never_loads() {
        let root = tempfile::tempdir().unwrap();
        let state = tempfile::tempdir().unwrap().keep().join("plugins.json");
        // Traps if it is ever started or invoked.
        write(
            root.path(),
            "p",
            manifest_json("t.p", &["a"], &[]),
            &Wat::new().init("unreachable").body("unreachable").build(),
        );
        write(root.path(), "q", manifest_json("t.q", &["a"], &[]), &good());
        let (h, registry) = host(root.path(), Some(state.clone()));
        h.scan();
        assert_eq!(
            h.get("t.p").unwrap().plugin.external.unwrap().status,
            PluginStatus::Error
        );
        let info = h.set_enabled("t.p", false).unwrap();
        assert!(!info.enabled);
        assert_eq!(info.plugin.external.unwrap().status, PluginStatus::Disabled);
        assert!(invoke(&registry, "t.p")
            .unwrap_err()
            .to_string()
            .contains("disabled"));

        // A new process: still disabled, and its module is not even started.
        let (h2, registry2) = host(root.path(), Some(state.clone()));
        h2.scan();
        let p = h2.get("t.p").unwrap();
        assert!(!p.enabled);
        assert_eq!(
            p.plugin.external.unwrap().status,
            PluginStatus::Disabled,
            "init would have trapped"
        );
        assert!(invoke(&registry2, "t.q").is_ok());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&state).unwrap().permissions().mode();
            assert_eq!(mode & 0o077, 0, "settings are private to the user");
        }
        // Re-enabling starts it again (and here it fails again, visibly).
        let info = h2.set_enabled("t.p", true).unwrap();
        assert!(info.enabled);
        assert_eq!(info.plugin.external.unwrap().status, PluginStatus::Error);
    }

    #[tokio::test]
    async fn a_failed_plugin_recovers_on_reload_after_the_files_are_fixed() {
        let root = tempfile::tempdir().unwrap();
        let dir = write(
            root.path(),
            "p",
            manifest_json("t.p", &["a"], &[]),
            &Wat::new().body("unreachable").build(),
        );
        let (h, registry) = host(root.path(), None);
        h.scan();
        assert!(invoke(&registry, "t.p").is_err());
        assert_eq!(
            h.get("t.p").unwrap().plugin.external.unwrap().status,
            PluginStatus::Error
        );
        // Same bad files: reload fails again but cleanly.
        assert_eq!(
            h.reload("t.p").unwrap().plugin.external.unwrap().status,
            PluginStatus::Enabled
        );
        assert!(invoke(&registry, "t.p").is_err());
        std::fs::write(dir.join("m.wasm"), good()).unwrap();
        // A rescan retries plugins that stopped on an error, and the files changed.
        let report = h.scan();
        assert_eq!(report.reloaded, 1);
        assert_eq!(invoke(&registry, "t.p").unwrap(), json!({"x": 1}));
    }

    #[tokio::test]
    async fn rescan_keeps_unchanged_plugins_running_and_reloads_changed_ones() {
        let first = r#"{"status":"ok","value":"first"}"#;
        let second = r#"{"status":"ok","value":"second"}"#;
        let counter = Wat::new()
            .data(0, first)
            .data(100, second)
            .body(&format!(
                "(global.set $calls (i32.add (global.get $calls) (i32.const 1)))
                 (if (i32.eq (global.get $calls) (i32.const 1)) (then {}) (else {}))
                 (i32.const 0)",
                respond_at(0, first.len()),
                respond_at(100, second.len())
            ))
            .build();
        let root = tempfile::tempdir().unwrap();
        let dir = write(
            root.path(),
            "p",
            manifest_json("t.p", &["a"], &[]),
            &counter,
        );
        write(
            root.path(),
            "gone",
            manifest_json("t.gone", &["a"], &[]),
            &good(),
        );
        let (h, registry) = host(root.path(), None);
        h.scan();
        assert_eq!(invoke(&registry, "t.p").unwrap(), json!("first"));
        assert_eq!(invoke(&registry, "t.p").unwrap(), json!("second"));

        let report = h.scan();
        assert_eq!(
            (report.unchanged, report.reloaded, report.loaded),
            (2, 0, 0)
        );
        assert_eq!(
            invoke(&registry, "t.p").unwrap(),
            json!("second"),
            "an unchanged plugin keeps its state across a rescan"
        );

        std::fs::write(dir.join("m.wasm"), {
            let mut b = counter.clone();
            b.extend_from_slice(&[0, 3, 1, 0]); // a trailing custom-section-free change in bytes
            b
        })
        .unwrap();
        // The extra bytes make it an invalid module: it must fail visibly, not crash.
        let report = h.scan();
        assert_eq!(report.reloaded, 1);
        assert_eq!(
            h.get("t.p").unwrap().plugin.external.unwrap().status,
            PluginStatus::Error
        );
        std::fs::write(dir.join("m.wasm"), &counter).unwrap();
        h.scan();
        assert_eq!(
            invoke(&registry, "t.p").unwrap(),
            json!("first"),
            "a reloaded plugin starts fresh"
        );

        std::fs::remove_dir_all(root.path().join("gone")).unwrap();
        let report = h.scan();
        assert_eq!(report.removed, 1);
        assert!(!registry.contains("t.gone"));
        assert!(matches!(
            invoke(&registry, "t.gone"),
            Err(ActionError::UnknownPlugin(_))
        ));
    }

    fn with_config(config: Value) -> Value {
        let mut m = manifest_json("t.cfg", &["a"], &[]);
        m["config"] = config;
        m
    }

    #[tokio::test]
    async fn configuration_is_validated_persisted_and_secrets_are_redacted() {
        let root = tempfile::tempdir().unwrap();
        let state = tempfile::tempdir().unwrap().keep().join("plugins.json");
        write(
            root.path(),
            "cfg",
            with_config(json!([
                {"key": "greeting", "type": "string", "default": "Hello"},
                {"key": "volume", "type": "number"},
                {"key": "token", "type": "string", "secret": true}
            ])),
            &good(),
        );
        let (h, _) = host(root.path(), Some(state.clone()));
        h.scan();
        let view = |h: &PluginHost, key: &str| {
            h.get("t.cfg")
                .unwrap()
                .config
                .into_iter()
                .find(|c| c.key == key)
                .unwrap()
        };
        assert_eq!(view(&h, "greeting").value, None);
        assert_eq!(view(&h, "greeting").default, Some(json!("Hello")));

        h.set_config("t.cfg", "greeting", json!("Hi")).unwrap();
        h.set_config("t.cfg", "token", json!("s3cret")).unwrap();
        assert_eq!(view(&h, "greeting").value, Some(json!("Hi")));
        let token = view(&h, "token");
        assert!(
            token.is_set && token.value.is_none(),
            "secrets never leave the host"
        );
        let listed = serde_json::to_string(&h.list()).unwrap();
        assert!(!listed.contains("s3cret"));

        // Wrong type, unknown key, null clears.
        assert!(matches!(
            h.set_config("t.cfg", "volume", json!("loud")),
            Err(HostError::Invalid(_))
        ));
        assert!(matches!(
            h.set_config("t.cfg", "nope", json!(1)),
            Err(HostError::Invalid(_))
        ));
        assert!(matches!(
            h.set_config("t.nope", "x", json!(1)),
            Err(HostError::Unknown(_))
        ));
        h.set_config("t.cfg", "greeting", Value::Null).unwrap();
        assert_eq!(view(&h, "greeting").value, None);

        // Persisted next to, not inside, the workspace; survives a restart.
        h.set_config("t.cfg", "volume", json!(7)).unwrap();
        let (h2, _) = host(root.path(), Some(state.clone()));
        h2.scan();
        assert_eq!(view(&h2, "volume").value, Some(json!(7)));
        assert!(view(&h2, "token").is_set);
    }

    #[tokio::test]
    async fn unreadable_settings_fall_back_to_defaults_and_are_kept_aside() {
        let root = tempfile::tempdir().unwrap();
        let dir = tempfile::tempdir().unwrap().keep();
        let state = dir.join("plugins.json");
        std::fs::write(&state, "{ this is not json").unwrap();
        write(root.path(), "p", manifest_json("t.p", &["a"], &[]), &good());
        let (h, _) = host(root.path(), Some(state.clone()));
        h.scan();
        assert!(h.get("t.p").unwrap().enabled);
        assert!(dir.join("plugins.json.bak").exists());
        // A file from the future is also set aside, never downgraded.
        std::fs::write(&state, r#"{"schemaVersion": 99, "plugins": {}}"#).unwrap();
        let (h, _) = host(root.path(), Some(state));
        h.scan();
        assert!(h.get("t.p").unwrap().enabled);
    }

    #[tokio::test]
    async fn unknown_plugins_are_errors_not_panics() {
        let root = tempfile::tempdir().unwrap();
        let (h, _) = host(root.path(), None);
        h.scan();
        assert!(matches!(
            h.set_enabled("t.nope", true),
            Err(HostError::Unknown(_))
        ));
        assert!(matches!(h.reload("t.nope"), Err(HostError::Unknown(_))));
        assert!(matches!(h.logs("t.nope"), Err(HostError::Unknown(_))));
        assert!(h.list().is_empty());
    }

    #[tokio::test]
    async fn logs_collect_what_the_host_says_about_a_plugin() {
        let root = tempfile::tempdir().unwrap();
        write(
            root.path(),
            "p",
            manifest_json("t.p", &["a"], &[]),
            &Wat::new().body("unreachable").build(),
        );
        let (h, registry) = host(root.path(), None);
        h.scan();
        let _ = invoke(&registry, "t.p");
        let logs = h.logs("t.p").unwrap();
        assert!(logs.iter().any(|l| l.message.contains("loaded")));
        assert!(logs.iter().any(|l| l.message.contains("stopped")));
    }
}
