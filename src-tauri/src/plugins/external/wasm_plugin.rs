//! `WasmPlugin`: an external plugin behind the existing [`Plugin`] trait.
//!
//! To the rest of LaunchDeck it is an ordinary plugin. Its actions come from
//! the manifest, `invoke` validates arguments, runs the module in the sandbox,
//! normalizes the answer into the usual `Result<Value, ActionError>` and
//! publishes the events the module emitted through the existing bus. Nothing
//! here can crash the host: every failure is a value.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, RwLock};

use serde_json::{json, Map, Value};

use super::log::{clean_line, Level, PluginLog};
use super::manifest::{ConfigField, ExternalManifest};
use super::network::{Broker, NetGate};
use super::sandbox::{CallError, CallOutput, Limits, Refusal, Sandbox, StateGate};
use super::{EventInfo, ExternalInfo, PluginStatus, ABI_VERSION, SDK_VERSION};
use crate::actions::definition::ProviderKind;
use crate::events::external::valid_ident;
use crate::plugins::{ActionDef, ActionError, Plugin, PluginContext, PluginManifest};
use crate::state::definition::{ObservableDefinition, Write};
use crate::state::{StateAddress, StateHub};

#[derive(Debug, Clone, PartialEq, Eq)]
struct StatusInfo {
    status: PluginStatus,
    error: Option<String>,
}

/// What a module said about one call.
#[derive(Debug, PartialEq)]
pub(crate) enum Reply {
    Ok(Value),
    Failed(String),
}

pub(crate) fn parse_reply(bytes: &[u8]) -> Result<Reply, String> {
    let value: Value = serde_json::from_slice(bytes)
        .map_err(|e| format!("the response is not valid JSON ({e})"))?;
    let Some(obj) = value.as_object() else {
        return Err("the response is not a JSON object".into());
    };
    match obj.get("status").and_then(Value::as_str) {
        Some("ok") => Ok(Reply::Ok(obj.get("value").cloned().unwrap_or(Value::Null))),
        Some("failed") => {
            let error = obj
                .get("error")
                .and_then(Value::as_str)
                .map(|s| clean_line(s.as_bytes()))
                .filter(|s| !s.trim().is_empty())
                .unwrap_or_else(|| "the action failed".to_string());
            let message = match obj.get("code").and_then(Value::as_str) {
                Some(code) if valid_ident(code) => format!("{error} ({code})"),
                _ => error,
            };
            Ok(Reply::Failed(message))
        }
        Some("pending") => Err("`pending` results are not supported by protocol 1".into()),
        Some(other) => Err(format!("unknown status `{}`", clean_line(other.as_bytes()))),
        None => Err("the response has no `status`".into()),
    }
}

/// Requests are written with `op` first, so a minimal guest can tell them apart
/// by looking at a fixed offset. serde_json would sort or keep insertion order
/// depending on features, which is not a contract worth depending on.
pub(crate) fn init_request(manifest: &ExternalManifest, config: &Map<String, Value>) -> String {
    let host =
        json!({ "name": "launchdeck", "version": env!("CARGO_PKG_VERSION"), "sdk": SDK_VERSION });
    let plugin = json!({ "id": manifest.id, "version": manifest.version });
    format!(
        r#"{{"op":"init","abi":{ABI_VERSION},"host":{host},"plugin":{plugin},"config":{}}}"#,
        Value::Object(config.clone())
    )
}

pub(crate) fn invoke_request(action: &str, args: &Value) -> String {
    format!(
        r#"{{"op":"invoke","action":{},"args":{args}}}"#,
        Value::String(action.to_string())
    )
}

/// Instances one refresh names. When more are being shown, each refresh takes
/// the next ones in turn, so every instance is asked about every few rounds.
pub const MAX_WATCHING: usize = 16;

