//! The context rules a file carries: the workspace's rules that show an
//! exported surface, in workspace order, each naming its application in a
//! way that works on other systems (plan 7.5, 7.8). Auto, the fallback and
//! rules for surfaces left out are never exported.

use std::collections::BTreeMap;

use super::preview::{Collected, ExportCode, ExportedRule};
use super::request::ExportRequest;
use super::select::{Chosen, IdTable};
use crate::artifacts::format::{AppRefV1, ContextRuleV1, InputRef, InputType};
use crate::context::portable::PortableApplication;
use crate::context::rules::ContextRule;
use crate::core::model::Workspace;

/// The rules to write, and the same with their workspace positions for the
/// preview.
pub(super) fn export_rules(
    workspace: &Workspace,
    request: &ExportRequest,
    chosen: &[Chosen<'_>],
    ids: &IdTable<'_>,
    found: &mut Collected,
) -> (Vec<ContextRuleV1>, Vec<ExportedRule>) {
    let mut exported: Vec<(usize, ContextRuleV1)> = Vec::new();
    if request.include_context_rules {
        for (index, rule) in workspace.context.rules.iter().enumerate() {
            if let Some(file_rule) = export_rule(rule, chosen, ids, found) {
                exported.push((index, file_rule));
            }
        }
    }
    let positions: BTreeMap<usize, usize> = exported
        .iter()
        .enumerate()
        .map(|(position, (index, _))| (*index, position))
        .collect();
    add_extra_identities(&mut exported, &positions, request, found);
    bind_applications(&mut exported, &positions, request, found);
    let preview = exported
        .iter()
        .map(|(index, r)| ExportedRule {
            rule: *index,
            application: r.application.clone(),
            surface: r.surface.clone(),
            page: r.page.clone(),
        })
        .collect();
    (exported.into_iter().map(|(_, r)| r).collect(), preview)
}

/// A rule whose surface is exported, as the file names it; `None` for one
/// whose surface is not exported or whose page no longer exists.
fn export_rule(
    rule: &ContextRule,
    chosen: &[Chosen<'_>],
    ids: &IdTable<'_>,
    found: &mut Collected,
) -> Option<ContextRuleV1> {
    let surface = ids.get(rule.surface_id.as_str())?;
    if let Some(page) = &rule.page_id {
        let model = chosen
            .iter()
            .find(|c| c.surface.id == rule.surface_id)
            .map(|c| c.surface);
        if model.is_some_and(|s| s.page(page).is_none()) {
            found.warn(
                ExportCode::RuleTargetMissing,
                format!(
                    "The context rule for {} shows the page \"{page}\", which the surface no longer has, so it is left out.",
                    rule.application.normalized().display_name
                ),
                None,
            );
            return None;
        }
    }
    Some(ContextRuleV1 {
        application: AppRefV1::Application(PortableApplication::from_identity(&rule.application)),
        surface: surface.to_string(),
        page: rule.page_id.clone(),
    })
}

fn not_exported(found: &mut Collected, rule: usize, what: &str) {
    found.error(
        ExportCode::InvalidBinding,
        format!(
            "{what} names the context rule {}, which is not part of this file.",
            rule + 1
        ),
        None,
    );
}

/// The ids the person adds for other systems, each checked as a rule's
/// application is.
fn add_extra_identities(
    exported: &mut [(usize, ContextRuleV1)],
    positions: &BTreeMap<usize, usize>,
    request: &ExportRequest,
    found: &mut Collected,
) {
    for extra in &request.extra_identities {
        let Some(&position) = positions.get(&extra.rule) else {
            not_exported(found, extra.rule, "An added identity");
            continue;
        };
        let at = format!("/content/contextRules/{position}/application");
        let rule = &mut exported[position].1;
        let AppRefV1::Application(app) = &rule.application else {
            continue;
        };
        match app.clone().with_identity(&extra.namespace, &extra.id) {
            Ok(widened) => rule.application = AppRefV1::Application(widened),
            Err(why) => found.error(
                ExportCode::InvalidIdentity,
                format!(
                    "The identity \"{}\" in \"{}\" cannot be added to {}: {why}.",
                    extra.id, extra.namespace, app.name
                ),
                Some(at),
            ),
        }
    }
}

/// Rules whose application the importer chooses: the application input
/// replaces the identities.
fn bind_applications(
    exported: &mut [(usize, ContextRuleV1)],
    positions: &BTreeMap<usize, usize>,
    request: &ExportRequest,
    found: &mut Collected,
) {
    for (i, bound) in request.rule_inputs.iter().enumerate() {
        let Some(&position) = positions.get(&bound.rule) else {
            not_exported(found, bound.rule, "An application input");
            continue;
        };
        let at = Some(format!("/content/contextRules/{position}/application"));
        let declared = request.inputs.iter().find(|d| d.id == bound.input);
        let why = if !declared.is_some_and(|d| d.kind == InputType::Application) {
            Some(format!(
                "A context rule is bound to \"{}\", which is not a declared application input.",
                bound.input
            ))
        } else if request.rule_inputs[..i]
            .iter()
            .any(|r| r.rule == bound.rule)
        {
            Some(format!(
                "The context rule {} is bound to an input twice.",
                bound.rule + 1
            ))
        } else if request
            .extra_identities
            .iter()
            .any(|e| e.rule == bound.rule)
        {
            Some(format!(
                "The context rule {} is bound to an input, so it carries no identities to add to.",
                bound.rule + 1
            ))
        } else {
            None
        };
        match why {
            Some(why) => found.error(ExportCode::InvalidBinding, why, at),
            None => {
                exported[position].1.application = AppRefV1::Input(InputRef {
                    input: bound.input.clone(),
                });
            }
        }
    }
}
