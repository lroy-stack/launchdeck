//! The `plugin.json` manifest of an external plugin.
//!
//! Small, versioned and boring on purpose: it identifies the plugin, says what
//! it needs to run, locates the module, declares the actions, events and
//! configuration the plugin offers, and carries display metadata. It holds no
//! executable content. Parsing is strict (unknown fields are errors) so a
//! typo is reported instead of silently ignored.

use std::collections::HashSet;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::capabilities::{Capabilities, HostSpec, Origin, MAX_HOSTS};
use super::{ProblemKind, ABI_VERSION, SDK_VERSION};
use crate::actions::definition::Appearance;
use crate::actions::params::{normalize, valid_param_name, validate_defs, ParamDef};
use crate::events::valid_ident;
use crate::plugins::ActionError;
use crate::state::address::valid_ident as valid_state_id;
use crate::state::definition::{
    observable_id, EventField, ObservableDefinition, ObservableKind, ValueOption, ValueSpec,
    ValueType, MAX_LABEL,
};
use crate::state::hub::MAX_STATES_PER_PROVIDER;

pub const MANIFEST_FILE: &str = "plugin.json";
/// The newest manifest schema this host reads. Schemas 1 to 3 are still read:
/// schema 1 `args` become parameters, schema 2 plugins simply declare no
/// states, and schema 3 plugins ask for no capability. The module and the
/// protocol (ABI 1) are the same for all four.
pub const MANIFEST_SCHEMA: u32 = 4;
/// The oldest manifest schema this host still reads.
pub(crate) const OLDEST_SCHEMA: u32 = 1;
const MAX_KEYWORDS: usize = 12;
pub const MAX_MANIFEST_BYTES: usize = 64 * 1024;

const MAX_ACTIONS: usize = 64;
const MAX_ARGS: usize = 16;
const MAX_EVENTS: usize = 64;
/// Observables one action may point at as "what a pad for this action usually shows".
const MAX_SUGGESTS: usize = 8;
const MAX_CONFIG: usize = 32;
const MAX_SECRETS: usize = 8;
/// How often a plugin may ask to be refreshed, in seconds.
pub const REFRESH_SECONDS: std::ops::RangeInclusive<u32> = 2..=3600;
const MAX_TEXT: usize = 280;

/// Event type prefixes that belong to LaunchDeck itself. A plugin may not
/// declare or publish them, so it cannot impersonate runtime behaviour.
const HOST_EVENT_PREFIXES: &[&str] = &[
    "control.", "action.", "surface.", "page.", "device.", "plugin.", "agent.", "runtime.",
    "system.",
];

#[derive(Debug, thiserror::Error, Clone, PartialEq, Eq)]
pub enum ManifestError {
    #[error("manifest is not valid JSON: {0}")]
    Malformed(String),
    #[error("manifest schema version {found} is not supported (this LaunchDeck reads versions 1 to {supported})")]
    UnsupportedSchema { found: String, supported: u32 },
    #[error("plugin targets protocol (ABI) {found}, this LaunchDeck provides {supported}")]
    IncompatibleAbi { found: u32, supported: u32 },
    #[error(
        "plugin was built with SDK {found}, newer than the {supported} this LaunchDeck understands"
    )]
    IncompatibleSdk { found: String, supported: String },
    #[error("capability `{0}` is not available in this LaunchDeck")]
    UnsupportedCapability(String),
    #[error("invalid manifest: {0}")]
    Invalid(String),
}

impl ManifestError {
    /// What kind of problem this is, for the Plugins screen.
    pub fn kind(&self) -> ProblemKind {
        match self {
            ManifestError::UnsupportedSchema { .. }
            | ManifestError::IncompatibleAbi { .. }
            | ManifestError::IncompatibleSdk { .. } => ProblemKind::Incompatible,
            ManifestError::Malformed(_)
            | ManifestError::Invalid(_)
            | ManifestError::UnsupportedCapability(_) => ProblemKind::InvalidManifest,
        }
    }
}

fn invalid<T>(msg: impl Into<String>) -> Result<T, ManifestError> {
    Err(ManifestError::Invalid(msg.into()))
}

/// `major.minor.patch`, numbers only.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Version {
    pub major: u32,
    pub minor: u32,
    pub patch: u32,
}

impl Version {
    pub fn parse(s: &str) -> Option<Version> {
        let mut parts = s.split('.');
        let mut next = || -> Option<u32> {
            let p = parts.next()?;
            if p.is_empty() || p.len() > 6 || !p.bytes().all(|b| b.is_ascii_digit()) {
                return None;
            }
            p.parse().ok()
        };
        let v = Version {
            major: next()?,
            minor: next()?,
            patch: next()?,
        };
        parts.next().is_none().then_some(v)
    }

    /// The same version as `semver` sees it, to match a version requirement
    /// against. Leading zeros, which [`Version::parse`] accepts, drop.
    pub fn to_semver(&self) -> semver::Version {
        semver::Version::new(
            u64::from(self.major),
            u64::from(self.minor),
            u64::from(self.patch),
        )
    }

    /// A plugin built with a newer SDK than the host knows is refused. Before
    /// 1.0 the minor version is the breaking one.
    pub fn plugin_sdk_ok(plugin: Version, host: Version) -> bool {
        if plugin.major != host.major {
            return plugin.major < host.major;
        }
        !(plugin.major == 0 && plugin.minor > host.minor)
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ConfigType {
    String,
    Number,
    Boolean,
}

impl ConfigType {
    pub fn accepts(self, value: &Value) -> bool {
        match self {
            ConfigType::String => value.is_string(),
            ConfigType::Number => value.is_number(),
            ConfigType::Boolean => value.is_boolean(),
        }
    }
}

/// The argument types of manifest schema 1.
#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ArgType {
    String,
    Number,
    Boolean,
    Json,
}

/// A schema 1 argument. Converted to a [`ParamDef`] when the manifest is read.
#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ManifestArg {
    pub name: String,
    #[serde(rename = "type")]
    pub kind: ArgType,
    #[serde(default)]
    pub required: bool,
}

impl ManifestArg {
    fn to_param(&self) -> ParamDef {
        let p = match self.kind {
            ArgType::String => ParamDef::string(&self.name, &self.name),
            ArgType::Number => ParamDef::number(&self.name, &self.name),
            ArgType::Boolean => ParamDef::boolean(&self.name, &self.name),
            ArgType::Json => ParamDef::json(&self.name, &self.name),
        };
        if self.required {
            p.required()
        } else {
            p
        }
    }
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ManifestAction {
    pub id: String,
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default)]
    pub description: String,
    /// Schema 1 only. Read into `params` and then empty.
    #[serde(default)]
    pub args: Vec<ManifestArg>,
    // Schema 2.
    /// Library category. The plugin's name when absent.
    #[serde(default)]
    pub category: Option<String>,
    #[serde(default)]
    pub keywords: Vec<String>,
    #[serde(default)]
    pub appearance: Option<Appearance>,
    /// The settings of the action. Schema 1 `args` end up here too.
    #[serde(default)]
    pub params: Vec<ParamDef>,
    // Schema 3.
    /// Ids of the states or events of this plugin that a pad running this
    /// action usually shows. A hint for the editor; it makes nothing happen.
    #[serde(default)]
    pub suggests: Vec<String>,
}

impl ManifestAction {
    /// Check call arguments against the declared parameters and fill in
    /// defaults. The plugin only ever sees arguments that passed this.
    pub fn normalize(&self, args: &Value) -> Result<Value, ActionError> {
        normalize(&self.params, args).map_err(|issues| {
            ActionError::InvalidArgs(
                issues
                    .into_iter()
                    .map(|i| i.message)
                    .collect::<Vec<_>>()
                    .join("; "),
            )
        })
    }
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ManifestEvent {
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(default)]
    pub description: String,
    // Schema 3.
    /// The name a person sees. The type when absent.
    #[serde(default)]
    pub name: Option<String>,
    /// The payload fields a rule may test, with their types.
    #[serde(default)]
    pub fields: Vec<EventField>,
}

