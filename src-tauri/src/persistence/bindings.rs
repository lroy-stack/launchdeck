//! Workspace schema 3 to 4: bindings become feedback rules.
//!
//! Version 3 stored, per control, `bindings`: for an event source, a list of
//! "when this event arrives (and its payload has these fields), set the
//! control's state to this name". That made the last event the control's
//! truth: nothing said what a control created *after* the event should show.
//!
//! Version 4 stores `feedback`, one ordered list of semantic rules (see
//! `state::feedback`). The conversion runs on the JSON before typed parsing,
//! so it can read what the current types no longer know. What each binding
//! rule becomes:
//!
//! ```text
//!   agent-control slot.changed    slot == n   -> $payload.status
//!       => state rule  agent-control.slot.status{slot:n}, hasValue, look from the value
//!   agent-control focus.changed   slot == n   -> focused
//!       => state rule  agent-control.slot.focused{slot:n} is true, show focused
//!      (the catch-all `focus.changed -> idle` that followed it is dropped: with a
//!       state rule, "not selected" is simply "no rule is true")
//!   agent-control command.state   command == c -> $payload.state
//!       => state rule  agent-control.command.state{command:c}, hasValue, look from the value
//!   demo plugin.state.changed     key == a    -> $payload.value
//!       => state rule  demo.a, hasValue, look from the value
//!   anything else
//!       => event rule, same source and event, same payload tests, same look,
//!          hold: latch (the look stays until the next event, as it did)
//!   a rule that cannot be expressed
//!       => kept word for word in `legacyBindings` as a one-rule binding
//! ```
//!
//! Only the first two shapes were states in disguise (the look *is* the
//! value), so only they become state rules. Everything else keeps the
//! behaviour it had. Nothing is dropped except the redundant catch-all, and
//! the step is idempotent: a control that has no `bindings` is left alone.

use serde_json::{json, Value};

use crate::agents::model::CommandKind;
use crate::agents::observables;
use crate::agents::service::{SLOT_COUNT, SOURCE as AGENT_CONTROL};
use crate::plugins::demo::{AGENTS as DEMO_AGENTS, ID as DEMO};
use crate::state::feedback::{FeedbackRule, Hold, Test, MAX_LOOK_NAME, MAX_RULES, MAX_WHERE};
use crate::state::{Scalar, StateAddress};

/// What the conversion did, for the log and for tests.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Report {
    /// Rules that became state rules.
    pub state_rules: usize,
    /// Rules that became event rules.
    pub event_rules: usize,
    /// Redundant catch-alls that were not needed any more.
    pub dropped: usize,
    /// Rules (or whole bindings) kept in `legacyBindings`.
    pub kept: usize,
}

impl Report {
    fn add(&mut self, other: Report) {
        self.state_rules += other.state_rules;
        self.event_rules += other.event_rules;
        self.dropped += other.dropped;
        self.kept += other.kept;
    }
}

/// Convert every control's bindings in a workspace document, in place.
pub fn upgrade_bindings(doc: &mut Value) -> Report {
    let mut report = Report::default();
    let Some(surfaces) = doc.get_mut("surfaces").and_then(Value::as_array_mut) else {
        return report;
    };
    for surface in surfaces {
        let Some(pages) = surface.get_mut("pages").and_then(Value::as_array_mut) else {
            continue;
        };
        for page in pages {
            let Some(controls) = page.get_mut("controls").and_then(Value::as_array_mut) else {
                continue;
            };
            for control in controls {
                report.add(upgrade_control(control));
            }
        }
    }
    report
}

