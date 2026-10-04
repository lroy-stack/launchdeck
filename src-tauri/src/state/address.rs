//! Where a state lives: a provider, one of its state definitions and, for a
//! definition with several instances, the arguments that pick one.
//!
//! ```text
//! agent-control . slot.status        ( slot = 2 )
//! agent-control . command.state      ( command = "approve" )
//! example.counter . level
//! homeassistant . entity.state       ( entity = "light.kitchen" )
//! ^ provider       ^ definition id     ^ arguments (the scope)
//! ```
//!
//! An address is plain data and nothing about it is specific to a provider.
//! The hub stores values under a [`StateKey`], the canonical form: the same
//! address written two ways (`slot: 2` and `slot: 2.0`, arguments in another
//! order) is the same key.

use std::collections::BTreeMap;
use std::fmt;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use super::value::Scalar;
use crate::actions::params::valid_param_name;

pub const MAX_ID_LEN: usize = 64;
pub const MAX_ARGS: usize = 4;
/// Longest text argument, in bytes.
pub const MAX_ARG_BYTES: usize = 64;

/// A lowercase dotted identifier such as `agent-control`, `example.counter`
/// or `slot.status`: segments of `[a-z][a-z0-9-]*` joined by dots.
pub fn valid_ident(s: &str) -> bool {
    if s.is_empty() || s.len() > MAX_ID_LEN {
        return false;
    }
    s.split('.').all(|seg| {
        let mut chars = seg.chars();
        chars.next().is_some_and(|c| c.is_ascii_lowercase())
            && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
    })
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AddressError {
    #[error("`{0}` is not a valid provider id")]
    Provider(String),
    #[error("`{0}` is not a valid state id")]
    Id(String),
    #[error("a state takes at most {MAX_ARGS} arguments")]
    TooManyArgs,
    #[error("`{0}` is not a valid argument name")]
    ArgName(String),
    #[error("argument `{0}` is longer than {MAX_ARG_BYTES} bytes")]
    ArgTooLong(String),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StateAddress {
    pub provider: String,
    pub id: String,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub args: BTreeMap<String, Scalar>,
}

impl StateAddress {
    pub fn new(provider: &str, id: &str) -> Self {
        Self {
            provider: provider.to_string(),
            id: id.to_string(),
            args: BTreeMap::new(),
        }
    }

    pub fn with(mut self, name: &str, value: impl Into<Scalar>) -> Self {
        self.args.insert(name.to_string(), value.into());
        self
    }

    /// Strict check. Parsing is lenient on purpose (a saved file with a bad
    /// address must still open); anything that is saved or written is checked.
    pub fn validate(&self) -> Result<(), AddressError> {
        if !valid_ident(&self.provider) {
            return Err(AddressError::Provider(self.provider.clone()));
        }
        if !valid_ident(&self.id) {
            return Err(AddressError::Id(self.id.clone()));
        }
        if self.args.len() > MAX_ARGS {
            return Err(AddressError::TooManyArgs);
        }
        for (name, value) in &self.args {
            if !valid_param_name(name) {
                return Err(AddressError::ArgName(name.clone()));
            }
            if matches!(value, Scalar::Str(s) if s.len() > MAX_ARG_BYTES) {
                return Err(AddressError::ArgTooLong(name.clone()));
            }
        }
        Ok(())
    }

    pub fn key(&self) -> StateKey {
        StateKey::new(&self.provider, &self.id, &self.args)
    }

    /// The arguments as a JSON object, for the parameter schema to check.
    pub fn args_json(&self) -> Value {
        Value::Object(
            self.args
                .iter()
                .map(|(k, v)| (k.clone(), v.to_json()))
                .collect::<Map<_, _>>(),
        )
    }

    pub fn from_args_json(provider: &str, id: &str, args: &Value) -> Result<Self, String> {
        let mut out = StateAddress::new(provider, id);
        match args {
            Value::Null => {}
            Value::Object(map) => {
                for (k, v) in map {
                    let s = Scalar::from_json(v).map_err(|e| format!("argument `{k}`: {e}"))?;
                    out.args.insert(k.clone(), s);
                }
            }
            _ => return Err("arguments must be an object".into()),
        }
        Ok(out)
    }
}

impl fmt::Display for StateAddress {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}", self.provider, self.id)?;
        if !self.args.is_empty() {
            let args: Vec<String> = self.args.iter().map(|(k, v)| format!("{k}={v}")).collect();
            write!(f, "({})", args.join(", "))?;
        }
        Ok(())
    }
}

/// The canonical identity of one state instance. Equal addresses give equal
/// keys. Keeping the three parts apart means `a.b` + `c` and `a` + `b.c` never
/// collide.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct StateKey {
    provider: String,
    id: String,
    /// Canonical JSON of the arguments (sorted, whole numbers without a
    /// fraction), empty when there are none.
    args: String,
}

