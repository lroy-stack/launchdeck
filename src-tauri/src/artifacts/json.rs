//! The JSON of a portable file: the pre-pass over the raw text, the content
//! digest, and the canonical writer.
//!
//! The pre-pass reads the whole document once, as tokens, before anything is
//! parsed into a type: a typed parse would silently keep the last of two
//! equal members and would have no say on depth or length. It allocates
//! nothing per value except member names, and stops at the first problem.

use std::cell::RefCell;
use std::collections::HashSet;
use std::fmt::{self, Write as _};

use serde::de::{self, DeserializeSeed, MapAccess, SeqAccess, Visitor};
use serde_json::Value;
use sha2::{Digest, Sha256};

use super::limits::{MAX_DEPTH, MAX_KEY_BYTES, MAX_STRING_BYTES};

/// What the pre-pass refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JsonProblem {
    /// Arrays and objects nested more than [`MAX_DEPTH`] deep.
    TooDeep,
    /// The same member name twice in one object, compared after unescaping.
    DuplicateKey(String),
    /// A string longer than [`MAX_STRING_BYTES`].
    StringTooLong,
    /// A member name longer than [`MAX_KEY_BYTES`].
    KeyTooLong,
    /// Not JSON: a syntax error, text after the value, a number too large.
    NotJson { line: usize, column: usize },
}

/// A problem and where it is: a JSON pointer (RFC 6901), empty for the whole
/// document. A member name too long is reported at its object.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrepassError {
    pub problem: JsonProblem,
    pub at: String,
}

/// Check the raw text of a file before it is parsed: nesting, duplicate
/// members, string and member-name lengths, and that it is one JSON value.
pub fn prepass(raw: &str) -> Result<(), PrepassError> {
    let state = RefCell::new(Walked::default());
    let mut de = serde_json::Deserializer::from_str(raw);
    let result = Walk {
        depth: 0,
        state: &state,
    }
    .deserialize(&mut de)
    .and_then(|()| de.end());
    match result {
        Ok(()) => Ok(()),
        Err(e) => Err(state.into_inner().found.unwrap_or(PrepassError {
            problem: JsonProblem::NotJson {
                line: e.line(),
                column: e.column(),
            },
            at: String::new(),
        })),
    }
}

#[derive(Default)]
struct Walked {
    /// Pointer segments to the value being read, already escaped.
    path: Vec<String>,
    found: Option<PrepassError>,
}

impl Walked {
    fn pointer(&self) -> String {
        self.path.iter().fold(String::new(), |mut p, s| {
            p.push('/');
            p.push_str(s);
            p
        })
    }
}

/// One member name or array index as a JSON pointer segment.
pub(super) fn segment(key: &str) -> String {
    key.replace('~', "~0").replace('/', "~1")
}

fn refuse<E: de::Error>(state: &RefCell<Walked>, problem: JsonProblem, at: String) -> E {
    let mut s = state.borrow_mut();
    if s.found.is_none() {
        s.found = Some(PrepassError { problem, at });
    }
    E::custom("refused by the pre-pass")
}

/// One value, `depth` arrays and objects down.
#[derive(Clone, Copy)]
struct Walk<'s> {
    depth: usize,
    state: &'s RefCell<Walked>,
}

impl Walk<'_> {
    fn enter<E: de::Error>(self) -> Result<Self, E> {
        let depth = self.depth + 1;
        if depth > MAX_DEPTH {
            let at = self.state.borrow().pointer();
            return Err(refuse(self.state, JsonProblem::TooDeep, at));
        }
        Ok(Walk { depth, ..self })
    }
}

impl<'de> DeserializeSeed<'de> for Walk<'_> {
    type Value = ();

    fn deserialize<D: de::Deserializer<'de>>(self, d: D) -> Result<(), D::Error> {
        d.deserialize_any(self)
    }
}