fn upgrade_control(control: &mut Value) -> Report {
    let mut report = Report::default();
    let Some(object) = control.as_object_mut() else {
        return report;
    };
    let Some(bindings) = object.remove("bindings") else {
        return report;
    };
    let mut feedback: Vec<Value> = object
        .remove("feedback")
        .and_then(|v| v.as_array().cloned())
        .unwrap_or_default();
    let mut legacy: Vec<Value> = object
        .remove("legacyBindings")
        .and_then(|v| v.as_array().cloned())
        .unwrap_or_default();

    let list = match bindings {
        Value::Array(list) => list,
        // Not a list at all: nothing here can be read, so keep it as it is.
        other => {
            legacy.push(other);
            report.kept += 1;
            vec![]
        }
    };
    for binding in list {
        let parsed = binding
            .get("source")
            .and_then(Value::as_str)
            .zip(binding.get("rules").and_then(Value::as_array));
        let Some((source, rules)) = parsed else {
            legacy.push(binding.clone());
            report.kept += 1;
            continue;
        };
        let mut previous_was_focus = false;
        for rule in rules {
            match convert(source, rule, previous_was_focus) {
                Converted::State(_) | Converted::Event(_) if feedback.len() >= MAX_RULES => {
                    // More rules than a control may carry now: keep the rest.
                    legacy.push(single(source, rule));
                    report.kept += 1;
                }
                Converted::State(r) => {
                    previous_was_focus = is_focus(&r);
                    feedback.push(to_json(&r));
                    report.state_rules += 1;
                }
                Converted::Event(r) => {
                    previous_was_focus = false;
                    feedback.push(to_json(&r));
                    report.event_rules += 1;
                }
                Converted::Dropped => report.dropped += 1,
                Converted::Keep => {
                    previous_was_focus = false;
                    legacy.push(single(source, rule));
                    report.kept += 1;
                }
            }
        }
    }
    if !feedback.is_empty() {
        object.insert("feedback".into(), Value::Array(feedback));
    }
    if !legacy.is_empty() {
        object.insert("legacyBindings".into(), Value::Array(legacy));
    }
    report
}

fn to_json(rule: &FeedbackRule) -> Value {
    serde_json::to_value(rule).expect("a feedback rule is always JSON")
}

/// A rule that could not be converted, as a binding of its own.
fn single(source: &str, rule: &Value) -> Value {
    json!({ "source": source, "rules": [rule] })
}

fn is_focus(rule: &FeedbackRule) -> bool {
    matches!(&rule.when, crate::state::feedback::When::State(c) if c.state.id == observables::SLOT_FOCUSED)
}

enum Converted {
    State(FeedbackRule),
    Event(FeedbackRule),
    Dropped,
    Keep,
}

/// One schema 3 binding rule: `{event, when: [{field, equals}], setState}`.
struct Old<'a> {
    event: &'a str,
    when: Vec<(&'a str, Scalar)>,
    set_state: &'a str,
}

fn read(rule: &Value) -> Option<Old<'_>> {
    let event = rule.get("event")?.as_str()?;
    let set_state = rule.get("setState")?.as_str()?;
    let mut when = Vec::new();
    if let Some(list) = rule.get("when") {
        for c in list.as_array()? {
            let field = c.get("field")?.as_str()?;
            when.push((field, Scalar::from_json(c.get("equals")?).ok()?));
        }
    }
    Some(Old {
        event,
        when,
        set_state,
    })
}

fn plain(s: &str, max: usize) -> bool {
    !s.trim().is_empty() && s.chars().count() <= max && !s.chars().any(char::is_control)
}

fn whole_number(v: &Scalar) -> Option<i64> {
    v.as_number()
        .filter(|n| n.fract() == 0.0 && n.abs() < 9.0e15)
        .map(|n| n as i64)
}

fn convert(source: &str, rule: &Value, after_focus: bool) -> Converted {
    let Some(old) = read(rule) else {
        return Converted::Keep;
    };
    if let Some(state) = as_state_rule(source, &old, after_focus) {
        return state;
    }
    match as_event_rule(source, &old) {
        Some(rule) => Converted::Event(rule),
        None => Converted::Keep,
    }
}

fn as_state_rule(source: &str, old: &Old<'_>, after_focus: bool) -> Option<Converted> {
    match (source, old.event, old.when.as_slice(), old.set_state) {
        (s, "slot.changed", [(field, n)], "$payload.status")
            if s == AGENT_CONTROL && *field == "slot" =>
        {
            let slot = whole_number(n).filter(|n| (0..SLOT_COUNT as i64).contains(n))?;
            Some(Converted::State(
                FeedbackRule::state(observables::slot_status(slot as usize), Test::HasValue)
                    .show_from_value(),
            ))
        }
        (s, "focus.changed", [(field, n)], "focused") if s == AGENT_CONTROL && *field == "slot" => {
            let slot = whole_number(n).filter(|n| (0..SLOT_COUNT as i64).contains(n))?;
            Some(Converted::State(
                FeedbackRule::state(
                    observables::slot_focused(slot as usize),
                    Test::Equals {
                        value: Scalar::Bool(true),
                    },
                )
                .show("focused"),
            ))
        }
        (s, "focus.changed", [], "idle") if s == AGENT_CONTROL && after_focus => {
            Some(Converted::Dropped)
        }
        (s, "command.state", [(field, c)], "$payload.state")
            if s == AGENT_CONTROL && *field == "command" =>
        {
            let name = c.as_text()?;
            let kind = CommandKind::ALL.into_iter().find(|k| k.as_str() == name)?;
            Some(Converted::State(
                FeedbackRule::state(observables::command_state(kind), Test::HasValue)
                    .show_from_value(),
            ))
        }
        (s, "plugin.state.changed", [(field, a)], "$payload.value")
            if s == DEMO && *field == "key" =>
        {
            let agent = a.as_text().filter(|a| DEMO_AGENTS.contains(a))?;
            Some(Converted::State(
                FeedbackRule::state(StateAddress::new(DEMO, agent), Test::HasValue)
                    .show_from_value(),
            ))
        }
        _ => None,
    }
}