/// A state the plugin lets a pad observe (schema 3). It is the same thing as an
/// [`ObservableDefinition`] of the hub, written the way a manifest author
/// writes it: the type and its values at the top, the provider implied.
#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ManifestState {
    /// Local id: lowercase dotted (`level`, `channel.status`). The provider is
    /// this plugin, so the id other parts of LaunchDeck use is `<plugin id>.<id>`.
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    /// Group in the editor's picker. The plugin's name when absent.
    #[serde(default)]
    pub category: Option<String>,
    #[serde(default)]
    pub keywords: Vec<String>,
    /// `enum`, `boolean`, `number` or `string`. A state is one scalar; anything
    /// with structure is an event payload, not a state.
    #[serde(rename = "type")]
    pub kind: ValueType,
    /// The values of an enum (required), or words for `true` and `false`.
    #[serde(default)]
    pub values: Vec<ValueOption>,
    /// What picks one instance when the state has several (`session`, `channel`).
    /// Same vocabulary as the settings of an action; only text, number, boolean
    /// and enum parameters are allowed.
    #[serde(default)]
    pub params: Vec<ParamDef>,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ConfigField {
    pub key: String,
    #[serde(rename = "type")]
    pub kind: ConfigType,
    #[serde(default)]
    pub default: Option<Value>,
    /// Schemas 1 to 3 only: a credential in the old format. It cannot be
    /// given a value and is never handed to the module, which reads it as not
    /// set; a plain value an earlier version kept is moved or removed on the
    /// person's choice (`legacy_secrets`). Schema 4 refuses it; a credential
    /// goes under `secrets`, which the module never receives either. May not
    /// have a default.
    #[serde(default)]
    pub secret: bool,
    #[serde(default)]
    pub description: String,
    /// Schema 4. The name a person sees. The key when absent.
    #[serde(default)]
    pub label: Option<String>,
}

/// A credential the plugin needs and never sees (schema 4). The host keeps it
/// in the credential store and adds it to requests that go to `host`.
#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ManifestSecret {
    pub key: String,
    pub label: String,
    #[serde(default)]
    pub description: String,
    /// One of the manifest's network hosts: the only origin it is sent to.
    pub host: HostSpec,
}

/// How often the host asks the plugin to report the states that are being
/// shown (schema 4).
#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RefreshDecl {
    pub interval_seconds: u32,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ExternalManifest {
    pub schema_version: u32,
    pub id: String,
    pub name: String,
    pub version: String,
    #[serde(default)]
    pub description: String,
    pub author: String,
    /// Protocol (ABI) version this module speaks.
    pub abi: u32,
    /// SDK version the plugin was built with. Informational and checked for
    /// major compatibility only.
    #[serde(default)]
    pub sdk: Option<String>,
    /// File name of the `.wasm` module, inside the plugin directory.
    pub main: String,
    #[serde(default)]
    pub actions: Vec<ManifestAction>,
    #[serde(default)]
    pub events: Vec<ManifestEvent>,
    /// Schema 3.
    #[serde(default)]
    pub states: Vec<ManifestState>,
    #[serde(default)]
    pub config: Vec<ConfigField>,
    /// What the plugin asks the host to do for it. An empty array in schemas 1
    /// to 3; an object in schema 4. See PLUGIN_CAPABILITIES_PLAN.md.
    #[serde(default)]
    pub capabilities: Capabilities,
    /// Schema 4.
    #[serde(default)]
    pub secrets: Vec<ManifestSecret>,
    /// Schema 4.
    #[serde(default)]
    pub refresh: Option<RefreshDecl>,
}

/// Ids under this prefix belong to LaunchDeck itself, so an installed plugin
/// can never pass for a first-party one.
const RESERVED_ID_PREFIX: &str = "launchdeck.";

/// Lowercase dotted id with at least two segments, for example `acme.notes`,
/// outside the reserved `launchdeck.` prefix.
pub fn valid_plugin_id(id: &str) -> bool {
    if id.len() < 3 || id.len() > 64 || id.starts_with(RESERVED_ID_PREFIX) {
        return false;
    }
    let mut segments = 0;
    for seg in id.split('.') {
        segments += 1;
        let mut chars = seg.chars();
        let Some(first) = chars.next() else {
            return false;
        };
        if !first.is_ascii_lowercase()
            || !chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        {
            return false;
        }
    }
    segments >= 2
}

fn valid_name(s: &str) -> bool {
    let t = s.trim();
    !t.is_empty() && t.chars().count() <= 64 && !t.chars().any(char::is_control)
}

fn valid_text(s: &str) -> bool {
    s.chars().count() <= MAX_TEXT && !s.chars().any(|c| c.is_control() && c != '\n')
}

/// A plain file name ending in `.wasm`. No separators, no `..`, no dot files.
fn valid_main(s: &str) -> bool {
    s.len() <= 64
        && s.ends_with(".wasm")
        && s.len() > ".wasm".len()
        && !s.starts_with('.')
        && !s.contains("..")
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'))
}

pub fn is_host_event_type(kind: &str) -> bool {
    HOST_EVENT_PREFIXES.iter().any(|p| kind.starts_with(p))
}

impl ExternalManifest {
    /// Parse and fully validate a manifest. Never panics on hostile input.
    pub fn parse(json: &str) -> Result<Self, ManifestError> {
        if json.len() > MAX_MANIFEST_BYTES {
            return invalid(format!(
                "manifest is larger than {MAX_MANIFEST_BYTES} bytes"
            ));
        }
        let raw: Value =
            serde_json::from_str(json).map_err(|e| ManifestError::Malformed(e.to_string()))?;
        let Some(obj) = raw.as_object() else {
            return Err(ManifestError::Malformed("expected a JSON object".into()));
        };
        // The schema version decides how the rest is read, so it is checked first.
        match obj.get("schemaVersion") {
            None => return invalid("`schemaVersion` is required"),
            Some(v)
                if v.as_u64().is_some_and(|n| {
                    (OLDEST_SCHEMA as u64..=MANIFEST_SCHEMA as u64).contains(&n)
                }) => {}
            Some(v) => {
                return Err(ManifestError::UnsupportedSchema {
                    found: v.to_string(),
                    supported: MANIFEST_SCHEMA,
                })
            }
        }
        let mut manifest: ExternalManifest =
            serde_json::from_value(raw).map_err(|e| ManifestError::Invalid(e.to_string()))?;
        manifest.validate()?;
        manifest.upgrade_args();
        Ok(manifest)
    }

    fn validate(&self) -> Result<(), ManifestError> {
        if self.id.starts_with(RESERVED_ID_PREFIX) {
            return invalid("ids starting with launchdeck. are reserved for LaunchDeck itself");
        }
        if !valid_plugin_id(&self.id) {
            return invalid(format!(
                "`id` \"{}\" must be lowercase dotted with at least two parts, for example \"acme.notes\"",
                self.id
            ));
        }
        if !valid_name(&self.name) {
            return invalid("`name` must be 1-64 characters");
        }
        if !valid_name(&self.author) {
            return invalid("`author` must be 1-64 characters");
        }
        if !valid_text(&self.description) {
            return invalid(format!(
                "`description` is longer than {MAX_TEXT} characters"
            ));
        }
        if Version::parse(&self.version).is_none() {
            return invalid(format!(
                "`version` \"{}\" must look like 1.2.3",
                self.version
            ));
        }
        if self.abi != ABI_VERSION {
            return Err(ManifestError::IncompatibleAbi {
                found: self.abi,
                supported: ABI_VERSION,
            });
        }
        if let Some(sdk) = &self.sdk {
            let Some(plugin_sdk) = Version::parse(sdk) else {
                return invalid(format!("`sdk` \"{sdk}\" must look like 1.2.3"));
            };
            let host = Version::parse(SDK_VERSION).expect("SDK_VERSION is valid");
            if !Version::plugin_sdk_ok(plugin_sdk, host) {
                return Err(ManifestError::IncompatibleSdk {
                    found: sdk.clone(),
                    supported: SDK_VERSION.to_string(),
                });
            }
        }
        if !valid_main(&self.main) {
            return invalid(format!(
                "`main` \"{}\" must be a plain file name ending in .wasm",
                self.main
            ));
        }
        self.validate_actions()?;
        self.validate_events()?;
        self.validate_states()?;
        self.validate_suggests()?;
        self.validate_config()?;
        self.validate_capabilities()?;
        self.validate_secrets()?;
        self.validate_refresh()
    }

