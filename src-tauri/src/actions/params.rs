//! The parameter schema of an action: small, closed and declarative.
//!
//! LaunchDeck draws the form from this and validates against it, so a provider
//! (built in or installed) never ships UI code. The same schema describes the
//! arguments of a stored assignment, which makes "is this assignment well
//! formed" a pure function of the definition and the saved values.

use std::collections::HashSet;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use super::definition::Issue;

pub const MAX_PARAMS: usize = 16;
pub const MAX_OPTIONS: usize = 64;
/// Upper bound for any text value, whatever a definition asks for.
pub const MAX_TEXT_VALUE: usize = 16 * 1024;
const MAX_LABEL: usize = 40;
const MAX_HELP: usize = 280;
const MAX_REF_ID: usize = 128;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ParamKind {
    String,
    Number,
    Boolean,
    Enum,
    /// An id of a surface or of a page, chosen from a list.
    Reference,
    /// Any JSON value, edited as text. Kept so manifest schema 1 plugins that
    /// declared `json` arguments keep working; new plugins should not use it.
    Json,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RefTarget {
    Surface,
    Page,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ParamOption {
    pub value: String,
    pub label: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ParamDef {
    pub name: String,
    pub label: String,
    #[serde(default)]
    pub description: String,
    #[serde(rename = "type")]
    pub kind: ParamKind,
    /// Missing means optional.
    #[serde(default)]
    pub required: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub placeholder: Option<String>,
    // string
    #[serde(default)]
    pub multiline: bool,
    #[serde(default)]
    pub monospace: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_length: Option<usize>,
    // number
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max: Option<f64>,
    #[serde(default)]
    pub integer: bool,
    // enum
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub options: Vec<ParamOption>,
    // reference
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<RefTarget>,
    /// For a page reference: the sibling parameter that holds the surface the
    /// page belongs to. Without it, the page is one of the surface being edited.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope: Option<String>,
}

impl ParamDef {
    fn base(name: &str, label: &str, kind: ParamKind) -> Self {
        Self {
            name: name.into(),
            label: label.into(),
            description: String::new(),
            kind,
            required: false,
            default: None,
            placeholder: None,
            multiline: false,
            monospace: false,
            max_length: None,
            min: None,
            max: None,
            integer: false,
            options: vec![],
            target: None,
            scope: None,
        }
    }

    pub fn string(name: &str, label: &str) -> Self {
        Self::base(name, label, ParamKind::String)
    }

    pub fn number(name: &str, label: &str) -> Self {
        Self::base(name, label, ParamKind::Number)
    }

    pub fn boolean(name: &str, label: &str) -> Self {
        Self::base(name, label, ParamKind::Boolean)
    }

    pub fn choice(name: &str, label: &str, options: &[(&str, &str)]) -> Self {
        let mut p = Self::base(name, label, ParamKind::Enum);
        p.options = options
            .iter()
            .map(|(value, label)| ParamOption {
                value: (*value).into(),
                label: (*label).into(),
            })
            .collect();
        p
    }

    pub fn reference(name: &str, label: &str, target: RefTarget) -> Self {
        let mut p = Self::base(name, label, ParamKind::Reference);
        p.target = Some(target);
        p
    }

    pub fn json(name: &str, label: &str) -> Self {
        Self::base(name, label, ParamKind::Json)
    }

    pub fn required(mut self) -> Self {
        self.required = true;
        self
    }

    pub fn help(mut self, text: &str) -> Self {
        self.description = text.into();
        self
    }

    pub fn placeholder(mut self, text: &str) -> Self {
        self.placeholder = Some(text.into());
        self
    }

    pub fn default_value(mut self, value: Value) -> Self {
        self.default = Some(value);
        self
    }

    pub fn multiline(mut self) -> Self {
        self.multiline = true;
        self
    }

    pub fn monospace(mut self) -> Self {
        self.monospace = true;
        self
    }

    pub fn max_length(mut self, n: usize) -> Self {
        self.max_length = Some(n);
        self
    }

    pub fn range(mut self, min: f64, max: f64) -> Self {
        self.min = Some(min);
        self.max = Some(max);
        self
    }

    pub fn integer(mut self) -> Self {
        self.integer = true;
        self
    }

    pub fn scope(mut self, sibling: &str) -> Self {
        self.scope = Some(sibling.into());
        self
    }
}

pub fn valid_param_name(s: &str) -> bool {
    let mut chars = s.chars();
    s.len() <= 32
        && chars
            .next()
            .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// A character a person cannot see, or that reorders the text around it: a
/// control character other than a line break or a tab, a line or paragraph
/// separator, and every default-ignorable code point. The one predicate of
/// what a reviewed action may not hide (an artifact's review and inspect)
/// and of the addresses the runtime refuses to send to.
pub(crate) fn hidden_char(c: char) -> bool {
    (c.is_control() && !matches!(c, '\n' | '\t'))
        || matches!(c, '\u{2028}' | '\u{2029}')
        || default_ignorable(c)
}

/// Unicode's `Default_Ignorable_Code_Point` (DerivedCoreProperties.txt):
/// what a renderer shows as nothing, such as zero-width spaces and joiners,
/// direction marks, embeddings, overrides and isolates, variation
/// selectors, fillers, tags and the soft hyphen.
fn default_ignorable(c: char) -> bool {
    matches!(
        u32::from(c),
        0x00AD
            | 0x034F
            | 0x061C
            | 0x115F..=0x1160
            | 0x17B4..=0x17B5
            | 0x180B..=0x180F
            | 0x200B..=0x200F
            | 0x202A..=0x202E
            | 0x2060..=0x206F
            | 0x3164
            | 0xFE00..=0xFE0F
            | 0xFEFF
            | 0xFFA0
            | 0xFFF0..=0xFFF8
            | 0x1BCA0..=0x1BCA3
            | 0x1D173..=0x1D17A
            | 0xE0000..=0xE0FFF
    )
}

fn text_ok(s: &str, max: usize) -> bool {
    s.chars().count() <= max && !s.chars().any(|c| c.is_control() && c != '\n')
}

fn number_of(v: &Value) -> Option<f64> {
    v.as_f64().filter(|n| n.is_finite())
}

/// Check that a list of definitions is itself well formed. Run on every
/// provider's declarations, so a bad schema is found when it is registered and
/// not when someone tries to use it.
pub fn validate_defs(defs: &[ParamDef]) -> Result<(), String> {
    if defs.len() > MAX_PARAMS {
        return Err(format!("more than {MAX_PARAMS} parameters"));
    }
    let mut seen = HashSet::new();
    for d in defs {
        let n = &d.name;
        if !valid_param_name(n) {
            return Err(format!("parameter \"{n}\" must be a plain identifier"));
        }
        if !seen.insert(n.as_str()) {
            return Err(format!("parameter \"{n}\" is declared twice"));
        }
        let label = d.label.trim();
        if label.is_empty() || label.chars().count() > MAX_LABEL || !text_ok(label, MAX_LABEL) {
            return Err(format!(
                "parameter \"{n}\" needs a label of 1-{MAX_LABEL} characters"
            ));
        }
        if !text_ok(&d.description, MAX_HELP) {
            return Err(format!("parameter \"{n}\" description is too long"));
        }
        if d.placeholder.as_deref().is_some_and(|p| !text_ok(p, 120)) {
            return Err(format!("parameter \"{n}\" placeholder is too long"));
        }
        let only = |ok: bool, what: &str| -> Result<(), String> {
            if ok {
                Ok(())
            } else {
                Err(format!(
                    "parameter \"{n}\": {what} does not apply to type {:?}",
                    d.kind
                ))
            }
        };
        let is_string = d.kind == ParamKind::String;
        let is_number = d.kind == ParamKind::Number;
        only(
            is_string || (!d.multiline && !d.monospace && d.max_length.is_none()),
            "multiline, monospace and maxLength",
        )?;
        only(
            is_number || (d.min.is_none() && d.max.is_none() && !d.integer),
            "min, max and integer",
        )?;
        only(d.kind == ParamKind::Enum || d.options.is_empty(), "options")?;
        only(
            d.kind == ParamKind::Reference || (d.target.is_none() && d.scope.is_none()),
            "target and scope",
        )?;
        if let Some(m) = d.max_length {
            if m == 0 || m > MAX_TEXT_VALUE {
                return Err(format!(
                    "parameter \"{n}\": maxLength must be 1-{MAX_TEXT_VALUE}"
                ));
            }
        }
        if let (Some(lo), Some(hi)) = (d.min, d.max) {
            if lo > hi {
                return Err(format!("parameter \"{n}\": min is greater than max"));
            }
        }
        match d.kind {
            ParamKind::Enum => {
                if d.options.is_empty() || d.options.len() > MAX_OPTIONS {
                    return Err(format!("parameter \"{n}\" needs 1-{MAX_OPTIONS} options"));
                }
                let mut values = HashSet::new();
                for o in &d.options {
                    if o.value.is_empty()
                        || o.value.len() > 64
                        || !values.insert(o.value.as_str())
                        || !text_ok(&o.label, MAX_LABEL)
                        || o.label.trim().is_empty()
                    {
                        return Err(format!(
                            "parameter \"{n}\" has an empty, repeated or oversized option"
                        ));
                    }
                }
            }
            ParamKind::Reference => {
                let Some(target) = d.target else {
                    return Err(format!(
                        "parameter \"{n}\" is a reference and needs a target"
                    ));
                };
                if let Some(scope) = &d.scope {
                    let ok = target == RefTarget::Page
                        && defs.iter().any(|o| {
                            &o.name == scope
                                && o.kind == ParamKind::Reference
                                && o.target == Some(RefTarget::Surface)
                        });
                    if !ok {
                        return Err(format!(
                            "parameter \"{n}\": scope must name a surface reference of the same action"
                        ));
                    }
                }
            }
            _ => {}
        }
        if let Some(default) = &d.default {
            // The default has to be acceptable on its own terms.
            let mut probe = d.clone();
            probe.default = None;
            probe.required = false;
            if let Err(issue) = check_value(&probe, default) {
                return Err(format!("default of parameter \"{n}\": {}", issue.message));
            }
        }
    }
    Ok(())
}

/// Check one present value against its definition.
fn check_value(d: &ParamDef, v: &Value) -> Result<(), Issue> {
    let bad = |msg: String| Err(Issue::param(&d.name, msg));
    let label = &d.label;
    match d.kind {
        ParamKind::String => {
            let Some(s) = v.as_str() else {
                return bad(format!("{label} must be text"));
            };
            let limit = d.max_length.unwrap_or(MAX_TEXT_VALUE).min(MAX_TEXT_VALUE);
            if s.chars().count() > limit {
                return bad(format!("{label} is longer than {limit} characters"));
            }
        }
        ParamKind::Number => {
            let Some(n) = number_of(v) else {
                return bad(format!("{label} must be a number"));
            };
            if d.integer && n.fract() != 0.0 {
                return bad(format!("{label} must be a whole number"));
            }
            if d.min.is_some_and(|lo| n < lo) || d.max.is_some_and(|hi| n > hi) {
                return bad(match (d.min, d.max) {
                    (Some(lo), Some(hi)) => format!("{label} must be between {lo} and {hi}"),
                    (Some(lo), None) => format!("{label} must be at least {lo}"),
                    (None, Some(hi)) => format!("{label} must be at most {hi}"),
                    (None, None) => unreachable!("a bound was exceeded"),
                });
            }
        }
        ParamKind::Boolean => {
            if !v.is_boolean() {
                return bad(format!("{label} must be on or off"));
            }
        }
        ParamKind::Enum => {
            let ok = v
                .as_str()
                .is_some_and(|s| d.options.iter().any(|o| o.value == s));
            if !ok {
                return bad(format!("{label} must be one of the listed choices"));
            }
        }
        ParamKind::Reference => {
            let ok = v
                .as_str()
                .is_some_and(|s| !s.is_empty() && s.len() <= MAX_REF_ID);
            if !ok {
                return bad(format!("{label} must be chosen from the list"));
            }
        }
        ParamKind::Json => {
            if v.is_null() {
                return bad(format!("{label} must be a value"));
            }
        }
    }
    Ok(())
}

/// A value that counts as "not given": missing, `null`, or empty text.
fn is_blank(d: &ParamDef, v: Option<&Value>) -> bool {
    match v {
        None | Some(Value::Null) => true,
        Some(Value::String(s)) => {
            matches!(d.kind, ParamKind::String | ParamKind::Reference) && s.trim().is_empty()
        }
        Some(_) => false,
    }
}

/// Validate `args` against `defs` and return the arguments to store or run:
/// declared parameters only, defaults filled in, blanks removed. Every problem
/// is reported, each naming its parameter, so a form can mark all fields at once.
pub fn normalize(defs: &[ParamDef], args: &Value) -> Result<Value, Vec<Issue>> {
    let empty = Map::new();
    let map = match args {
        Value::Null => &empty,
        Value::Object(m) => m,
        _ => return Err(vec![Issue::new("arguments must be an object")]),
    };
    let mut issues = Vec::new();
    for key in map.keys() {
        if !defs.iter().any(|d| &d.name == key) {
            issues.push(Issue::param(
                key,
                format!("`{key}` is not a setting of this action"),
            ));
        }
    }
    let mut out = Map::new();
    for d in defs {
        let given = map.get(&d.name);
        let value = if is_blank(d, given) {
            match &d.default {
                Some(default) => Some(default.clone()),
                None if d.required => {
                    issues.push(Issue::param(&d.name, format!("{} is required", d.label)));
                    None
                }
                None => None,
            }
        } else {
            given.cloned()
        };
        if let Some(v) = value {
            match check_value(d, &v) {
                Ok(()) => {
                    out.insert(d.name.clone(), v);
                }
                Err(issue) => issues.push(issue),
            }
        }
    }
    if issues.is_empty() {
        Ok(Value::Object(out))
    } else {
        Err(issues)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// Every default-ignorable code point of Unicode is hidden, and nothing
    /// a person reads as text is.
    #[test]
    fn hidden_is_every_default_ignorable_code_point() {
        for c in [
            'a',
            'é',
            ' ',
            '\n',
            '\t',
            '/',
            '\u{2215}',
            '\u{1F600}',
            '\u{3000}',
        ] {
            assert!(!hidden_char(c), "{c:?}");
        }
        let ignorable = [
            0xAD, 0x34F, 0x61C, 0x115F, 0x1160, 0x17B4, 0x17B5, 0x180B, 0x180E, 0x180F, 0x200B,
            0x200F, 0x202A, 0x2060, 0x2064, 0x2065, 0x206F, 0x3164, 0xFE00, 0xFE0F, 0xFEFF, 0xFFA0,
            0xFFF0, 0xFFF8, 0x1BCA0, 0x1BCA3, 0x1D173, 0x1D17A, 0xE0000, 0xE0001, 0xE007F, 0xE0100,
            0xE01EF, 0xE0FFF,
        ];
        for code in ignorable {
            assert!(hidden_char(char::from_u32(code).unwrap()), "U+{code:04X}");
        }
        for code in [0xAC, 0x34E, 0x1161, 0x17B6, 0x2070, 0xFE10, 0xFFF9, 0xE1000] {
            assert!(!hidden_char(char::from_u32(code).unwrap()), "U+{code:04X}");
        }
        for c in ['\0', '\r', '\u{1b}', '\u{85}', '\u{2028}', '\u{2029}'] {
            assert!(hidden_char(c), "{c:?}");
        }
    }

    fn defs() -> Vec<ParamDef> {
        vec![
            ParamDef::string("url", "Link").required().max_length(20),
            ParamDef::choice("method", "Method", &[("GET", "GET"), ("POST", "POST")])
                .default_value(json!("GET")),
            ParamDef::number("slot", "Slot").range(0.0, 7.0).integer(),
            ParamDef::boolean("loud", "Loud"),
        ]
    }

    fn messages(r: Result<Value, Vec<Issue>>) -> Vec<String> {
        r.unwrap_err().into_iter().map(|i| i.message).collect()
    }

    #[test]
    fn defaults_are_applied_and_unset_optionals_stay_out() {
        let out = normalize(&defs(), &json!({ "url": "https://a.b" })).unwrap();
        assert_eq!(out, json!({ "url": "https://a.b", "method": "GET" }));
    }

    #[test]
    fn required_and_blank_values_are_reported_by_parameter() {
        for args in [
            json!({}),
            json!(null),
            json!({ "url": null }),
            json!({ "url": "   " }),
        ] {
            let issues = normalize(&defs(), &args).unwrap_err();
            assert_eq!(issues.len(), 1, "{args}");
            assert_eq!(issues[0].param.as_deref(), Some("url"));
            assert_eq!(issues[0].message, "Link is required");
        }
    }

    #[test]
    fn blank_optional_text_falls_back_to_the_default_or_disappears() {
        let d = vec![
            ParamDef::string("text", "Text").default_value(json!("continue")),
            ParamDef::string("cwd", "Folder"),
        ];
        let out = normalize(&d, &json!({ "text": "", "cwd": " " })).unwrap();
        assert_eq!(out, json!({ "text": "continue" }));
    }

    #[test]
    fn types_ranges_and_choices_are_enforced() {
        let a = json!({ "url": "x", "method": "TRACE", "slot": 2.5, "loud": "yes" });
        let m = messages(normalize(&defs(), &a));
        assert_eq!(m.len(), 3);
        assert!(m.iter().any(|s| s.contains("listed choices")));
        assert!(m.iter().any(|s| s.contains("whole number")));
        assert!(m.iter().any(|s| s.contains("on or off")));
        let m = messages(normalize(&defs(), &json!({ "url": "x", "slot": 9 })));
        assert_eq!(m, vec!["Slot must be between 0 and 7"]);
        let m = messages(normalize(&defs(), &json!({ "url": "x".repeat(21) })));
        assert_eq!(m, vec!["Link is longer than 20 characters"]);
        let m = messages(normalize(&defs(), &json!({ "url": 5 })));
        assert_eq!(m, vec!["Link must be text"]);
    }

    #[test]
    fn unknown_arguments_and_non_objects_are_errors() {
        let m = normalize(&defs(), &json!({ "url": "x", "extra": 1 })).unwrap_err();
        assert_eq!(m.len(), 1);
        assert_eq!(m[0].param.as_deref(), Some("extra"));
        assert!(normalize(&defs(), &json!([1])).is_err());
        assert!(normalize(&defs(), &json!("x")).is_err());
    }

    #[test]
    fn references_and_json_values_have_shape_checks_only() {
        let d = vec![
            ParamDef::reference("surfaceId", "Surface", RefTarget::Surface).required(),
            ParamDef::reference("pageId", "Page", RefTarget::Page).scope("surfaceId"),
            ParamDef::json("data", "Data"),
        ];
        let ok = normalize(&d, &json!({ "surfaceId": "main", "data": { "a": [1] } })).unwrap();
        assert_eq!(ok["data"], json!({ "a": [1] }));
        assert!(normalize(&d, &json!({ "surfaceId": 3 })).is_err());
        assert!(normalize(&d, &json!({ "surfaceId": "main", "data": null }))
            .unwrap()
            .get("data")
            .is_none());
    }

    #[test]
    fn the_schema_itself_is_validated() {
        assert!(validate_defs(&defs()).is_ok());
        let dup = vec![ParamDef::string("a", "A"), ParamDef::string("a", "B")];
        assert!(validate_defs(&dup).unwrap_err().contains("twice"));
        assert!(validate_defs(&[ParamDef::string("bad name", "A")]).is_err());
        assert!(validate_defs(&[ParamDef::string("a", "  ")]).is_err());
        let no_options = ParamDef::choice("c", "C", &[]);
        assert!(validate_defs(&[no_options]).is_err());
        let repeated = ParamDef::choice("c", "C", &[("a", "A"), ("a", "B")]);
        assert!(validate_defs(&[repeated]).is_err());
        let wrong_default = ParamDef::choice("c", "C", &[("a", "A")]).default_value(json!("b"));
        assert!(validate_defs(&[wrong_default])
            .unwrap_err()
            .contains("default"));
        let misplaced = ParamDef::boolean("b", "B").range(0.0, 1.0);
        assert!(validate_defs(&[misplaced])
            .unwrap_err()
            .contains("does not apply"));
        let bad_scope = vec![ParamDef::reference("p", "P", RefTarget::Page).scope("nope")];
        assert!(validate_defs(&bad_scope).unwrap_err().contains("scope"));
        let inverted = ParamDef::number("n", "N").range(5.0, 1.0);
        assert!(validate_defs(&[inverted]).is_err());
        let many: Vec<ParamDef> = (0..17)
            .map(|i| ParamDef::string(&format!("p{i}"), "P"))
            .collect();
        assert!(validate_defs(&many).is_err());
    }

    #[test]
    fn definitions_round_trip_through_the_manifest_vocabulary() {
        let p: ParamDef = serde_json::from_value(json!({
            "name": "n", "label": "N", "type": "number", "min": 1, "max": 3, "integer": true
        }))
        .unwrap();
        assert_eq!(p.kind, ParamKind::Number);
        assert!(!p.required, "required defaults to false");
        assert!(serde_json::from_value::<ParamDef>(
            json!({ "name": "n", "label": "N", "type": "number", "surprise": 1 })
        )
        .is_err());
        let back = serde_json::to_value(&p).unwrap();
        assert_eq!(back["type"], "number");
        assert!(back.get("default").is_none());
    }
}