fn as_event_rule(source: &str, old: &Old<'_>) -> Option<FeedbackRule> {
    if !plain(source, 64) || !plain(old.event, 64) || old.when.len() > MAX_WHERE {
        return None;
    }
    let mut rule = FeedbackRule::event(source, old.event);
    for (field, equals) in &old.when {
        if !plain(field, 64) {
            return None;
        }
        rule = rule.where_field(field, equals.clone());
    }
    rule = match old.set_state.strip_prefix("$payload.") {
        Some(field) if plain(field, 64) => rule.show_from_field(field),
        Some(_) => return None,
        None if plain(old.set_state, MAX_LOOK_NAME) => rule.show(old.set_state),
        None => return None,
    };
    Some(rule.hold(Hold::Latch))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn control(bindings: Value) -> Value {
        json!({ "id": "g.0.0", "bindings": bindings })
    }

    fn converted(bindings: Value) -> (Value, Report) {
        let mut doc = json!({ "surfaces": [{ "id": "s", "pages": [{ "id": "p",
            "controls": [control(bindings)] }] }] });
        let report = upgrade_bindings(&mut doc);
        (
            doc["surfaces"][0]["pages"][0]["controls"][0].clone(),
            report,
        )
    }

    #[test]
    fn a_slot_binding_becomes_a_state_rule_on_the_track() {
        let (c, r) = converted(json!([{ "source": "agent-control", "rules": [
            { "event": "slot.changed", "when": [{ "field": "slot", "equals": 3 }],
              "setState": "$payload.status" }]}]));
        assert!(c.get("bindings").is_none());
        assert_eq!(
            c["feedback"],
            json!([{
                "when": { "state": { "provider": "agent-control", "id": "slot.status",
                                      "args": { "slot": 3 } },
                          "is": { "op": "hasValue" } },
                "showFromValue": true
            }])
        );
        assert_eq!((r.state_rules, r.event_rules, r.kept), (1, 0, 0));
    }

    #[test]
    fn the_focus_marker_keeps_one_rule_and_drops_the_catch_all() {
        let (c, r) = converted(json!([{ "source": "agent-control", "rules": [
            { "event": "focus.changed", "when": [{ "field": "slot", "equals": 2 }],
              "setState": "focused" },
            { "event": "focus.changed", "when": [], "setState": "idle" }]}]));
        assert_eq!(c["feedback"].as_array().unwrap().len(), 1);
        assert_eq!(c["feedback"][0]["show"], "focused");
        assert_eq!(
            c["feedback"][0]["when"]["is"],
            json!({ "op": "equals", "value": true })
        );
        assert_eq!((r.state_rules, r.dropped, r.kept), (1, 1, 0));
    }

    #[test]
    fn a_catch_all_idle_without_a_focus_rule_before_it_is_an_event_rule() {
        let (c, r) = converted(json!([{ "source": "agent-control", "rules": [
            { "event": "focus.changed", "when": [], "setState": "idle" }]}]));
        assert_eq!(c["feedback"][0]["when"]["event"]["id"], "focus.changed");
        assert_eq!((r.event_rules, r.dropped), (1, 0));
    }

    #[test]
    fn a_command_binding_becomes_a_state_rule_on_the_command() {
        let (c, _) = converted(json!([{ "source": "agent-control", "rules": [
            { "event": "command.state", "when": [{ "field": "command", "equals": "send_instruction" }],
              "setState": "$payload.state" }]}]));
        assert_eq!(
            c["feedback"][0]["when"]["state"]["args"],
            json!({ "command": "send_instruction" })
        );
    }

    #[test]
    fn an_unknown_command_or_slot_stays_an_event_rule() {
        let (c, r) = converted(json!([{ "source": "agent-control", "rules": [
            { "event": "command.state", "when": [{ "field": "command", "equals": "dance" }],
              "setState": "$payload.state" },
            { "event": "slot.changed", "when": [{ "field": "slot", "equals": 99 }],
              "setState": "$payload.status" }]}]));
        // Nothing is guessed: they are payload tests like any other, and an
        // unknown command is one that never matches.
        assert_eq!(r.event_rules, 2);
        assert!(c.get("legacyBindings").is_none());
    }

    #[test]
    fn a_demo_mirror_becomes_a_state_rule_and_other_demo_bindings_stay_events() {
        let (c, r) = converted(json!([
            { "source": "demo", "rules": [
                { "event": "plugin.state.changed", "when": [{ "field": "key", "equals": "alpha" }],
                  "setState": "$payload.value" }]},
            { "source": "demo", "rules": [
                { "event": "plugin.state.changed",
                  "when": [{ "field": "value", "equals": "approval-required" }],
                  "setState": "pending" }]}
        ]));
        assert_eq!(c["feedback"][0]["when"]["state"]["id"], "alpha");
        assert_eq!(
            c["feedback"][1]["when"]["event"]["id"],
            "plugin.state.changed"
        );
        assert_eq!(c["feedback"][1]["hold"], "latch");
        assert_eq!((r.state_rules, r.event_rules), (1, 1));
    }

    #[test]
    fn other_sources_become_latching_event_rules_in_order() {
        let (c, r) = converted(json!([{ "source": "ci", "rules": [
            { "event": "build.finished", "when": [{ "field": "ok", "equals": false }],
              "setState": "failed" },
            { "event": "build.finished", "when": [], "setState": "$payload.result" }]}]));
        let rules = c["feedback"].as_array().unwrap();
        assert_eq!(rules.len(), 2);
        assert_eq!(rules[0]["show"], "failed");
        assert_eq!(
            rules[0]["when"]["where"],
            json!([{ "field": "ok", "equals": false }])
        );
        assert_eq!(rules[1]["showFromField"], "result");
        assert!(rules.iter().all(|r| r["hold"] == "latch"));
        assert_eq!(r.event_rules, 2);
    }

    #[test]
    fn what_cannot_be_expressed_is_kept_word_for_word() {
        let odd = json!({ "event": "x", "when": [{ "field": "a", "equals": [1, 2] }],
                          "setState": "y" });
        let (c, r) = converted(json!([
            { "source": "ci", "rules": [odd.clone()] },
            { "source": "ci" },
            "not even an object"
        ]));
        assert!(c.get("feedback").is_none());
        assert_eq!(
            c["legacyBindings"],
            json!([
                { "source": "ci", "rules": [odd] },
                { "source": "ci" },
                "not even an object"
            ])
        );
        assert_eq!(r.kept, 3);
    }

    #[test]
    fn a_control_without_bindings_is_untouched_and_the_step_is_idempotent() {
        let mut doc = json!({ "surfaces": [{ "id": "s", "pages": [{ "id": "p", "controls": [
            { "id": "a" }, control(json!([{ "source": "ci", "rules": [
                { "event": "e", "when": [], "setState": "go" }]}])) ]}]}]});
        upgrade_bindings(&mut doc);
        let once = doc.clone();
        let again = upgrade_bindings(&mut doc);
        assert_eq!(doc, once);
        assert_eq!(again, Report::default());
        assert_eq!(
            doc["surfaces"][0]["pages"][0]["controls"][0],
            json!({ "id": "a" })
        );
    }

    #[test]
    fn more_rules_than_a_control_may_carry_overflow_into_legacy() {
        let rules: Vec<Value> = (0..MAX_RULES + 3)
            .map(|i| json!({ "event": format!("e{i}"), "when": [], "setState": "go" }))
            .collect();
        let (c, r) = converted(json!([{ "source": "ci", "rules": rules }]));
        assert_eq!(c["feedback"].as_array().unwrap().len(), MAX_RULES);
        assert_eq!(c["legacyBindings"].as_array().unwrap().len(), 3);
        assert_eq!((r.event_rules, r.kept), (MAX_RULES, 3));
    }
}
