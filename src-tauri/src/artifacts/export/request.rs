//! What a person asks to export (plan 7.19): which surfaces, as which kind
//! of file, under what name, and what the importer will be asked for.

use serde::{Deserialize, Serialize};

use crate::artifacts::format::{ArtifactKind, Author, InputDef};
use crate::artifacts::review::Location;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ExportRequest {
    pub kind: ArtifactKind,
    /// Local surface ids. The file holds them in workspace order.
    pub surface_ids: Vec<String>,
    #[serde(default)]
    pub metadata: ExportMetadata,
    /// Pack only: the workspace's context rules that show an exported surface.
    #[serde(default)]
    pub include_context_rules: bool,
    /// Pack only, in this order.
    #[serde(default)]
    pub inputs: Vec<InputDef>,
    #[serde(default)]
    pub bindings: Vec<InputBinding>,
    #[serde(default)]
    pub rule_inputs: Vec<RuleInput>,
    #[serde(default)]
    pub extra_identities: Vec<ExtraIdentity>,
}

/// The manifest's words. An empty `id`, `name` or `version` is proposed by
/// the exporter, and the preview's manifest shows what it chose.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ExportMetadata {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub version: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub author: Option<Author>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub license: Option<String>,
}

/// The answer to `input` replaces the text argument `arg` of the press or
/// long press `at` (the preview's `parameters` name them).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct InputBinding {
    pub at: Location,
    pub arg: String,
    pub input: String,
}

/// The importer chooses the application of a context rule: `rule` is the
/// rule's position in the workspace's context rules, as the preview's
/// `contextRules` give it, and `input` an application input.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RuleInput {
    pub rule: usize,
    pub input: String,
}

/// The id the application of rule `rule` has on another system, added so
/// the rule also works there.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ExtraIdentity {
    pub rule: usize,
    pub namespace: String,
    pub id: String,
}