impl StateKey {
    pub fn new(provider: &str, id: &str, args: &BTreeMap<String, Scalar>) -> Self {
        let args = if args.is_empty() {
            String::new()
        } else {
            let sorted: BTreeMap<&str, Value> = args
                .iter()
                .map(|(k, v)| (k.as_str(), v.to_json()))
                .collect();
            serde_json::to_string(&sorted).unwrap_or_default()
        };
        Self {
            provider: provider.to_string(),
            id: id.to_string(),
            args,
        }
    }

    pub fn provider(&self) -> &str {
        &self.provider
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn address(&self) -> StateAddress {
        let args = if self.args.is_empty() {
            Value::Null
        } else {
            serde_json::from_str(&self.args).unwrap_or(Value::Null)
        };
        StateAddress::from_args_json(&self.provider, &self.id, &args)
            .unwrap_or_else(|_| StateAddress::new(&self.provider, &self.id))
    }
}

impl fmt::Display for StateKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.address().fmt(f)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn identifiers_are_strict() {
        for ok in [
            "demo",
            "agent-control",
            "example.counter",
            "slot.status",
            "a1",
        ] {
            assert!(valid_ident(ok), "{ok}");
        }
        for bad in [
            "",
            "Demo",
            "1a",
            "a..b",
            ".a",
            "a.",
            "a_b",
            "a b",
            "a/b",
            "a:b",
            "é",
            &"a".repeat(65),
        ] {
            assert!(!valid_ident(bad), "{bad:?}");
        }
    }

    #[test]
    fn the_same_address_written_two_ways_is_the_same_key() {
        let a = StateAddress::new("agent-control", "slot.status").with("slot", 2.0);
        let b: StateAddress = serde_json::from_value(
            json!({ "provider": "agent-control", "id": "slot.status", "args": { "slot": 2 } }),
        )
        .unwrap();
        assert_eq!(a.key(), b.key());
        let one = StateAddress::new("p.q", "s").with("b", "x").with("a", 1.0);
        let two = StateAddress::new("p.q", "s").with("a", 1i64).with("b", "x");
        assert_eq!(one.key(), two.key());
    }

    #[test]
    fn different_instances_and_different_parts_are_different_keys() {
        let s2 = StateAddress::new("agent-control", "slot.status").with("slot", 2i64);
        let s3 = StateAddress::new("agent-control", "slot.status").with("slot", 3i64);
        assert_ne!(s2.key(), s3.key());
        // "2" the text is not 2 the number.
        let text = StateAddress::new("agent-control", "slot.status").with("slot", "2");
        assert_ne!(s2.key(), text.key());
        // The three parts stay apart.
        assert_ne!(
            StateAddress::new("a.b", "c").key(),
            StateAddress::new("a", "b.c").key()
        );
        // Instances of one definition in different scopes.
        let k = StateAddress::new("ha", "entity.state").with("entity", "light.kitchen");
        let o = StateAddress::new("ha", "entity.state").with("entity", "light.office");
        assert_ne!(k.key(), o.key());
        assert_eq!(k.key().provider(), "ha");
        assert_eq!(k.key().id(), "entity.state");
    }

    #[test]
    fn a_key_converts_back_to_its_address() {
        let a = StateAddress::new("agent-control", "command.state").with("command", "approve");
        assert_eq!(a.key().address(), a);
        let n = StateAddress::new("demo", "alpha");
        assert_eq!(n.key().address(), n);
        assert_eq!(
            a.key().to_string(),
            "agent-control.command.state(command=approve)"
        );
        assert_eq!(n.to_string(), "demo.alpha");
    }

    #[test]
    fn validation_names_the_problem() {
        let ok = StateAddress::new("agent-control", "slot.status").with("slot", 1i64);
        assert!(ok.validate().is_ok());
        assert_eq!(
            StateAddress::new("Bad", "x").validate(),
            Err(AddressError::Provider("Bad".into()))
        );
        assert_eq!(
            StateAddress::new("good", "bad id").validate(),
            Err(AddressError::Id("bad id".into()))
        );
        assert_eq!(
            StateAddress::new("good", "x").with("1x", 1i64).validate(),
            Err(AddressError::ArgName("1x".into()))
        );
        assert_eq!(
            StateAddress::new("good", "x")
                .with("a", "x".repeat(65).as_str())
                .validate(),
            Err(AddressError::ArgTooLong("a".into()))
        );
        let mut many = StateAddress::new("good", "x");
        for n in ["a", "b", "c", "d", "e"] {
            many = many.with(n, 1i64);
        }
        assert_eq!(many.validate(), Err(AddressError::TooManyArgs));
    }

    #[test]
    fn unknown_fields_and_non_scalar_arguments_do_not_parse() {
        assert!(serde_json::from_value::<StateAddress>(
            json!({ "provider": "a", "id": "b", "scope": "x" })
        )
        .is_err());
        assert!(serde_json::from_value::<StateAddress>(
            json!({ "provider": "a", "id": "b", "args": { "x": [1] } })
        )
        .is_err());
    }
}
