//! Inspect (plan 7.3): the bytes of a file, treated as hostile, read in a
//! fixed order. Each step refuses with a code, a sentence and, where the
//! problem has a place, a JSON pointer, and nothing after it runs:
//!
//! 1. size, before anything is parsed: `empty`, `tooLarge`
//! 2. UTF-8 without a byte order mark: `notUtf8`
//! 3. the pre-pass over the raw text (`json::prepass`): `tooDeep`,
//!    `duplicateKey`, `stringTooLong`, `notJson`
//! 4. the envelope, its members still raw: `notArtifact`, `unsupportedSignature`,
//!    `unsupportedAsset`, `unknownField`, `tooNew`, `unsupportedFormatVersion`
//! 5. the manifest, typed and checked: `invalidManifest`, `tooMany`
//! 6. the digest of the content as it is in the file: `integrity`
//! 7. the content, typed and checked, and no hidden character in what an
//!    action would run: `invalidContent`, `tooMany`
//! 8. the checks across both: `kindMismatch`, `invalidManifest`, `invalidContent`
//!
//! It writes nothing, runs nothing and reads nothing but its arguments.

use std::collections::BTreeMap;

use serde::Serialize;
use serde_json::value::RawValue;
use serde_json::Value;

use super::format::{
    check_content, check_cross, check_manifest, echo, plain_message, ArtifactKind, ContentV1,
    ManifestV1, Rejection, RejectionCode,
};
use super::json::{content_digest, prepass, segment};
use super::limits::MAX_FILE_BYTES;
use super::review::check_visible;
use super::FORMAT_VERSION;
use crate::actions::params::hidden_char;

/// Characters of a file name kept for display.
const MAX_FILE_NAME: usize = 128;
const UTF8_BOM: &[u8] = b"\xEF\xBB\xBF";
/// The categories of LaunchDeck's own surfaces (its starters and a blank
/// surface); any other imports as `custom`.
const KNOWN_CATEGORIES: [&str; 6] = [
    "agents",
    "development",
    "productivity",
    "smart-home",
    "accessibility",
    "custom",
];

/// A file that passed every step: its kind and version, the typed manifest
/// and content, the verified content digest, and what a person should know
/// that does not refuse it.
#[derive(Debug, Clone, PartialEq)]
pub struct ParsedArtifact {
    pub kind: ArtifactKind,
    pub format_version: u32,
    pub manifest: ManifestV1,
    pub content: ContentV1,
    /// `integrity.content`, checked against the content as it is in the file.
    pub content_digest: String,
    pub size_bytes: usize,
    /// The name the file came with, without control characters or any other
    /// character a person cannot see, and at most 128 characters long.
    /// Shown, never used as a path.
    pub file_name: String,
    pub warnings: Vec<InspectWarning>,
}

/// Something about a file a person should know that does not refuse it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InspectWarning {
    pub code: InspectWarningCode,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub at: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum InspectWarningCode {
    /// The name ends in the other kind's extension, or in none.
    ExtensionMismatch,
    /// A surface's category this version does not know: it imports as `custom`.
    UnknownCategory,
}

/// Read `bytes` as a Surface file or a Pack, in the order of plan 7.3.
/// `file_name` is only shown and compared with the kind; the file's
/// `format` decides what it is.
pub fn inspect(bytes: &[u8], file_name: &str) -> Result<ParsedArtifact, Rejection> {
    check_size(bytes)?;
    let text = utf8(bytes)?;
    prepass(text)?;
    let envelope = envelope(text)?;
    let manifest = manifest(envelope.manifest)?;
    let content_digest = integrity(envelope.content, &manifest)?;
    let content = content(envelope.content)?;
    check_cross(envelope.kind, &manifest, &content)?;
    let warnings = warnings(envelope.kind, file_name, &content);
    Ok(ParsedArtifact {
        kind: envelope.kind,
        format_version: FORMAT_VERSION,
        manifest,
        content,
        content_digest,
        size_bytes: bytes.len(),
        file_name: shown_name(file_name),
        warnings,
    })
}

/// Step 1: counted in bytes, before anything is read.
fn check_size(bytes: &[u8]) -> Result<(), Rejection> {
    if bytes.is_empty() {
        return Err(Rejection::new(RejectionCode::Empty, "The file is empty."));
    }
    if bytes.len() > MAX_FILE_BYTES {
        return Err(Rejection::new(
            RejectionCode::TooLarge,
            format!(
                "The file takes {} KiB; a Surface file or a Pack takes at most {} KiB.",
                bytes.len().div_ceil(1024),
                MAX_FILE_BYTES / 1024
            ),
        ));
    }
    Ok(())
}

