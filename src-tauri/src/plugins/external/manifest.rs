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

use super::{ABI_VERSION, SDK_VERSION};
use crate::actions::definition::Appearance;
use crate::actions::params::{normalize, validate_defs, ParamDef};
use crate::events::external::valid_ident;
use crate::plugins::ActionError;

pub const MANIFEST_FILE: &str = "plugin.json";
/// The newest manifest schema this host reads. Schema 1 is still read: its
/// `args` become parameters, and the host needs no change to the module.
pub const MANIFEST_SCHEMA: u32 = 2;
const OLDEST_SCHEMA: u32 = 1;
const MAX_KEYWORDS: usize = 12;
pub const MAX_MANIFEST_BYTES: usize = 64 * 1024;

const MAX_ACTIONS: usize = 64;
const MAX_ARGS: usize = 16;
const MAX_EVENTS: usize = 64;
const MAX_CONFIG: usize = 32;
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

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ManifestEvent {
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(default)]
    pub description: String,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ConfigField {
    pub key: String,
    #[serde(rename = "type")]
    pub kind: ConfigType,
    #[serde(default)]
    pub default: Option<Value>,
    /// Never shown, logged or put in a snapshot. May not have a default.
    #[serde(default)]
    pub secret: bool,
    #[serde(default)]
    pub description: String,
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
    #[serde(default)]
    pub config: Vec<ConfigField>,
    /// Reserved. Must be empty in schema 1; see PLUGIN_SDK_PLAN.md section 6.
    #[serde(default)]
    pub capabilities: Vec<String>,
}

/// Lowercase dotted id with at least two segments, for example `acme.notes`.
pub fn valid_plugin_id(id: &str) -> bool {
    if id.len() < 3 || id.len() > 64 {
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

fn valid_param(s: &str) -> bool {
    let mut chars = s.chars();
    s.len() <= 32
        && chars
            .next()
            .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
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
        if let Some(cap) = self.capabilities.first() {
            return Err(ManifestError::UnsupportedCapability(cap.clone()));
        }
        self.validate_actions()?;
        self.validate_events()?;
        self.validate_config()
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
                if !valid_param(&arg.name) {
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
        }
        Ok(())
    }

    fn validate_config(&self) -> Result<(), ManifestError> {
        if self.config.len() > MAX_CONFIG {
            return invalid(format!("more than {MAX_CONFIG} config keys"));
        }
        let mut seen = HashSet::new();
        for c in &self.config {
            if !valid_param(&c.key) {
                return invalid(format!(
                    "config key \"{}\" must be a plain identifier",
                    c.key
                ));
            }
            if !seen.insert(c.key.as_str()) {
                return invalid(format!("config key \"{}\" is declared twice", c.key));
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
        v["schemaVersion"] = json!(3);
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
}