impl<'de> Visitor<'de> for Walk<'_> {
    type Value = ();

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("a JSON value")
    }

    fn visit_bool<E: de::Error>(self, _: bool) -> Result<(), E> {
        Ok(())
    }

    fn visit_i64<E: de::Error>(self, _: i64) -> Result<(), E> {
        Ok(())
    }

    fn visit_u64<E: de::Error>(self, _: u64) -> Result<(), E> {
        Ok(())
    }

    fn visit_f64<E: de::Error>(self, _: f64) -> Result<(), E> {
        Ok(())
    }

    fn visit_unit<E: de::Error>(self) -> Result<(), E> {
        Ok(())
    }

    fn visit_str<E: de::Error>(self, v: &str) -> Result<(), E> {
        if v.len() > MAX_STRING_BYTES {
            let at = self.state.borrow().pointer();
            return Err(refuse(self.state, JsonProblem::StringTooLong, at));
        }
        Ok(())
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<(), A::Error> {
        let inner = self.enter()?;
        for index in 0usize.. {
            self.state.borrow_mut().path.push(index.to_string());
            let next = seq.next_element_seed(inner)?;
            self.state.borrow_mut().path.pop();
            if next.is_none() {
                break;
            }
        }
        Ok(())
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<(), A::Error> {
        let inner = self.enter()?;
        let mut seen = HashSet::new();
        while let Some(key) = map.next_key_seed(Key(self.state))? {
            let escaped = segment(&key);
            if !seen.insert(key.clone()) {
                let at = format!("{}/{escaped}", self.state.borrow().pointer());
                return Err(refuse(self.state, JsonProblem::DuplicateKey(key), at));
            }
            self.state.borrow_mut().path.push(escaped);
            map.next_value_seed(inner)?;
            self.state.borrow_mut().path.pop();
        }
        Ok(())
    }
}

/// A member name, unescaped.
struct Key<'s>(&'s RefCell<Walked>);

impl<'de> DeserializeSeed<'de> for Key<'_> {
    type Value = String;

    fn deserialize<D: de::Deserializer<'de>>(self, d: D) -> Result<String, D::Error> {
        d.deserialize_str(self)
    }
}

impl<'de> Visitor<'de> for Key<'_> {
    type Value = String;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("a member name")
    }

    fn visit_str<E: de::Error>(self, v: &str) -> Result<String, E> {
        if v.len() > MAX_KEY_BYTES {
            let at = self.0.borrow().pointer();
            return Err(refuse(self.0, JsonProblem::KeyTooLong, at));
        }
        Ok(v.to_string())
    }
}

/// `sha256:` and the lowercase hex SHA-256 of `raw_json` with the whitespace
/// between tokens (space, tab, LF, CR outside strings) left out. Indentation
/// and line endings do not change it; any change of a value, a member name,
/// an order or an escape does.
pub fn content_digest(raw_json: &str) -> String {
    let bytes = raw_json.as_bytes();
    let mut hasher = Sha256::new();
    let (mut in_string, mut escaped, mut kept_from) = (false, false, 0);
    for (i, &b) in bytes.iter().enumerate() {
        if in_string {
            if escaped {
                escaped = false;
            } else if b == b'\\' {
                escaped = true;
            } else if b == b'"' {
                in_string = false;
            }
        } else if b == b'"' {
            in_string = true;
        } else if matches!(b, b' ' | b'\t' | b'\n' | b'\r') {
            hasher.update(&bytes[kept_from..i]);
            kept_from = i + 1;
        }
    }
    hasher.update(&bytes[kept_from..]);
    let mut out = String::from("sha256:");
    for b in hasher.finalize() {
        let _ = write!(out, "{b:02x}");
    }
    out
}

/// The canonical text of a value: object members sorted by their names as
/// UTF-16 code units, two-space indentation, LF line ends and one LF at the
/// end, strings and numbers as serde_json writes them.
pub fn to_canonical(value: &Value) -> String {
    let mut out = String::new();
    write_value(&mut out, value, 0);
    out.push('\n');
    out
}

/// A top-level object whose members keep the order given (the envelope),
/// every value below it canonical.
pub fn to_canonical_members(members: &[(&str, &Value)]) -> String {
    let mut out = String::new();
    write_members(&mut out, members.iter().copied(), 0);
    out.push('\n');
    out
}

fn write_value(out: &mut String, value: &Value, indent: usize) {
    match value {
        Value::Array(items) if !items.is_empty() => {
            out.push('[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                new_line(out, indent + 1);
                write_value(out, item, indent + 1);
            }
            new_line(out, indent);
            out.push(']');
        }
        Value::Object(map) if !map.is_empty() => {
            let mut members: Vec<(&str, &Value)> =
                map.iter().map(|(k, v)| (k.as_str(), v)).collect();
            members.sort_by(|(a, _), (b, _)| a.encode_utf16().cmp(b.encode_utf16()));
            write_members(out, members, indent);
        }
        // Scalars, `[]` and `{}`: serde_json's own escapes and numbers.
        other => out.push_str(&other.to_string()),
    }
}

