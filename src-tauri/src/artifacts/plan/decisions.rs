//! What the person decides before an import (plan 7.12): which earlier
//! import a file replaces, whether pads that run things are kept, the
//! answers to the file's inputs, and the context rules to add.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::artifacts::format::InputValue;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ImportDecisions {
    /// Needed only when the file was imported before.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolution: Option<Resolution>,
    /// Needed only when a pad runs a command, sends a request or starts a
    /// program.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runs: Option<Runs>,
    /// Answers, by input id.
    #[serde(default)]
    pub inputs: BTreeMap<String, InputValue>,
    /// The file's context rules to add, by position. None by default.
    #[serde(default)]
    pub context_rules: Vec<usize>,
    /// Replace surfaces that were changed here since they were imported.
    #[serde(default)]
    pub confirm_overwrite_edits: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum Resolution {
    /// Another instance, with surfaces of their own.
    ImportAsNew,
    /// Replace the surfaces of this instance in place.
    Replace { instance: u32 },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Runs {
    Keep,
    /// Empty those slots; the controls keep their labels, looks and feedback.
    Remove,
}
