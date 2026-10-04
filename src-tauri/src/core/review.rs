//! Reviewing a draft of a control's feedback before it is saved.
//!
//! The editor changes a draft; the draft cannot be judged without saving it,
//! and saving is where rules are checked. A review closes that gap without
//! storing anything: it is the same checks, asked of a draft.
//!
//! ```text
//!   draft control ──┬─► prepare_rules ───────────► issues   what saving would refuse
//!   saved control ──┤   (saved rules stay as they are)
//!                   ├─► lint, looks, catalog ────► notes    what Rust can prove, saved or not
//!                   └─► explain::preview ────────► preview  what the draft would show right now
//! ```
//!
//! * **issues** are exactly the ones `update_control` would refuse, with the
//!   part of the rule they are about. A rule that is already saved is never an
//!   issue, even when its provider is gone.
//! * **notes** never block anything. They say something true about a rule:
//!   it names a look the control lacks, an earlier rule always wins over it,
//!   nothing installed offers what it reads, or the provider is off.
//! * **preview** is the explanation of the draft against the values the hub
//!   holds now. It is present only when the rules or the looks differ from the
//!   saved control; when they do not, the saved control's own explanation is
//!   the truth (it also knows the temporary look).
//!
//! Nothing here touches the workspace, the overlays or the derived looks.

use serde::Serialize;

use super::explain::{self, ControlExplanation};
use super::model::Control;
use crate::actions::definition::Availability;
use crate::state::catalog::{self, ObservableCatalog, Source};
use crate::state::definition::ObservableKind;
use crate::state::feedback::{prepare_rules, FeedbackLimits, FeedbackRule, RuleIssue, When, IDLE};
use crate::state::lint;
use crate::state::StateHub;

/// Something true about one rule that the person may want to know.
///
/// Notes are ordered by rule, then in the order listed here.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum RuleNote {
    /// The rule shows a look by name and the control has no look of that name,
    /// so the pad would rest instead.
    MissingLook { rule: usize, look: String },
    /// An earlier rule always decides first: this one can never show anything.
    Shadowed { rule: usize, by: usize },
    /// No installed provider offers what the rule reads. For an event this only
    /// means nothing installed announces it; it may still arrive from outside.
    MissingObservable {
        rule: usize,
        observable_kind: ObservableKind,
        provider: String,
        id: String,
    },
    /// The provider offers it but cannot answer now (turned off, failed).
    ProviderUnavailable {
        rule: usize,
        provider: String,
        provider_name: String,
        reason: String,
    },
}

impl RuleNote {
    pub fn rule(&self) -> usize {
        match self {
            RuleNote::MissingLook { rule, .. }
            | RuleNote::Shadowed { rule, .. }
            | RuleNote::MissingObservable { rule, .. }
            | RuleNote::ProviderUnavailable { rule, .. } => *rule,
        }
    }

    /// The position of the kind in the order notes are listed.
    fn kind_order(&self) -> u8 {
        match self {
            RuleNote::MissingLook { .. } => 0,
            RuleNote::Shadowed { .. } => 1,
            RuleNote::MissingObservable { .. } => 2,
            RuleNote::ProviderUnavailable { .. } => 3,
        }
    }
}

/// What a draft of a control's feedback comes to. See the module documentation.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FeedbackReview {
    pub issues: Vec<RuleIssue>,
    pub notes: Vec<RuleNote>,
    pub limits: FeedbackLimits,
    /// `None` when the draft's rules and looks are the saved control's.
    pub preview: Option<ControlExplanation>,
}

