//! The pure evaluator: current states + rules + looks in, a look out.
//!
//! ```text
//!   rules (ordered) ──┐
//!   read(state key) ──┼──► evaluate ──► derived look (or none = rest) + why
//!   which looks exist ┘
//! ```
//!
//! Nothing here holds a lock, reads a clock or remembers anything. The answer
//! is a function of the current values and the visible order of the rules, so
//! it does not depend on the order in which values arrived, on whether the
//! control existed when they did, or on whether the app just started.
//!
//! Only state rules take part. Event rules are momentary and are handled by the
//! runtime as overlays when their event arrives.

use serde::Serialize;

use super::address::StateKey;
use super::feedback::{FeedbackRule, When, IDLE};
use super::hub::Observed;

/// Something worth telling the person about a rule.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Diagnostic {
    /// The rule that decided names a look the control does not have, so the
    /// pad shows its resting look. Today this is silent.
    MissingLook { rule: usize, look: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Evaluation {
    /// The look to show, or `None` for the resting look.
    pub look: Option<String>,
    /// The position of the rule that decided, if one did.
    pub winner: Option<usize>,
    pub diagnostics: Vec<Diagnostic>,
}

/// Evaluate the state rules of one control. The first true rule decides.
///
/// * `read` returns the observation of a state.
/// * `has_look` says whether the control defines a look of that name.
/// * The look name `idle` means "rest", unless the control has a look called
///   `idle` of its own.
pub fn evaluate(
    rules: &[FeedbackRule],
    read: &dyn Fn(&StateKey) -> Observed,
    has_look: &dyn Fn(&str) -> bool,
) -> Evaluation {
    for (i, rule) in rules.iter().enumerate() {
        let When::State(c) = &rule.when else {
            continue;
        };
        let observed = read(&c.state.key());
        if !c.is.holds(&observed) {
            continue;
        }
        let Some(name) = rule.look_for_state(observed.value.as_ref()) else {
            continue;
        };
        return decide(i, name, has_look);
    }
    Evaluation::default()
}

fn decide(rule: usize, name: String, has_look: &dyn Fn(&str) -> bool) -> Evaluation {
    if name == IDLE && !has_look(IDLE) {
        return Evaluation {
            look: None,
            winner: Some(rule),
            diagnostics: Vec::new(),
        };
    }
    let diagnostics = if has_look(&name) {
        Vec::new()
    } else {
        vec![Diagnostic::MissingLook {
            rule,
            look: name.clone(),
        }]
    };
    Evaluation {
        look: Some(name),
        winner: Some(rule),
        diagnostics,
    }
}

/// What one rule said about the current values.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuleReport {
    pub index: usize,
    /// `state` or `event`.
    pub kind: &'static str,
    /// For a state rule: its test holds right now. Event rules are momentary
    /// and never "hold".
    pub holds: bool,
    /// For a state rule: what the state reads right now.
    pub observed: Option<Observed>,
    /// The look the rule would show if it decided.
    pub look: Option<String>,
    /// Whether that look exists on the control.
    pub look_exists: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Examination {
    pub rules: Vec<RuleReport>,
    pub result: ExaminedResult,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExaminedResult {
    pub look: Option<String>,
    pub winner: Option<usize>,
    pub diagnostics: Vec<Diagnostic>,
}

/// The same decision as [`evaluate`], with the reasoning written out for every
/// rule: which are true now, what each state reads, which look each would show.
pub fn examine(
    rules: &[FeedbackRule],
    read: &dyn Fn(&StateKey) -> Observed,
    has_look: &dyn Fn(&str) -> bool,
) -> Examination {
    let reports = rules
        .iter()
        .enumerate()
        .map(|(index, rule)| match &rule.when {
            When::Event(_) => RuleReport {
                index,
                kind: "event",
                holds: false,
                observed: None,
                look: rule.show.clone(),
                look_exists: rule.show.as_deref().map(has_look),
            },
            When::State(c) => {
                let observed = read(&c.state.key());
                let holds = c.is.holds(&observed);
                let look = rule.look_for_state(observed.value.as_ref());
                RuleReport {
                    index,
                    kind: "state",
                    holds,
                    look_exists: look.as_deref().map(|l| has_look(l) || l == IDLE),
                    look,
                    observed: Some(observed),
                }
            }
        })
        .collect();
    let e = evaluate(rules, read, has_look);
    Examination {
        rules: reports,
        result: ExaminedResult {
            look: e.look,
            winner: e.winner,
            diagnostics: e.diagnostics,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::address::StateAddress;
    use crate::state::feedback::Test;
    use crate::state::hub::Status;
    use crate::state::value::Scalar;
    use std::collections::HashMap;

    /// A world of values for the table tests.
    struct World(HashMap<StateKey, Observed>);

    impl World {
        fn new() -> Self {
            World(HashMap::new())
        }
        fn with(mut self, addr: &StateAddress, v: Scalar) -> Self {
            self.0.insert(addr.key(), Observed::known(v, 1));
            self
        }
        fn unknown(mut self, addr: &StateAddress) -> Self {
            self.0.insert(addr.key(), Observed::unknown());
            self
        }
        fn read(&self) -> impl Fn(&StateKey) -> Observed + '_ {
            |k| self.0.get(k).cloned().unwrap_or_else(Observed::unavailable)
        }
    }

    fn addr(id: &str) -> StateAddress {
        StateAddress::new("demo", id)
    }

    fn eq(id: &str, v: &str, look: &str) -> FeedbackRule {
        FeedbackRule::state(
            addr(id),
            Test::Equals {
                value: Scalar::text(v),
            },
        )
        .show(look)
    }

    fn looks(names: &'static [&'static str]) -> impl Fn(&str) -> bool {
        move |n| names.contains(&n)
    }

    #[test]
    fn the_first_true_rule_wins_and_the_order_is_the_priority() {
        let rules = [eq("alpha", "running", "a"), eq("beta", "running", "b")];
        let both = World::new()
            .with(&addr("alpha"), Scalar::text("running"))
            .with(&addr("beta"), Scalar::text("running"));
        let e = evaluate(&rules, &both.read(), &looks(&["a", "b"]));
        assert_eq!((e.look.as_deref(), e.winner), (Some("a"), Some(0)));

        // Reordering the list is what changes the outcome, nothing else.
        let swapped = [rules[1].clone(), rules[0].clone()];
        let e = evaluate(&swapped, &both.read(), &looks(&["a", "b"]));
        assert_eq!((e.look.as_deref(), e.winner), (Some("b"), Some(0)));

        let only_beta = World::new().with(&addr("beta"), Scalar::text("running"));
        let e = evaluate(&rules, &only_beta.read(), &looks(&["a", "b"]));
        assert_eq!((e.look.as_deref(), e.winner), (Some("b"), Some(1)));
    }

    #[test]
    fn the_answer_does_not_depend_on_the_order_values_arrived() {
        let rules = [eq("alpha", "running", "a"), eq("beta", "running", "b")];
        let (alpha, beta) = (addr("alpha"), addr("beta"));
        let one = World::new()
            .with(&alpha, Scalar::text("running"))
            .with(&beta, Scalar::text("running"));
        let other = World::new()
            .with(&beta, Scalar::text("running"))
            .with(&alpha, Scalar::text("running"));
        let l = looks(&["a", "b"]);
        assert_eq!(
            evaluate(&rules, &one.read(), &l),
            evaluate(&rules, &other.read(), &l)
        );
    }

    #[test]
    fn nothing_true_means_rest_not_the_previous_look() {
        let rules = [eq("alpha", "running", "a")];
        let w = World::new().with(&addr("alpha"), Scalar::text("error"));
        assert_eq!(
            evaluate(&rules, &w.read(), &looks(&["a"])),
            Evaluation::default()
        );
        assert_eq!(evaluate(&[], &w.read(), &looks(&[])), Evaluation::default());
    }

    #[test]
    fn unknown_and_unavailable_are_handled_by_name() {
        let alpha = addr("alpha");
        let rules = [
            FeedbackRule::state(alpha.clone(), Test::Unavailable).show("gone"),
            FeedbackRule::state(alpha.clone(), Test::NoValue).show("waiting"),
            FeedbackRule::state(
                alpha.clone(),
                Test::NotEquals {
                    value: Scalar::text("idle"),
                },
            )
            .show("busy"),
        ];
        let l = looks(&["gone", "waiting", "busy"]);
        let gone = World::new();
        assert_eq!(
            evaluate(&rules, &gone.read(), &l).look.as_deref(),
            Some("gone")
        );
        let unknown = World::new().unknown(&alpha);
        assert_eq!(
            evaluate(&rules, &unknown.read(), &l).look.as_deref(),
            Some("waiting")
        );
        let known = World::new().with(&alpha, Scalar::text("running"));
        assert_eq!(
            evaluate(&rules, &known.read(), &l).look.as_deref(),
            Some("busy")
        );
        let idle = World::new().with(&alpha, Scalar::text("idle"));
        assert_eq!(evaluate(&rules, &idle.read(), &l).look, None);
        // "is not idle" must not light up for a state that has no value.
        let only_ne = &rules[2..];
        assert_eq!(evaluate(only_ne, &unknown.read(), &l).look, None);
        assert_eq!(evaluate(only_ne, &gone.read(), &l).look, None);
    }

    #[test]
    fn a_look_from_the_value_names_the_look_after_it() {
        let rule = FeedbackRule::state(addr("alpha"), Test::HasValue).show_from_value();
        let w = World::new().with(&addr("alpha"), Scalar::text("running"));
        let e = evaluate(std::slice::from_ref(&rule), &w.read(), &looks(&["running"]));
        assert_eq!(e.look.as_deref(), Some("running"));
        assert!(e.diagnostics.is_empty());
        // Numbers and booleans name looks too.
        let n = World::new().with(&addr("alpha"), Scalar::Num(3.0));
        assert_eq!(
            evaluate(std::slice::from_ref(&rule), &n.read(), &looks(&["3"]))
                .look
                .as_deref(),
            Some("3")
        );
        let b = World::new().with(&addr("alpha"), Scalar::Bool(true));
        assert_eq!(
            evaluate(&[rule], &b.read(), &looks(&["true"]))
                .look
                .as_deref(),
            Some("true")
        );
    }

    #[test]
    fn a_missing_look_is_reported_and_idle_means_rest() {
        let rule = FeedbackRule::state(addr("alpha"), Test::HasValue).show_from_value();
        let w = World::new().with(&addr("alpha"), Scalar::text("buffering"));
        let e = evaluate(std::slice::from_ref(&rule), &w.read(), &looks(&["running"]));
        assert_eq!(e.look.as_deref(), Some("buffering"));
        assert_eq!(
            e.diagnostics,
            vec![Diagnostic::MissingLook {
                rule: 0,
                look: "buffering".into()
            }]
        );
        // `idle` rests without a complaint when the control has no `idle` look...
        let idle = World::new().with(&addr("alpha"), Scalar::text("idle"));
        let e = evaluate(
            std::slice::from_ref(&rule),
            &idle.read(),
            &looks(&["running"]),
        );
        assert_eq!((e.look, e.winner, e.diagnostics.len()), (None, Some(0), 0));
        // ...and shows it when it has one.
        let e = evaluate(&[rule], &idle.read(), &looks(&["idle"]));
        assert_eq!(e.look.as_deref(), Some("idle"));
    }

    #[test]
    fn a_rule_that_cannot_name_a_look_is_skipped() {
        // "Show from value" with a test that is true while there is no value.
        let broken = FeedbackRule::state(addr("alpha"), Test::NoValue).show_from_value();
        let fallback = FeedbackRule::state(addr("alpha"), Test::NoValue).show("waiting");
        let w = World::new().unknown(&addr("alpha"));
        let e = evaluate(&[broken, fallback], &w.read(), &looks(&["waiting"]));
        assert_eq!((e.look.as_deref(), e.winner), (Some("waiting"), Some(1)));
    }

    #[test]
    fn event_rules_do_not_take_part() {
        let rules = [
            FeedbackRule::event("demo", "x").show("momentary"),
            eq("alpha", "running", "a"),
        ];
        let w = World::new().with(&addr("alpha"), Scalar::text("running"));
        let e = evaluate(&rules, &w.read(), &looks(&["momentary", "a"]));
        assert_eq!((e.look.as_deref(), e.winner), (Some("a"), Some(1)));
    }

    #[test]
    fn the_same_inputs_always_give_the_same_answer() {
        let rules = [eq("alpha", "running", "a"), eq("beta", "idle", "b")];
        let w = World::new()
            .with(&addr("alpha"), Scalar::text("running"))
            .with(&addr("beta"), Scalar::text("idle"));
        let l = looks(&["a", "b"]);
        let first = evaluate(&rules, &w.read(), &l);
        for _ in 0..100 {
            assert_eq!(evaluate(&rules, &w.read(), &l), first);
        }
    }

    #[test]
    fn examining_explains_every_rule_and_agrees_with_evaluating() {
        let alpha = addr("alpha");
        let rules = [
            FeedbackRule::event("demo", "x").show("momentary"),
            eq("alpha", "running", "a"),
            FeedbackRule::state(alpha.clone(), Test::HasValue).show_from_value(),
        ];
        let w = World::new().with(&alpha, Scalar::text("running"));
        let l = looks(&["momentary", "a"]);
        let x = examine(&rules, &w.read(), &l);
        assert_eq!(x.result.winner, Some(1));
        assert_eq!(x.rules.len(), 3);
        assert_eq!(x.rules[0].kind, "event");
        assert!(!x.rules[0].holds);
        assert!(x.rules[1].holds && x.rules[1].look_exists == Some(true));
        // Rule 3 is also true, would show "running", and the control has no such look:
        // the explanation says so even though rule 2 won.
        assert!(x.rules[2].holds);
        assert_eq!(x.rules[2].look.as_deref(), Some("running"));
        assert_eq!(x.rules[2].look_exists, Some(false));
        assert_eq!(x.rules[2].observed.as_ref().unwrap().status, Status::Known);
        assert_eq!(x.result, {
            let e = evaluate(&rules, &w.read(), &l);
            ExaminedResult {
                look: e.look,
                winner: e.winner,
                diagnostics: e.diagnostics,
            }
        });
    }
}