/// "Report these states of yours: something is showing them."
pub(crate) fn refresh_request(watching: &[StateAddress]) -> String {
    let list: Vec<Value> = watching
        .iter()
        .take(MAX_WATCHING)
        .map(|a| json!({ "state": a.id, "args": a.args_json() }))
        .collect();
    format!(r#"{{"op":"refresh","watching":{}}}"#, Value::Array(list))
}

/// The configuration a module receives: declared keys only, saved values when
/// they still fit the declared type, defaults otherwise.
pub fn effective_config(fields: &[ConfigField], saved: &Map<String, Value>) -> Map<String, Value> {
    let mut out = Map::new();
    for f in fields {
        let value = saved
            .get(&f.key)
            .filter(|v| f.kind.accepts(v))
            .or(f.default.as_ref());
        if let Some(v) = value {
            out.insert(f.key.clone(), v.clone());
        }
    }
    out
}

/// What a module may write: the states its own manifest declares, and nothing
/// else. This is the whole ownership rule: the provider is never named by the
/// module, so there is no way to write as another provider, and an id that this
/// manifest does not declare (`slot.status` of Agent Control, a state of
/// another plugin) is simply not found.
struct DeclaredStates {
    defs: HashMap<String, ObservableDefinition>,
}

impl DeclaredStates {
    fn of(manifest: &ExternalManifest) -> Arc<DeclaredStates> {
        Arc::new(DeclaredStates {
            defs: manifest
                .state_definitions()
                .into_iter()
                .map(|d| (d.local.clone(), d))
                .collect(),
        })
    }
}

impl StateGate for DeclaredStates {
    fn check(&self, id: &str, args: &Value, value: Option<&Value>) -> Result<Write, Refusal> {
        let def = self.defs.get(id).ok_or(Refusal::NotDeclared)?;
        def.check_write(args, value).map_err(Refusal::Invalid)
    }
}

pub struct WasmPlugin {
    manifest: ExternalManifest,
    dir: PathBuf,
    fingerprint: u64,
    declared_events: Arc<HashSet<String>>,
    gate: Arc<DeclaredStates>,
    log: Arc<PluginLog>,
    limits: Limits,
    /// The plugin's way to the network, when its manifest asks for one.
    net: Option<Arc<Broker>>,
    /// Kept apart from the sandbox so listing plugins never waits for a call.
    status: RwLock<StatusInfo>,
    sandbox: Mutex<Option<Sandbox>>,
    /// The state hub of the runtime. A plugin's lifecycle is reported here at
    /// the one place its status changes (`set_status`), so the hub can never
    /// disagree with the plugin about whether it is running.
    hub: StateHub,
}

impl WasmPlugin {
    /// A plugin that is known but not running, with a state hub of its own.
    pub fn new(
        manifest: ExternalManifest,
        dir: PathBuf,
        fingerprint: u64,
        log: Arc<PluginLog>,
        limits: Limits,
    ) -> Arc<WasmPlugin> {
        Self::with_hub(manifest, dir, fingerprint, log, limits, StateHub::new())
    }

    /// A plugin that is known but not running, reporting to `hub`.
    pub fn with_hub(
        manifest: ExternalManifest,
        dir: PathBuf,
        fingerprint: u64,
        log: Arc<PluginLog>,
        limits: Limits,
        hub: StateHub,
    ) -> Arc<WasmPlugin> {
        Self::with_network(manifest, dir, fingerprint, log, limits, hub, None)
    }

    /// The same, with the broker its requests go through.
    pub fn with_network(
        manifest: ExternalManifest,
        dir: PathBuf,
        fingerprint: u64,
        log: Arc<PluginLog>,
        limits: Limits,
        hub: StateHub,
        net: Option<Arc<Broker>>,
    ) -> Arc<WasmPlugin> {
        let declared_events = Arc::new(manifest.events.iter().map(|e| e.kind.clone()).collect());
        let gate = DeclaredStates::of(&manifest);
        Arc::new(WasmPlugin {
            manifest,
            dir,
            fingerprint,
            declared_events,
            gate,
            log,
            limits,
            net,
            status: RwLock::new(StatusInfo {
                status: PluginStatus::Disabled,
                error: None,
            }),
            sandbox: Mutex::new(None),
            hub,
        })
    }

    pub fn id(&self) -> &str {
        &self.manifest.id
    }

    pub fn dir(&self) -> &PathBuf {
        &self.dir
    }

    pub fn fingerprint(&self) -> u64 {
        self.fingerprint
    }

    pub fn spec(&self) -> &ExternalManifest {
        &self.manifest
    }

    pub fn log(&self) -> &Arc<PluginLog> {
        &self.log
    }

    /// The broker this plugin's requests go through, if it has one.
    pub fn network(&self) -> Option<&Arc<Broker>> {
        self.net.as_ref()
    }

    pub fn status(&self) -> (PluginStatus, Option<String>) {
        let s = self.status.read().unwrap_or_else(|e| e.into_inner());
        (s.status, s.error.clone())
    }

    /// The only place the status changes, and therefore the only place the
    /// hub hears about it. Running again means everything this plugin said
    /// before is gone (`unknown`); not running means `unavailable`.
    ///
    /// The status lock is held while the hub is told, and a call applies its
    /// states under the same lock (`apply_states`), so a write can never land
    /// between "stopped" and "stopped, everything it said is gone".
    fn set_status(&self, status: PluginStatus, error: Option<String>) {
        let mut guard = self.status.write().unwrap_or_else(|e| e.into_inner());
        *guard = StatusInfo { status, error };
        match status {
            PluginStatus::Enabled => self.hub.provider_up(&self.manifest.id),
            PluginStatus::Disabled | PluginStatus::Error => {
                self.hub.provider_down(&self.manifest.id)
            }
        }
    }

    /// Store what a finished call said about the plugin's states, if the plugin
    /// is still the running one. A call that was in flight when the plugin was
    /// disabled, failed or replaced by a newer copy must not leave a value
    /// behind: "stopped" means everything it said is gone, and a restarted
    /// plugin starts from `unknown`.
    pub(crate) fn apply_states(&self, writes: Vec<Write>) {
        if writes.is_empty() {
            return;
        }
        let guard = self.status.read().unwrap_or_else(|e| e.into_inner());
        if guard.status != PluginStatus::Enabled {
            self.log.host(
                Level::Warn,
                "states from a call that ended after the plugin stopped were dropped",
            );
            return;
        }
        for write in writes {
            // The gate already built the address from this manifest; this is the
            // same rule once more, at the one place that touches the hub.
            if write.address.provider != self.manifest.id {
                continue;
            }
            match write.value {
                Some(value) => {
                    if let Err(e) = self.hub.set(&write.address, value) {
                        self.log
                            .host(Level::Warn, &format!("state was not stored: {e}"));
                    }
                }
                None => {
                    self.hub.clear(&write.address);
                }
            }
        }
    }

    /// This copy has been replaced by a newer one (a reload, a changed setting):
    /// stop it, so that nothing it still has in flight can reach the hub. The
    /// provider is reported stopped, and the replacement then reports it
    /// running again with every state `unknown`.
    pub fn retire(&self) {
        self.set_status(PluginStatus::Disabled, None);
        *self.sandbox_slot() = None;
    }

    fn sandbox_slot(&self) -> std::sync::MutexGuard<'_, Option<Sandbox>> {
        match self.sandbox.lock() {
            Ok(g) => g,
            Err(poisoned) => {
                // Something panicked mid-call: the instance cannot be trusted.
                let mut g = poisoned.into_inner();
                *g = None;
                g
            }
        }
    }

    /// Load the module and run `init`. Any failure leaves the plugin in
    /// `error` with a readable reason; it never panics or propagates.
    pub fn activate(&self, wasm: &[u8], config: &Map<String, Value>) {
        let mut slot = self.sandbox_slot();
        *slot = None; // Drop any previous instance first.
        let started = std::time::Instant::now();
        let result = Sandbox::load(
            wasm,
            self.manifest.abi,
            self.declared_events.clone(),
            self.gate.clone(),
            self.net.clone().map(|b| b as Arc<dyn NetGate>),
            self.log.clone(),
            &self.limits,
        )
        .and_then(|mut sandbox| {
            let request = init_request(&self.manifest, config);
            let out = match sandbox.call_offline(request.as_bytes()) {
                Ok(out) => out,
                Err(CallError::Fatal(m) | CallError::Rejected(m)) => {
                    return Err(format!("init {m}"))
                }
            };
            match parse_reply(&out.response) {
                Ok(Reply::Ok(_)) => {}
                Ok(Reply::Failed(m)) => return Err(format!("init failed: {m}")),
                Err(m) => return Err(format!("init returned an unusable response: {m}")),
            }
            if !out.events.is_empty() {
                self.log.host(
                    Level::Warn,
                    "events published during init are dropped; publish from an action instead",
                );
            }
            if !out.states.is_empty() {
                self.log.host(
                    Level::Warn,
                    "states set during init are dropped: a plugin that starts has not reported yet, so its states read unknown until an action sets them",
                );
            }
            Ok(sandbox)
        });
        match result {
            Ok(sandbox) => {
                *slot = Some(sandbox);
                self.set_status(PluginStatus::Enabled, None);
                self.log.host(
                    Level::Info,
                    &format!("loaded in {} ms", started.elapsed().as_millis()),
                );
            }
            Err(message) => {
                self.set_status(PluginStatus::Error, Some(message.clone()));
                self.log
                    .host(Level::Error, &format!("could not load: {message}"));
            }
        }
    }

    /// Put the plugin in `error` without starting it (its folder is unusable).
    pub fn mark_error(&self, message: &str) {
        *self.sandbox_slot() = None;
        self.set_status(PluginStatus::Error, Some(message.to_string()));
        self.log
            .host(Level::Error, &format!("could not load: {message}"));
    }

    /// Stop the plugin. It will not receive invocations until activated again.
    pub fn deactivate(&self) {
        *self.sandbox_slot() = None;
        self.set_status(PluginStatus::Disabled, None);
        self.log.host(Level::Info, "disabled");
    }

    fn fail(&self, slot: &mut Option<Sandbox>, message: &str) -> ActionError {
        *slot = None;
        self.set_status(PluginStatus::Error, Some(message.to_string()));
        self.log.host(Level::Error, &format!("stopped: {message}"));
        ActionError::Failed(format!("plugin `{}` failed: {message}", self.manifest.id))
    }

    fn refuse_if_stopped(&self) -> Result<(), ActionError> {
        let id = &self.manifest.id;
        match self.status() {
            (PluginStatus::Disabled, _) => {
                Err(ActionError::Failed(format!("plugin `{id}` is disabled")))
            }
            (PluginStatus::Error, error) => Err(ActionError::Failed(format!(
                "plugin `{id}` is unavailable: {}",
                error.unwrap_or_else(|| "it failed to load".into())
            ))),
            (PluginStatus::Enabled, _) => Ok(()),
        }
    }

    /// One request to the running module: what it answered and what it
    /// published. The instance is held for the whole call, network waits
    /// included, so a plugin handles one thing at a time.
    fn call(&self, what: &str, request: &str) -> Result<(Reply, CallOutput), ActionError> {
        let id = &self.manifest.id;
        let mut slot = self.sandbox_slot();
        let Some(sandbox) = slot.as_mut() else {
            // Disabled or stopped while this call waited for the lock.
            return Err(ActionError::Failed(format!("plugin `{id}` is not running")));
        };
        let output = match sandbox.call(request.as_bytes()) {
            Ok(out) => out,
            Err(CallError::Fatal(m)) => return Err(self.fail(&mut slot, &m)),
            Err(CallError::Rejected(m)) => {
                self.log
                    .host(Level::Warn, &format!("call to `{what}` rejected: {m}"));
                return Err(ActionError::Failed(format!("plugin `{id}` {m}")));
            }
        };
        drop(slot);
        match parse_reply(&output.response) {
            Ok(reply) => Ok((reply, output)),
            Err(m) => {
                self.log.host(
                    Level::Warn,
                    &format!("`{what}` returned an unusable response: {m}"),
                );
                Err(ActionError::Failed(format!(
                    "plugin `{id}` returned an unusable response: {m}"
                )))
            }
        }
    }

    /// What the module published is real once the call ended normally. States
    /// first, then the events about them, so that anything listening to an
    /// event finds the value it is about already in the hub.
    fn deliver(&self, ctx: &PluginContext, output: CallOutput) {
        self.apply_states(output.states);
        for event in output.events {
            ctx.publish(&event.kind, event.payload);
        }
    }

    /// Whether the manifest asks to be refreshed, and how often.
    pub fn refresh_interval(&self) -> Option<std::time::Duration> {
        self.manifest
            .refresh
            .map(|r| std::time::Duration::from_secs(u64::from(r.interval_seconds)))
    }

    /// Ask the plugin to report the instances of its states that something is
    /// showing. Nothing is asked of a plugin that did not declare `refresh`.
    pub fn refresh(
        &self,
        ctx: &PluginContext,
        watching: &[StateAddress],
    ) -> Result<(), ActionError> {
        if self.manifest.refresh.is_none() {
            return Ok(());
        }
        self.refuse_if_stopped()?;
        let (reply, output) = self.call("refresh", &refresh_request(watching))?;
        self.deliver(ctx, output);
        match reply {
            Reply::Ok(_) => Ok(()),
            Reply::Failed(message) => {
                self.log
                    .host(Level::Warn, &format!("refresh failed: {message}"));
                Err(ActionError::Failed(message))
            }
        }
    }
}

