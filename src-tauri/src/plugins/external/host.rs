//! `PluginHost`: the lifecycle of installed external plugins.
//!
//! It scans plugin folders, remembers per plugin whether it is enabled and what
//! its configuration is (`plugins.json`, owner-only), loads plugins in the
//! sandbox and registers them in the shared [`PluginRegistry`]. Registering is
//! all it takes for the action engine to see a plugin; nothing else in
//! LaunchDeck knows this module exists.
//!
//! It also holds what a plugin was granted (`permissions.json`) and where its
//! secrets are kept, and builds each running plugin the broker its network
//! requests go through. Grants and secrets are never part of a plugin's
//! settings: a setting is given to the module, these are not.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use super::capabilities::{Decision, Grants, NetworkRequest, NetworkView};
use super::discovery::{self, Candidate, Issue};
use super::log::{Level, LogEntry, PluginLog};
use super::manifest::{ConfigType, ExternalManifest};
use super::network::{Broker, NetFailure, SecretBinding};
use super::sandbox::Limits;
use super::secrets::{SecretRef, SecretStore, StoreKind};
use super::wasm_plugin::{effective_config, WasmPlugin, MAX_WATCHING};
use super::PluginStatus;
use crate::persistence::write_private;
use crate::plugins::{ActionError, Plugin, PluginContext, PluginManifest, PluginRegistry};
use crate::state::{StateAddress, StateHub};

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
    /// The name a person sees. The key when the manifest gives none.
    pub label: String,
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

/// A secret of a plugin, as the Plugins screen shows it: everything but the value.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SecretView {
    pub key: String,
    pub label: String,
    pub description: String,
    pub is_set: bool,
    /// The only origin it is sent to. Absent while the setting that names it is not set.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub origin: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PluginInfo {
    pub plugin: PluginManifest,
    pub dir: String,
    /// What the person chose; `plugin.external.status` is what actually happened.
    pub enabled: bool,
    pub config: Vec<ConfigView>,
    /// What the plugin asks of the network and what was decided. Absent for a
    /// plugin that asks for nothing.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub network: Option<NetworkView>,
    /// The last request that was refused or failed, unless one succeeded since.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub network_failure: Option<NetFailure>,
    pub secrets: Vec<SecretView>,
    /// Where secrets are kept on this computer.
    pub secret_store: StoreKind,
    /// How often the plugin is asked to report the states being shown.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub refresh_seconds: Option<u32>,
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

/// When a plugin is next asked to report its states.
struct RefreshSlot {
    next: Instant,
    /// A refresh is running now; another is not started on top of it.
    running: bool,
    /// Something changed while it ran (a grant, a secret): run again right after.
    again: bool,
    /// Where the next refresh starts in the list of what is being shown, when
    /// the list is longer than one refresh takes.
    cursor: usize,
}

