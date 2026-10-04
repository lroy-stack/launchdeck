//! The only values a state can hold: a boolean, a number or a short string.
//!
//! Numbers are `f64`, so `1` and `1.0` are the same value everywhere (in a rule,
//! in an address and in the hub). Integers beyond 2^53 lose precision; no state
//! this milestone needs one.

use std::fmt;

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::{Number, Value};

/// Longest string a state value or an address argument may hold, in bytes.
pub const MAX_STRING_BYTES: usize = 256;
/// Integers up to this magnitude are exactly representable as `f64`.
const MAX_EXACT_INT: f64 = 9_007_199_254_740_992.0;

#[derive(Debug, Clone, PartialEq)]
pub enum Scalar {
    Bool(bool),
    Num(f64),
    Str(String),
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ScalarError {
    #[error("a value must be a boolean, a number or a text, not {0}")]
    NotScalar(&'static str),
    #[error("a number must be finite")]
    NotFinite,
    #[error("a text is longer than {MAX_STRING_BYTES} bytes")]
    TooLong,
}

impl Scalar {
    pub fn text(s: impl Into<String>) -> Scalar {
        Scalar::Str(s.into())
    }

    pub fn from_json(v: &Value) -> Result<Scalar, ScalarError> {
        match v {
            Value::Bool(b) => Ok(Scalar::Bool(*b)),
            Value::Number(n) => match n.as_f64() {
                Some(f) if f.is_finite() => Ok(Scalar::Num(f)),
                _ => Err(ScalarError::NotFinite),
            },
            Value::String(s) if s.len() > MAX_STRING_BYTES => Err(ScalarError::TooLong),
            Value::String(s) => Ok(Scalar::Str(s.clone())),
            Value::Null => Err(ScalarError::NotScalar("null")),
            Value::Array(_) => Err(ScalarError::NotScalar("a list")),
            Value::Object(_) => Err(ScalarError::NotScalar("an object")),
        }
    }

    /// The JSON form. A whole number is written without a fraction, so `2.0`
    /// and `2` serialise identically.
    pub fn to_json(&self) -> Value {
        match self {
            Scalar::Bool(b) => Value::Bool(*b),
            Scalar::Str(s) => Value::String(s.clone()),
            Scalar::Num(n) => Value::Number(number(*n)),
        }
    }

    /// Typed equality. A text never equals a number, and a number never
    /// equals a boolean.
    pub fn same_as(&self, other: &Scalar) -> bool {
        match (self, other) {
            (Scalar::Bool(a), Scalar::Bool(b)) => a == b,
            (Scalar::Num(a), Scalar::Num(b)) => a == b,
            (Scalar::Str(a), Scalar::Str(b)) => a == b,
            _ => false,
        }
    }

    pub fn as_number(&self) -> Option<f64> {
        match self {
            Scalar::Num(n) => Some(*n),
            _ => None,
        }
    }

    pub fn as_text(&self) -> Option<&str> {
        match self {
            Scalar::Str(s) => Some(s),
            _ => None,
        }
    }

    /// The name of the look a value points at when a rule says "show the look
    /// named after the value": a text as it is, `true`, `false`, or a number
    /// written without a pointless fraction.
    pub fn look_name(&self) -> String {
        match self {
            Scalar::Str(s) => s.clone(),
            other => other.to_string(),
        }
    }

    pub fn kind(&self) -> &'static str {
        match self {
            Scalar::Bool(_) => "a boolean",
            Scalar::Num(_) => "a number",
            Scalar::Str(_) => "a text",
        }
    }
}

fn number(n: f64) -> Number {
    if n.fract() == 0.0 && n.abs() < MAX_EXACT_INT {
        Number::from(n as i64)
    } else {
        // Finite by construction, so this cannot fail.
        Number::from_f64(n).unwrap_or_else(|| Number::from(0))
    }
}

impl fmt::Display for Scalar {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Scalar::Bool(b) => write!(f, "{b}"),
            Scalar::Str(s) => f.write_str(s),
            Scalar::Num(n) => write!(f, "{}", number(*n)),
        }
    }
}

impl Serialize for Scalar {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        self.to_json().serialize(s)
    }
}

impl<'de> Deserialize<'de> for Scalar {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let v = Value::deserialize(d)?;
        Scalar::from_json(&v).map_err(serde::de::Error::custom)
    }
}

impl From<bool> for Scalar {
    fn from(b: bool) -> Self {
        Scalar::Bool(b)
    }
}

impl From<f64> for Scalar {
    fn from(n: f64) -> Self {
        Scalar::Num(n)
    }
}

impl From<i64> for Scalar {
    fn from(n: i64) -> Self {
        Scalar::Num(n as f64)
    }
}

impl From<&str> for Scalar {
    fn from(s: &str) -> Self {
        Scalar::Str(s.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn integers_and_floats_are_the_same_number() {
        let a = Scalar::from_json(&json!(1)).unwrap();
        let b = Scalar::from_json(&json!(1.0)).unwrap();
        assert!(a.same_as(&b));
        assert_eq!(a.to_json(), b.to_json());
        assert_eq!(a.to_json().to_string(), "1");
        assert_eq!(Scalar::Num(2.5).to_json().to_string(), "2.5");
        assert_eq!(Scalar::Num(-0.0).to_json().to_string(), "0");
        assert!(Scalar::Num(-0.0).same_as(&Scalar::Num(0.0)));
    }

    #[test]
    fn equality_is_typed() {
        assert!(!Scalar::text("1").same_as(&Scalar::Num(1.0)));
        assert!(!Scalar::Bool(true).same_as(&Scalar::Num(1.0)));
        assert!(!Scalar::text("true").same_as(&Scalar::Bool(true)));
        assert!(Scalar::text("a").same_as(&Scalar::text("a")));
    }

    #[test]
    fn only_scalars_are_accepted() {
        for bad in [json!(null), json!([1]), json!({ "a": 1 })] {
            assert!(matches!(
                Scalar::from_json(&bad),
                Err(ScalarError::NotScalar(_))
            ));
        }
        assert_eq!(
            Scalar::from_json(&json!("x".repeat(MAX_STRING_BYTES + 1))),
            Err(ScalarError::TooLong)
        );
        assert!(Scalar::from_json(&json!("x".repeat(MAX_STRING_BYTES))).is_ok());
    }

    #[test]
    fn look_names_follow_the_value() {
        assert_eq!(Scalar::text("running").look_name(), "running");
        assert_eq!(Scalar::Bool(true).look_name(), "true");
        assert_eq!(Scalar::Num(3.0).look_name(), "3");
        assert_eq!(Scalar::Num(0.5).look_name(), "0.5");
    }

    #[test]
    fn serde_round_trips() {
        for v in [json!(true), json!(7), json!(0.25), json!("high")] {
            let s: Scalar = serde_json::from_value(v.clone()).unwrap();
            assert_eq!(serde_json::to_value(&s).unwrap(), v);
        }
        assert!(serde_json::from_value::<Scalar>(json!(null)).is_err());
    }
}