impl Plugin for WasmPlugin {
    /// The states and the events the manifest declares. A plugin of schema 1 or 2
    /// declares no states, and its events are listed with the name they have.
    fn observables(&self) -> Vec<ObservableDefinition> {
        self.manifest.observables()
    }

    fn manifest(&self) -> PluginManifest {
        let (status, error) = self.status();
        PluginManifest {
            id: self.manifest.id.clone(),
            name: self.manifest.name.clone(),
            version: self.manifest.version.clone(),
            description: self.manifest.description.clone(),
            kind: ProviderKind::Plugin,
            actions: self
                .manifest
                .actions
                .iter()
                .map(|a| ActionDef {
                    id: a.id.clone(),
                    label: a.label.clone().unwrap_or_else(|| a.id.clone()),
                    description: a.description.clone(),
                    category: a.category.clone(),
                    keywords: a.keywords.clone(),
                    params: a.params.clone(),
                    privileged: false,
                    appearance: a.appearance.clone(),
                    suggests: a.suggests.clone(),
                })
                .collect(),
            states: vec![],
            permissions: if self.manifest.capabilities.network.is_some() {
                vec!["network".to_string()]
            } else {
                vec![]
            },
            external: Some(ExternalInfo {
                status,
                error,
                author: self.manifest.author.clone(),
                abi: self.manifest.abi,
                sdk: self.manifest.sdk.clone(),
                events: self
                    .manifest
                    .events
                    .iter()
                    .map(|e| EventInfo {
                        kind: e.kind.clone(),
                        description: e.description.clone(),
                    })
                    .collect(),
            }),
        }
    }

