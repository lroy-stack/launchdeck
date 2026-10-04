//! What a provider says it can be observed doing: [`ObservableDefinition`].
//!
//! A definition describes; it never holds a value. The current value, its
//! status and its age live in the hub, in the same way an `Assignment` is
//! separate from an `ActionDefinition`.
//!
//! One list covers states and events because the editor offers one searchable
//! source picker. The semantics stay apart: only states are stored, only
//! states have a status, only events carry a payload.

use std::collections::HashSet;

use serde::{Deserialize, Serialize};

use super::address::valid_ident;
use super::value::Scalar;
use crate::actions::params::{validate_defs, ParamDef, ParamKind};

pub const MAX_NAME: usize = 64;
pub const MAX_DESCRIPTION: usize = 280;
pub const MAX_LABEL: usize = 40;
pub const MAX_VALUES: usize = 32;
pub const MAX_FIELDS: usize = 16;
pub const MAX_KEYWORDS: usize = 8;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ObservableKind {
    /// A named current value with a status. Stored in the hub.
    State,
    /// Something that happened. Never stored.
    Event,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ValueType {
    Enum,
    Boolean,
    Number,
    String,
}

/// A suggestion a provider may attach to a value: an intent, never a colour.
/// LaunchDeck owns what each role looks like, and a person's look always wins.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    Ok,
    Active,
    Attention,
    Warning,
    Error,
    Inactive,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ValueOption {
    pub value: Scalar,
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<Role>,
}

impl ValueOption {
    pub fn new(value: impl Into<Scalar>, label: &str, role: Option<Role>) -> Self {
        Self {
            value: value.into(),
            label: label.to_string(),
            role,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ValueSpec {
    #[serde(rename = "type")]
    pub kind: ValueType,
    /// Listed values: required for an enum, optional labels for a boolean.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub values: Vec<ValueOption>,
}

/// How a value relates to what its definition declares.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fit {
    /// Right type, and listed when the definition lists values.
    Declared,
    /// Right type but an enum value the definition does not list. Stored
    /// anyway so a provider can add a value without breaking an older host.
    Unlisted,
    /// The wrong type for this state. Rejected.
    WrongType,
}

impl ValueSpec {
    pub fn fit(&self, value: &Scalar) -> Fit {
        let right_type = matches!(
            (self.kind, value),
            (ValueType::Enum | ValueType::String, Scalar::Str(_))
                | (ValueType::Boolean, Scalar::Bool(_))
                | (ValueType::Number, Scalar::Num(_))
        );
        if !right_type {
            return Fit::WrongType;
        }
        if self.kind == ValueType::Enum && !self.values.iter().any(|o| o.value.same_as(value)) {
            return Fit::Unlisted;
        }
        Fit::Declared
    }

    fn check(&self, what: &str) -> Result<(), String> {
        match self.kind {
            ValueType::Enum => {
                if self.values.is_empty() || self.values.len() > MAX_VALUES {
                    return Err(format!("{what}: an enum lists 1-{MAX_VALUES} values"));
                }
            }
            ValueType::Boolean => {
                if self.values.len() > 2 {
                    return Err(format!("{what}: a boolean has at most two labelled values"));
                }
            }
            ValueType::Number | ValueType::String => {
                if !self.values.is_empty() {
                    return Err(format!("{what}: only an enum or a boolean lists values"));
                }
            }
        }
        let mut seen: Vec<&Scalar> = Vec::new();
        for o in &self.values {
            let label = o.label.trim();
            if label.is_empty()
                || label.chars().count() > MAX_LABEL
                || label.chars().any(char::is_control)
            {
                return Err(format!(
                    "{what}: every value needs a label of 1-{MAX_LABEL} characters"
                ));
            }
            let typed = match self.kind {
                ValueType::Enum => {
                    matches!(&o.value, Scalar::Str(s) if !s.is_empty() && s.len() <= 64)
                }
                ValueType::Boolean => matches!(o.value, Scalar::Bool(_)),
                _ => false,
            };
            if !typed {
                return Err(format!("{what}: a listed value does not match the type"));
            }
            if seen.iter().any(|s| s.same_as(&o.value)) {
                return Err(format!("{what}: a value is listed twice"));
            }
            seen.push(&o.value);
        }
        Ok(())
    }
}

/// One payload field of an event that a rule may test.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EventField {
    pub name: String,
    pub label: String,
    #[serde(rename = "type")]
    pub kind: ValueType,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub values: Vec<ValueOption>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ObservableDefinition {
    /// `provider.local`. Stable and never shown.
    pub id: String,
    pub provider: String,
    /// The id inside the provider (`slot.status`).
    pub local: String,
    pub kind: ObservableKind,
    pub name: String,
    pub description: String,
    pub category: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub keywords: Vec<String>,
    /// What picks one instance of a state (`slot`, `entity`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub params: Vec<ParamDef>,
    /// States only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<ValueSpec>,
    /// Events only.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fields: Vec<EventField>,
}

pub fn observable_id(provider: &str, local: &str) -> String {
    format!("{provider}.{local}")
}

impl ObservableDefinition {
    pub fn state(provider: &str, local: &str, name: &str, description: &str) -> Self {
        Self {
            id: observable_id(provider, local),
            provider: provider.to_string(),
            local: local.to_string(),
            kind: ObservableKind::State,
            name: name.to_string(),
            description: description.to_string(),
            category: String::new(),
            keywords: Vec::new(),
            params: Vec::new(),
            value: None,
            fields: Vec::new(),
        }
    }

    pub fn event(provider: &str, local: &str, name: &str, description: &str) -> Self {
        let mut d = Self::state(provider, local, name, description);
        d.kind = ObservableKind::Event;
        d
    }

    pub fn category(mut self, category: &str) -> Self {
        self.category = category.to_string();
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

    /// An enum state: `(value, label, role)` per value.
    pub fn one_of(mut self, values: &[(&str, &str, Option<Role>)]) -> Self {
        self.value = Some(ValueSpec {
            kind: ValueType::Enum,
            values: values
                .iter()
                .map(|(v, l, r)| ValueOption::new(*v, l, *r))
                .collect(),
        });
        self
    }

    /// A boolean state with words for `true` and `false`.
    pub fn on_off(mut self, on: (&str, Option<Role>), off: (&str, Option<Role>)) -> Self {
        self.value = Some(ValueSpec {
            kind: ValueType::Boolean,
            values: vec![
                ValueOption::new(true, on.0, on.1),
                ValueOption::new(false, off.0, off.1),
            ],
        });
        self
    }

    pub fn number(mut self) -> Self {
        self.value = Some(ValueSpec {
            kind: ValueType::Number,
            values: vec![],
        });
        self
    }

    pub fn text(mut self) -> Self {
        self.value = Some(ValueSpec {
            kind: ValueType::String,
            values: vec![],
        });
        self
    }

    pub fn is_state(&self) -> bool {
        self.kind == ObservableKind::State
    }

    /// Is this definition well formed? Run when a provider declares it, so a
    /// bad definition is found there and not when someone uses it.
    pub fn check(&self) -> Result<(), String> {
        let id = &self.id;
        if !valid_ident(&self.provider) || !valid_ident(&self.local) {
            return Err(format!(
                "{id}: provider and id must be lowercase dotted identifiers"
            ));
        }
        if *id != observable_id(&self.provider, &self.local) {
            return Err(format!(
                "{id}: the id must be the provider and the local id joined by a dot"
            ));
        }
        let text = |s: &str, max: usize| {
            s.chars().count() <= max && !s.chars().any(|c| c.is_control() && c != '\n')
        };
        if self.name.trim().is_empty() || !text(&self.name, MAX_NAME) {
            return Err(format!(
                "{id}: a name of 1-{MAX_NAME} characters is required"
            ));
        }
        if !text(&self.description, MAX_DESCRIPTION) || !text(&self.category, MAX_LABEL) {
            return Err(format!("{id}: description or category is too long"));
        }
        if self.keywords.len() > MAX_KEYWORDS || self.keywords.iter().any(|k| !text(k, 32)) {
            return Err(format!(
                "{id}: at most {MAX_KEYWORDS} keywords of 32 characters"
            ));
        }
        match self.kind {
            ObservableKind::State => {
                let Some(spec) = &self.value else {
                    return Err(format!("{id}: a state declares its value type"));
                };
                if !self.fields.is_empty() {
                    return Err(format!("{id}: a state has no payload fields"));
                }
                spec.check(id)?;
                validate_defs(&self.params).map_err(|e| format!("{id}: {e}"))?;
                if self
                    .params
                    .iter()
                    .any(|p| matches!(p.kind, ParamKind::Json | ParamKind::Reference))
                {
                    return Err(format!(
                        "{id}: an argument is a boolean, a number or a text"
                    ));
                }
            }
            ObservableKind::Event => {
                if self.value.is_some() || !self.params.is_empty() {
                    return Err(format!("{id}: an event has no value and no arguments"));
                }
                if self.fields.len() > MAX_FIELDS {
                    return Err(format!("{id}: at most {MAX_FIELDS} payload fields"));
                }
                let mut seen = HashSet::new();
                for f in &self.fields {
                    if !crate::actions::params::valid_param_name(&f.name) || !seen.insert(&f.name) {
                        return Err(format!(
                            "{id}: field `{}` is not a unique plain name",
                            f.name
                        ));
                    }
                    ValueSpec {
                        kind: f.kind,
                        values: f.values.clone(),
                    }
                    .check(&format!("{id}.{}", f.name))?;
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn level() -> ObservableDefinition {
        ObservableDefinition::state(
            "example.counter",
            "level",
            "Counter level",
            "How full it is.",
        )
        .category("Counter")
        .one_of(&[
            ("zero", "Empty", Some(Role::Inactive)),
            ("low", "Low", Some(Role::Ok)),
            ("high", "High", Some(Role::Warning)),
        ])
    }

    #[test]
    fn a_well_formed_state_passes_and_serialises_as_planned() {
        let d = level();
        assert!(d.check().is_ok());
        let v = serde_json::to_value(&d).unwrap();
        assert_eq!(v["id"], "example.counter.level");
        assert_eq!(v["kind"], "state");
        assert_eq!(v["value"]["type"], "enum");
        assert_eq!(v["value"]["values"][1]["role"], "ok");
        assert!(v.get("params").is_none() && v.get("fields").is_none());
        let back: ObservableDefinition = serde_json::from_value(v).unwrap();
        assert_eq!(back, d);
    }

    #[test]
    fn bad_definitions_are_refused_with_their_id() {
        let mut d = level();
        d.id = "wrong".into();
        assert!(d.check().unwrap_err().contains("joined by a dot"));
        let mut d = level();
        d.value = None;
        assert!(d.check().unwrap_err().contains("value type"));
        let d = ObservableDefinition::state("a.b", "x", "X", "").one_of(&[]);
        assert!(d.check().is_err());
        let d = ObservableDefinition::state("a.b", "x", "X", "")
            .one_of(&[("a", "A", None), ("a", "Again", None)]);
        assert!(d.check().unwrap_err().contains("twice"));
        let d = ObservableDefinition::state("a.b", "x", "X", "")
            .number()
            .param(ParamDef::json("j", "J"));
        assert!(d.check().is_err());
        let d = ObservableDefinition::state("a.b", "Bad Id", "X", "").number();
        assert!(d.check().is_err());
        let mut d = ObservableDefinition::event("a.b", "e", "E", "");
        d.value = Some(ValueSpec {
            kind: ValueType::Number,
            values: vec![],
        });
        assert!(d.check().is_err());
    }

    #[test]
    fn parameterised_states_validate_their_arguments_schema() {
        let d = ObservableDefinition::state("agent-control", "slot.status", "Track", "")
            .param(
                ParamDef::number("slot", "Track")
                    .range(0.0, 7.0)
                    .integer()
                    .required(),
            )
            .one_of(&[("idle", "Idle", None)]);
        assert!(d.check().is_ok());
        let dup = d.clone().param(ParamDef::number("slot", "Again"));
        assert!(dup.check().is_err());
    }

    #[test]
    fn values_fit_their_declaration() {
        let spec = level().value.unwrap();
        assert_eq!(spec.fit(&Scalar::text("low")), Fit::Declared);
        assert_eq!(spec.fit(&Scalar::text("buffering")), Fit::Unlisted);
        assert_eq!(spec.fit(&Scalar::Num(1.0)), Fit::WrongType);
        let n = ObservableDefinition::state("a.b", "n", "N", "")
            .number()
            .value
            .unwrap();
        assert_eq!(n.fit(&Scalar::Num(3.0)), Fit::Declared);
        assert_eq!(n.fit(&Scalar::text("3")), Fit::WrongType);
        let b = ObservableDefinition::state("a.b", "b", "B", "")
            .on_off(("On", None), ("Off", None))
            .value
            .unwrap();
        assert_eq!(b.fit(&Scalar::Bool(false)), Fit::Declared);
        assert_eq!(b.fit(&Scalar::text("false")), Fit::WrongType);
    }
}