/// Review `draft`, the control as the editor holds it, against `stored`, the
/// control as it is saved (`None` when it is not saved yet).
///
/// When the rules pass the gate, notes and preview describe them as they
/// would be saved (arguments completed with their defaults); when they do not,
/// the draft as written.
pub fn review(
    ids: (&str, &str),
    draft: &Control,
    stored: Option<&Control>,
    hub: &StateHub,
    sources: &[Source],
) -> FeedbackReview {
    let saved = stored.map_or(&[][..], |c| &c.feedback[..]);
    let (issues, rules) = match prepare_rules(&draft.feedback, saved, &|provider, id| {
        hub.definition(provider, id)
    }) {
        Ok(prepared) => (Vec::new(), prepared),
        Err(issues) => (issues, draft.feedback.clone()),
    };

    let mut notes = missing_looks(&rules, draft);
    notes.extend(
        lint::shadowed(&rules)
            .into_iter()
            .map(|s| RuleNote::Shadowed {
                rule: s.rule,
                by: s.by,
            }),
    );
    notes.extend(observable_notes(
        &rules,
        &catalog::build(sources.to_vec(), hub),
    ));
    notes.sort_by_key(|n| (n.rule(), n.kind_order()));

    let changed = stored.is_none_or(|s| s.feedback != rules || s.states != draft.states);
    let preview = changed.then(|| {
        let as_saved = Control {
            feedback: rules,
            ..draft.clone()
        };
        explain::preview((ids.0, ids.1, &draft.id), &as_saved, hub, sources)
    });

    FeedbackReview {
        issues,
        notes,
        limits: FeedbackLimits::new(),
        preview,
    }
}

/// Rules that show a look by name when the draft has no look of that name.
/// `idle` is the resting look unless the control draws one of its own (the
/// evaluator's own rule), and a look taken from a value or a payload field
/// cannot be known before the value arrives.
fn missing_looks(rules: &[FeedbackRule], draft: &Control) -> Vec<RuleNote> {
    rules
        .iter()
        .enumerate()
        .filter_map(|(rule, r)| {
            let look = r.show.as_deref()?;
            (look != IDLE && !draft.states.contains_key(look)).then(|| RuleNote::MissingLook {
                rule,
                look: look.to_string(),
            })
        })
        .collect()
}

