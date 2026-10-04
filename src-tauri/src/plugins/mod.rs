//! Minimal internal plugin runtime.
//!
//! A plugin exposes *semantic capabilities* (actions and states). It never
//! sees MIDI note numbers, never drives LEDs and never knows where a pad
//! lives: surfaces map capabilities to controls and the renderer maps controls
//! to hardware.
//!
//! Permissions are declared in the manifest and surfaced in the UI. They are
//! documented but NOT enforced in v0.1 (see docs/plugins.md).

pub mod demo;
pub mod external;
pub mod keyboard;
pub mod system;

use std::collections::BTreeMap;
use std::sync::{Arc, RwLock};
use std::time::Duration;

use serde::Serialize;
use serde_json::Value;

use crate::actions::definition::{Appearance, Availability, Issue, ProviderKind};
use crate::actions::params::ParamDef;
use crate::core::model::Rgb;
use crate::events::{Event, EventBus};

#[derive(Debug, thiserror::Error, Clone, PartialEq, Eq)]
pub enum ActionError {
    #[error("unknown plugin `{0}`")]
    UnknownPlugin(String),
    #[error("plugin `{plugin}` has no action `{action}`")]
    UnknownAction { plugin: String, action: String },
    #[error("invalid arguments: {0}")]
    InvalidArgs(String),
    #[error("{0}")]
    Failed(String),
}

impl ActionError {
    /// The sentence without the error's own prefix, for showing next to a
    /// field ("scheme `ftp` is not allowed") instead of "invalid arguments: ...".
    pub fn message(&self) -> String {
        match self {
            ActionError::InvalidArgs(m) | ActionError::Failed(m) => m.clone(),
            other => other.to_string(),
        }
    }
}

/// What a provider declares about one action. The registry turns this, plus
/// the provider's identity and live availability, into an `ActionDefinition`.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ActionDef {
    /// Stable within the provider. Never derived from the label.
    pub id: String,
    /// The display name.
    pub label: String,
    pub description: String,
    /// Library category. The provider's name when absent.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub keywords: Vec<String>,
    /// The settings a person fills in. Empty for an action without any.
    pub params: Vec<ParamDef>,
    /// Runs a command or a request on behalf of the person.
    pub privileged: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub appearance: Option<Appearance>,
}

