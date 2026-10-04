//! What can be proved about a list of rules without any value.
//!
//! The evaluator answers "what does this control show now". This module
//! answers one question that does not depend on the values at all: *can this
//! state rule ever decide?* It claims a rule is shadowed only in the two cases
//! below, which are provable from the rules alone, and says nothing else.
//!
//! A state rule `j` is **shadowed** by the first earlier state rule `i` when
//!
//! 1. both read exactly the same state (provider, id and arguments),
//! 2. whenever `i`'s test holds, `i` names a look, so it always decides, and
//! 3. `i`'s test holds whenever `j`'s does:
//!    * the two tests are identical (`equals high` after `equals high`), or
//!    * `i` is `hasValue` and `j` needs a value (every operator except
//!      `noValue` and `unavailable`).
//!
//! No ranges, no ordering of values, no reasoning about enums: `atLeast 5`
//! after `atLeast 3` is shadowed in fact, and this does not say so. A rule
//! that is not reported may still be dead; one that is reported certainly is.
//! Event rules are momentary and never shadow one another.

use serde::Serialize;

use super::feedback::{FeedbackRule, StateCondition, Test, When};

/// `rule` can never decide, because `by` (an earlier rule) always does first.
/// Both are 0-based positions in the control's list.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Shadow {
    pub rule: usize,
    pub by: usize,
}

/// Every state rule that an earlier state rule always overrides, in list order.
pub fn shadowed(rules: &[FeedbackRule]) -> Vec<Shadow> {
    let mut out = Vec::new();
    for (j, later) in rules.iter().enumerate() {
        let When::State(later_c) = &later.when else {
            continue;
        };
        let first = rules[..j].iter().position(|earlier| {
            let When::State(earlier_c) = &earlier.when else {
                return false;
            };
            same_state(earlier_c, later_c)
                && always_names_a_look(earlier, earlier_c)
                && covers(&earlier_c.is, &later_c.is)
        });
        if let Some(by) = first {
            out.push(Shadow { rule: j, by });
        }
    }
    out
}

/// Both rules read the same instance of the same state.
fn same_state(a: &StateCondition, b: &StateCondition) -> bool {
    a.state.key() == b.state.key()
}

/// Whenever the rule's test holds, the rule names a look, so it decides.
/// `show` always names one; a look from the value does while the test implies
/// a value (the evaluator skips a rule that holds with no value to name).
fn always_names_a_look(rule: &FeedbackRule, c: &StateCondition) -> bool {
    rule.show.is_some() || (rule.show_from_value && implies_value(&c.is))
}

/// A test that can only hold while the state is `known`.
fn implies_value(test: &Test) -> bool {
    !matches!(test, Test::NoValue | Test::Unavailable)
}

/// `earlier` holds whenever `later` does. Only the two provable cases.
fn covers(earlier: &Test, later: &Test) -> bool {
    identical(earlier, later) || (matches!(earlier, Test::HasValue) && implies_value(later))
}

