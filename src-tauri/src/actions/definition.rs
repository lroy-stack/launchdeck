//! The vocabulary of the Action Library.
//!
//! Two things with different lifetimes:
//!
//! * [`ActionDefinition`] belongs to a provider and says what can be done. It
//!   is computed from what is registered, sent to the UI as the catalog and
//!   never persisted.
//! * [`Assignment`] belongs to a control and says what this pad does. It is
//!   the only thing saved: a provider id, an action id and the arguments.

use serde::{Deserialize, Deserializer, Serialize};
use serde_json::{Map, Value};

use super::params::ParamDef;
use crate::core::model::Rgb;
use crate::state::ObservableDefinition;

/// One problem found while checking an assignment. Names the parameter when
/// the problem is about a single field, so a form can mark it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Issue {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub param: Option<String>,
    pub message: String,
}

impl Issue {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            param: None,
            message: message.into(),
        }
    }

    pub fn param(param: &str, message: impl Into<String>) -> Self {
        Self {
            param: Some(param.to_string()),
            message: message.into(),
        }
    }
}

/// What a control slot does: a provider, one of its actions and the settings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Assignment {
    pub provider: String,
    pub action: String,
    /// Always a JSON object.
    #[serde(default = "no_args", deserialize_with = "args_object")]
    pub args: Value,
}

fn no_args() -> Value {
    Value::Object(Map::new())
}

fn args_object<'de, D: Deserializer<'de>>(d: D) -> Result<Value, D::Error> {
    match Value::deserialize(d)? {
        Value::Null => Ok(no_args()),
        v @ Value::Object(_) => Ok(v),
        _ => Err(serde::de::Error::custom("args must be an object")),
    }
}

impl Assignment {
    pub fn new(provider: &str, action: &str, args: Value) -> Self {
        let args = match args {
            Value::Object(_) => args,
            _ => no_args(),
        };
        Self {
            provider: provider.to_string(),
            action: action.to_string(),
            args,
        }
    }

    /// The id of the definition this assignment points at: `provider.action`.
    pub fn definition_id(&self) -> String {
        definition_id(&self.provider, &self.action)
    }
}

pub fn definition_id(provider: &str, action: &str) -> String {
    format!("{provider}.{action}")
}

/// Where a provider comes from, so the library can say "Built-in", the module
/// name, or "Plugin: Name" without knowing any provider by name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProviderKind {
    /// Native code that ships with LaunchDeck.
    #[default]
    Builtin,
    /// A LaunchDeck module that contributes actions (Agent Control).
    Module,
    /// Installed from a folder, running in the sandbox.
    Plugin,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ProviderStatus {
    Ready,
    Disabled,
    Error,
}

/// Why an action cannot run right now.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Cause {
    /// The provider is off or failed to load. Fixed in the Plugins screen.
    Provider,
    /// The action works, but not in the current state (nothing is waiting).
    State,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "status", rename_all = "camelCase")]
pub enum Availability {
    Available,
    Unavailable { cause: Cause, reason: String },
}

impl Availability {
    pub fn unavailable(cause: Cause, reason: impl Into<String>) -> Self {
        Availability::Unavailable {
            cause,
            reason: reason.into(),
        }
    }

    pub fn is_available(&self) -> bool {
        matches!(self, Availability::Available)
    }
}

/// A suggested look for a control that runs this action. A hint, never applied
/// over something a person already set.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Appearance {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<Rgb>,
    /// A glyph (emoji or one or two characters), as controls already use.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
}

impl Appearance {
    pub fn new(label: &str, color: Rgb, icon: &str) -> Self {
        Self {
            label: Some(label.into()),
            color: Some(color),
            icon: Some(icon.into()),
        }
    }

    pub fn check(&self) -> Result<(), String> {
        let text = |s: &str, max: usize| {
            !s.trim().is_empty() && s.chars().count() <= max && !s.chars().any(char::is_control)
        };
        if self.label.as_deref().is_some_and(|l| !text(l, 40)) {
            return Err("appearance label must be 1-40 characters".into());
        }
        if self.icon.as_deref().is_some_and(|i| !text(i, 8)) {
            return Err("appearance icon must be 1-8 characters".into());
        }
        Ok(())
    }
}

/// One thing a person can put on a control.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ActionDefinition {
    /// `provider.action`. Stable; never derived from the display name.
    pub id: String,
    pub provider: String,
    pub action: String,
    pub name: String,
    pub description: String,
    pub category: String,
    pub keywords: Vec<String>,
    pub params: Vec<ParamDef>,
    pub availability: Availability,
    /// Runs something on the computer or the network on behalf of the person
    /// (a shell command, an HTTP request). Always shown as such.
    pub privileged: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub appearance: Option<Appearance>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderInfo {
    pub id: String,
    pub name: String,
    pub kind: ProviderKind,
    pub version: String,
    pub description: String,
    pub status: ProviderStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

/// Everything the Action Library shows, evaluated at one moment.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Catalog {
    pub providers: Vec<ProviderInfo>,
    pub actions: Vec<ActionDefinition>,
    /// What a control can observe: states (and events) offered by providers.
    /// Definitions only; the values are in the state hub and change far more
    /// often than the catalog does.
    pub observables: Vec<ObservableDefinition>,
    /// Definitions that were left out and why (a duplicate id, a bad schema).
    pub issues: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn assignments_store_the_two_ids_and_an_object() {
        let a: Assignment =
            serde_json::from_value(json!({ "provider": "system", "action": "openUrl" })).unwrap();
        assert_eq!(a.args, json!({}));
        assert_eq!(a.definition_id(), "system.openUrl");
        let b: Assignment = serde_json::from_value(
            json!({ "provider": "http", "action": "request", "args": null }),
        )
        .unwrap();
        assert_eq!(b.args, json!({}));
        assert_eq!(
            serde_json::to_value(&a).unwrap(),
            json!({ "provider": "system", "action": "openUrl", "args": {} })
        );
    }

    #[test]
    fn malformed_assignments_do_not_parse() {
        for bad in [
            json!({ "provider": "a", "action": "b", "args": [1] }),
            json!({ "provider": "a", "action": "b", "args": "x" }),
            json!({ "provider": "a" }),
            json!({ "action": "b" }),
            json!({ "provider": "a", "action": "b", "label": "Approve" }),
            json!({ "type": "openUrl", "url": "https://a.b" }),
        ] {
            assert!(
                serde_json::from_value::<Assignment>(bad.clone()).is_err(),
                "{bad}"
            );
        }
    }

    #[test]
    fn availability_serialises_with_a_status_tag() {
        assert_eq!(
            serde_json::to_value(Availability::Available).unwrap(),
            json!({ "status": "available" })
        );
        assert_eq!(
            serde_json::to_value(Availability::unavailable(
                Cause::State,
                "nothing is waiting"
            ))
            .unwrap(),
            json!({ "status": "unavailable", "cause": "state", "reason": "nothing is waiting" })
        );
    }

    #[test]
    fn appearance_hints_are_checked() {
        assert!(Appearance::new("Approve", Rgb::new(0, 200, 0), "✓")
            .check()
            .is_ok());
        assert!(Appearance::new("", Rgb::new(0, 0, 0), "✓").check().is_err());
        assert!(Appearance::new("A", Rgb::new(0, 0, 0), "123456789")
            .check()
            .is_err());
    }
}
