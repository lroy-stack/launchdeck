//! What an export shows before anything is saved (plan 7.19), and the
//! lists the exporter fills on its way through the workspace.

use serde::Serialize;

use crate::artifacts::format::{
    AppRefV1, DependencyV1, ManifestV1, Rejection, RejectionCode, Requirements, TestedWith,
};
use crate::artifacts::review::{Location, ReviewItem};

/// The preview, and the file when nothing in the preview blocks it.
#[derive(Debug, Clone, PartialEq)]
pub struct ExportResult {
    pub preview: ExportPreview,
    /// Only when `preview.errors` is empty.
    pub bytes: Option<Vec<u8>>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportPreview {
    /// `<slug(name)>-<version>.ldsurface` or `.ldpack`.
    pub file_name: String,
    pub size_bytes: usize,
    /// `integrity.content` of the file.
    pub content_digest: String,
    /// As written, with the proposed id, name and version.
    pub manifest: ManifestV1,
    pub surfaces: Vec<ExportedSurface>,
    pub context_rules: Vec<ExportedRule>,
    pub dependencies: Vec<DependencyV1>,
    pub requirements: Requirements,
    pub tested_with: Vec<TestedWith>,
    /// The pads that run things, for the person to look at before sharing.
    pub review: Vec<ReviewItem>,
    /// What was left out.
    pub stripped: Stripped,
    /// Text arguments that look like something to ask the importer for.
    pub suggestions: Vec<Suggestion>,
    /// Every text argument, so the person can choose to ask the importer;
    /// one already asked of the importer without its value.
    pub parameters: Vec<Parameter>,
    pub warnings: Vec<ExportIssue>,
    /// Anything here blocks the file.
    pub errors: Vec<ExportIssue>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportedSurface {
    pub local_id: String,
    /// The id the file gives it.
    pub artifact_id: String,
    pub name: String,
    pub pages: usize,
    pub controls: usize,
}

/// A context rule as the file holds it, with its position in the workspace.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportedRule {
    pub rule: usize,
    pub application: AppRefV1,
    pub surface: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub page: Option<String>,
}

/// Counts of what a file never carries: a control's unconverted schema 3
/// bindings, actions only an old version knew, a surface's context hint.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Stripped {
    pub legacy_bindings: usize,
    pub legacy_actions: usize,
    pub context_hints: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Suggestion {
    pub at: Location,
    pub arg: String,
    pub value: String,
    pub suggested_type: SuggestedType,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum SuggestedType {
    Path,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Parameter {
    pub at: Location,
    pub arg: String,
    /// The value on this computer; absent when the argument is asked of the
    /// importer, so it never leaves this computer.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
    /// The input that asks the importer for it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input: Option<String>,
}

/// A warning or an error: a code, a sentence for a person, and the JSON
/// pointer into the file when the problem has a place there.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportIssue {
    pub code: ExportCode,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub at: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ExportCode {
    // Errors.
    NoSurfaces,
    UnknownSurface,
    /// Something only a Pack holds, asked of a Surface file.
    KindMismatch,
    /// The id, name, version, description, author or license.
    InvalidMetadata,
    InvalidInput,
    /// A binding, rule input or extra identity that names nothing exported.
    InvalidBinding,
    InvalidIdentity,
    /// A value shaped like a credential (an address with a user name or
    /// password, a token in an address, an authorization header, a webhook
    /// id) in an argument not asked of the importer, or in an input's default.
    Credential,
    TooMany,
    TooLarge,
    InvalidContent,
    // Warnings.
    /// A pad that runs things: check it holds no password or token.
    ReviewBeforeSharing,
    /// Navigation to a surface that is not in the file.
    ExternalSurfaceReference,
    /// A plugin the content uses that is not installed here.
    NotInstalled,
    /// A text argument that looks like a path on this computer.
    LocalPath,
    /// A context rule whose page no longer exists, left out.
    RuleTargetMissing,
}

/// The lists the exporter fills while it reads the workspace.
#[derive(Debug, Default)]
pub(super) struct Collected {
    pub review: Vec<ReviewItem>,
    pub stripped: Stripped,
    pub suggestions: Vec<Suggestion>,
    pub parameters: Vec<Parameter>,
    pub warnings: Vec<ExportIssue>,
    pub errors: Vec<ExportIssue>,
}

fn issue(code: ExportCode, message: String, at: Option<String>) -> ExportIssue {
    ExportIssue { code, message, at }
}

impl Collected {
    pub fn warn(&mut self, code: ExportCode, message: String, at: Option<String>) {
        self.warnings.push(issue(code, message, at));
    }

    pub fn error(&mut self, code: ExportCode, message: String, at: Option<String>) {
        self.errors.push(issue(code, message, at));
    }

    /// A problem the format's own checks found in what would be written.
    pub fn rejected(&mut self, rejection: Rejection) {
        let at = rejection.at.as_deref().unwrap_or_default();
        let code = match rejection.code {
            RejectionCode::TooMany => ExportCode::TooMany,
            RejectionCode::KindMismatch => ExportCode::KindMismatch,
            RejectionCode::InvalidManifest if at.starts_with("/manifest/inputs") => {
                ExportCode::InvalidInput
            }
            RejectionCode::InvalidManifest
                if [
                    "/manifest/id",
                    "/manifest/name",
                    "/manifest/version",
                    "/manifest/description",
                    "/manifest/author",
                    "/manifest/license",
                ]
                .iter()
                .any(|field| at.starts_with(field)) =>
            {
                ExportCode::InvalidMetadata
            }
            _ => ExportCode::InvalidContent,
        };
        self.error(code, rejection.message, rejection.at);
    }
}