/// Step 2: UTF-8, and no byte order mark, which JSON does not allow.
fn utf8(bytes: &[u8]) -> Result<&str, Rejection> {
    if bytes.starts_with(UTF8_BOM) {
        return Err(Rejection::new(
            RejectionCode::NotUtf8,
            "The file starts with a byte order mark; a LaunchDeck file is UTF-8 text without one.",
        ));
    }
    std::str::from_utf8(bytes).map_err(|_| {
        Rejection::new(
            RejectionCode::NotUtf8,
            "The file is not UTF-8 text, so it is not a LaunchDeck file.",
        )
    })
}

/// The top level after step 4: the kind, and the two members later steps
/// read, still as they are in the file.
struct Envelope<'a> {
    kind: ArtifactKind,
    manifest: &'a RawValue,
    content: &'a RawValue,
}

/// Step 4: an object with a known `format`, no member beyond the four, and
/// a format version this build reads, then a manifest and a content.
fn envelope(text: &str) -> Result<Envelope<'_>, Rejection> {
    let members: BTreeMap<String, &RawValue> = serde_json::from_str(text).map_err(|_| {
        Rejection::new(
            RejectionCode::NotArtifact,
            "The file is not a LaunchDeck file: it is not a JSON object.",
        )
    })?;
    let kind = kind(members.get("format").copied())?;
    check_members(&members)?;
    format_version(members.get("formatVersion").copied())?;
    let manifest = members.get("manifest").copied().ok_or_else(|| {
        Rejection::at(
            RejectionCode::InvalidManifest,
            "/manifest",
            "The file has no manifest.",
        )
    })?;
    let content = members.get("content").copied().ok_or_else(|| {
        Rejection::at(
            RejectionCode::InvalidContent,
            "/content",
            "The file has no content.",
        )
    })?;
    Ok(Envelope {
        kind,
        manifest,
        content,
    })
}

fn kind(format: Option<&RawValue>) -> Result<ArtifactKind, Rejection> {
    let Some(raw) = format else {
        return Err(Rejection::new(
            RejectionCode::NotArtifact,
            "The file is not a LaunchDeck file: it does not say which format it is.",
        ));
    };
    let named = serde_json::from_str::<String>(raw.get()).ok();
    named
        .as_deref()
        .and_then(ArtifactKind::from_format)
        .ok_or_else(|| {
            Rejection::at(
                RejectionCode::NotArtifact,
                "/format",
                format!(
                    "The file is not a Surface file or a Pack: its format is {}.",
                    named.map_or_else(|| "not a name".to_string(), |f| format!("\"{}\"", echo(&f)))
                ),
            )
        })
}

/// Only the four members of the envelope. `signatures` and `assets` are
/// reserved for a later format and refused by name.
fn check_members(members: &BTreeMap<String, &RawValue>) -> Result<(), Rejection> {
    if members.contains_key("signatures") {
        return Err(Rejection::at(
            RejectionCode::UnsupportedSignature,
            "/signatures",
            "The file is signed, and this version of LaunchDeck reads no signature.",
        ));
    }
    if members.contains_key("assets") {
        return Err(Rejection::at(
            RejectionCode::UnsupportedAsset,
            "/assets",
            "The file carries embedded files, which a LaunchDeck file may not.",
        ));
    }
    let known = ["format", "formatVersion", "manifest", "content"];
    match members.keys().find(|k| !known.contains(&k.as_str())) {
        Some(other) => Err(Rejection::at(
            RejectionCode::UnknownField,
            format!("/{}", segment(other)),
            format!(
                "The file has a member \"{}\" that is not part of the format.",
                echo(other)
            ),
        )),
        None => Ok(()),
    }
}