/// The same operator with the same operand. Values compare with the type's own
/// equality, so `1` and `1.0` are the same and a text never equals a number.
fn identical(a: &Test, b: &Test) -> bool {
    match (a, b) {
        (Test::Equals { value: x }, Test::Equals { value: y })
        | (Test::NotEquals { value: x }, Test::NotEquals { value: y }) => x.same_as(y),
        (Test::LessThan { value: x }, Test::LessThan { value: y })
        | (Test::AtMost { value: x }, Test::AtMost { value: y })
        | (Test::GreaterThan { value: x }, Test::GreaterThan { value: y })
        | (Test::AtLeast { value: x }, Test::AtLeast { value: y }) => x == y,
        (Test::HasValue, Test::HasValue)
        | (Test::NoValue, Test::NoValue)
        | (Test::Unavailable, Test::Unavailable) => true,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::address::StateAddress;
    use crate::state::value::Scalar;

    fn level() -> StateAddress {
        StateAddress::new("example.counter", "level")
    }

    fn eq(v: &str) -> Test {
        Test::Equals {
            value: Scalar::text(v),
        }
    }

    fn ne(v: &str) -> Test {
        Test::NotEquals {
            value: Scalar::text(v),
        }
    }

    fn on_level(test: Test) -> FeedbackRule {
        FeedbackRule::state(level(), test)
    }

    fn shows(test: Test, look: &str) -> FeedbackRule {
        on_level(test).show(look)
    }

    fn pairs(rules: &[FeedbackRule]) -> Vec<(usize, usize)> {
        shadowed(rules).iter().map(|s| (s.rule, s.by)).collect()
    }

    #[test]
    fn an_identical_test_on_the_same_state_never_decides() {
        let rules = [shows(eq("high"), "a"), shows(eq("high"), "b")];
        assert_eq!(pairs(&rules), vec![(1, 0)]);
        // The same look is just as redundant: the rule still never decides.
        let rules = [shows(eq("high"), "a"), shows(eq("high"), "a")];
        assert_eq!(pairs(&rules), vec![(1, 0)]);
    }

    #[test]
    fn every_operator_is_compared_with_itself() {
        let n = |op: fn(f64) -> Test| [shows(op(3.0), "a"), shows(op(3.0), "b")];
        for rules in [
            n(|value| Test::LessThan { value }),
            n(|value| Test::AtMost { value }),
            n(|value| Test::GreaterThan { value }),
            n(|value| Test::AtLeast { value }),
            [shows(ne("x"), "a"), shows(ne("x"), "b")],
            [shows(Test::HasValue, "a"), shows(Test::HasValue, "b")],
            [shows(Test::NoValue, "a"), shows(Test::NoValue, "b")],
            [shows(Test::Unavailable, "a"), shows(Test::Unavailable, "b")],
        ] {
            assert_eq!(pairs(&rules), vec![(1, 0)], "{:?}", rules[0].when);
        }
    }

    #[test]
    fn has_value_before_a_test_that_needs_a_value_shadows_it() {
        for later in [
            eq("high"),
            ne("high"),
            Test::LessThan { value: 1.0 },
            Test::AtMost { value: 1.0 },
            Test::GreaterThan { value: 1.0 },
            Test::AtLeast { value: 1.0 },
            Test::HasValue,
        ] {
            let rules = [shows(Test::HasValue, "a"), shows(later.clone(), "b")];
            assert_eq!(pairs(&rules), vec![(1, 0)], "{later:?}");
        }
    }

    #[test]
    fn the_first_rule_that_shadows_is_named() {
        let rules = [
            shows(eq("low"), "a"),
            shows(Test::HasValue, "b"),
            shows(Test::HasValue, "c"),
            shows(eq("low"), "d"),
        ];
        // Rule 2 is hidden by rule 1, rule 3 by rule 0 (the first that covers it).
        assert_eq!(pairs(&rules), vec![(2, 1), (3, 0)]);
    }

    #[test]
    fn a_look_from_the_value_counts_when_the_test_has_a_value() {
        let mirror = on_level(Test::HasValue).show_from_value();
        let later = shows(eq("high"), "b");
        assert_eq!(pairs(&[mirror, later.clone()]), vec![(1, 0)]);
        let mirror = on_level(ne("zero")).show_from_value();
        assert_eq!(pairs(&[mirror, shows(ne("zero"), "b")]), vec![(1, 0)]);
        // A look from the value cannot name one while there is no value.
        for test in [Test::NoValue, Test::Unavailable] {
            let mirror = on_level(test.clone()).show_from_value();
            let later = shows(test, "b");
            assert!(pairs(&[mirror, later]).is_empty());
        }
    }

    #[test]
    fn an_earlier_rule_that_names_no_look_decides_nothing() {
        let unnamed = on_level(Test::HasValue);
        assert!(pairs(&[unnamed, shows(eq("high"), "b")]).is_empty());
        // A look from an event field never names one for a state rule.
        let from_field = on_level(Test::HasValue).show_from_field("x");
        assert!(pairs(&[from_field, shows(eq("high"), "b")]).is_empty());
    }

    #[test]
    fn different_states_and_arguments_are_different_states() {
        let other_id =
            FeedbackRule::state(StateAddress::new("example.counter", "value"), eq("a")).show("x");
        let other_provider =
            FeedbackRule::state(StateAddress::new("example.other", "level"), eq("a")).show("x");
        let rules = [shows(eq("a"), "x"), other_id, other_provider];
        assert!(pairs(&rules).is_empty());

        let chan =
            |name: &str| StateAddress::new("example.channels", "status").with("channel", name);
        let rules = [
            FeedbackRule::state(chan("alpha"), Test::HasValue).show("up"),
            FeedbackRule::state(chan("beta"), Test::HasValue).show("up"),
        ];
        assert!(pairs(&rules).is_empty());
        // Same arguments written two ways are the same state.
        let slot = |n: Scalar| StateAddress::new("agent-control", "slot.status").with("slot", n);
        let rules = [
            FeedbackRule::state(slot(Scalar::Num(2.0)), Test::HasValue).show("a"),
            FeedbackRule::state(
                slot(Scalar::from_json(&serde_json::json!(2)).unwrap()),
                eq("x"),
            )
            .show("b"),
        ];
        assert_eq!(pairs(&rules), vec![(1, 0)]);
        // An argument that one rule has and the other lacks is another instance.
        let rules = [
            FeedbackRule::state(
                StateAddress::new("example.channels", "status"),
                Test::HasValue,
            )
            .show("a"),
            FeedbackRule::state(chan("alpha"), Test::HasValue).show("b"),
        ];
        assert!(pairs(&rules).is_empty());
    }

    #[test]
    fn tests_that_are_not_provably_covered_are_left_alone() {
        let cases: Vec<(Test, Test)> = vec![
            // No value yet is not "has a value".
            (Test::NoValue, Test::HasValue),
            (Test::HasValue, Test::NoValue),
            (Test::HasValue, Test::Unavailable),
            (Test::Unavailable, Test::NoValue),
            // notEquals is not equals, in either direction.
            (ne("high"), eq("high")),
            (eq("high"), ne("high")),
            // Different operands.
            (eq("high"), eq("low")),
            (ne("high"), ne("low")),
            // A narrower test first does not cover a wider one.
            (eq("high"), Test::HasValue),
            (ne("high"), Test::HasValue),
            // No range reasoning, even where it would be true.
            (Test::AtLeast { value: 3.0 }, Test::AtLeast { value: 5.0 }),
            (Test::LessThan { value: 5.0 }, Test::LessThan { value: 3.0 }),
            (Test::LessThan { value: 3.0 }, Test::AtMost { value: 3.0 }),
            // A text is not a number.
            (
                Test::Equals {
                    value: Scalar::text("1"),
                },
                Test::Equals {
                    value: Scalar::Num(1.0),
                },
            ),
        ];
        for (first, second) in cases {
            let rules = [shows(first.clone(), "a"), shows(second.clone(), "b")];
            assert!(pairs(&rules).is_empty(), "{first:?} then {second:?}");
        }
    }

    #[test]
    fn numbers_are_compared_as_numbers() {
        let one = Test::Equals {
            value: Scalar::Num(1.0),
        };
        let also_one = Test::Equals {
            value: Scalar::from_json(&serde_json::json!(1)).unwrap(),
        };
        assert_eq!(
            pairs(&[shows(one, "a"), shows(also_one, "b")]),
            vec![(1, 0)]
        );
    }

    #[test]
    fn event_rules_neither_shadow_nor_are_shadowed() {
        let event = || FeedbackRule::event("example.counter", "counter.changed").show("blink");
        let rules = [event(), event(), shows(Test::HasValue, "a"), event()];
        assert!(pairs(&rules).is_empty());
        // An event rule in between changes nothing about the state rules around it.
        let rules = [shows(Test::HasValue, "a"), event(), shows(eq("high"), "b")];
        assert_eq!(pairs(&rules), vec![(2, 0)]);
    }

    #[test]
    fn nothing_to_say_about_short_lists() {
        assert!(shadowed(&[]).is_empty());
        assert!(shadowed(&[shows(Test::HasValue, "a")]).is_empty());
    }

    #[test]
    fn a_shadow_writes_camel_case() {
        let json = serde_json::to_value(Shadow { rule: 2, by: 0 }).unwrap();
        assert_eq!(json, serde_json::json!({ "rule": 2, "by": 0 }));
    }
}
