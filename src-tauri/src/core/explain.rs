//! Why a control looks the way it does.
//!
//! The answer is assembled from what Rust already knows and nothing else: the
//! pure evaluator ([`eval::examine`]) says which rule decides and what each
//! rule reads, the hub says what each state holds, the Observable Catalog's
//! sources say who offers it and why it may not answer. The UI shows this and
//! never rebuilds it.
//!
//! ```text
//!   overlay   a temporary look on top (an event rule's, or an action running)
//!   ─────────
//!   state rules, in order: the first one that holds decides the derived look
//!   ─────────
//!   rest      no rule holds: the control's resting look
//! ```

use serde::Serialize;

use super::model::Control;
use crate::actions::definition::ProviderStatus;
use crate::state::catalog::Source;
use crate::state::definition::{ObservableDefinition, ObservableKind};
use crate::state::eval::{self, Diagnostic, Examination};
use crate::state::feedback::{FeedbackRule, When};
use crate::state::hub::{Observed, Status};
use crate::state::value::Scalar;
use crate::state::StateHub;

/// What put a temporary look on a control.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum OverlaySource {
    /// An event rule matched its event.
    Event,
    /// The control's action is running, or just finished (controls without
    /// feedback rules only).
    Action,
}

/// The temporary look on a control, as the runtime holds it.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OverlayExplanation {
    pub look: String,
    pub source: OverlaySource,
    /// The event rule that put it there (position in the control's rules).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rule: Option<usize>,
    /// Milliseconds until it ends by itself, when it has an end.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub remaining_ms: Option<u64>,
    /// An event rule that holds `latch`: stays until something replaces it.
    pub latched: bool,
}

/// What a rule reads: the state or the event, with the names a person knows.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ObservableRef {
    /// `provider.local`, as the catalog lists it.
    pub id: String,
    /// The same with its arguments: `agent-control.slot.status(slot=2)`.
    pub address: String,
    pub provider: String,
    /// `None` when no installed provider offers it any more.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    pub kind: ObservableKind,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuleExplanation {
    /// Position in the control's list.
    pub index: usize,
    /// `state` or `event`.
    pub kind: &'static str,
    /// A state rule: its test holds right now.
    pub holds: bool,
    /// This rule decides what is shown below the overlay (a state rule), or
    /// put the overlay there (an event rule).
    pub decided: bool,
    /// What the state reads now (state rules).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub observed: Option<Observed>,
    /// The value in the words of its definition (`High`), when it has one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value_label: Option<String>,
    /// The look the rule would show if it decided.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub look: Option<String>,
    /// Whether the control has that look; `false` is why a pad shows its
    /// resting look although a rule decided.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub look_exists: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub observable: Option<ObservableRef>,
    /// Why the state reads `unknown` or `unavailable`, in a sentence.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ControlExplanation {
    pub surface_id: String,
    pub page_id: String,
    pub control_id: String,
    /// The look on screen: `None` is the resting look.
    pub shown: Option<String>,
    /// `overlay`, `state` or `rest`.
    pub layer: &'static str,
    /// The temporary look on top, if one is on.
    pub overlay: Option<OverlayExplanation>,
    /// The look the state rules give, whether or not an overlay covers it.
    pub derived: Option<String>,
    /// The state rule that decided the derived look.
    pub winner: Option<usize>,
    /// A rule that decided on a look the control does not have.
    pub diagnostics: Vec<Diagnostic>,
    pub rules: Vec<RuleExplanation>,
    /// The evaluator's own report, for tests and diagnostics. Not sent.
    #[serde(skip)]
    pub state_rules: Examination,
}

fn find_definition(
    sources: &[Source],
    provider: &str,
    local: &str,
    kind: ObservableKind,
) -> Option<(String, ObservableDefinition)> {
    sources
        .iter()
        .find(|s| s.info.id == provider)
        .and_then(|s| {
            s.observables
                .iter()
                .find(|d| d.provider == provider && d.local == local && d.kind == kind)
                .map(|d| (s.info.name.clone(), d.clone()))
        })
}

fn label_of(def: &ObservableDefinition, value: &Scalar) -> Option<String> {
    def.value
        .as_ref()?
        .values
        .iter()
        .find(|o| o.value.same_as(value))
        .map(|o| o.label.clone())
}