    fn validate_capabilities(&self) -> Result<(), ManifestError> {
        // The array of schemas 1 to 3 names nothing this host ever provided that way.
        if let Some(cap) = self.capabilities.legacy.first() {
            return Err(ManifestError::UnsupportedCapability(cap.clone()));
        }
        if self.capabilities.object && self.schema_version < 4 {
            return invalid("`capabilities` as an object needs \"schemaVersion\": 4");
        }
        let Some(net) = &self.capabilities.network else {
            return Ok(());
        };
        if net.purpose.trim().is_empty() || !valid_text(&net.purpose) {
            return invalid(format!(
                "network `purpose` must say in 1-{MAX_TEXT} characters why the plugin needs the network"
            ));
        }
        if net.hosts.is_empty() || net.hosts.len() > MAX_HOSTS {
            return invalid(format!(
                "network `hosts` must list 1 to {MAX_HOSTS} destinations"
            ));
        }
        let mut seen = HashSet::new();
        for host in &net.hosts {
            let key = match host {
                HostSpec::Origin(text) => {
                    let origin = Origin::declared(text).map_err(|e| {
                        ManifestError::Invalid(format!("network host \"{text}\": {e}"))
                    })?;
                    origin.as_str().to_string()
                }
                HostSpec::Setting { setting } => {
                    let Some(field) = self.config.iter().find(|c| &c.key == setting) else {
                        return invalid(format!(
                            "network host names the setting \"{setting}\", which `config` does not declare"
                        ));
                    };
                    if field.kind != ConfigType::String {
                        return invalid(format!(
                            "network host names the setting \"{setting}\", which must be a string (a URL)"
                        ));
                    }
                    if let Some(default) = field.default.as_ref().and_then(Value::as_str) {
                        Origin::of(default).map_err(|e| {
                            ManifestError::Invalid(format!("default of setting \"{setting}\": {e}"))
                        })?;
                    }
                    format!("setting:{setting}")
                }
            };
            if !seen.insert(key) {
                return invalid("a network host is listed twice");
            }
        }
        Ok(())
    }

    fn validate_secrets(&self) -> Result<(), ManifestError> {
        if self.secrets.is_empty() {
            return Ok(());
        }
        if self.schema_version < 4 {
            return invalid("`secrets` needs \"schemaVersion\": 4");
        }
        if self.secrets.len() > MAX_SECRETS {
            return invalid(format!("more than {MAX_SECRETS} secrets"));
        }
        let hosts = self
            .capabilities
            .network
            .as_ref()
            .map(|n| n.hosts.as_slice())
            .unwrap_or(&[]);
        let mut seen = HashSet::new();
        for s in &self.secrets {
            if !valid_param_name(&s.key) {
                return invalid(format!(
                    "secret key \"{}\" must be a plain identifier",
                    s.key
                ));
            }
            if !seen.insert(s.key.as_str()) || self.config.iter().any(|c| c.key == s.key) {
                return invalid(format!(
                    "\"{}\" is declared twice (secrets and settings share one namespace)",
                    s.key
                ));
            }
            if !valid_name(&s.label) {
                return invalid(format!(
                    "secret \"{}\" needs a label of 1-64 characters",
                    s.key
                ));
            }
            if !valid_text(&s.description) {
                return invalid(format!("secret \"{}\" description is too long", s.key));
            }
            // A secret is only ever sent somewhere the manifest asks to reach.
            if !hosts.contains(&s.host) {
                return invalid(format!(
                    "secret \"{}\" must name, in `host`, one of the hosts in `capabilities.network.hosts`",
                    s.key
                ));
            }
        }
        Ok(())
    }

    fn validate_refresh(&self) -> Result<(), ManifestError> {
        let Some(refresh) = self.refresh else {
            return Ok(());
        };
        if self.schema_version < 4 {
            return invalid("`refresh` needs \"schemaVersion\": 4");
        }
        if !REFRESH_SECONDS.contains(&refresh.interval_seconds) {
            return invalid(format!(
                "`refresh.intervalSeconds` must be between {} and {}",
                REFRESH_SECONDS.start(),
                REFRESH_SECONDS.end()
            ));
        }
        if self.states.is_empty() {
            return invalid("`refresh` is for a plugin that declares states");
        }
        Ok(())
    }

    fn validate_actions(&self) -> Result<(), ManifestError> {
        if self.actions.len() > MAX_ACTIONS {
            return invalid(format!("more than {MAX_ACTIONS} actions"));
        }
        let mut seen = HashSet::new();
        for a in &self.actions {
            if !valid_ident(&a.id) {
                return invalid(format!(
                    "action id \"{}\" must be 1-64 characters of [A-Za-z0-9_.:-]",
                    a.id
                ));
            }
            if !seen.insert(a.id.as_str()) {
                return invalid(format!("action \"{}\" is declared twice", a.id));
            }
            if a.label.as_deref().is_some_and(|l| !valid_name(l)) {
                return invalid(format!("action \"{}\" has an invalid label", a.id));
            }
            if !valid_text(&a.description) {
                return invalid(format!("action \"{}\" description is too long", a.id));
            }
            if self.schema_version < 3 && !a.suggests.is_empty() {
                return invalid(format!(
                    "action \"{}\" uses `suggests`, which needs \"schemaVersion\": 3",
                    a.id
                ));
            }
            if self.schema_version == 1 {
                if a.category.is_some()
                    || !a.keywords.is_empty()
                    || a.appearance.is_some()
                    || !a.params.is_empty()
                {
                    return invalid(format!(
                        "action \"{}\" uses category, keywords, appearance or params, which need \"schemaVersion\": 2",
                        a.id
                    ));
                }
            } else if !a.args.is_empty() {
                return invalid(format!(
                    "action \"{}\" uses `args`; schema 2 calls them `params`",
                    a.id
                ));
            }
            if a.args.len() > MAX_ARGS {
                return invalid(format!("action \"{}\" has more than {MAX_ARGS} args", a.id));
            }
            let mut names = HashSet::new();
            for arg in &a.args {
                if !valid_param_name(&arg.name) {
                    return invalid(format!(
                        "action \"{}\" arg \"{}\" must be a plain identifier",
                        a.id, arg.name
                    ));
                }
                if !names.insert(arg.name.as_str()) {
                    return invalid(format!(
                        "action \"{}\" declares arg \"{}\" twice",
                        a.id, arg.name
                    ));
                }
            }
            if a.category
                .as_deref()
                .is_some_and(|c| !valid_name(c) || c.chars().count() > 40)
            {
                return invalid(format!("action \"{}\" has an invalid category", a.id));
            }
            if a.keywords.len() > MAX_KEYWORDS
                || a.keywords.iter().any(|k| {
                    k.trim().is_empty() || k.chars().count() > 32 || k.chars().any(char::is_control)
                })
            {
                return invalid(format!(
                    "action \"{}\" keywords must be at most {MAX_KEYWORDS} short words",
                    a.id
                ));
            }
            if let Some(look) = &a.appearance {
                look.check()
                    .map_err(|e| ManifestError::Invalid(format!("action \"{}\": {e}", a.id)))?;
            }
            validate_defs(&a.params)
                .map_err(|e| ManifestError::Invalid(format!("action \"{}\": {e}", a.id)))?;
        }
        Ok(())
    }

    fn validate_events(&self) -> Result<(), ManifestError> {
        if self.events.len() > MAX_EVENTS {
            return invalid(format!("more than {MAX_EVENTS} events"));
        }
        let mut seen = HashSet::new();
        for e in &self.events {
            if !valid_ident(&e.kind) {
                return invalid(format!(
                    "event type \"{}\" must be 1-64 characters of [A-Za-z0-9_.:-]",
                    e.kind
                ));
            }
            if is_host_event_type(&e.kind) {
                return invalid(format!(
                    "event type \"{}\" uses a prefix that belongs to LaunchDeck",
                    e.kind
                ));
            }
            if !seen.insert(e.kind.as_str()) {
                return invalid(format!("event \"{}\" is declared twice", e.kind));
            }
            if !valid_text(&e.description) {
                return invalid(format!("event \"{}\" description is too long", e.kind));
            }
            if self.schema_version < 3 && (e.name.is_some() || !e.fields.is_empty()) {
                return invalid(format!(
                    "event \"{}\" uses `name` or `fields`, which need \"schemaVersion\": 3",
                    e.kind
                ));
            }
            if e.name.as_deref().is_some_and(|n| !valid_name(n)) {
                return invalid(format!("event \"{}\" has an invalid name", e.kind));
            }
            // What the catalog will list must be well formed, so the manifest and
            // the catalog can never disagree about it.
            self.event_definition(e)
                .check()
                .map_err(|why| ManifestError::Invalid(format!("event \"{}\": {why}", e.kind)))?;
        }
        Ok(())
    }

