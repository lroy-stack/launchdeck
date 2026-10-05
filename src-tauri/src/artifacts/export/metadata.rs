//! The manifest's words and the file name: what the person gave, or a
//! proposal where they left a field empty; the version of LaunchDeck that
//! made the file; the devices it was used with.

use super::request::ExportMetadata;
use crate::artifacts::env::Environment;
use crate::artifacts::format::{ArtifactKind, TestedKind, TestedWith};
use crate::artifacts::limits::{MAX_ID, MAX_MODEL, MAX_TESTED_WITH, MAX_VERSION};
use crate::devices::DeviceKind;
use crate::plugins::external::manifest::valid_plugin_id;

/// The version a file gets when the person gives none.
const FIRST_VERSION: &str = "1.0.0";
/// The first part of a proposed id when no author is named.
const LOCAL: &str = "local";
/// Characters of the author's part of a proposed id.
const MAX_AUTHOR_SEGMENT: usize = 24;
/// Characters of a file name.
const MAX_FILE_NAME: usize = 64;

/// The id, name and version the file is written with.
pub(super) struct Words {
    pub id: String,
    pub name: String,
    pub version: String,
}

/// What the person gave, trimmed; an empty name is the first surface's,
/// an empty version 1.0.0, an empty id `<author or local>.<name>`.
pub(super) fn words(
    metadata: &ExportMetadata,
    first_surface: Option<&str>,
    kind: ArtifactKind,
) -> Words {
    let name = match metadata.name.trim() {
        "" => first_surface.unwrap_or_default().to_string(),
        given => given.to_string(),
    };
    let version = match metadata.version.trim() {
        "" => FIRST_VERSION.to_string(),
        given => given.to_string(),
    };
    let id = match metadata.id.trim() {
        "" => proposed_id(metadata, &name, kind),
        given => given.to_string(),
    };
    Words { id, name, version }
}

/// Lowercase ASCII letters and digits, every other run of characters one dash.
fn slug(text: &str) -> String {
    let mut out = String::new();
    for c in text.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
        } else if !out.is_empty() && !out.ends_with('-') {
            out.push('-');
        }
    }
    out.trim_end_matches('-').to_string()
}

/// At most `max` characters of a slug, not ending in a dash.
fn cut(slug: &str, max: usize) -> String {
    slug.chars()
        .take(max)
        .collect::<String>()
        .trim_end_matches('-')
        .to_string()
}

fn kind_word(kind: ArtifactKind) -> &'static str {
    match kind {
        ArtifactKind::Surface => "surface",
        ArtifactKind::Pack => "pack",
    }
}

/// `<slug(author)|local>.<slug(name)>`, valid as a plugin-style id: each
/// part starts with a letter, the whole is at most 64 characters and not
/// under the reserved `launchdeck.` prefix.
fn proposed_id(metadata: &ExportMetadata, name: &str, kind: ArtifactKind) -> String {
    let segment = |text: &str, max: usize| {
        let s = slug(text);
        let s = if s.starts_with(|c: char| c.is_ascii_digit()) {
            format!("{}-{s}", kind_word(kind))
        } else {
            s
        };
        cut(&s, max)
    };
    let author = metadata
        .author
        .as_ref()
        .map(|a| segment(&a.name, MAX_AUTHOR_SEGMENT))
        .filter(|a| !a.is_empty())
        .unwrap_or_else(|| LOCAL.to_string());
    let rest = MAX_ID - author.len() - 1;
    let name = Some(segment(name, rest))
        .filter(|n| !n.is_empty())
        .unwrap_or_else(|| kind_word(kind).to_string());
    [format!("{author}.{name}"), format!("{LOCAL}.{name}")]
        .into_iter()
        .find(|id| valid_plugin_id(id))
        .unwrap_or_else(|| format!("{LOCAL}.{}", kind_word(kind)))
}

/// `<slug(name)>-<version>.ld{pack|surface}`: `[a-z0-9.-]`, at most 64 characters.
pub(super) fn file_name(name: &str, version: &str, kind: ArtifactKind) -> String {
    let version: String = version
        .to_ascii_lowercase()
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '.' {
                c
            } else {
                '-'
            }
        })
        .take(MAX_VERSION)
        .collect();
    let extension = kind.extension();
    let room = MAX_FILE_NAME - version.len() - extension.len() - 2;
    let name = Some(cut(&slug(name), room))
        .filter(|n| !n.is_empty())
        .unwrap_or_else(|| kind_word(kind).to_string());
    format!("{name}-{version}.{extension}")
}

/// The models of the devices connected now, each once, hardware first.
pub(super) fn tested_with(env: &Environment) -> Vec<TestedWith> {
    let mut tested: Vec<TestedWith> = env
        .devices
        .iter()
        .filter(|d| d.connected && is_model(&d.model))
        .map(|d| TestedWith {
            model: d.model.clone(),
            kind: match d.kind {
                DeviceKind::Hardware => TestedKind::Hardware,
                DeviceKind::Virtual => TestedKind::Virtual,
            },
        })
        .collect();
    tested.sort_by_key(|t| (t.model.clone(), t.kind == TestedKind::Virtual));
    tested.dedup();
    tested.truncate(MAX_TESTED_WITH);
    tested
}

/// A model the format can name: 1 to 32 lowercase letters, digits and dashes.
fn is_model(model: &str) -> bool {
    (1..=MAX_MODEL).contains(&model.len())
        && model
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}