/// Why a state that is not `known` is not. One sentence.
fn why_not_known(
    status: Status,
    provider: &str,
    id: &str,
    scoped: bool,
    hub: &StateHub,
    sources: &[Source],
) -> String {
    let source = sources.iter().find(|s| s.info.id == provider);
    let name = source.map_or(provider, |s| s.info.name.as_str());
    match status {
        Status::Known => String::new(),
        Status::Unknown => {
            if scoped {
                format!("{name} is running but has not reported `{id}` for these arguments yet.")
            } else {
                format!("{name} is running but has not reported `{id}` yet.")
            }
        }
        Status::Unavailable => {
            let Some(source) = source else {
                return format!("Nothing named `{provider}` is installed.");
            };
            if source.info.status != ProviderStatus::Ready {
                return source
                    .info
                    .reason
                    .clone()
                    .unwrap_or_else(|| format!("{name} is not running."));
            }
            if hub.definition(provider, id).is_none() {
                return format!("{name} does not offer a state called `{id}`.");
            }
            format!("{name} is not running.")
        }
    }
}

pub fn explain(
    ids: (&str, &str, &str),
    control: &Control,
    overlay: Option<OverlayExplanation>,
    stored_derived: Option<String>,
    hub: &StateHub,
    sources: &[Source],
) -> ControlExplanation {
    let (surface_id, page_id, control_id) = ids;
    let examination = eval::examine(&control.feedback, &|k| hub.read(k), &|name| {
        control.states.contains_key(name)
    });
    let rules = control
        .feedback
        .iter()
        .zip(&examination.rules)
        .map(|(rule, report)| {
            explain_rule(rule, report, &examination, overlay.as_ref(), hub, sources)
        })
        .collect();
    let (shown, layer) = match (&overlay, &stored_derived) {
        (Some(o), _) => (Some(o.look.clone()), "overlay"),
        (None, Some(d)) => (Some(d.clone()), "state"),
        (None, None) => (None, "rest"),
    };
    ControlExplanation {
        surface_id: surface_id.to_string(),
        page_id: page_id.to_string(),
        control_id: control_id.to_string(),
        shown,
        layer,
        overlay,
        derived: examination.result.look.clone(),
        winner: examination.result.winner,
        diagnostics: examination.result.diagnostics.clone(),
        rules,
        state_rules: examination,
    }
}

fn explain_rule(
    rule: &FeedbackRule,
    report: &eval::RuleReport,
    examination: &Examination,
    overlay: Option<&OverlayExplanation>,
    hub: &StateHub,
    sources: &[Source],
) -> RuleExplanation {
    match &rule.when {
        When::State(c) => {
            let observed = report
                .observed
                .clone()
                .unwrap_or_else(Observed::unavailable);
            let found = find_definition(
                sources,
                &c.state.provider,
                &c.state.id,
                ObservableKind::State,
            );
            // The hub keeps the declaration even while the provider is down.
            let def = found
                .as_ref()
                .map(|(_, d)| d.clone())
                .or_else(|| hub.definition(&c.state.provider, &c.state.id));
            let provider_name = found.map(|(n, _)| n).or_else(|| {
                sources
                    .iter()
                    .find(|s| s.info.id == c.state.provider)
                    .map(|s| s.info.name.clone())
            });
            let scoped = !c.state.args.is_empty();
            let reason = (observed.status != Status::Known).then(|| {
                why_not_known(
                    observed.status,
                    &c.state.provider,
                    &c.state.id,
                    scoped,
                    hub,
                    sources,
                )
            });
            RuleExplanation {
                index: report.index,
                kind: "state",
                holds: report.holds,
                decided: examination.result.winner == Some(report.index),
                value_label: observed
                    .value
                    .as_ref()
                    .and_then(|v| def.as_ref().and_then(|d| label_of(d, v))),
                observed: Some(observed),
                look: report.look.clone(),
                look_exists: report.look_exists,
                observable: Some(ObservableRef {
                    id: format!("{}.{}", c.state.provider, c.state.id),
                    address: c.state.to_string(),
                    provider: c.state.provider.clone(),
                    provider_name,
                    name: def.map(|d| d.name),
                    kind: ObservableKind::State,
                }),
                reason,
            }
        }
        When::Event(c) => {
            let found = find_definition(
                sources,
                &c.event.provider,
                &c.event.id,
                ObservableKind::Event,
            );
            RuleExplanation {
                index: report.index,
                kind: "event",
                holds: false,
                decided: overlay.is_some_and(|o| o.rule == Some(report.index)),
                observed: None,
                value_label: None,
                look: report.look.clone(),
                look_exists: report.look_exists,
                observable: Some(ObservableRef {
                    id: format!("{}.{}", c.event.provider, c.event.id),
                    address: format!("{}.{}", c.event.provider, c.event.id),
                    provider: c.event.provider.clone(),
                    provider_name: found.as_ref().map(|(n, _)| n.clone()),
                    name: found.map(|(_, d)| d.name),
                    kind: ObservableKind::Event,
                }),
                reason: None,
            }
        }
    }
}