/// Exactly the version this build reads: a newer one is `tooNew`, any other
/// value, a number that is not whole or a text included, is
/// `unsupportedFormatVersion`.
fn format_version(raw: Option<&RawValue>) -> Result<(), Rejection> {
    let at = "/formatVersion";
    let Some(raw) = raw else {
        return Err(Rejection::new(
            RejectionCode::UnsupportedFormatVersion,
            "The file does not say which version of the format it is written in.",
        ));
    };
    let value: Value = serde_json::from_str(raw.get()).unwrap_or(Value::Null);
    let version = value.as_u64().or_else(|| value.as_i64().map(|_| 0));
    match version {
        Some(v) if v == u64::from(FORMAT_VERSION) => Ok(()),
        Some(v) if v > u64::from(FORMAT_VERSION) => Err(Rejection::at(
            RejectionCode::TooNew,
            at,
            format!(
                "The file is written in format version {v}, which is newer than this version \
                 of LaunchDeck reads (version {FORMAT_VERSION}). Update LaunchDeck to import it."
            ),
        )),
        Some(_) => Err(Rejection::at(
            RejectionCode::UnsupportedFormatVersion,
            at,
            format!(
                "The file is written in format version {}, which LaunchDeck does not read.",
                echo(raw.get())
            ),
        )),
        None => Err(Rejection::at(
            RejectionCode::UnsupportedFormatVersion,
            at,
            format!(
                "The format version {} is not a whole number.",
                echo(raw.get())
            ),
        )),
    }
}

/// Step 5.
fn manifest(raw: &RawValue) -> Result<ManifestV1, Rejection> {
    let manifest: ManifestV1 = serde_json::from_str(raw.get()).map_err(|e| {
        Rejection::at(
            RejectionCode::InvalidManifest,
            "/manifest",
            format!("The manifest is not valid: {}.", reason(&e)),
        )
    })?;
    check_manifest(&manifest)?;
    Ok(manifest)
}

/// Step 6: the content as it is in the file has the digest the manifest
/// names (whose form step 5 checked: `sha256:` and lowercase hex).
fn integrity(content: &RawValue, manifest: &ManifestV1) -> Result<String, Rejection> {
    let digest = content_digest(content.get());
    if digest == manifest.integrity.content {
        Ok(digest)
    } else {
        Err(Rejection::at(
            RejectionCode::Integrity,
            "/manifest/integrity/content",
            "The content does not match the digest in the manifest: the file was changed after \
             it was written, or damaged.",
        ))
    }
}

/// Step 7.
fn content(raw: &RawValue) -> Result<ContentV1, Rejection> {
    let content: ContentV1 = serde_json::from_str(raw.get()).map_err(|e| {
        Rejection::at(
            RejectionCode::InvalidContent,
            "/content",
            format!("The content is not valid: {}.", reason(&e)),
        )
    })?;
    check_content(&content)?;
    check_visible(&content)?;
    Ok(content)
}

/// Why serde refused a member, fit to show: without the line and column,
/// which count inside the member, not the file.
fn reason(e: &serde_json::Error) -> String {
    let full = e.to_string();
    let position = format!(" at line {} column {}", e.line(), e.column());
    plain_message(full.strip_suffix(&position).unwrap_or(&full))
}

/// The name as it may be shown: no control character and nothing else a
/// person cannot see (the review's [`hidden_char`]), at most
/// [`MAX_FILE_NAME`] characters.
fn shown_name(file_name: &str) -> String {
    file_name
        .chars()
        .filter(|&c| !c.is_control() && !hidden_char(c))
        .take(MAX_FILE_NAME)
        .collect()
}