fn write_members<'a>(
    out: &mut String,
    members: impl IntoIterator<Item = (&'a str, &'a Value)>,
    indent: usize,
) {
    out.push('{');
    let mut any = false;
    for (i, (name, value)) in members.into_iter().enumerate() {
        any = true;
        if i > 0 {
            out.push(',');
        }
        new_line(out, indent + 1);
        out.push_str(&Value::from(name).to_string());
        out.push_str(": ");
        write_value(out, value, indent + 1);
    }
    if any {
        new_line(out, indent);
    }
    out.push('}');
}

fn new_line(out: &mut String, indent: usize) {
    out.push('\n');
    out.extend(std::iter::repeat_n("  ", indent));
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn problem(raw: &str) -> (JsonProblem, String) {
        let e = prepass(raw).unwrap_err();
        (e.problem, e.at)
    }

    fn nested(depth: usize) -> String {
        format!("{}{}", "[".repeat(depth), "]".repeat(depth))
    }

    #[test]
    fn a_plain_document_passes() {
        prepass(r#"{ "a": [1, 2.5, -3, true, null, "x"], "b": { "c": {} }, "d": [] }"#).unwrap();
        prepass("\"just a string\"").unwrap();
        prepass(" \r\n\t[]\n").unwrap();
    }

    #[test]
    fn a_member_twice_is_refused_also_when_escaped() {
        assert_eq!(
            problem(r#"{ "a": 1, "a": 2 }"#),
            (JsonProblem::DuplicateKey("a".into()), "/a".into())
        );
        assert_eq!(
            problem(r#"{ "x": [ { "looks": { "a": 1, "\u0061": 2 } } ] }"#),
            (JsonProblem::DuplicateKey("a".into()), "/x/0/looks/a".into())
        );
        // The same name in two objects is not a duplicate.
        prepass(r#"{ "a": { "a": 1 }, "b": { "a": 2 } }"#).unwrap();
        // A pointer escapes `~` and `/`.
        assert_eq!(
            problem(r#"{ "a/b": { "c~d": 1, "c~d": 2 } }"#).1,
            "/a~1b/c~0d"
        );
    }

    #[test]
    fn depth_32_is_read_and_33_is_too_deep() {
        prepass(&nested(MAX_DEPTH)).unwrap();
        assert_eq!(problem(&nested(MAX_DEPTH + 1)).0, JsonProblem::TooDeep);
        let objects = format!(
            "{}1{}",
            "{\"a\":".repeat(MAX_DEPTH + 1),
            "}".repeat(MAX_DEPTH + 1)
        );
        let (found, at) = problem(&objects);
        assert_eq!(found, JsonProblem::TooDeep);
        assert_eq!(at, "/a".repeat(MAX_DEPTH));
        // Far deeper than serde_json's own limit: refused, not a crash.
        assert_eq!(problem(&nested(10_000)).0, JsonProblem::TooDeep);
    }

    #[test]
    fn long_strings_and_member_names_are_refused() {
        let string = |n: usize| format!(r#"{{ "s": "{}" }}"#, "x".repeat(n));
        prepass(&string(MAX_STRING_BYTES)).unwrap();
        assert_eq!(
            problem(&string(MAX_STRING_BYTES + 1)),
            (JsonProblem::StringTooLong, "/s".into())
        );
        // Counted after unescaping: six bytes of text, one byte of string.
        let escaped = format!(r#"["{}"]"#, "\\u0078".repeat(MAX_STRING_BYTES));
        prepass(&escaped).unwrap();
        let key = |n: usize| format!(r#"{{ "o": {{ "{}": 1 }} }}"#, "k".repeat(n));
        prepass(&key(MAX_KEY_BYTES)).unwrap();
        assert_eq!(
            problem(&key(MAX_KEY_BYTES + 1)),
            (JsonProblem::KeyTooLong, "/o".into())
        );
    }

    #[test]
    fn what_is_not_one_json_value_is_not_json() {
        for raw in [
            "",
            "{",
            r#"{ "a": 1 "#,
            "garbage",
            "{} {}",
            "[1,]",
            r#"{ "a": 1e400 }"#,
            r#"["\ud800"]"#,
            "PK\u{3}\u{4}",
        ] {
            assert!(
                matches!(problem(raw).0, JsonProblem::NotJson { .. }),
                "{raw:?}"
            );
        }
        let (found, _) = problem("{\n  \"a\": ?\n}");
        assert_eq!(found, JsonProblem::NotJson { line: 2, column: 8 });
    }

    #[test]
    fn the_digest_ignores_whitespace_between_tokens_only() {
        let compact = r#"{"a":[1,2],"b":"x y"}"#;
        let digest = content_digest(compact);
        assert!(digest.starts_with("sha256:"));
        let hex = &digest["sha256:".len()..];
        assert_eq!(hex.len(), 64);
        assert!(hex
            .chars()
            .all(|c| c.is_ascii_digit() || ('a'..='f').contains(&c)));
        // Re-indented, and with CRLF line ends.
        let pretty = "{\n  \"a\": [\n    1,\n    2\n  ],\n  \"b\": \"x y\"\n}\n";
        assert_eq!(content_digest(pretty), digest);
        assert_eq!(content_digest(&pretty.replace('\n', "\r\n")), digest);
        assert_eq!(content_digest(&format!("\t {compact} \t")), digest);
        // A one-character edit, an order, an escape, a space inside a string.
        for changed in [
            r#"{"a":[1,3],"b":"x y"}"#,
            r#"{"b":"x y","a":[1,2]}"#,
            r#"{"a":[1,2],"b":"x\u0020y"}"#,
            r#"{"a":[1,2],"b":"x  y"}"#,
            r#"{"a":[1,2],"b":"xy"}"#,
        ] {
            assert_ne!(content_digest(changed), digest, "{changed}");
        }
        // An escaped quote does not end the string.
        assert_ne!(
            content_digest(r#"["a\" b"]"#),
            content_digest(r#"["a\"b"]"#)
        );
        // The SHA-256 of the empty string, as a known answer.
        assert_eq!(
            content_digest(" \n"),
            "sha256:e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    #[test]
    fn canonical_text_sorts_members_by_utf16_code_units() {
        // U+FF61 sorts before U+1F600 by code point, after it in UTF-16
        // (the astral character is a surrogate pair, 0xD83D 0xDE00).
        let value = json!({ "b": 1, "\u{FF61}": 2, "a": 3, "\u{1F600}": 4, "é": 5, "Z": 6 });
        let text = to_canonical(&value);
        let order: Vec<usize> = ["Z", "a", "b", "é", "\u{1F600}", "\u{FF61}"]
            .iter()
            .map(|k| text.find(&format!("\"{k}\"")).unwrap())
            .collect();
        assert!(order.windows(2).all(|w| w[0] < w[1]), "{text}");
    }

    #[test]
    fn canonical_text_is_indented_with_one_line_end() {
        let value = json!({
            "list": [1, 2.5, "x\"\n\u{7}", null, true, [], {}],
            "empty": {},
            "nested": { "z": [ { "b": 1, "a": 2 } ] }
        });
        let text = to_canonical(&value);
        assert_eq!(
            text,
            "{\n  \"empty\": {},\n  \"list\": [\n    1,\n    2.5,\n    \"x\\\"\\n\\u0007\",\n    null,\n    true,\n    [],\n    {}\n  ],\n  \"nested\": {\n    \"z\": [\n      {\n        \"a\": 2,\n        \"b\": 1\n      }\n    ]\n  }\n}\n"
        );
        // ASCII names: the same as serde_json's pretty printer, plus the LF.
        assert_eq!(text, serde_json::to_string_pretty(&value).unwrap() + "\n");
        assert_eq!(to_canonical(&json!("é")), "\"é\"\n");
        assert_eq!(to_canonical(&json!([])), "[]\n");
        // Reading it back gives the same value, and writing it again the same text.
        let back: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(back, value);
        assert_eq!(to_canonical(&back), text);
    }

    #[test]
    fn the_envelope_keeps_its_order_and_its_content_digest() {
        let manifest = json!({ "name": "N", "id": "a.b" });
        let content = json!({ "surfaces": [ { "z": 1, "a": 2 } ] });
        let text = to_canonical_members(&[
            ("format", &json!("launchdeck.pack")),
            ("formatVersion", &json!(1)),
            ("manifest", &manifest),
            ("content", &content),
        ]);
        assert!(text.starts_with("{\n  \"format\": \"launchdeck.pack\",\n  \"formatVersion\": 1,\n  \"manifest\": {\n    \"id\": \"a.b\","));
        assert!(text.ends_with("}\n") && !text.ends_with("}\n\n"));
        // The content as it appears in the file has the digest of its canonical text.
        #[derive(serde::Deserialize)]
        struct Raw<'a> {
            #[serde(borrow)]
            content: &'a serde_json::value::RawValue,
        }
        let raw: Raw = serde_json::from_str(&text).unwrap();
        assert_eq!(
            content_digest(raw.content.get()),
            content_digest(&to_canonical(&content))
        );
        assert_eq!(to_canonical_members(&[]), "{}\n");
    }
}