impl ActionDef {
    pub fn new(id: &str, label: &str, description: &str) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            description: description.into(),
            category: None,
            keywords: vec![],
            params: vec![],
            privileged: false,
            appearance: None,
        }
    }

    pub fn category(mut self, category: &str) -> Self {
        self.category = Some(category.into());
        self
    }

    pub fn keywords(mut self, words: &[&str]) -> Self {
        self.keywords = words.iter().map(|w| (*w).to_string()).collect();
        self
    }

    pub fn param(mut self, param: ParamDef) -> Self {
        self.params.push(param);
        self
    }

    pub fn privileged(mut self) -> Self {
        self.privileged = true;
        self
    }

    /// A suggested look for a control that runs this action.
    pub fn look(mut self, label: &str, color: Rgb, icon: &str) -> Self {
        self.appearance = Some(Appearance::new(label, color, icon));
        self
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct StateDef {
    pub key: String,
    pub description: String,
    pub values: Vec<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PluginManifest {
    pub id: String,
    pub name: String,
    pub version: String,
    pub description: String,
    /// Where this provider comes from; decides how the library labels it.
    pub kind: ProviderKind,
    pub actions: Vec<ActionDef>,
    pub states: Vec<StateDef>,
    /// `network`, `filesystem`, `shell`, `automation`, `accessibility`, `midi`.
    pub permissions: Vec<String>,
    /// Present for plugins installed from disk, absent for built-in ones.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub external: Option<external::ExternalInfo>,
}

/// What the runtime gives a plugin while it runs an action.
#[derive(Clone)]
pub struct PluginContext {
    plugin_id: String,
    bus: EventBus,
    handle: tokio::runtime::Handle,
}

impl PluginContext {
    pub fn new(plugin_id: &str, bus: EventBus, handle: tokio::runtime::Handle) -> Self {
        Self {
            plugin_id: plugin_id.to_string(),
            bus,
            handle,
        }
    }

    /// Publish a semantic state change (`plugin.state.changed`).
    pub fn publish_state(&self, key: &str, value: &str) {
        self.bus
            .publish(Event::plugin_state(&self.plugin_id, key, value));
    }

    /// Publish a semantic event with this plugin's id as its source.
    pub fn publish(&self, kind: &str, payload: Value) {
        self.bus.publish(Event::new(&self.plugin_id, kind, payload));
    }

    /// Drive a future to completion from the blocking thread a plugin runs on.
    /// Only native plugins use this; the sandbox has no such door.
    pub fn block_on<F: std::future::Future>(&self, future: F) -> F::Output {
        self.handle.block_on(future)
    }

    /// Run `f` after `delay` on the runtime.
    pub fn spawn_after(&self, delay: Duration, f: impl FnOnce() + Send + 'static) {
        self.handle.spawn(async move {
            tokio::time::sleep(delay).await;
            f();
        });
    }
}

pub trait Plugin: Send + Sync {
    fn manifest(&self) -> PluginManifest;
    fn invoke(&self, ctx: &PluginContext, action: &str, args: &Value)
        -> Result<Value, ActionError>;

    /// Whether `action` can run right now. Defaults to yes. A provider that
    /// knows why an action cannot run (it is disabled, nothing is waiting)
    /// says so here, and the Action Library shows the reason.
    fn availability(&self, _action: &str) -> Availability {
        Availability::Available
    }

    /// Checks that a parameter schema cannot express (a URL scheme, a key
    /// combination). Called with arguments that already passed the schema.
    fn check(&self, _action: &str, _args: &Value) -> Result<(), Issue> {
        Ok(())
    }
}

/// Plugins by id. Clones share the same table, so a plugin registered after the
/// action engine was built (an installed plugin, a rescan) is visible to it.
#[derive(Default, Clone)]
pub struct PluginRegistry {
    plugins: Arc<RwLock<BTreeMap<String, Arc<dyn Plugin>>>>,
}

impl PluginRegistry {
    pub fn register(&self, plugin: Arc<dyn Plugin>) {
        let id = plugin.manifest().id;
        self.write().insert(id, plugin);
    }

    /// Remove a plugin. Returns whether it was registered.
    pub fn unregister(&self, id: &str) -> bool {
        self.write().remove(id).is_some()
    }

    pub fn get(&self, id: &str) -> Option<Arc<dyn Plugin>> {
        self.read().get(id).cloned()
    }

    pub fn contains(&self, id: &str) -> bool {
        self.read().contains_key(id)
    }

    /// Every registered plugin. The handles are cloned out, so callers never
    /// run plugin code under the table lock.
    pub fn all(&self) -> Vec<Arc<dyn Plugin>> {
        self.read().values().cloned().collect()
    }

    pub fn manifests(&self) -> Vec<PluginManifest> {
        // Clone the handles first: a manifest call must never run under the table lock.
        let plugins: Vec<Arc<dyn Plugin>> = self.read().values().cloned().collect();
        plugins.iter().map(|p| p.manifest()).collect()
    }

    fn read(&self) -> std::sync::RwLockReadGuard<'_, BTreeMap<String, Arc<dyn Plugin>>> {
        self.plugins.read().unwrap_or_else(|e| e.into_inner())
    }

    fn write(&self) -> std::sync::RwLockWriteGuard<'_, BTreeMap<String, Arc<dyn Plugin>>> {
        self.plugins.write().unwrap_or_else(|e| e.into_inner())
    }
}

pub(crate) fn str_arg<'a>(args: &'a Value, key: &str) -> Result<&'a str, ActionError> {
    args.get(key)
        .and_then(Value::as_str)
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| ActionError::InvalidArgs(format!("`{key}` (string) is required")))
}