    fn invoke(
        &self,
        ctx: &PluginContext,
        action: &str,
        args: &Value,
    ) -> Result<Value, ActionError> {
        let id = &self.manifest.id;
        self.refuse_if_stopped()?;
        let Some(spec) = self.manifest.action(action) else {
            return Err(ActionError::UnknownAction {
                plugin: id.clone(),
                action: action.to_string(),
            });
        };
        let args = spec.normalize(args)?;
        let (reply, output) = self.call(action, &invoke_request(action, &args))?;
        self.deliver(ctx, output);
        match reply {
            Reply::Ok(value) => Ok(value),
            Reply::Failed(message) => Err(ActionError::Failed(message)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugins::external::manifest::ConfigType;

    #[test]
    fn replies_are_normalized() {
        assert_eq!(
            parse_reply(br#"{"status":"ok","value":{"a":1}}"#).unwrap(),
            Reply::Ok(json!({"a": 1}))
        );
        assert_eq!(
            parse_reply(br#"{"status":"ok"}"#).unwrap(),
            Reply::Ok(Value::Null)
        );
        assert_eq!(
            parse_reply(br#"{"status":"failed","error":"nope","code":"bad_thing"}"#).unwrap(),
            Reply::Failed("nope (bad_thing)".into())
        );
        assert_eq!(
            parse_reply(br#"{"status":"failed"}"#).unwrap(),
            Reply::Failed("the action failed".into())
        );
        // A forged code is not echoed.
        assert_eq!(
            parse_reply(br#"{"status":"failed","error":"x","code":"a b\nc"}"#).unwrap(),
            Reply::Failed("x".into())
        );
    }

    #[test]
    fn malformed_replies_are_errors_not_panics() {
        for bad in [
            &b""[..],
            b"not json",
            b"[1]",
            b"\"ok\"",
            b"{}",
            b"{\"status\":7}",
            b"{\"status\":\"pending\",\"token\":\"t\"}",
            b"{\"status\":\"maybe\"}",
            &[0xff, 0xfe][..],
        ] {
            assert!(
                parse_reply(bad).is_err(),
                "{:?}",
                String::from_utf8_lossy(bad)
            );
        }
    }

    #[test]
    fn effective_config_keeps_declared_typed_values_only() {
        let fields = vec![
            ConfigField {
                key: "greeting".into(),
                kind: ConfigType::String,
                default: Some(json!("Hello")),
                secret: false,
                description: String::new(),
                label: None,
            },
            ConfigField {
                key: "n".into(),
                kind: ConfigType::Number,
                default: None,
                secret: false,
                description: String::new(),
                label: None,
            },
        ];
        let mut saved = Map::new();
        saved.insert("greeting".into(), json!("Hi"));
        saved.insert("n".into(), json!("not a number"));
        saved.insert("undeclared".into(), json!(1));
        let c = effective_config(&fields, &saved);
        assert_eq!(c.get("greeting"), Some(&json!("Hi")));
        assert!(c.get("n").is_none());
        assert!(c.get("undeclared").is_none());
        assert_eq!(
            effective_config(&fields, &Map::new()).get("greeting"),
            Some(&json!("Hello"))
        );
    }
}
