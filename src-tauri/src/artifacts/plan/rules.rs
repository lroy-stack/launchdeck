//! The file's context rules (plan 7.14 D): which the person selected, what
//! each names on this system, and whether it is added after the local rules
//! or skipped. Auto Context's switch and fallback never change.

use std::collections::BTreeMap;

use super::inputs::Answers;
use super::types::{AddedRule, PlannedRule, RuleOutcome};
use crate::artifacts::format::{AppRefV1, InputDef, InputValue};
use crate::artifacts::inspect::ParsedArtifact;
use crate::artifacts::review::visible;
use crate::context::identity::ApplicationIdentity;
use crate::context::portable::Resolved;
use crate::context::rules::{checked, ContextRule, ContextSettings, MAX_RULES};
use crate::core::model::Workspace;

pub(super) struct RulePlan {
    pub rules: Vec<PlannedRule>,
    pub added: Vec<AddedRule>,
}

/// What the rule names on this system: its name for people and, when it
/// names one here, the identity.
fn application(
    app: &AppRefV1,
    namespace: Option<&str>,
    defs: &[InputDef],
    answers: &Answers,
) -> (String, Option<ApplicationIdentity>) {
    match app {
        AppRefV1::Application(portable) => {
            let identity = namespace.and_then(|ns| match portable.resolve(ns) {
                Resolved::Identity(identity) => Some(identity),
                Resolved::Unmapped => None,
            });
            (portable.name.clone(), identity)
        }
        AppRefV1::Input(r) => match answers.values.get(&r.input) {
            Some(InputValue::Application {
                namespace,
                id,
                display_name,
            }) => {
                let identity = ApplicationIdentity::new(namespace, id, display_name);
                (identity.display_name.clone(), Some(identity))
            }
            _ => {
                let label = defs
                    .iter()
                    .find(|d| d.id == r.input)
                    .map_or_else(|| r.input.clone(), |d| d.label.clone());
                (label, None)
            }
        },
    }
}

/// Each rule of the file in order. A rule is added only when selected,
/// named on this system, accepted by the rules' own check against the
/// workspace as it would be, not about an application a local rule already
/// names (the local rule wins) or an earlier added rule names, and while
/// there is room; added rules follow every local rule.
pub(super) fn plan_rules(
    parsed: &ParsedArtifact,
    workspace: &Workspace,
    would_be: &Workspace,
    namespace: Option<&str>,
    selected: &[usize],
    answers: &Answers,
    local_ids: &BTreeMap<String, String>,
) -> RulePlan {
    let (content, defs) = (&parsed.content, &parsed.manifest.inputs);
    let local = &workspace.context.rules;
    let mut added: Vec<AddedRule> = Vec::new();
    let mut rules = Vec::with_capacity(content.context_rules.len());
    for (index, rule) in content.context_rules.iter().enumerate() {
        let surface_local_id = local_ids
            .get(&rule.surface)
            .cloned()
            .unwrap_or_else(|| rule.surface.clone());
        let (application_name, identity) = application(&rule.application, namespace, defs, answers);
        let candidate = identity.as_ref().and_then(|identity| {
            accepted(identity, &surface_local_id, rule.page.as_deref(), would_be)
        });
        let outcome = if !selected.contains(&index) {
            RuleOutcome::NotSelected
        } else if let (AppRefV1::Input(r), None) = (&rule.application, &identity) {
            RuleOutcome::NeedsInput {
                input: r.input.clone(),
            }
        } else if let Some(saved) = candidate.as_ref() {
            let same = |other: &ApplicationIdentity| other.is(&saved.application);
            if let Some(local_rule) = local.iter().position(|r| same(&r.application)) {
                RuleOutcome::SkipConflict { local_rule }
            } else if let Some(by) = added.iter().find(|a| same(&a.application)) {
                RuleOutcome::SkipShadowed { by: by.index }
            } else if local.len() + added.len() >= MAX_RULES {
                RuleOutcome::SkipCapacity
            } else {
                let position = local.len() + added.len();
                added.push(AddedRule {
                    index,
                    application: saved.application.clone(),
                    surface_local_id: surface_local_id.clone(),
                    page_id: rule.page.clone(),
                });
                RuleOutcome::Add { position }
            }
        } else {
            RuleOutcome::SkipNoIdentity
        };
        rules.push(PlannedRule {
            index,
            application_name: visible(&application_name, false),
            application: candidate.map(|r| r.application).or(identity),
            surface_local_id,
            page_id: rule.page.clone(),
            outcome,
        });
    }
    RulePlan { rules, added }
}

/// The rule as it would be saved, when the rules' own check accepts it
/// against the workspace as it would be after the import: the identity
/// normalised, its surface and page there.
fn accepted(
    identity: &ApplicationIdentity,
    surface_id: &str,
    page_id: Option<&str>,
    would_be: &Workspace,
) -> Option<ContextRule> {
    let rule = ContextRule {
        application: identity.clone(),
        surface_id: surface_id.to_string(),
        page_id: page_id.map(str::to_string),
    };
    let settings = ContextSettings {
        rules: vec![rule],
        ..ContextSettings::default()
    };
    checked(&settings, &ContextSettings::default(), would_be)
        .ok()
        .and_then(|s| s.rules.into_iter().next())
}