fn warnings(kind: ArtifactKind, file_name: &str, content: &ContentV1) -> Vec<InspectWarning> {
    let mut warnings = Vec::new();
    let extension = file_name.rsplit_once('.').map(|(_, ext)| ext);
    if !extension.is_some_and(|ext| ext.eq_ignore_ascii_case(kind.extension())) {
        let what = match kind {
            ArtifactKind::Surface => "a Surface file",
            ArtifactKind::Pack => "a Pack",
        };
        warnings.push(InspectWarning {
            code: InspectWarningCode::ExtensionMismatch,
            message: format!(
                "The file is {what}, but its name does not end in .{}.",
                kind.extension()
            ),
            at: None,
        });
    }
    for (i, surface) in content.surfaces.iter().enumerate() {
        if !KNOWN_CATEGORIES.contains(&surface.category.as_str()) {
            warnings.push(InspectWarning {
                code: InspectWarningCode::UnknownCategory,
                message: format!(
                    "The surface \"{}\" has the category \"{}\", which LaunchDeck does not know; \
                     it is imported as custom.",
                    echo(&surface.name),
                    echo(&surface.category)
                ),
                at: Some(format!("/content/surfaces/{i}/category")),
            });
        }
    }
    warnings
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::Platform;
    use crate::surfaces::default_workspace_for;

    fn file(kind: &str, version: &str) -> String {
        format!(
            "{{\"format\": \"launchdeck.{kind}\", \"formatVersion\": {version}, \
             \"manifest\": {{}}, \"content\": {{}}}}"
        )
    }

    fn code(text: &str) -> RejectionCode {
        inspect(text.as_bytes(), "x.ldpack").unwrap_err().code
    }

    /// A problem of an early step hides the problems of every later one.
    #[test]
    fn each_step_refuses_before_the_next_runs() {
        use RejectionCode as R;
        // The pre-pass before the envelope: a duplicate in a file of an
        // unknown format.
        assert_eq!(
            code("{\"format\": \"other\", \"a\": 1, \"a\": 2}"),
            R::DuplicateKey
        );
        // The format before the members, the members before the version.
        assert_eq!(code("{\"format\": \"x\", \"assets\": []}"), R::NotArtifact);
        assert_eq!(
            code("{\"format\": \"launchdeck.pack\", \"formatVersion\": 9, \"signatures\": []}"),
            R::UnsupportedSignature
        );
        // The version before the manifest: a newer file may look different.
        assert_eq!(code(&file("pack", "2")), R::TooNew);
        assert_eq!(code(&file("pack", "-1")), R::UnsupportedFormatVersion);
        assert_eq!(code(&file("pack", "1.0")), R::UnsupportedFormatVersion);
        assert_eq!(
            code("{\"format\": \"launchdeck.pack\", \"manifest\": {}}"),
            R::UnsupportedFormatVersion
        );
        // The manifest before the content's digest.
        assert_eq!(code(&file("surface", "1")), R::InvalidManifest);
        assert_eq!(
            code("{\"format\": \"launchdeck.pack\", \"formatVersion\": 1, \"manifest\": {}}"),
            R::InvalidContent
        );
    }

    #[test]
    fn a_serde_reason_has_no_position() {
        let r = inspect(file("pack", "1").as_bytes(), "x.ldpack").unwrap_err();
        assert_eq!(r.at.as_deref(), Some("/manifest"));
        assert!(r
            .message
            .starts_with("The manifest is not valid: missing field \"id\""));
        assert!(!r.message.contains("line"), "{}", r.message);
    }

    #[test]
    fn a_file_name_is_shown_without_hidden_characters() {
        assert_eq!(shown_name("a\u{202E}b\u{0}c\nd.ldpack"), "abcd.ldpack");
        // Every character a person cannot see, the review's own set.
        assert_eq!(
            shown_name("star\u{200B}ters\u{FEFF}\u{AD}\u{E0041}\u{2028}.ldpack"),
            "starters.ldpack"
        );
        assert_eq!(shown_name(&"é".repeat(200)).chars().count(), MAX_FILE_NAME);
    }

    #[test]
    fn the_extension_is_a_hint() {
        let content = ContentV1 {
            surfaces: vec![],
            context_rules: vec![],
        };
        let codes = |kind, name: &str| -> Vec<InspectWarningCode> {
            warnings(kind, name, &content)
                .iter()
                .map(|w| w.code)
                .collect()
        };
        assert!(codes(ArtifactKind::Pack, "a.ldpack").is_empty());
        assert!(codes(ArtifactKind::Pack, "A.LDPACK").is_empty());
        assert!(codes(ArtifactKind::Surface, "a.ldsurface").is_empty());
        for (kind, name) in [
            (ArtifactKind::Pack, "a.ldsurface"),
            (ArtifactKind::Surface, "a.ldpack"),
            (ArtifactKind::Pack, "pack"),
            (ArtifactKind::Pack, "a.json"),
        ] {
            assert_eq!(
                codes(kind, name),
                [InspectWarningCode::ExtensionMismatch],
                "{name}"
            );
        }
    }

    /// The categories known here are those of LaunchDeck's own surfaces.
    #[test]
    fn every_starter_category_is_known() {
        for platform in [Platform::Linux, Platform::MacOs, Platform::Windows] {
            for surface in default_workspace_for(platform).surfaces {
                assert!(
                    KNOWN_CATEGORIES.contains(&surface.category.as_str()),
                    "{}",
                    surface.category
                );
            }
        }
        let blank = crate::surfaces::blank_surface("b", "Blank");
        assert!(KNOWN_CATEGORIES.contains(&blank.category.as_str()));
    }
}