    fn validate_states(&self) -> Result<(), ManifestError> {
        if self.schema_version < 3 {
            if !self.states.is_empty() {
                return invalid("`states` needs \"schemaVersion\": 3");
            }
            return Ok(());
        }
        if self.states.len() > MAX_STATES_PER_PROVIDER {
            return invalid(format!("more than {MAX_STATES_PER_PROVIDER} states"));
        }
        let mut seen = HashSet::new();
        for s in &self.states {
            if !valid_state_id(&s.id) {
                return invalid(format!(
                    "state id \"{}\" must be lowercase letters, digits and `-` in dotted parts, for example \"level\" or \"channel.status\"",
                    s.id
                ));
            }
            if !seen.insert(s.id.as_str()) {
                return invalid(format!("state \"{}\" is declared twice", s.id));
            }
            if self.events.iter().any(|e| e.kind == s.id) {
                return invalid(format!(
                    "\"{}\" is declared as a state and as an event; a state is how things are now, an event is something that happened, so they need different ids",
                    s.id
                ));
            }
            if s.category
                .as_deref()
                .is_some_and(|c| !valid_name(c) || c.chars().count() > MAX_LABEL)
            {
                return invalid(format!("state \"{}\" has an invalid category", s.id));
            }
            self.state_definition(s)
                .check()
                .map_err(|why| ManifestError::Invalid(format!("state \"{}\": {why}", s.id)))?;
        }
        Ok(())
    }

    fn validate_suggests(&self) -> Result<(), ManifestError> {
        for a in &self.actions {
            if a.suggests.len() > MAX_SUGGESTS {
                return invalid(format!(
                    "action \"{}\" suggests more than {MAX_SUGGESTS} observables",
                    a.id
                ));
            }
            let mut seen = HashSet::new();
            for target in &a.suggests {
                let known = self.states.iter().any(|s| &s.id == target)
                    || self.events.iter().any(|e| &e.kind == target);
                if !known {
                    return invalid(format!(
                        "action \"{}\" suggests \"{target}\", which this plugin declares neither as a state nor as an event",
                        a.id
                    ));
                }
                if !seen.insert(target.as_str()) {
                    return invalid(format!("action \"{}\" suggests \"{target}\" twice", a.id));
                }
            }
        }
        Ok(())
    }

    fn validate_config(&self) -> Result<(), ManifestError> {
        if self.config.len() > MAX_CONFIG {
            return invalid(format!("more than {MAX_CONFIG} config keys"));
        }
        let mut seen = HashSet::new();
        for c in &self.config {
            if !valid_param_name(&c.key) {
                return invalid(format!(
                    "config key \"{}\" must be a plain identifier",
                    c.key
                ));
            }
            if !seen.insert(c.key.as_str()) {
                return invalid(format!("config key \"{}\" is declared twice", c.key));
            }
            if c.secret && self.schema_version >= 4 {
                return invalid(format!(
                    "config key \"{}\" is marked secret; from schema 4 a credential is declared under `secrets`, where the module never receives it",
                    c.key
                ));
            }
            if c.label.is_some() && self.schema_version < 4 {
                return invalid(format!(
                    "config key \"{}\" uses `label`, which needs \"schemaVersion\": 4",
                    c.key
                ));
            }
            if c.label.as_deref().is_some_and(|l| !valid_name(l)) {
                return invalid(format!("config key \"{}\" has an invalid label", c.key));
            }
            if let Some(default) = &c.default {
                if c.secret {
                    return invalid(format!(
                        "secret config key \"{}\" may not ship a default",
                        c.key
                    ));
                }
                if !c.kind.accepts(default) {
                    return invalid(format!(
                        "default of config key \"{}\" does not match its type",
                        c.key
                    ));
                }
            }
            if !valid_text(&c.description) {
                return invalid(format!("config key \"{}\" description is too long", c.key));
            }
        }
        Ok(())
    }

    /// Schema 1 arguments become parameters, so everything downstream reads
    /// one vocabulary. Their label is their name, as the UI showed before.
    fn upgrade_args(&mut self) {
        for a in &mut self.actions {
            if a.params.is_empty() && !a.args.is_empty() {
                a.params = a.args.iter().map(ManifestArg::to_param).collect();
            }
            a.args.clear();
        }
    }

    /// The group a state or event is listed under when it names none: the
    /// plugin's name, cut to what a category may hold.
    fn default_category(&self) -> String {
        self.name.trim().chars().take(MAX_LABEL).collect()
    }

    fn state_definition(&self, s: &ManifestState) -> ObservableDefinition {
        ObservableDefinition {
            id: observable_id(&self.id, &s.id),
            provider: self.id.clone(),
            local: s.id.clone(),
            kind: ObservableKind::State,
            name: s.name.clone(),
            description: s.description.clone(),
            category: s
                .category
                .clone()
                .unwrap_or_else(|| self.default_category()),
            keywords: s.keywords.clone(),
            params: s.params.clone(),
            value: Some(ValueSpec {
                kind: s.kind,
                values: s.values.clone(),
            }),
            fields: Vec::new(),
        }
    }

    fn event_definition(&self, e: &ManifestEvent) -> ObservableDefinition {
        let mut def = ObservableDefinition::event(
            &self.id,
            &e.kind,
            e.name.as_deref().unwrap_or(&e.kind),
            &e.description,
        )
        .category(&self.default_category());
        def.fields = e.fields.clone();
        def
    }

    /// The states this plugin declares, as the hub takes them.
    pub fn state_definitions(&self) -> Vec<ObservableDefinition> {
        self.states
            .iter()
            .map(|s| self.state_definition(s))
            .collect()
    }

    /// Everything a pad can observe from this plugin: its states and its
    /// events. They are different things and stay different in the catalog.
    pub fn observables(&self) -> Vec<ObservableDefinition> {
        let mut out = self.state_definitions();
        out.extend(self.events.iter().map(|e| self.event_definition(e)));
        out
    }

    pub fn action(&self, id: &str) -> Option<&ManifestAction> {
        self.actions.iter().find(|a| a.id == id)
    }

