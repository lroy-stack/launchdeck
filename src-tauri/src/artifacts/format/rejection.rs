//! Why a file was refused: a code for the program, a sentence for a person,
//! and where in the file.

use serde::Serialize;

use crate::artifacts::json::{JsonProblem, PrepassError};
use crate::artifacts::limits::{MAX_DEPTH, MAX_KEY_BYTES, MAX_STRING_BYTES};

/// Characters of a value from the file that a message repeats.
const ECHO_CHARS: usize = 40;

/// What kind of problem refused a file. The inspect order (plan 7.3) decides
/// which one a file with several problems gets.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum RejectionCode {
    Empty,
    TooLarge,
    NotUtf8,
    TooDeep,
    DuplicateKey,
    StringTooLong,
    NotJson,
    /// Not a LaunchDeck artifact: no or another `format`.
    NotArtifact,
    /// A `formatVersion` newer than this build reads.
    TooNew,
    UnsupportedFormatVersion,
    /// A top-level member that is not part of the envelope.
    UnknownField,
    UnsupportedSignature,
    UnsupportedAsset,
    InvalidManifest,
    /// The content does not have the digest the manifest names.
    Integrity,
    InvalidContent,
    /// More of something than a file may hold.
    TooMany,
    /// Something only the other kind of file may hold.
    KindMismatch,
}

/// A refused file: `message` is a sentence for a person, `at` a JSON pointer
/// (`/content/surfaces/0/pages/1/controls/3/press`) when the problem has a place.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Rejection {
    pub code: RejectionCode,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub at: Option<String>,
}

impl Rejection {
    pub fn new(code: RejectionCode, message: impl Into<String>) -> Rejection {
        Rejection {
            code,
            message: message.into(),
            at: None,
        }
    }

    pub fn at(code: RejectionCode, at: impl Into<String>, message: impl Into<String>) -> Rejection {
        let at = at.into();
        Rejection {
            code,
            message: message.into(),
            at: (!at.is_empty()).then_some(at),
        }
    }
}

impl From<PrepassError> for Rejection {
    fn from(e: PrepassError) -> Rejection {
        let (code, message) = match e.problem {
            JsonProblem::TooDeep => (
                RejectionCode::TooDeep,
                format!("The file nests lists and objects more than {MAX_DEPTH} levels deep."),
            ),
            JsonProblem::DuplicateKey(key) => (
                RejectionCode::DuplicateKey,
                format!("The file names \"{}\" twice in one object.", echo(&key)),
            ),
            JsonProblem::StringTooLong => (
                RejectionCode::StringTooLong,
                format!(
                    "The file holds a text longer than {} KiB.",
                    MAX_STRING_BYTES / 1024
                ),
            ),
            JsonProblem::KeyTooLong => (
                RejectionCode::StringTooLong,
                format!("The file holds a member name longer than {MAX_KEY_BYTES} bytes."),
            ),
            JsonProblem::NotJson { line, column } => (
                RejectionCode::NotJson,
                format!("The file is not valid JSON (line {line}, column {column})."),
            ),
        };
        Rejection::at(code, e.at, message)
    }
}

/// A value from the file as a message may repeat it: without control or
/// direction characters, and at most 40 characters long.
pub fn echo(value: &str) -> String {
    let mut kept = value.chars().filter(|&c| !c.is_control() && !is_bidi(c));
    let mut out: String = kept.by_ref().take(ECHO_CHARS).collect();
    if kept.next().is_some() {
        out.push('…');
    }
    out
}

/// A message from another module, which puts values between backticks, made
/// fit to show: each value echoed and quoted, no control or direction
/// character anywhere.
pub(in crate::artifacts) fn plain_message(message: &str) -> String {
    message
        .split('`')
        .enumerate()
        .map(|(i, part)| {
            if i % 2 == 1 {
                format!("\"{}\"", echo(part))
            } else {
                part.chars()
                    .filter(|&c| !c.is_control() && !is_bidi(c))
                    .collect()
            }
        })
        .collect()
}

/// The explicit direction controls: embeddings, overrides and isolates.
pub(in crate::artifacts) fn is_bidi(c: char) -> bool {
    matches!(c, '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}')
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn an_echo_is_short_and_plain() {
        assert_eq!(echo("plain"), "plain");
        assert_eq!(echo("a\u{0}b\nc\u{202E}d\u{2066}e"), "abcde");
        let long = "x".repeat(41);
        assert_eq!(echo(&long), format!("{}…", "x".repeat(40)));
        assert_eq!(echo(&"é".repeat(40)), "é".repeat(40));
    }

    #[test]
    fn a_message_from_elsewhere_echoes_its_values() {
        let long = format!("Ac\u{202E}me{}", "x".repeat(60));
        assert_eq!(
            plain_message(&format!("`{long}` is not a valid provider id")),
            format!("\"Acme{}…\" is not a valid provider id", "x".repeat(36))
        );
        assert_eq!(plain_message("overlaps `Pad`"), "overlaps \"Pad\"");
        assert_eq!(plain_message("no value\u{7} here"), "no value here");
    }

    #[test]
    fn a_rejection_is_written_in_camel_case() {
        let r = Rejection::at(
            RejectionCode::KindMismatch,
            "/content/contextRules",
            "A Surface file has no context rules.",
        );
        assert_eq!(
            serde_json::to_value(&r).unwrap(),
            json!({ "code": "kindMismatch", "message": "A Surface file has no context rules.", "at": "/content/contextRules" })
        );
        let whole = Rejection::new(RejectionCode::Empty, "The file is empty.");
        assert_eq!(
            serde_json::to_value(&whole).unwrap(),
            json!({ "code": "empty", "message": "The file is empty." })
        );
        assert_eq!(Rejection::at(RejectionCode::NotJson, "", "m").at, None);
        for (code, word) in [
            (RejectionCode::TooLarge, "tooLarge"),
            (RejectionCode::NotUtf8, "notUtf8"),
            (RejectionCode::TooNew, "tooNew"),
            (
                RejectionCode::UnsupportedFormatVersion,
                "unsupportedFormatVersion",
            ),
            (RejectionCode::UnsupportedSignature, "unsupportedSignature"),
            (RejectionCode::UnsupportedAsset, "unsupportedAsset"),
            (RejectionCode::StringTooLong, "stringTooLong"),
        ] {
            assert_eq!(serde_json::to_value(code).unwrap(), json!(word));
        }
    }

    #[test]
    fn a_prepass_problem_becomes_a_rejection() {
        let r = Rejection::from(crate::artifacts::prepass("{\"a\":1,\"a\":2}").unwrap_err());
        assert_eq!(r.code, RejectionCode::DuplicateKey);
        assert_eq!(r.at.as_deref(), Some("/a"));
        assert_eq!(r.message, "The file names \"a\" twice in one object.");
        let r = Rejection::from(crate::artifacts::prepass("{").unwrap_err());
        assert_eq!(r.code, RejectionCode::NotJson);
        assert_eq!(r.at, None);
        let deep = format!("{}{}", "[".repeat(33), "]".repeat(33));
        assert_eq!(
            Rejection::from(crate::artifacts::prepass(&deep).unwrap_err()).code,
            RejectionCode::TooDeep
        );
    }
}
