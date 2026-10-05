//! The two kinds of file, the top level as it is read, and a whole file as
//! it is written.

use std::borrow::Cow;

use serde::{Deserialize, Serialize};
use serde_json::value::RawValue;
use serde_json::Value;

use super::content::ContentV1;
use super::manifest::ManifestV1;
use crate::artifacts::json::{content_digest, to_canonical, to_canonical_members};
use crate::artifacts::{
    EXTENSION_PACK, EXTENSION_SURFACE, FORMAT_PACK, FORMAT_SURFACE, FORMAT_VERSION,
    MEDIA_TYPE_PACK, MEDIA_TYPE_SURFACE,
};

/// A Surface file holds one surface; a Pack up to 16, with context rules and
/// inputs. `format` decides the kind; the file extension is only a hint.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ArtifactKind {
    Surface,
    Pack,
}

impl ArtifactKind {
    pub fn from_format(format: &str) -> Option<ArtifactKind> {
        match format {
            FORMAT_SURFACE => Some(ArtifactKind::Surface),
            FORMAT_PACK => Some(ArtifactKind::Pack),
            _ => None,
        }
    }

    pub fn format(self) -> &'static str {
        match self {
            ArtifactKind::Surface => FORMAT_SURFACE,
            ArtifactKind::Pack => FORMAT_PACK,
        }
    }

    pub fn extension(self) -> &'static str {
        match self {
            ArtifactKind::Surface => EXTENSION_SURFACE,
            ArtifactKind::Pack => EXTENSION_PACK,
        }
    }

    pub fn media_type(self) -> &'static str {
        match self {
            ArtifactKind::Surface => MEDIA_TYPE_SURFACE,
            ArtifactKind::Pack => MEDIA_TYPE_PACK,
        }
    }
}

/// The top level as it is read (plan 7.3, step 4): `manifest` and `content`
/// stay raw, the content for its digest over the bytes as they are in the file.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Envelope<'a> {
    #[serde(borrow)]
    pub format: Cow<'a, str>,
    pub format_version: u32,
    #[serde(borrow)]
    pub manifest: &'a RawValue,
    #[serde(borrow)]
    pub content: &'a RawValue,
}

/// A whole file, typed.
#[derive(Debug, Clone, PartialEq)]
pub struct ArtifactV1 {
    pub kind: ArtifactKind,
    pub manifest: ManifestV1,
    pub content: ContentV1,
}

impl ArtifactV1 {
    /// The file: `format`, `formatVersion`, `manifest`, `content` in this
    /// order, everything below them canonical. The manifest's integrity is
    /// written as it is; [`ContentV1::digest`] is what it should say.
    pub fn to_canonical(&self) -> Result<String, serde_json::Error> {
        let manifest = serde_json::to_value(&self.manifest)?;
        let content = serde_json::to_value(&self.content)?;
        Ok(to_canonical_members(&[
            ("format", &Value::from(self.kind.format())),
            ("formatVersion", &Value::from(FORMAT_VERSION)),
            ("manifest", &manifest),
            ("content", &content),
        ]))
    }
}

impl ContentV1 {
    /// The digest of this content as a file writes it, for `integrity.content`.
    pub fn digest(&self) -> Result<String, serde_json::Error> {
        Ok(content_digest(&to_canonical(&serde_json::to_value(self)?)))
    }
}