    pub fn declares_event(&self, kind: &str) -> bool {
        self.events.iter().any(|e| e.kind == kind)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn base() -> Value {
        json!({
            "schemaVersion": 1,
            "id": "acme.notes",
            "name": "Notes",
            "version": "1.2.3",
            "description": "Takes notes.",
            "author": "Acme",
            "abi": 1,
            "sdk": "0.1.0",
            "main": "notes.wasm",
            "actions": [{
                "id": "note.add", "label": "Add note",
                "args": [{"name": "text", "type": "string", "required": true},
                         {"name": "n", "type": "number"}]
            }],
            "events": [{"type": "note.added"}],
            "config": [{"key": "folder", "type": "string", "default": "x"},
                       {"key": "token", "type": "string", "secret": true}],
            "capabilities": []
        })
    }

    fn parse(v: &Value) -> Result<ExternalManifest, ManifestError> {
        ExternalManifest::parse(&v.to_string())
    }

    fn with(key: &str, value: Value) -> Value {
        let mut v = base();
        v[key] = value;
        v
    }

    #[test]
    fn a_complete_manifest_parses() {
        let m = parse(&base()).unwrap();
        assert_eq!(m.id, "acme.notes");
        assert_eq!(m.actions[0].params.len(), 2, "schema 1 args become params");
        assert!(m.actions[0].args.is_empty());
        assert!(m.declares_event("note.added"));
        assert!(!m.declares_event("note.other"));
    }

    #[test]
    fn a_minimal_manifest_parses() {
        let v = json!({"schemaVersion":1,"id":"a.b","name":"n","version":"0.0.1",
            "author":"x","abi":1,"main":"m.wasm"});
        let m = parse(&v).unwrap();
        assert!(m.actions.is_empty() && m.events.is_empty() && m.sdk.is_none());
    }

    #[test]
    fn rejects_malformed_json_and_non_objects() {
        assert!(matches!(
            ExternalManifest::parse("{nope"),
            Err(ManifestError::Malformed(_))
        ));
        assert!(matches!(
            ExternalManifest::parse("[1,2]"),
            Err(ManifestError::Malformed(_))
        ));
        assert!(ExternalManifest::parse("").is_err());
    }

    #[test]
    fn rejects_oversized_manifests() {
        let big = format!("{{\"pad\":\"{}\"}}", "x".repeat(MAX_MANIFEST_BYTES));
        assert!(matches!(
            ExternalManifest::parse(&big),
            Err(ManifestError::Invalid(_))
        ));
    }

    #[test]
    fn schema_version_is_checked_before_anything_else() {
        let mut v = base();
        v["schemaVersion"] = json!(5);
        v["someFutureField"] = json!(true);
        assert!(matches!(
            parse(&v),
            Err(ManifestError::UnsupportedSchema { .. })
        ));
        let mut v = base();
        v.as_object_mut().unwrap().remove("schemaVersion");
        assert!(matches!(parse(&v), Err(ManifestError::Invalid(_))));
        assert!(matches!(
            parse(&with("schemaVersion", json!("1"))),
            Err(ManifestError::UnsupportedSchema { .. })
        ));
    }

    #[test]
    fn unknown_fields_are_errors() {
        assert!(matches!(
            parse(&with("exec", json!("rm -rf /"))),
            Err(ManifestError::Invalid(_))
        ));
        let mut v = base();
        v["actions"][0]["shell"] = json!("ls");
        assert!(parse(&v).is_err());
    }

    #[test]
    fn rejects_invalid_ids() {
        for bad in [
            "",
            "x",
            "hello",
            "Acme.Notes",
            "acme..notes",
            ".acme",
            "acme.",
            "acme notes",
            "1acme.notes",
            "acme.-notes",
            "../etc",
            "acme/notes",
            // Built-in providers have single-word ids; a plugin cannot claim them,
            // which is what keeps shell and HTTP out of reach of installed code.
            "system",
            "shell",
            "http",
            "navigation",
            "agent-control",
        ] {
            assert!(
                parse(&with("id", json!(bad))).is_err(),
                "{bad:?} should be rejected"
            );
        }
        assert!(parse(&with("id", json!("a".repeat(70) + ".b"))).is_err());
        assert!(parse(&with("id", json!("dev.acme.my-notes2"))).is_ok());
    }

    #[test]
    fn ids_starting_with_launchdeck_are_reserved() {
        for reserved in ["launchdeck.notes", "launchdeck.system", "launchdeck.a.b"] {
            assert!(!valid_plugin_id(reserved), "{reserved}");
            let e = parse(&with("id", json!(reserved))).unwrap_err();
            assert_eq!(e.kind(), ProblemKind::InvalidManifest);
            assert!(
                e.to_string()
                    .contains("ids starting with launchdeck. are reserved for LaunchDeck itself"),
                "{e}"
            );
        }
        for free in ["launchdecks.notes", "acme.launchdeck", "launch-deck.notes"] {
            assert!(parse(&with("id", json!(free))).is_ok(), "{free}");
        }
    }

    #[test]
    fn rejects_incompatible_protocol_and_sdk() {
        assert_eq!(
            parse(&with("abi", json!(2))).unwrap_err(),
            ManifestError::IncompatibleAbi {
                found: 2,
                supported: 1
            }
        );
        assert!(matches!(
            parse(&with("abi", json!(0))),
            Err(ManifestError::IncompatibleAbi { .. })
        ));
        assert!(matches!(
            parse(&with("sdk", json!("9.0.0"))),
            Err(ManifestError::IncompatibleSdk { .. })
        ));
        assert!(matches!(
            parse(&with("sdk", json!("0.9.0"))),
            Err(ManifestError::IncompatibleSdk { .. })
        ));
        assert!(parse(&with("sdk", json!("0.1.9"))).is_ok());
        assert!(parse(&with("sdk", json!("banana"))).is_err());
    }

    #[test]
    fn every_manifest_error_says_what_kind_of_problem_it_is() {
        use crate::plugins::external::ProblemKind::{Incompatible, InvalidManifest};
        let kind = |v: &Value| parse(v).unwrap_err().kind();
        assert_eq!(kind(&with("schemaVersion", json!(99))), Incompatible);
        assert_eq!(kind(&with("abi", json!(2))), Incompatible);
        assert_eq!(kind(&with("sdk", json!("9.0.0"))), Incompatible);
        assert_eq!(
            ExternalManifest::parse("{nope").unwrap_err().kind(),
            InvalidManifest
        );
        assert_eq!(kind(&with("id", json!("Nope"))), InvalidManifest);
        assert_eq!(kind(&with("capabilities", json!(["fs"]))), InvalidManifest);
    }

    #[test]
    fn rejects_bad_versions() {
        for bad in [
            "1",
            "1.2",
            "1.2.3.4",
            "1.2.x",
            "v1.2.3",
            "1.2.3-beta",
            "",
            "-1.0.0",
        ] {
            assert!(parse(&with("version", json!(bad))).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn main_must_be_a_plain_wasm_file_name() {
        for bad in [
            "../evil.wasm",
            "/etc/passwd.wasm",
            "sub/x.wasm",
            "x.exe",
            ".wasm",
            ".hidden.wasm",
            "a..b.wasm",
            "x.wasm ",
            "C:\\x.wasm",
            "",
        ] {
            assert!(parse(&with("main", json!(bad))).is_err(), "{bad:?}");
        }
        assert!(parse(&with("main", json!("my-plugin_1.wasm"))).is_ok());
    }

    #[test]
    fn capabilities_fail_closed() {
        assert_eq!(
            parse(&with("capabilities", json!(["network"]))).unwrap_err(),
            ManifestError::UnsupportedCapability("network".into())
        );
    }

    #[test]
    fn actions_events_and_config_are_validated() {
        let mut v = base();
        v["actions"] = json!([{"id":"a"},{"id":"a"}]);
        assert!(parse(&v).is_err(), "duplicate action");
        let mut v = base();
        v["actions"] = json!([{"id":"bad id"}]);
        assert!(parse(&v).is_err());
        let mut v = base();
        v["actions"][0]["args"] =
            json!([{"name":"x","type":"string"},{"name":"x","type":"number"}]);
        assert!(parse(&v).is_err(), "duplicate arg");
        let mut v = base();
        v["actions"][0]["args"] = json!([{"name":"x","type":"color"}]);
        assert!(parse(&v).is_err(), "unknown arg type");
        let mut v = base();
        v["events"] = json!([{"type":"control.pressed"}]);
        assert!(parse(&v).is_err(), "host event prefix");
        let mut v = base();
        v["events"] = json!([{"type":"plugin.state.changed"}]);
        assert!(parse(&v).is_err(), "host event prefix");
        let mut v = base();
        v["events"] = json!([{"type":"a"},{"type":"a"}]);
        assert!(parse(&v).is_err(), "duplicate event");
        let mut v = base();
        v["config"] = json!([{"key":"a","type":"number","default":"x"}]);
        assert!(parse(&v).is_err(), "default type");
        let mut v = base();
        v["config"] = json!([{"key":"a","type":"string","secret":true,"default":"s3cret"}]);
        assert!(parse(&v).is_err(), "secret default");
        let mut v = base();
        v["config"] = json!([{"key":"a b","type":"string"}]);
        assert!(parse(&v).is_err(), "config key");
    }

    #[test]
    fn arguments_are_checked_against_the_manifest() {
        let m = parse(&base()).unwrap();
        let a = m.action("note.add").unwrap();
        assert_eq!(
            a.normalize(&json!({"text":"hi"})).unwrap(),
            json!({"text":"hi"})
        );
        assert_eq!(
            a.normalize(&json!({"text":"hi","n":null})).unwrap(),
            json!({"text":"hi"})
        );
        assert!(a.normalize(&json!({})).is_err(), "required missing");
        assert!(a.normalize(&Value::Null).is_err(), "required missing");
        assert!(a.normalize(&json!({"text":5})).is_err(), "wrong type");
        assert!(a.normalize(&json!({"text":"x","n":"1"})).is_err());
        assert!(
            a.normalize(&json!({"text":"x","extra":1})).is_err(),
            "unknown arg"
        );
        assert!(a.normalize(&json!([1])).is_err(), "not an object");
    }

    fn v2() -> Value {
        let mut v = base();
        v["schemaVersion"] = json!(2);
        v["actions"] = json!([{
            "id": "note.add",
            "label": "Add note",
            "description": "Save a note.",
            "category": "Notes",
            "keywords": ["write", "memo"],
            "appearance": { "label": "Note", "color": "#3366ff", "icon": "N" },
            "params": [
                { "name": "text", "label": "Text", "type": "string", "required": true, "multiline": true },
                { "name": "kind", "label": "Kind", "type": "enum", "default": "memo",
                  "options": [{ "value": "memo", "label": "Memo" }, { "value": "todo", "label": "To do" }] },
                { "name": "pin", "label": "Pin it", "type": "boolean" }
            ]
        }]);
        v
    }

    #[test]
    fn schema_two_declares_discovery_metadata_and_rich_params() {
        let m = parse(&v2()).unwrap();
        let a = m.action("note.add").unwrap();
        assert_eq!(a.category.as_deref(), Some("Notes"));
        assert_eq!(a.keywords, ["write", "memo"]);
        assert_eq!(a.params.len(), 3);
        assert_eq!(
            a.normalize(&json!({"text":"hi"})).unwrap(),
            json!({"text":"hi","kind":"memo"}),
            "defaults are applied"
        );
        assert!(a.normalize(&json!({"text":"hi","kind":"nope"})).is_err());
    }

    #[test]
    fn schema_one_and_two_do_not_mix() {
        let mut v = base();
        v["actions"][0]["category"] = json!("Notes");
        assert!(parse(&v).is_err(), "schema 1 cannot use schema 2 fields");
        let mut v = v2();
        v["actions"][0]["args"] = json!([{"name":"x","type":"string"}]);
        assert!(parse(&v).is_err(), "schema 2 uses params, not args");
    }

    #[test]
    fn bad_discovery_metadata_and_params_are_refused() {
        for patch in [
            ("category", json!("")),
            (
                "keywords",
                json!((0..13).map(|i| format!("k{i}")).collect::<Vec<_>>()),
            ),
            ("keywords", json!(["   "])),
            ("appearance", json!({"label": ""})),
            ("appearance", json!({"color": "red"})),
            (
                "params",
                json!([{"name":"a b","label":"A","type":"string"}]),
            ),
            ("params", json!([{"name":"a","label":"A","type":"enum"}])),
            (
                "params",
                json!([{"name":"a","label":"A","type":"string","default":5}]),
            ),
            (
                "params",
                json!([{"name":"a","label":"A","type":"string","extra":1}]),
            ),
        ] {
            let mut v = v2();
            v["actions"][0][patch.0] = patch.1.clone();
            assert!(parse(&v).is_err(), "{patch:?} should be refused");
        }
    }

    #[test]
    fn version_rules() {
        let v = |s| Version::parse(s).unwrap();
        assert!(Version::parse("01.2.3").is_some());
        assert!(Version::parse("1.2").is_none());
        assert!(Version::plugin_sdk_ok(v("0.1.0"), v("0.1.4")));
        assert!(!Version::plugin_sdk_ok(v("0.2.0"), v("0.1.4")));
        assert!(Version::plugin_sdk_ok(v("1.0.0"), v("2.0.0")));
        assert!(!Version::plugin_sdk_ok(v("2.0.0"), v("1.9.0")));
        assert!(Version::plugin_sdk_ok(v("1.3.0"), v("1.1.0")));
    }

    /// A plugin version as `semver` sees it, so a dependency requirement can
    /// be matched against it. Leading zeros keep loading and simply drop.
    #[test]
    fn a_version_converts_to_semver() {
        let v = |s| Version::parse(s).unwrap().to_semver();
        assert_eq!(v("1.2.3"), semver::Version::new(1, 2, 3));
        assert_eq!(v("01.002.0003"), semver::Version::new(1, 2, 3));
        assert_eq!(v("0.0.0"), semver::Version::new(0, 0, 0));
        let req = semver::VersionReq::parse("^0.4").unwrap();
        assert!(req.matches(&v("0.4.1")) && !req.matches(&v("0.5.0")));
    }

    // ---- schema 3: states, event fields, suggestions ----------------------

    fn v3() -> Value {
        json!({
            "schemaVersion": 3,
            "id": "example.counter",
            "name": "Counter",
            "version": "1.0.0",
            "author": "Acme",
            "abi": 1,
            "sdk": "0.1.0",
            "main": "counter.wasm",
            "actions": [{ "id": "counter.increment", "label": "Increment",
                          "suggests": ["level"] }],
            "events": [{ "type": "counter.changed", "name": "Counter changed",
                         "fields": [
                             { "name": "value", "label": "Value", "type": "number" },
                             { "name": "level", "label": "Level", "type": "enum",
                               "values": [{ "value": "zero", "label": "Empty" },
                                          { "value": "high", "label": "High" }] }
                         ] }],
            "states": [
                { "id": "level", "name": "Counter level", "description": "How full.",
                  "type": "enum",
                  "values": [
                      { "value": "zero", "label": "Empty", "role": "inactive" },
                      { "value": "low", "label": "Low", "role": "ok" },
                      { "value": "high", "label": "High", "role": "warning" },
                      { "value": "max", "label": "Full", "role": "error" }
                  ] },
                { "id": "channel.status", "name": "Channel status", "category": "Channels",
                  "type": "enum",
                  "values": [{ "value": "up", "label": "Up" }, { "value": "down", "label": "Down" }],
                  "params": [{ "name": "channel", "label": "Channel", "type": "string",
                               "required": true, "maxLength": 32 }] },
                { "id": "muted", "name": "Muted", "type": "boolean",
                  "values": [{ "value": true, "label": "Muted" }, { "value": false, "label": "Live" }] },
                { "id": "volume", "name": "Volume", "type": "number" },
                { "id": "title", "name": "Title", "type": "string" }
            ]
        })
    }

    fn v3_with(edit: impl FnOnce(&mut Value)) -> Result<ExternalManifest, ManifestError> {
        let mut v = v3();
        edit(&mut v);
        parse(&v)
    }

    fn why(r: Result<ExternalManifest, ManifestError>) -> String {
        match r {
            Err(e) => e.to_string(),
            Ok(_) => panic!("the manifest was accepted"),
        }
    }

    #[test]
    fn a_schema_3_manifest_declares_states_events_and_suggestions() {
        let m = parse(&v3()).unwrap();
        assert_eq!(m.states.len(), 5);
        let defs = m.observables();
        // States first, then events; the provider is the plugin and the id is `<plugin>.<local>`.
        let level = defs
            .iter()
            .find(|d| d.id == "example.counter.level")
            .unwrap();
        assert_eq!(level.provider, "example.counter");
        assert_eq!(level.kind, ObservableKind::State);
        let spec = level.value.as_ref().unwrap();
        assert_eq!(spec.kind, ValueType::Enum);
        assert_eq!(spec.values.len(), 4);
        assert_eq!(level.category, "Counter", "the plugin's name by default");
        let channel = defs
            .iter()
            .find(|d| d.id == "example.counter.channel.status")
            .unwrap();
        assert_eq!(channel.category, "Channels");
        assert_eq!(channel.params[0].name, "channel");
        let event = defs
            .iter()
            .find(|d| d.id == "example.counter.counter.changed")
            .unwrap();
        assert_eq!(event.kind, ObservableKind::Event);
        assert_eq!(event.name, "Counter changed");
        assert_eq!(event.fields.len(), 2);
        assert_eq!(m.actions[0].suggests, vec!["level".to_string()]);
    }

    #[test]
    fn manifest_states_are_exactly_what_the_hub_accepts() {
        // The agreement the milestone asks for: whatever the manifest accepts, the
        // hub declares, and every type the core supports can be declared.
        let m = parse(&v3()).unwrap();
        let hub = crate::state::StateHub::new();
        hub.declare("example.counter", m.state_definitions())
            .unwrap();
        for d in m.observables() {
            d.check().unwrap();
        }
        let kinds: Vec<ValueType> = m
            .state_definitions()
            .iter()
            .map(|d| d.value.as_ref().unwrap().kind)
            .collect();
        for k in [
            ValueType::Enum,
            ValueType::Boolean,
            ValueType::Number,
            ValueType::String,
        ] {
            assert!(kinds.contains(&k), "{k:?}");
        }
    }

    #[test]
    fn schema_1_and_2_manifests_still_load_and_declare_no_states() {
        let one = parse(&base()).unwrap();
        assert!(one.states.is_empty());
        // Their events are still listed, under the name they have.
        let defs = one.observables();
        assert_eq!(defs.len(), 1);
        assert_eq!(defs[0].id, "acme.notes.note.added");
        assert_eq!(defs[0].name, "note.added");
        assert!(defs[0].fields.is_empty());

        let mut two = base();
        two["schemaVersion"] = json!(2);
        two["actions"] = json!([{
            "id": "note.add", "label": "Add", "category": "Notes", "keywords": ["x"],
            "params": [{ "name": "text", "label": "Text", "type": "string", "required": true }]
        }]);
        let m = parse(&two).unwrap();
        assert!(m.states.is_empty() && m.actions[0].suggests.is_empty());
        assert_eq!(m.observables().len(), 1);
    }

    #[test]
    fn schema_3_features_need_schema_3() {
        for edit in [
            |v: &mut Value| v["states"] = json!([{ "id": "x", "name": "X", "type": "number" }]),
            |v: &mut Value| v["actions"][0]["suggests"] = json!(["note.added"]),
            |v: &mut Value| v["events"][0]["name"] = json!("Note added"),
            |v: &mut Value| {
                v["events"][0]["fields"] = json!([{ "name": "a", "label": "A", "type": "number" }])
            },
        ] {
            for schema in [1, 2] {
                let mut v = base();
                v["schemaVersion"] = json!(schema);
                if schema == 2 {
                    // Schema 2 calls the arguments of an action `params`.
                    v["actions"] = json!([{ "id": "note.add" }]);
                }
                edit(&mut v);
                let msg = why(parse(&v));
                assert!(msg.contains("schemaVersion"), "{msg}");
            }
        }
    }

    #[test]
    fn duplicate_and_invalid_state_ids_are_refused() {
        let msg = why(v3_with(|v| {
            let first = v["states"][0].clone();
            v["states"].as_array_mut().unwrap().push(first);
        }));
        assert!(msg.contains("declared twice"), "{msg}");
        for bad in [
            "Level",
            "1level",
            "a..b",
            "a_b",
            "has space",
            "",
            &"a".repeat(65),
        ] {
            let msg = why(v3_with(|v| v["states"][0]["id"] = json!(bad)));
            assert!(msg.contains("state id"), "{bad:?}: {msg}");
        }
    }

    #[test]
    fn a_state_and_an_event_cannot_share_an_id() {
        let msg = why(v3_with(|v| {
            v["states"][0]["id"] = json!("counter.changed");
        }));
        assert!(msg.contains("state and as an event"), "{msg}");
    }

    #[test]
    fn invalid_state_types_and_values_are_refused() {
        // A type the core does not have (no JSON, no lists, no objects).
        for t in ["json", "object", "array", "integer", "Enum"] {
            assert!(
                v3_with(|v| v["states"][0]["type"] = json!(t)).is_err(),
                "{t}"
            );
        }
        // An enum needs its values; the others do not list any.
        let msg = why(v3_with(|v| v["states"][0]["values"] = json!([])));
        assert!(msg.contains("enum lists"), "{msg}");
        let msg = why(v3_with(|v| {
            v["states"][3]["values"] = json!([{ "value": "a", "label": "A" }])
        }));
        assert!(
            msg.contains("only an enum or a boolean lists values"),
            "{msg}"
        );
        // Duplicates, a value of the wrong type, a missing label, an unknown role.
        let msg = why(v3_with(|v| {
            v["states"][0]["values"][1]["value"] = json!("zero");
        }));
        assert!(msg.contains("twice"), "{msg}");
        let msg = why(v3_with(|v| v["states"][0]["values"][0]["value"] = json!(7)));
        assert!(msg.contains("does not match the type"), "{msg}");
        assert!(v3_with(|v| {
            v["states"][0]["values"][0]
                .as_object_mut()
                .unwrap()
                .remove("label");
        })
        .is_err());
        assert!(v3_with(|v| v["states"][0]["values"][0]["role"] = json!("purple")).is_err());
        // A colour is never part of a state.
        assert!(v3_with(|v| v["states"][0]["values"][0]["color"] = json!("#ff0000")).is_err());
        // Unknown fields on the state itself.
        assert!(v3_with(|v| v["states"][0]["provider"] = json!("agent-control")).is_err());
        // Too many values.
        let msg = why(v3_with(|v| {
            v["states"][0]["values"] = Value::Array(
                (0..40)
                    .map(|n| json!({ "value": format!("v{n}"), "label": "V" }))
                    .collect(),
            );
        }));
        assert!(msg.contains("enum lists"), "{msg}");
    }

    #[test]
    fn invalid_scope_arguments_are_refused() {
        let param = |extra: Value| {
            let mut p = json!({ "name": "channel", "label": "Channel", "type": "string",
                                "required": true, "maxLength": 32 });
            for (k, val) in extra.as_object().unwrap() {
                p[k] = val.clone();
            }
            p
        };
        // Structured arguments are not addresses.
        for kind in ["json", "reference"] {
            let msg = why(v3_with(|v| {
                v["states"][1]["params"] = json!([param(json!({ "type": kind }))])
            }));
            assert!(
                msg.contains("boolean, a number or a text") || msg.contains("parameter"),
                "{msg}"
            );
        }
        // A text argument must say how long it can be, and no longer than an address holds.
        let msg = why(v3_with(|v| {
            v["states"][1]["params"][0]
                .as_object_mut()
                .unwrap()
                .remove("maxLength");
        }));
        assert!(msg.contains("maxLength"), "{msg}");
        let msg = why(v3_with(|v| {
            v["states"][1]["params"][0]["maxLength"] = json!(65)
        }));
        assert!(msg.contains("maxLength"), "{msg}");
        // Names: plain, unique, at most four.
        let msg = why(v3_with(|v| {
            v["states"][1]["params"] = json!([param(json!({})), param(json!({}))])
        }));
        assert!(msg.contains("twice"), "{msg}");
        assert!(v3_with(|v| v["states"][1]["params"][0]["name"] = json!("not valid")).is_err());
        let msg = why(v3_with(|v| {
            v["states"][1]["params"] = Value::Array(
                ["a", "b", "c", "d", "e"]
                    .iter()
                    .map(|n| param(json!({ "name": n })))
                    .collect(),
            );
        }));
        assert!(msg.contains("at most 4 arguments"), "{msg}");
        // A default has to fit its own type.
        assert!(v3_with(|v| v["states"][1]["params"][0]["default"] = json!(5)).is_err());
        // A number argument may have a range and a default.
        assert!(v3_with(|v| {
            v["states"][1]["params"] = json!([{ "name": "n", "label": "N", "type": "number",
                "integer": true, "min": 0, "max": 7, "default": 0 }]);
        })
        .is_ok());
    }

    #[test]
    fn state_limits_and_text_are_bounded() {
        let msg = why(v3_with(|v| {
            v["states"] = Value::Array(
                (0..65)
                    .map(|n| json!({ "id": format!("s{n}"), "name": "S", "type": "number" }))
                    .collect(),
            );
        }));
        assert!(msg.contains("more than 64 states"), "{msg}");
        assert!(v3_with(|v| v["states"][0]["name"] = json!("")).is_err());
        assert!(v3_with(|v| v["states"][0]["name"] = json!("x".repeat(65))).is_err());
        assert!(v3_with(|v| v["states"][0]["description"] = json!("x".repeat(281))).is_err());
        assert!(v3_with(|v| v["states"][0]["category"] = json!("x".repeat(41))).is_err());
        assert!(v3_with(|v| {
            v["states"][0]["keywords"] = json!(["a", "b", "c", "d", "e", "f", "g", "h", "i"])
        })
        .is_err());
        assert!(v3_with(|v| v["states"][0]["keywords"] = json!(["x".repeat(33)])).is_err());
        // A long plugin name still gives a legal default category.
        let m = v3_with(|v| v["name"] = json!("N".repeat(64))).unwrap();
        assert_eq!(m.state_definitions()[0].category.chars().count(), 40);
    }

    #[test]
    fn event_fields_are_checked() {
        let msg = why(v3_with(|v| {
            let f = v["events"][0]["fields"][0].clone();
            v["events"][0]["fields"].as_array_mut().unwrap().push(f);
        }));
        assert!(msg.contains("unique plain name"), "{msg}");
        assert!(v3_with(|v| v["events"][0]["fields"][0]["type"] = json!("json")).is_err());
        assert!(v3_with(|v| v["events"][0]["name"] = json!("")).is_err());
        // An event keeps the grammar it has on the bus.
        let m = v3_with(|v| v["events"][0]["type"] = json!("build_done:ok")).unwrap();
        assert!(m.observables().iter().any(|d| d.local == "build_done:ok"));
        // Event types under a LaunchDeck prefix are still refused.
        assert!(v3_with(|v| v["events"][0]["type"] = json!("system.reboot")).is_err());
    }

    #[test]
    fn suggestions_must_point_at_this_plugin() {
        let ok = v3_with(|v| v["actions"][0]["suggests"] = json!(["level", "counter.changed"]));
        assert!(ok.is_ok());
        let msg = why(v3_with(|v| v["actions"][0]["suggests"] = json!(["nope"])));
        assert!(msg.contains("neither as a state nor as an event"), "{msg}");
        // Another plugin's state is not this plugin's to suggest.
        let msg = why(v3_with(|v| {
            v["actions"][0]["suggests"] = json!(["agent-control.slot.status"])
        }));
        assert!(msg.contains("neither"), "{msg}");
        let msg = why(v3_with(|v| {
            v["actions"][0]["suggests"] = json!(["level", "level"])
        }));
        assert!(msg.contains("twice"), "{msg}");
        let msg = why(v3_with(|v| {
            v["actions"][0]["suggests"] = Value::Array((0..9).map(|_| json!("level")).collect())
        }));
        assert!(msg.contains("more than 8"), "{msg}");
    }

    #[test]
    fn an_incompatible_schema_3_manifest_is_still_refused_for_the_old_reasons() {
        assert!(matches!(
            v3_with(|v| v["abi"] = json!(2)),
            Err(ManifestError::IncompatibleAbi { .. })
        ));
        assert!(matches!(
            v3_with(|v| v["sdk"] = json!("9.0.0")),
            Err(ManifestError::IncompatibleSdk { .. })
        ));
        assert!(matches!(
            v3_with(|v| v["schemaVersion"] = json!(5)),
            Err(ManifestError::UnsupportedSchema { .. })
        ));
        assert!(matches!(
            v3_with(|v| v["capabilities"] = json!(["network"])),
            Err(ManifestError::UnsupportedCapability(_))
        ));
        assert!(v3_with(|v| v["exec"] = json!("rm")).is_err());
    }

    // -- schema 4: capabilities, secrets, refresh ---------------------------

    fn v4() -> Value {
        let mut v = v3();
        v["schemaVersion"] = json!(4);
        v["config"] = json!([
            { "key": "baseUrl", "type": "string", "label": "Server address",
              "default": "http://homeassistant.local:8123" },
            { "key": "retries", "type": "number" }
        ]);
        v["capabilities"] = json!({ "network": {
            "purpose": "Connect to your server.",
            "hosts": ["https://api.example.com", { "setting": "baseUrl" }]
        }});
        v["secrets"] =
            json!([{ "key": "token", "label": "Access token", "host": { "setting": "baseUrl" } }]);
        v["refresh"] = json!({ "intervalSeconds": 5 });
        v
    }

    fn v4_with(edit: impl FnOnce(&mut Value)) -> Result<ExternalManifest, ManifestError> {
        let mut v = v4();
        edit(&mut v);
        parse(&v)
    }

    #[test]
    fn a_schema_4_manifest_asks_for_exact_destinations() {
        let m = parse(&v4()).unwrap();
        let net = m.capabilities.network.unwrap();
        assert_eq!(net.purpose, "Connect to your server.");
        assert_eq!(
            net.hosts,
            vec![
                HostSpec::Origin("https://api.example.com".into()),
                HostSpec::Setting {
                    setting: "baseUrl".into()
                }
            ]
        );
        assert_eq!(m.secrets[0].key, "token");
        assert_eq!(
            m.refresh,
            Some(RefreshDecl {
                interval_seconds: 5
            })
        );
        assert_eq!(m.config[0].label.as_deref(), Some("Server address"));
        // A schema 4 manifest that asks for nothing is as good as a schema 3 one.
        assert!(v4_with(|v| {
            for key in ["capabilities", "secrets", "refresh"] {
                v.as_object_mut().unwrap().remove(key);
            }
        })
        .is_ok());
    }

    #[test]
    fn capabilities_need_schema_4_and_only_network_exists() {
        let msg = why(v3_with(|v| {
            v["capabilities"] = v4()["capabilities"].clone()
        }));
        assert!(msg.contains("schemaVersion\": 4"), "{msg}");
        // The empty array of older manifests is still fine, in any schema.
        assert!(v3_with(|v| v["capabilities"] = json!([])).is_ok());
        assert!(matches!(
            v4_with(|v| v["capabilities"] = json!(["network"])),
            Err(ManifestError::UnsupportedCapability(_))
        ));
        for other in ["filesystem", "shell", "secrets", "system-control"] {
            assert!(
                v4_with(|v| v["capabilities"] = json!({ other: {} })).is_err(),
                "{other}"
            );
        }
    }

    #[test]
    fn network_hosts_are_explicit_origins_never_patterns() {
        let hosts = |h: Value| {
            why(v4_with(|v| {
                v["capabilities"]["network"]["hosts"] = h;
                v["secrets"] = json!([]);
            }))
        };
        for bad in [
            json!(["*"]),
            json!(["https://*.example.com"]),
            json!(["api.example.com"]),
            json!(["https://api.example.com/v1"]),
            json!(["ftp://example.com"]),
            json!(["https://user:pw@example.com"]),
            json!(["http://0.0.0.0"]),
            json!([]),
        ] {
            let msg = hosts(bad.clone());
            assert!(msg.contains("network"), "{bad}: {msg}");
        }
        assert!(hosts(json!(["https://a.example", "https://A.example:443/"])).contains("twice"));
        assert!(hosts(json!([{ "setting": "nope" }])).contains("does not declare"));
        assert!(hosts(json!([{ "setting": "retries" }])).contains("must be a string"));
        assert!(hosts(Value::Array(
            (0..17)
                .map(|i| json!(format!("https://h{i}.example")))
                .collect()
        ))
        .contains("1 to 16"));
        let msg = why(v4_with(|v| {
            v["capabilities"]["network"]["purpose"] = json!("  ")
        }));
        assert!(msg.contains("purpose"), "{msg}");
        let msg = why(v4_with(|v| v["config"][0]["default"] = json!("not a url")));
        assert!(msg.contains("default of setting"), "{msg}");
        // Explicit ports and local destinations are allowed to be asked for.
        assert!(v4_with(|v| {
            v["capabilities"]["network"]["hosts"] =
                json!(["http://192.168.1.20:8123", "http://localhost:8080", { "setting": "baseUrl" }])
        })
        .is_ok());
    }

    #[test]
    fn a_secret_is_bound_to_a_host_the_manifest_asks_for() {
        let msg = why(v4_with(|v| {
            v["secrets"][0]["host"] = json!("https://elsewhere.example")
        }));
        assert!(msg.contains("one of the hosts"), "{msg}");
        let msg = why(v4_with(|v| {
            v.as_object_mut().unwrap().remove("capabilities");
        }));
        assert!(msg.contains("one of the hosts"), "{msg}");
        assert!(v4_with(|v| v["secrets"][0]["host"] = json!("https://api.example.com")).is_ok());
        let msg = why(v4_with(|v| v["secrets"][0]["key"] = json!("baseUrl")));
        assert!(msg.contains("twice"), "{msg}");
        let msg = why(v4_with(|v| v["secrets"][0]["label"] = json!("")));
        assert!(msg.contains("label"), "{msg}");
        let msg =
            why(v3_with(
                |v| {
                    v["secrets"] =
                        json!([{ "key": "t", "label": "T", "host": "https://a.example" }])
                },
            ));
        assert!(msg.contains("schemaVersion\": 4"), "{msg}");
    }

    #[test]
    fn a_schema_4_setting_cannot_be_a_secret_the_module_receives() {
        let msg = why(v4_with(|v| {
            v["config"] = json!([{ "key": "baseUrl", "type": "string" }, { "key": "token", "type": "string", "secret": true }]);
            v["secrets"] = json!([]);
        }));
        assert!(msg.contains("declared under `secrets`"), "{msg}");
        // Older manifests keep what they had.
        assert!(v3_with(
            |v| v["config"] = json!([{ "key": "token", "type": "string", "secret": true }])
        )
        .is_ok());
    }

    #[test]
    fn refresh_is_bounded_and_needs_states() {
        for bad in [0, 1, 3601] {
            let msg = why(v4_with(|v| v["refresh"]["intervalSeconds"] = json!(bad)));
            assert!(msg.contains("between 2 and 3600"), "{msg}");
        }
        let msg = why(v4_with(|v| {
            v["states"] = json!([]);
            v["actions"][0]["suggests"] = json!([]);
        }));
        assert!(msg.contains("declares states"), "{msg}");
        let msg = why(v3_with(|v| v["refresh"] = json!({ "intervalSeconds": 5 })));
        assert!(msg.contains("schemaVersion\": 4"), "{msg}");
    }
}