struct Data {
    /// Secrets known to hold a value: `(plugin, key)`. Read from the store when
    /// a plugin is installed, so listing plugins never opens the credential store.
    secrets_set: HashSet<(String, String)>,
    refresh: HashMap<String, RefreshSlot>,
    /// Plugins whose states this host declared in the hub, so that a plugin
    /// whose new version declares none has its old declarations cleared.
    declared: HashSet<String>,
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
    hub: StateHub,
    /// What each plugin was granted. Its own lock, never held during a call.
    grants: Arc<Grants>,
    secrets: Arc<dyn SecretStore>,
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
        Self::with_hub(roots, state_path, registry, limits, StateHub::new())
    }

    /// A host whose plugins report their lifecycle and states to `hub`. Grants
    /// and the audit trail live in the folder of `state_path` (in memory without
    /// one). Secrets are kept in memory: the app, which wants them in the
    /// system's credential store, builds its host with [`with_parts`](PluginHost::with_parts).
    pub fn with_hub(
        roots: Vec<PathBuf>,
        state_path: Option<PathBuf>,
        registry: PluginRegistry,
        limits: Limits,
        hub: StateHub,
    ) -> PluginHost {
        let dir = state_path.as_deref().and_then(Path::parent);
        let grants = Arc::new(Grants::new(
            dir.map(|d| d.join("permissions.json")),
            dir.map(|d| d.join("audit.jsonl")),
        ));
        let secrets = Arc::new(super::secrets::MemoryStore::new());
        Self::with_parts(roots, state_path, registry, limits, hub, grants, secrets)
    }

    /// A host with the grants and the secret store it is given.
    pub fn with_parts(
        roots: Vec<PathBuf>,
        state_path: Option<PathBuf>,
        registry: PluginRegistry,
        limits: Limits,
        hub: StateHub,
        grants: Arc<Grants>,
        secrets: Arc<dyn SecretStore>,
    ) -> PluginHost {
        let saved = state_path.as_deref().map(load_state).unwrap_or_default();
        PluginHost {
            inner: Arc::new(Inner {
                roots,
                state_path,
                registry,
                limits,
                hub,
                grants,
                secrets,
                data: Mutex::new(Data {
                    secrets_set: HashSet::new(),
                    refresh: HashMap::new(),
                    declared: HashSet::new(),
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

    pub fn grants(&self) -> &Arc<Grants> {
        &self.inner.grants
    }

    pub fn secret_store(&self) -> &Arc<dyn SecretStore> {
        &self.inner.secrets
    }

    /// What a manifest asks of the network with these settings.
    fn network_request(
        manifest: &ExternalManifest,
        saved: &Map<String, Value>,
    ) -> Option<NetworkRequest> {
        let decl = manifest.capabilities.network.as_ref()?;
        Some(NetworkRequest::resolve(
            decl,
            &effective_config(&manifest.config, saved),
        ))
    }

    /// The broker a plugin's requests go through: what it asks for now, where
    /// each of its secrets may go, and the shared grants and store.
    fn broker(
        &self,
        manifest: &ExternalManifest,
        saved: &Map<String, Value>,
        log: &Arc<PluginLog>,
    ) -> Option<Arc<Broker>> {
        let request = Self::network_request(manifest, saved)?;
        self.inner.grants.note_requested(&manifest.id, &request);
        let secrets = manifest
            .secrets
            .iter()
            .map(|s| {
                (
                    s.key.clone(),
                    SecretBinding {
                        origin: request.origin_of(&s.host).cloned(),
                    },
                )
            })
            .collect();
        Some(Arc::new(Broker::new(
            &manifest.id,
            request,
            secrets,
            self.inner.grants.clone(),
            self.inner.secrets.clone(),
            log.clone(),
        )))
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
        // The copy being replaced may still have a call in flight. Retiring it
        // first means nothing it says afterwards can reach the hub, so the new
        // copy starts from `unknown` and stays there until it reports.
        if let Some(old) = data.plugins.get(&id) {
            old.retire();
        }
        let log = data
            .logs
            .entry(id.clone())
            .or_insert_with(|| Arc::new(PluginLog::new(&id)))
            .clone();
        let settings = data.saved.plugins.get(&id).cloned().unwrap_or_default();
        let fields = cand.manifest.config.clone();
        let net = self.broker(&cand.manifest, &settings.config, &log);
        // Which secrets hold a value is looked up once, here, so that listing
        // plugins never has to open the credential store.
        data.secrets_set.retain(|(plugin, _)| plugin != &id);
        for secret in &cand.manifest.secrets {
            let found = self.inner.secrets.get(&SecretRef::new(&id, &secret.key));
            if matches!(found, Ok(Some(_))) {
                data.secrets_set.insert((id.clone(), secret.key.clone()));
            }
        }
        let refreshes = cand.manifest.refresh.is_some();
        let plugin = WasmPlugin::with_network(
            cand.manifest,
            cand.dir,
            cand.fingerprint,
            log.clone(),
            self.inner.limits.clone(),
            self.inner.hub.clone(),
            net,
        );
        // What the plugin offers is declared before it starts, so the first
        // thing the hub sees of it is a provider with states and no values.
        let defs = plugin.states();
        if !defs.is_empty() || data.declared.contains(&id) {
            let has_any = !defs.is_empty();
            match self.inner.hub.declare(&id, defs) {
                Ok(()) if has_any => {
                    data.declared.insert(id.clone());
                }
                Ok(()) => {
                    data.declared.remove(&id);
                }
                Err(why) => log.host(Level::Error, &format!("states were not declared: {why}")),
            }
        }
        if settings.enabled {
            plugin.activate(&cand.wasm, &effective_config(&fields, &settings.config));
        } else {
            log.host(Level::Info, "installed but disabled");
        }
        let arc: Arc<dyn Plugin> = plugin.clone();
        self.inner.registry.register(arc);
        // Said after the registry lists it, so whoever reads the catalog on this
        // signal sees the new version (the declaration above came before).
        self.inner.hub.catalog_changed(&id);
        // A plugin that starts knows nothing yet: ask it as soon as possible.
        data.refresh.remove(&id);
        if refreshes {
            data.refresh.insert(
                id.clone(),
                RefreshSlot {
                    next: Instant::now(),
                    running: false,
                    again: false,
                    cursor: 0,
                },
            );
        }
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
            self.inner.hub.remove_provider(&id);
            data.declared.remove(&id);
            data.refresh.remove(&id);
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
                    label: f.label.clone().unwrap_or_else(|| f.key.clone()),
                    kind: f.kind,
                    description: f.description.clone(),
                    secret: f.secret,
                    value: if f.secret { None } else { stored.cloned() },
                    is_set: stored.is_some(),
                    default: f.default.clone(),
                }
            })
            .collect();
        // From the settings as they are now, so the screen is right for a
        // plugin that is not running too.
        let request = Self::network_request(plugin.spec(), &settings.config);
        let secrets = plugin
            .spec()
            .secrets
            .iter()
            .map(|s| SecretView {
                key: s.key.clone(),
                label: s.label.clone(),
                description: s.description.clone(),
                is_set: data
                    .secrets_set
                    .contains(&(plugin.id().to_string(), s.key.clone())),
                origin: request
                    .as_ref()
                    .and_then(|r| r.origin_of(&s.host))
                    .map(|o| o.as_str().to_string()),
            })
            .collect();
        PluginInfo {
            plugin: plugin.manifest(),
            dir: plugin.dir().display().to_string(),
            enabled: settings.enabled,
            config,
            network: request
                .as_ref()
                .map(|r| self.inner.grants.view(plugin.id(), r)),
            network_failure: plugin.network().and_then(|b| b.last_failure()),
            secrets,
            secret_store: self.inner.secrets.kind(),
            refresh_seconds: plugin.spec().refresh.map(|r| r.interval_seconds),
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
        let config = entry.config.clone();
        self.persist(&saved)?;
        data.saved = saved;
        if enabled && plugin.status().0 == PluginStatus::Enabled {
            self.reload_locked(&mut data, id)?;
        } else if let Some(request) = Self::network_request(plugin.spec(), &config) {
            // Not running, but a new address is still something it now asks for.
            self.inner.grants.note_requested(id, &request);
        }
        let plugin = data.plugins.get(id).cloned().unwrap_or(plugin);
        Ok(self.info(&data, &plugin))
    }

    // -- permissions --------------------------------------------------------

    /// Grant or deny one origin a plugin asks for now, or (with `None`) take
    /// the decision back. An origin it does not ask for cannot be granted.
    pub fn set_permission(
        &self,
        id: &str,
        origin: &str,
        decision: Option<Decision>,
    ) -> Result<PluginInfo, HostError> {
        let mut data = self.data();
        let plugin = data
            .plugins
            .get(id)
            .cloned()
            .ok_or_else(|| HostError::Unknown(id.to_string()))?;
        let settings = data.saved.plugins.get(id).cloned().unwrap_or_default();
        let request = Self::network_request(plugin.spec(), &settings.config).ok_or_else(|| {
            HostError::Invalid(format!("plugin `{id}` does not ask for network access"))
        })?;
        let asked = request
            .hosts
            .iter()
            .map(|h| &h.origin)
            .find(|o| o.as_str() == origin)
            .cloned();
        match (asked, decision) {
            (Some(origin), decision) => self
                .inner
                .grants
                .decide(id, &origin, decision)
                .map_err(HostError::Storage)?,
            // A decision about something it no longer asks for can only be removed.
            (None, None) => self
                .inner
                .grants
                .forget(id, origin)
                .map_err(HostError::Storage)?,
            (None, Some(_)) => {
                let shown: String = origin
                    .chars()
                    .filter(|c| !c.is_control())
                    .take(80)
                    .collect();
                return Err(HostError::Invalid(format!(
                    "plugin `{id}` does not ask for access to `{shown}`"
                )));
            }
        }
        Self::refresh_now(&mut data, id);
        Ok(self.info(&data, &plugin))
    }

    // -- secrets ------------------------------------------------------------

    fn secret_ref(plugin: &WasmPlugin, key: &str) -> Result<SecretRef, HostError> {
        if !plugin.spec().secrets.iter().any(|s| s.key == key) {
            return Err(HostError::Invalid(format!(
                "plugin `{}` has no secret `{key}`",
                plugin.id()
            )));
        }
        Ok(SecretRef::new(plugin.id(), key))
    }

    /// Store a secret's value. It goes to the store and nowhere else: not to
    /// the module, not to `plugins.json`, not to a log.
    pub fn set_secret(&self, id: &str, key: &str, value: &str) -> Result<PluginInfo, HostError> {
        let mut data = self.data();
        let plugin = data
            .plugins
            .get(id)
            .cloned()
            .ok_or_else(|| HostError::Unknown(id.to_string()))?;
        let secret = Self::secret_ref(&plugin, key)?;
        self.inner
            .secrets
            .set(&secret, value)
            .map_err(|e| HostError::Invalid(format!("the secret was not stored: {e}")))?;
        data.secrets_set.insert((id.to_string(), key.to_string()));
        plugin
            .log()
            .host(Level::Info, &format!("secret `{key}` was set"));
        Self::refresh_now(&mut data, id);
        Ok(self.info(&data, &plugin))
    }

    pub fn clear_secret(&self, id: &str, key: &str) -> Result<PluginInfo, HostError> {
        let mut data = self.data();
        let plugin = data
            .plugins
            .get(id)
            .cloned()
            .ok_or_else(|| HostError::Unknown(id.to_string()))?;
        let secret = Self::secret_ref(&plugin, key)?;
        self.inner
            .secrets
            .delete(&secret)
            .map_err(|e| HostError::Invalid(format!("the secret was not removed: {e}")))?;
        data.secrets_set.remove(&(id.to_string(), key.to_string()));
        plugin
            .log()
            .host(Level::Info, &format!("secret `{key}` was removed"));
        Self::refresh_now(&mut data, id);
        Ok(self.info(&data, &plugin))
    }

    // -- refresh ------------------------------------------------------------

    fn refresh_now(data: &mut Data, id: &str) {
        if let Some(slot) = data.refresh.get_mut(id) {
            slot.next = Instant::now();
            slot.again = slot.running;
        }
    }

    /// Ask every plugin that refreshes to do so at the next opportunity (what
    /// is being shown changed).
    pub fn refresh_all_soon(&self) {
        let mut data = self.data();
        let now = Instant::now();
        for slot in data.refresh.values_mut() {
            slot.next = now;
            slot.again = slot.running;
        }
    }

    /// The plugins whose refresh is due and not already running. Each one
    /// returned is marked as running: call [`refresh`](PluginHost::refresh) for it.
    pub fn due_refreshes(&self) -> Vec<String> {
        let mut data = self.data();
        let now = Instant::now();
        let running: HashSet<String> = data
            .plugins
            .iter()
            .filter(|(_, p)| p.status().0 == PluginStatus::Enabled)
            .map(|(id, _)| id.clone())
            .collect();
        data.refresh
            .iter_mut()
            .filter(|(id, slot)| !slot.running && slot.next <= now && running.contains(*id))
            .map(|(id, slot)| {
                slot.running = true;
                slot.again = false;
                id.clone()
            })
            .collect()
    }

    /// Run one plugin's refresh with the instances being shown. With nothing
    /// to report the plugin is not called. Blocks for as long as the plugin
    /// takes (network included), so call it from a blocking thread.
    pub fn refresh(
        &self,
        id: &str,
        ctx: &PluginContext,
        watching: &[StateAddress],
    ) -> Result<(), ActionError> {
        let (plugin, round) = {
            let mut data = self.data();
            // One refresh asks about a bounded number of instances; a longer
            // list is taken a window at a time, in turn.
            let round: Vec<StateAddress> = match data.refresh.get_mut(id) {
                Some(slot) if watching.len() > MAX_WATCHING => {
                    let start = slot.cursor % watching.len();
                    slot.cursor = (start + MAX_WATCHING) % watching.len();
                    watching
                        .iter()
                        .cycle()
                        .skip(start)
                        .take(MAX_WATCHING)
                        .cloned()
                        .collect()
                }
                _ => watching.to_vec(),
            };
            (data.plugins.get(id).cloned(), round)
        };
        let outcome = match &plugin {
            Some(plugin) if !round.is_empty() => plugin.refresh(ctx, &round),
            _ => Ok(()),
        };
        let mut data = self.data();
        let wait = plugin
            .as_ref()
            .and_then(|p| p.refresh_interval())
            .unwrap_or(Duration::from_secs(60));
        if let Some(slot) = data.refresh.get_mut(id) {
            slot.running = false;
            slot.next = if slot.again {
                Instant::now()
            } else {
                Instant::now() + wait
            };
            slot.again = false;
        }
        outcome
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