/// Rules that read something nothing offers, or something whose provider
/// cannot answer now. Asked of the Observable Catalog, the one list the editor
/// picks from, so the words agree with what the picker shows.
fn observable_notes(rules: &[FeedbackRule], catalog: &ObservableCatalog) -> Vec<RuleNote> {
    rules
        .iter()
        .enumerate()
        .filter_map(|(rule, r)| {
            let (provider, id, kind) = match &r.when {
                When::State(c) => (&c.state.provider, &c.state.id, ObservableKind::State),
                When::Event(c) => (&c.event.provider, &c.event.id, ObservableKind::Event),
            };
            let entry = catalog.observables.iter().find(|e| {
                e.definition.provider == *provider
                    && e.definition.local == *id
                    && e.definition.kind == kind
            });
            match entry {
                None => Some(RuleNote::MissingObservable {
                    rule,
                    observable_kind: kind,
                    provider: provider.clone(),
                    id: id.clone(),
                }),
                Some(e) => match &e.live.availability {
                    Availability::Available => None,
                    Availability::Unavailable { reason, .. } => {
                        Some(RuleNote::ProviderUnavailable {
                            rule,
                            provider: provider.clone(),
                            provider_name: e.provider_name.clone(),
                            reason: reason.clone(),
                        })
                    }
                },
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::actions::definition::{ProviderInfo, ProviderKind, ProviderStatus};
    use crate::core::model::{ControlPos, Rgb, Visual};
    use crate::state::definition::{ObservableDefinition, Role};
    use crate::state::feedback::{Hold, IssueField, Test, MAX_RULES};
    use crate::state::value::Scalar;
    use crate::state::StateAddress;

    fn info(id: &str, status: ProviderStatus) -> ProviderInfo {
        ProviderInfo {
            id: id.into(),
            name: format!("{id} plugin"),
            kind: ProviderKind::Plugin,
            version: "1.0.0".into(),
            description: String::new(),
            status,
            reason: match status {
                ProviderStatus::Ready => None,
                ProviderStatus::Disabled => Some("Turned off in Plugins.".into()),
                ProviderStatus::Error => Some("It could not start.".into()),
            },
        }
    }

    fn level() -> ObservableDefinition {
        ObservableDefinition::state("acme.counter", "level", "Level", "").one_of(&[
            ("zero", "Empty", Some(Role::Inactive)),
            ("high", "High", None),
        ])
    }

    fn changed() -> ObservableDefinition {
        ObservableDefinition::event("acme.counter", "counter.changed", "Changed", "")
    }

    fn off_state() -> ObservableDefinition {
        ObservableDefinition::state("acme.off", "thing", "Thing", "").text()
    }

    /// A counter that runs, a plugin that is turned off, and a hub that knows
    /// the declarations of both (it keeps them while a provider is down).
    fn world() -> (StateHub, Vec<Source>) {
        let hub = StateHub::new();
        hub.declare("acme.counter", vec![level()]).unwrap();
        hub.provider_up("acme.counter");
        hub.declare("acme.off", vec![off_state()]).unwrap();
        hub.provider_down("acme.off");
        let sources = vec![
            Source {
                info: info("acme.counter", ProviderStatus::Ready),
                observables: vec![level(), changed()],
            },
            Source {
                info: info("acme.off", ProviderStatus::Disabled),
                observables: vec![off_state()],
            },
        ];
        (hub, sources)
    }

    fn at_level() -> StateAddress {
        StateAddress::new("acme.counter", "level")
    }

    fn eq(v: &str) -> Test {
        Test::Equals {
            value: Scalar::text(v),
        }
    }

    fn pad(feedback: Vec<FeedbackRule>) -> Control {
        let mut c = Control::new(ControlPos::grid(0, 0), "Level", Rgb::new(10, 10, 10));
        c.states
            .insert("busy".into(), Visual::solid(Rgb::new(255, 0, 0)));
        c.states
            .insert("calm".into(), Visual::solid(Rgb::new(0, 255, 0)));
        c.feedback = feedback;
        c
    }

    fn run(draft: &Control, stored: Option<&Control>) -> FeedbackReview {
        let (hub, sources) = world();
        review(("s", "p"), draft, stored, &hub, &sources)
    }

    fn kinds(r: &FeedbackReview) -> Vec<&'static str> {
        r.notes
            .iter()
            .map(|n| match n {
                RuleNote::MissingLook { .. } => "missingLook",
                RuleNote::Shadowed { .. } => "shadowed",
                RuleNote::MissingObservable { .. } => "missingObservable",
                RuleNote::ProviderUnavailable { .. } => "providerUnavailable",
            })
            .collect()
    }

    fn good_rules() -> Vec<FeedbackRule> {
        vec![
            FeedbackRule::state(at_level(), eq("zero")).show("calm"),
            FeedbackRule::state(at_level(), eq("high")).show("busy"),
            FeedbackRule::event("acme.counter", "counter.changed")
                .show("busy")
                .hold(Hold::For(500)),
        ]
    }

    // -- issues ------------------------------------------------------------

    #[test]
    fn a_good_draft_has_no_issues_and_the_limits_are_the_gates() {
        let draft = pad(good_rules());
        let r = run(&draft, None);
        assert!(r.issues.is_empty() && r.notes.is_empty(), "{r:?}");
        assert_eq!(r.limits.max_rules, MAX_RULES);
        assert_eq!(r.limits, FeedbackLimits::default());
    }

    #[test]
    fn issues_are_what_saving_would_refuse_and_name_the_field() {
        let state = || at_level();
        let rules = vec![
            FeedbackRule::state(state(), Test::HasValue), // no look
            FeedbackRule::state(StateAddress::new("acme.counter", "nope"), Test::HasValue)
                .show("busy"), // unknown state
            FeedbackRule::state(state(), Test::LessThan { value: 3.0 }).show("busy"), // numeric on enum
            FeedbackRule::state(state(), eq("overflowing")).show("busy"), // value not listed
            FeedbackRule::event("acme.counter", "counter.changed")
                .show("busy")
                .hold(Hold::For(100)), // hold range
        ];
        let draft = pad(rules);
        let r = run(&draft, None);
        let got: Vec<_> = r.issues.iter().map(|i| (i.rule, i.field)).collect();
        assert_eq!(
            got,
            vec![
                (0, Some(IssueField::Look)),
                (1, Some(IssueField::State)),
                (2, Some(IssueField::Test)),
                (3, Some(IssueField::Value)),
                (4, Some(IssueField::Hold)),
            ]
        );
        // The same words the save refuses with.
        let (hub, _) = world();
        let refused =
            prepare_rules(&draft.feedback, &[], &|p, i| hub.definition(p, i)).unwrap_err();
        assert_eq!(r.issues, refused);
    }

    #[test]
    fn a_saved_rule_is_never_an_issue_only_a_note() {
        let orphan = FeedbackRule::state(StateAddress::new("acme.gone", "thing"), Test::HasValue)
            .show("busy");
        let stored = pad(vec![orphan.clone()]);
        let mut draft = stored.clone();
        draft.label = "Renamed".into();
        let r = run(&draft, Some(&stored));
        assert!(r.issues.is_empty(), "{:?}", r.issues);
        assert_eq!(kinds(&r), vec!["missingObservable"]);
        assert!(r.preview.is_none(), "rules and looks are the saved ones");
        // Changing it makes it a new rule, and a new rule must name a real state.
        draft.feedback = vec![orphan.show("calm")];
        let r = run(&draft, Some(&stored));
        assert_eq!(r.issues.len(), 1);
        assert_eq!(r.issues[0].field, Some(IssueField::State));
    }

    // -- notes -------------------------------------------------------------

    #[test]
    fn a_look_that_does_not_exist_is_noted_unless_it_cannot_be_known() {
        let rules = vec![
            FeedbackRule::state(at_level(), eq("zero")).show("ghost"),
            // `idle` is the resting look unless the control draws one.
            FeedbackRule::state(at_level(), eq("high")).show("idle"),
            // Taken from the value: only known when the value arrives.
            FeedbackRule::state(at_level(), Test::HasValue).show_from_value(),
            FeedbackRule::event("acme.counter", "counter.changed")
                .show("phantom")
                .hold(Hold::Latch),
            FeedbackRule::event("acme.counter", "counter.changed").show_from_field("level"),
        ];
        let r = run(&pad(rules), None);
        assert!(r.issues.is_empty(), "{:?}", r.issues);
        assert_eq!(
            r.notes,
            vec![
                RuleNote::MissingLook {
                    rule: 0,
                    look: "ghost".into()
                },
                RuleNote::MissingLook {
                    rule: 3,
                    look: "phantom".into()
                },
            ]
        );
        // Once the control has its own `idle`, naming it is fine, and so is any look it has.
        let mut own = pad(vec![
            FeedbackRule::state(at_level(), eq("zero")).show("idle"),
            FeedbackRule::state(at_level(), eq("high")).show("ghost"),
        ]);
        own.states
            .insert("idle".into(), Visual::solid(Rgb::new(1, 2, 3)));
        own.states
            .insert("ghost".into(), Visual::solid(Rgb::new(4, 5, 6)));
        assert!(run(&own, None).notes.is_empty());
    }

    #[test]
    fn the_looks_that_count_are_the_drafts_not_the_saved_controls() {
        let rules = vec![FeedbackRule::state(at_level(), eq("zero")).show("fresh")];
        let stored = pad(rules.clone());
        let mut draft = stored.clone();
        assert_eq!(kinds(&run(&draft, Some(&stored))), vec!["missingLook"]);
        draft
            .states
            .insert("fresh".into(), Visual::solid(Rgb::new(9, 9, 9)));
        let r = run(&draft, Some(&stored));
        assert!(r.notes.is_empty());
        assert!(r.preview.is_some(), "a new look changes the preview");
    }

    #[test]
    fn a_rule_that_an_earlier_rule_always_wins_over_is_noted() {
        let rules = vec![
            FeedbackRule::state(at_level(), Test::HasValue).show("busy"),
            FeedbackRule::state(at_level(), eq("high")).show("calm"),
        ];
        let r = run(&pad(rules), None);
        assert_eq!(r.notes, vec![RuleNote::Shadowed { rule: 1, by: 0 }]);
    }

    #[test]
    fn something_nothing_offers_is_noted_for_states_and_events() {
        let saved = vec![
            FeedbackRule::state(StateAddress::new("acme.gone", "thing"), Test::HasValue)
                .show("busy"),
            FeedbackRule::state(StateAddress::new("acme.counter", "other"), Test::HasValue)
                .show("busy"),
            FeedbackRule::event("ci", "build.finished").show("busy"),
            FeedbackRule::event("acme.counter", "unheard").show("busy"),
        ];
        let stored = pad(saved);
        let r = run(&stored, Some(&stored));
        assert!(r.issues.is_empty());
        let got: Vec<_> = r
            .notes
            .iter()
            .map(|n| match n {
                RuleNote::MissingObservable {
                    rule,
                    observable_kind,
                    provider,
                    id,
                } => (*rule, *observable_kind, provider.as_str(), id.as_str()),
                other => panic!("{other:?}"),
            })
            .collect();
        assert_eq!(
            got,
            vec![
                (0, ObservableKind::State, "acme.gone", "thing"),
                (1, ObservableKind::State, "acme.counter", "other"),
                (2, ObservableKind::Event, "ci", "build.finished"),
                (3, ObservableKind::Event, "acme.counter", "unheard"),
            ]
        );
        // A state is not an event: the kind decides which list is asked.
        let mixed = pad(vec![
            FeedbackRule::event("acme.counter", "level").show("busy")
        ]);
        let r = run(&mixed, Some(&mixed));
        assert_eq!(kinds(&r), vec!["missingObservable"]);
    }

    #[test]
    fn a_provider_that_is_off_is_noted_with_its_own_reason() {
        let draft = pad(vec![FeedbackRule::state(
            StateAddress::new("acme.off", "thing"),
            Test::HasValue,
        )
        .show("busy")]);
        let r = run(&draft, None);
        assert!(r.issues.is_empty(), "the hub keeps the declaration");
        assert_eq!(
            r.notes,
            vec![RuleNote::ProviderUnavailable {
                rule: 0,
                provider: "acme.off".into(),
                provider_name: "acme.off plugin".into(),
                reason: "Turned off in Plugins.".into(),
            }]
        );
    }

    #[test]
    fn notes_come_by_rule_and_then_by_kind() {
        let rules = vec![
            FeedbackRule::state(at_level(), Test::HasValue).show("busy"),
            // Rule 1: a missing look, shadowed by rule 0, on a state nothing offers.
            FeedbackRule::state(StateAddress::new("acme.counter", "level"), eq("high"))
                .show("ghost"),
            FeedbackRule::state(StateAddress::new("acme.off", "thing"), Test::HasValue)
                .show("ghost"),
        ];
        let r = run(&pad(rules), None);
        let order: Vec<_> = r.notes.iter().map(|n| (n.rule(), kinds_of(n))).collect();
        assert_eq!(
            order,
            vec![
                (1, "missingLook"),
                (1, "shadowed"),
                (2, "missingLook"),
                (2, "providerUnavailable"),
            ]
        );
    }

    fn kinds_of(n: &RuleNote) -> &'static str {
        match n {
            RuleNote::MissingLook { .. } => "missingLook",
            RuleNote::Shadowed { .. } => "shadowed",
            RuleNote::MissingObservable { .. } => "missingObservable",
            RuleNote::ProviderUnavailable { .. } => "providerUnavailable",
        }
    }

    // -- preview -----------------------------------------------------------

    #[test]
    fn the_preview_is_there_for_a_changed_or_unsaved_control_only() {
        let stored = pad(good_rules());
        // Unsaved: always.
        assert!(run(&stored, None).preview.is_some());
        // Same rules and looks, other label, other colour: nothing to preview.
        let mut same = stored.clone();
        same.label = "Other".into();
        same.visual = Visual::solid(Rgb::new(1, 1, 1));
        assert!(run(&same, Some(&stored)).preview.is_none());
        // Another rule, another look.
        let mut other = stored.clone();
        other.feedback.pop();
        assert!(run(&other, Some(&stored)).preview.is_some());
        let mut other = stored.clone();
        other.states.remove("calm");
        assert!(run(&other, Some(&stored)).preview.is_some());
    }

    #[test]
    fn the_preview_is_the_explanation_of_the_draft_against_the_values_now() {
        let (hub, sources) = world();
        hub.set(&at_level(), Scalar::text("high")).unwrap();
        let draft = pad(good_rules());
        let r = review(("s", "p"), &draft, None, &hub, &sources);
        let p = r.preview.expect("an unsaved control has a preview");
        assert_eq!(
            (
                p.surface_id.as_str(),
                p.page_id.as_str(),
                p.control_id.as_str()
            ),
            ("s", "p", draft.id.as_str())
        );
        assert_eq!((p.derived.as_deref(), p.winner), (Some("busy"), Some(1)));
        assert_eq!((p.shown.as_deref(), p.layer), (Some("busy"), "state"));
        assert!(p.overlay.is_none());
        assert_eq!(p.rules.len(), 3);
        assert!(p.rules[1].holds && p.rules[1].decided);
        // The same code as the saved control's explanation, not a copy.
        let direct = explain::explain(
            ("s", "p", &draft.id),
            &draft,
            None,
            p.derived.clone(),
            &hub,
            &sources,
        );
        assert_eq!(direct, p);

        // Nothing holds: the resting look.
        hub.clear(&at_level());
        let p = review(("s", "p"), &draft, None, &hub, &sources)
            .preview
            .unwrap();
        assert_eq!((p.shown, p.layer, p.derived), (None, "rest", None));
    }

    #[test]
    fn the_preview_uses_the_rules_as_they_would_be_saved() {
        // The scoped state's argument has a default that saving fills in.
        let scoped = ObservableDefinition::state("acme.counter", "slot", "Slot", "")
            .param(
                crate::actions::params::ParamDef::number("slot", "Track")
                    .range(0.0, 7.0)
                    .integer()
                    .default_value(serde_json::json!(0)),
            )
            .one_of(&[("on", "On", None)]);
        let hub = StateHub::new();
        hub.declare("acme.counter", vec![scoped.clone()]).unwrap();
        hub.provider_up("acme.counter");
        let sources = vec![Source {
            info: info("acme.counter", ProviderStatus::Ready),
            observables: vec![scoped],
        }];
        let draft = pad(vec![FeedbackRule::state(
            StateAddress::new("acme.counter", "slot"),
            Test::HasValue,
        )
        .show("busy")]);
        let r = review(("s", "p"), &draft, None, &hub, &sources);
        assert!(r.issues.is_empty());
        let rule = &r.preview.unwrap().rules[0];
        assert_eq!(
            rule.observable.as_ref().unwrap().address,
            "acme.counter.slot(slot=0)"
        );
    }

    // -- wire ---------------------------------------------------------------

    #[test]
    fn a_review_is_written_in_camel_case() {
        let rules = vec![
            FeedbackRule::state(at_level(), Test::HasValue).show("ghost"),
            FeedbackRule::state(StateAddress::new("acme.off", "thing"), Test::HasValue)
                .show("busy"),
            FeedbackRule::state(StateAddress::new("acme.gone", "thing"), Test::HasValue)
                .show("busy"),
            FeedbackRule::state(at_level(), eq("high")).show("busy"),
        ];
        let stored = pad(rules);
        let mut draft = stored.clone();
        draft
            .feedback
            .push(FeedbackRule::state(at_level(), Test::LessThan { value: 1.0 }).show("busy"));
        let json = serde_json::to_value(run(&draft, Some(&stored))).unwrap();
        assert!(json["preview"].is_object());
        assert_eq!(json["limits"]["maxRules"], MAX_RULES);
        assert_eq!(json["limits"]["defaultHoldMs"], 3_000);
        assert_eq!(json["issues"][0]["field"], "test");
        assert_eq!(json["issues"][0]["rule"], 4);
        assert!(json["issues"][0].get("param").is_none());
        let notes = json["notes"].as_array().unwrap();
        let by_kind = |k: &str| {
            notes
                .iter()
                .find(|n| n["kind"] == k)
                .unwrap_or_else(|| panic!("{k}"))
        };
        assert_eq!(by_kind("missingLook")["look"], "ghost");
        assert_eq!(by_kind("shadowed")["by"], 0);
        assert_eq!(by_kind("missingObservable")["observableKind"], "state");
        assert_eq!(by_kind("missingObservable")["provider"], "acme.gone");
        assert_eq!(
            by_kind("providerUnavailable")["providerName"],
            "acme.off plugin"
        );
        // No change: the preview is an explicit null, not an absent key.
        let json = serde_json::to_value(run(&stored, Some(&stored))).unwrap();
        assert!(json["preview"].is_null() && json.get("preview").is_some());
    }

    #[test]
    fn too_many_rules_is_an_issue_on_the_list() {
        let rules = vec![FeedbackRule::state(at_level(), eq("zero")).show("busy"); MAX_RULES + 1];
        let r = run(&pad(rules), None);
        let issue = r.issues.iter().find(|i| i.rule == MAX_RULES).unwrap();
        assert_eq!(issue.field, Some(IssueField::Rules));
    }
}
