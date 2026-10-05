//! Feedback rules: what a control should look like, written as data.
//!
//! A control keeps one ordered list of rules. There are two kinds:
//!
//! * a **state rule** is continuous: while a state holds a given value, show
//!   this look. It is evaluated from the hub, so it is right the moment a
//!   control exists, whether or not any event is happening;
//! * an **event rule** is momentary: when this event arrives, show this look
//!   for a while ([`Hold`]) and then return to what the state rules say.
//!
//! The first state rule that is true decides; the first event rule that
//! matches starts the overlay. There are no priorities to maintain: the order
//! a person sees is the order that is saved. Rules choose a look by name from
//! the control's own looks; they never carry a colour.
//!
//! ```jsonc
//! { "when": { "state": { "provider": "example.counter", "id": "level" },
//!             "is":    { "op": "equals", "value": "high" } },
//!   "show": "warning" }
//!
//! { "when": { "state": { "provider": "agent-control", "id": "slot.status",
//!                        "args": { "slot": 2 } },
//!             "is":    { "op": "hasValue" } },
//!   "showFromValue": true }
//!
//! { "when": { "event": { "provider": "ci", "id": "build.finished" },
//!             "where": [ { "field": "result", "equals": "failed" } ] },
//!   "show": "failed", "hold": { "ms": 4000 } }
//! ```

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use super::address::StateAddress;
use super::definition::{ObservableDefinition, ValueType};
use super::hub::{Observed, Status};
use super::value::Scalar;
use crate::actions::params::normalize;

pub const MAX_RULES: usize = 32;
pub const MAX_LOOK_NAME: usize = 40;
pub const MAX_WHERE: usize = 8;
pub const DEFAULT_HOLD_MS: u32 = 3_000;
pub const MIN_HOLD_MS: u32 = 200;
pub const MAX_HOLD_MS: u32 = 60_000;
/// The look name that always means "the resting look", unless the control
/// defines a look of that name itself.
pub const IDLE: &str = "idle";

/// The test of a state rule. Only a `known` value can pass a comparison; an
/// `unknown` or `unavailable` state is never "not equal" to anything.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "camelCase")]
pub enum Test {
    Equals {
        value: Scalar,
    },
    NotEquals {
        value: Scalar,
    },
    LessThan {
        value: f64,
    },
    AtMost {
        value: f64,
    },
    GreaterThan {
        value: f64,
    },
    AtLeast {
        value: f64,
    },
    /// The state is `known`.
    HasValue,
    /// The state is `unknown`: it exists and nothing has reported yet.
    NoValue,
    /// The state is `unavailable`: its provider is gone or stopped.
    Unavailable,
}

impl Test {
    /// Does this observation satisfy the test? A pure function.
    pub fn holds(&self, o: &Observed) -> bool {
        let known = o.status == Status::Known;
        let value = o.value.as_ref();
        match self {
            Test::HasValue => known,
            Test::NoValue => o.status == Status::Unknown,
            Test::Unavailable => o.status == Status::Unavailable,
            Test::Equals { value: want } => known && value.is_some_and(|v| v.same_as(want)),
            // Different from `!equals`: a state with no value is not "different".
            Test::NotEquals { value: want } => known && value.is_some_and(|v| !v.same_as(want)),
            Test::LessThan { value: n } => known && number(value).is_some_and(|v| v < *n),
            Test::AtMost { value: n } => known && number(value).is_some_and(|v| v <= *n),
            Test::GreaterThan { value: n } => known && number(value).is_some_and(|v| v > *n),
            Test::AtLeast { value: n } => known && number(value).is_some_and(|v| v >= *n),
        }
    }

    fn is_numeric(&self) -> bool {
        matches!(
            self,
            Test::LessThan { .. }
                | Test::AtMost { .. }
                | Test::GreaterThan { .. }
                | Test::AtLeast { .. }
        )
    }
}

fn number(v: Option<&Scalar>) -> Option<f64> {
    v.and_then(Scalar::as_number)
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StateCondition {
    pub state: StateAddress,
    pub is: Test,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EventRef {
    /// The event's source: a plugin id, `agent-control`, `demo`, `ci`...
    pub provider: String,
    /// The event type: `build.finished`, `slot.changed`...
    pub id: String,
}

/// One payload field that must equal a value for an event rule to match.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FieldTest {
    pub field: String,
    pub equals: Scalar,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EventCondition {
    pub event: EventRef,
    #[serde(default, rename = "where", skip_serializing_if = "Vec::is_empty")]
    pub where_: Vec<FieldTest>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum When {
    State(StateCondition),
    Event(EventCondition),
}

/// How long an event's look lasts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hold {
    /// For this many milliseconds, then back to what the state rules say.
    For(u32),
    /// Until another event rule on the same control replaces it, the control
    /// is edited, or the app restarts. The one place a stale look is chosen
    /// on purpose; migrated bindings use it so they behave as they did.
    Latch,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum HoldRepr {
    Word(String),
    Timed { ms: u32 },
}

impl Serialize for Hold {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        match self {
            Hold::Latch => s.serialize_str("latch"),
            Hold::For(ms) => {
                use serde::ser::SerializeMap;
                let mut m = s.serialize_map(Some(1))?;
                m.serialize_entry("ms", ms)?;
                m.end()
            }
        }
    }
}

impl<'de> Deserialize<'de> for Hold {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        match HoldRepr::deserialize(d)? {
            HoldRepr::Word(w) if w == "latch" => Ok(Hold::Latch),
            HoldRepr::Word(w) => Err(serde::de::Error::custom(format!(
                "`{w}` is not a hold; use {{\"ms\": 3000}} or \"latch\""
            ))),
            HoldRepr::Timed { ms } => Ok(Hold::For(ms)),
        }
    }
}

fn is_false(b: &bool) -> bool {
    !*b
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FeedbackRule {
    pub when: When,
    /// Show the look with this name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub show: Option<String>,
    /// State rules: show the look named after the state's value.
    #[serde(default, skip_serializing_if = "is_false")]
    pub show_from_value: bool,
    /// Event rules: show the look named after this payload field.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub show_from_field: Option<String>,
    /// Event rules only. Defaults to [`DEFAULT_HOLD_MS`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hold: Option<Hold>,
}

impl FeedbackRule {
    pub fn state(address: StateAddress, is: Test) -> Self {
        Self {
            when: When::State(StateCondition { state: address, is }),
            show: None,
            show_from_value: false,
            show_from_field: None,
            hold: None,
        }
    }

    pub fn event(provider: &str, id: &str) -> Self {
        Self {
            when: When::Event(EventCondition {
                event: EventRef {
                    provider: provider.to_string(),
                    id: id.to_string(),
                },
                where_: Vec::new(),
            }),
            show: None,
            show_from_value: false,
            show_from_field: None,
            hold: None,
        }
    }

    pub fn show(mut self, look: &str) -> Self {
        self.show = Some(look.to_string());
        self
    }

    pub fn show_from_value(mut self) -> Self {
        self.show_from_value = true;
        self
    }

    pub fn show_from_field(mut self, field: &str) -> Self {
        self.show_from_field = Some(field.to_string());
        self
    }

    pub fn hold(mut self, hold: Hold) -> Self {
        self.hold = Some(hold);
        self
    }

    pub fn where_field(mut self, field: &str, equals: impl Into<Scalar>) -> Self {
        if let When::Event(e) = &mut self.when {
            e.where_.push(FieldTest {
                field: field.to_string(),
                equals: equals.into(),
            });
        }
        self
    }

    pub fn is_state(&self) -> bool {
        matches!(self.when, When::State(_))
    }

    pub fn is_event(&self) -> bool {
        matches!(self.when, When::Event(_))
    }

    /// The look this rule names for a state with `value`, if it can name one.
    pub fn look_for_state(&self, value: Option<&Scalar>) -> Option<String> {
        if let Some(name) = &self.show {
            return Some(name.clone());
        }
        if self.show_from_value {
            return value.map(Scalar::look_name);
        }
        None
    }

    pub fn hold_or_default(&self) -> Hold {
        self.hold.unwrap_or(Hold::For(DEFAULT_HOLD_MS))
    }
}

// ---------------------------------------------------------------------------
// The gate: checking rules before they are saved
// ---------------------------------------------------------------------------

/// Which part of a rule a [`RuleIssue`] is about, so an editor can put the
/// message next to the input that causes it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum IssueField {
    /// Which look the rule shows, and the look's name.
    Look,
    /// The state a rule reads: its address, or nothing offers it.
    State,
    /// The state's arguments (the scope). See [`RuleIssue::param`].
    Args,
    /// The operator of a state rule.
    Test,
    /// The value a state rule compares with.
    Value,
    /// The event an event rule waits for.
    Event,
    /// The payload conditions of an event rule, and the field it shows a look from.
    Where,
    /// How long an event's look lasts.
    Hold,
    /// The list as a whole.
    Rules,
}

/// One problem found in one rule (0-based position in the list).
///
/// `field` and `param` only say where the problem is; the message is the same
/// with or without them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuleIssue {
    pub rule: usize,
    pub message: String,
    /// The part of the rule the problem is about.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub field: Option<IssueField>,
    /// For `args`: the name of the argument, when the problem is about one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub param: Option<String>,
}

fn issue(rule: usize, field: IssueField, message: impl Into<String>) -> RuleIssue {
    RuleIssue {
        rule,
        message: message.into(),
        field: Some(field),
        param: None,
    }
}

/// The limits the gate enforces, so an editor shows them without copying them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FeedbackLimits {
    /// Rules in one control.
    pub max_rules: usize,
    /// Characters in a look name.
    pub max_look_name: usize,
    /// Payload conditions on one event rule.
    pub max_where: usize,
    /// How long an event's look lasts when the rule does not say.
    pub default_hold_ms: u32,
    pub min_hold_ms: u32,
    pub max_hold_ms: u32,
}

impl FeedbackLimits {
    pub const fn new() -> Self {
        Self {
            max_rules: MAX_RULES,
            max_look_name: MAX_LOOK_NAME,
            max_where: MAX_WHERE,
            default_hold_ms: DEFAULT_HOLD_MS,
            min_hold_ms: MIN_HOLD_MS,
            max_hold_ms: MAX_HOLD_MS,
        }
    }
}

impl Default for FeedbackLimits {
    fn default() -> Self {
        Self::new()
    }
}

fn plain(s: &str, max: usize) -> bool {
    let t = s.trim();
    !t.is_empty() && s.chars().count() <= max && !s.chars().any(char::is_control)
}

/// Check a control's rules and return them in the form to store: state
/// arguments checked against the definition and completed with their defaults.
///
/// Only a rule that is new or changed is checked. A rule that is already saved
/// stays as it is even when its provider is gone, so a pad whose plugin was
/// uninstalled can still be renamed and recoloured, exactly as with a saved
/// action.
pub fn prepare_rules(
    rules: &[FeedbackRule],
    saved: &[FeedbackRule],
    state_def: &dyn Fn(&str, &str) -> Option<ObservableDefinition>,
) -> Result<Vec<FeedbackRule>, Vec<RuleIssue>> {
    let mut issues = Vec::new();
    if rules.len() > MAX_RULES {
        issues.push(issue(
            MAX_RULES,
            IssueField::Rules,
            format!("a control has at most {MAX_RULES} feedback rules"),
        ));
    }
    let mut out = Vec::with_capacity(rules.len());
    for (i, rule) in rules.iter().enumerate() {
        if saved.contains(rule) {
            out.push(rule.clone());
            continue;
        }
        match prepare_rule(i, rule, state_def) {
            Ok(r) => out.push(r),
            Err(mut found) => issues.append(&mut found),
        }
    }
    if issues.is_empty() {
        Ok(out)
    } else {
        Err(issues)
    }
}

/// The checks of one rule that need no provider: which look and its name,
/// the kind of rule against what it shows, the hold, the event, its payload
/// conditions and field names, and the state address's own grammar. The gate
/// runs them first; a portable file runs them where no provider is installed.
pub fn check_rule_shape(i: usize, rule: &FeedbackRule) -> Vec<RuleIssue> {
    let mut issues = Vec::new();

    // Which look.
    let ways = usize::from(rule.show.is_some())
        + usize::from(rule.show_from_value)
        + usize::from(rule.show_from_field.is_some());
    if ways != 1 {
        issues.push(issue(
            i,
            IssueField::Look,
            "choose exactly one look to show",
        ));
    }
    if rule
        .show
        .as_deref()
        .is_some_and(|s| !plain(s, MAX_LOOK_NAME))
    {
        issues.push(issue(
            i,
            IssueField::Look,
            format!("a look name has 1-{MAX_LOOK_NAME} characters"),
        ));
    }
    match &rule.when {
        When::State(c) => {
            if rule.show_from_field.is_some() {
                issues.push(issue(
                    i,
                    IssueField::Look,
                    "a state rule cannot show a look from an event field",
                ));
            }
            if rule.hold.is_some() {
                issues.push(issue(i, IssueField::Hold, "only an event rule has a hold"));
            }
            if rule.show_from_value && matches!(c.is, Test::NoValue | Test::Unavailable) {
                issues.push(issue(
                    i,
                    IssueField::Look,
                    "a look from the value needs a test that has a value, not \"no value yet\" or \"not available\"",
                ));
            }
            if let Err(e) = c.state.validate() {
                issues.push(issue(i, IssueField::State, e.to_string()));
            }
        }
        When::Event(c) => {
            if rule.show_from_value {
                issues.push(issue(
                    i,
                    IssueField::Look,
                    "an event rule cannot show a look from a state value",
                ));
            }
            if !plain(&c.event.provider, 64) || !plain(&c.event.id, 64) {
                issues.push(issue(
                    i,
                    IssueField::Event,
                    "an event is a source and a type of 1-64 characters",
                ));
            }
            if c.where_.len() > MAX_WHERE {
                issues.push(issue(
                    i,
                    IssueField::Where,
                    format!("at most {MAX_WHERE} conditions on an event"),
                ));
            }
            if c.where_.iter().any(|w| !plain(&w.field, 64)) {
                issues.push(issue(
                    i,
                    IssueField::Where,
                    "a payload field name has 1-64 characters",
                ));
            }
            if rule
                .show_from_field
                .as_deref()
                .is_some_and(|f| !plain(f, 64))
            {
                issues.push(issue(
                    i,
                    IssueField::Where,
                    "a payload field name has 1-64 characters",
                ));
            }
            if let Some(Hold::For(ms)) = rule.hold {
                if !(MIN_HOLD_MS..=MAX_HOLD_MS).contains(&ms) {
                    issues.push(issue(
                        i,
                        IssueField::Hold,
                        format!("a hold lasts {MIN_HOLD_MS}-{MAX_HOLD_MS} ms, or latch"),
                    ));
                }
            }
        }
    }
    issues
}

fn prepare_rule(
    i: usize,
    rule: &FeedbackRule,
    state_def: &dyn Fn(&str, &str) -> Option<ObservableDefinition>,
) -> Result<FeedbackRule, Vec<RuleIssue>> {
    let mut issues = check_rule_shape(i, rule);
    let mut rule = rule.clone();
    if let When::State(c) = &mut rule.when {
        if c.state.validate().is_ok() {
            check_state(i, c, state_def, &mut issues);
        }
    }
    if issues.is_empty() {
        Ok(rule)
    } else {
        Err(issues)
    }
}

/// The checks of a state rule that need the state's definition: the state
/// exists, its arguments (completed with their defaults), and the test.
fn check_state(
    i: usize,
    c: &mut StateCondition,
    state_def: &dyn Fn(&str, &str) -> Option<ObservableDefinition>,
    issues: &mut Vec<RuleIssue>,
) {
    let Some(def) = state_def(&c.state.provider, &c.state.id) else {
        issues.push(issue(
            i,
            IssueField::State,
            format!("`{}` is not a state anything offers right now", c.state),
        ));
        return;
    };
    match normalize(&def.params, &c.state.args_json()) {
        Ok(args) => match StateAddress::from_args_json(&c.state.provider, &c.state.id, &args) {
            Ok(a) => c.state = a,
            Err(e) => issues.push(issue(i, IssueField::Args, e)),
        },
        Err(found) => {
            for f in found {
                issues.push(RuleIssue {
                    param: f.param,
                    ..issue(i, IssueField::Args, f.message)
                });
            }
        }
    }
    if let Some(spec) = &def.value {
        check_test(i, &c.is, spec.kind, &def, issues);
    }
}

fn check_test(
    i: usize,
    test: &Test,
    kind: ValueType,
    def: &ObservableDefinition,
    issues: &mut Vec<RuleIssue>,
) {
    if test.is_numeric() && kind != ValueType::Number {
        issues.push(issue(
            i,
            IssueField::Test,
            format!(
                "`{}` is not a number, so it cannot be compared with below, above or at",
                def.name
            ),
        ));
        return;
    }
    if let Test::Equals { value } | Test::NotEquals { value } = test {
        let right = matches!(
            (kind, value),
            (ValueType::Enum | ValueType::String, Scalar::Str(_))
                | (ValueType::Boolean, Scalar::Bool(_))
                | (ValueType::Number, Scalar::Num(_))
        );
        if !right {
            issues.push(issue(
                i,
                IssueField::Value,
                format!("`{}` cannot be compared with {}", def.name, value.kind()),
            ));
        } else if kind == ValueType::Enum {
            let listed = def
                .value
                .as_ref()
                .is_some_and(|s| s.values.iter().any(|o| o.value.same_as(value)));
            if !listed {
                issues.push(issue(
                    i,
                    IssueField::Value,
                    format!("`{value}` is not one of the values of `{}`", def.name),
                ));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::actions::params::ParamDef;
    use crate::state::definition::Role;
    use serde_json::json;

    fn observed(status: Status, v: Option<Scalar>) -> Observed {
        Observed {
            status,
            value: v,
            updated_at: None,
        }
    }

    #[test]
    fn rules_read_and_write_the_documented_json() {
        let state: FeedbackRule = serde_json::from_value(json!({
            "when": { "state": { "provider": "example.counter", "id": "level" },
                      "is": { "op": "equals", "value": "high" } },
            "show": "warning"
        }))
        .unwrap();
        assert!(state.is_state());
        assert_eq!(state.show.as_deref(), Some("warning"));
        let event: FeedbackRule = serde_json::from_value(json!({
            "when": { "event": { "provider": "ci", "id": "build.finished" },
                      "where": [ { "field": "result", "equals": "failed" } ] },
            "show": "failed", "hold": { "ms": 4000 }
        }))
        .unwrap();
        assert!(event.is_event());
        assert_eq!(event.hold, Some(Hold::For(4000)));
        let latch: FeedbackRule = serde_json::from_value(json!({
            "when": { "event": { "provider": "ci", "id": "x" } },
            "showFromField": "status", "hold": "latch"
        }))
        .unwrap();
        assert_eq!(latch.hold, Some(Hold::Latch));
        for r in [state, event, latch] {
            let v = serde_json::to_value(&r).unwrap();
            assert_eq!(serde_json::from_value::<FeedbackRule>(v).unwrap(), r);
        }
    }

    #[test]
    fn unknown_fields_and_bad_holds_do_not_parse() {
        for bad in [
            json!({ "when": { "state": { "provider": "a", "id": "b" }, "is": { "op": "explodes" } }, "show": "x" }),
            json!({ "when": { "state": { "provider": "a", "id": "b" }, "is": { "op": "hasValue" } }, "show": "x", "colour": "red" }),
            json!({ "when": { "event": { "provider": "a", "id": "b" } }, "show": "x", "hold": "forever" }),
            json!({ "when": {}, "show": "x" }),
        ] {
            assert!(
                serde_json::from_value::<FeedbackRule>(bad.clone()).is_err(),
                "{bad}"
            );
        }
    }

    /// Every operator against every status, and the types that matter.
    #[test]
    fn operators_against_statuses_and_types() {
        let known = |v: Scalar| observed(Status::Known, Some(v));
        let unknown = observed(Status::Unknown, None);
        let gone = observed(Status::Unavailable, None);
        let eq = |v: Scalar| Test::Equals { value: v };
        let ne = |v: Scalar| Test::NotEquals { value: v };

        // equals: typed, numeric-normalised.
        assert!(eq(Scalar::text("high")).holds(&known(Scalar::text("high"))));
        assert!(!eq(Scalar::text("high")).holds(&known(Scalar::text("low"))));
        assert!(eq(Scalar::Num(1.0)).holds(&known(Scalar::from_json(&json!(1)).unwrap())));
        assert!(eq(Scalar::from_json(&json!(1)).unwrap()).holds(&known(Scalar::Num(1.0))));
        assert!(!eq(Scalar::text("1")).holds(&known(Scalar::Num(1.0))));
        assert!(!eq(Scalar::Num(1.0)).holds(&known(Scalar::text("1"))));
        assert!(eq(Scalar::Bool(true)).holds(&known(Scalar::Bool(true))));
        assert!(!eq(Scalar::Bool(true)).holds(&known(Scalar::Num(1.0))));
        for t in [eq(Scalar::text("x")), ne(Scalar::text("x"))] {
            assert!(!t.holds(&unknown) && !t.holds(&gone));
        }

        // notEquals is false for unknown and unavailable, true for a known different value.
        assert!(ne(Scalar::text("high")).holds(&known(Scalar::text("low"))));
        assert!(!ne(Scalar::text("high")).holds(&known(Scalar::text("high"))));
        assert!(
            ne(Scalar::text("1")).holds(&known(Scalar::Num(1.0))),
            "different types differ"
        );

        // numeric comparisons.
        let n = |v: f64| known(Scalar::Num(v));
        let cases: [(Test, f64, bool); 12] = [
            (Test::LessThan { value: 21.0 }, 20.0, true),
            (Test::LessThan { value: 21.0 }, 21.0, false),
            (Test::AtMost { value: 21.0 }, 21.0, true),
            (Test::AtMost { value: 21.0 }, 21.5, false),
            (Test::GreaterThan { value: 0.0 }, 0.0, false),
            (Test::GreaterThan { value: 0.0 }, 0.1, true),
            (Test::AtLeast { value: 5.0 }, 5.0, true),
            (Test::AtLeast { value: 5.0 }, 4.99, false),
            (Test::AtLeast { value: 5.0 }, 5.0000001, true),
            (Test::LessThan { value: 1.0 }, -3.0, true),
            (Test::AtMost { value: -1.0 }, -1.0, true),
            (Test::GreaterThan { value: -1.0 }, -1.0, false),
        ];
        for (t, v, want) in cases {
            assert_eq!(t.holds(&n(v)), want, "{t:?} on {v}");
        }
        for t in [
            Test::LessThan { value: 1.0 },
            Test::AtMost { value: 1.0 },
            Test::GreaterThan { value: 1.0 },
            Test::AtLeast { value: 1.0 },
        ] {
            assert!(!t.holds(&unknown) && !t.holds(&gone), "{t:?}");
            assert!(!t.holds(&known(Scalar::text("0"))), "{t:?} on a text");
        }

        // presence tests.
        assert!(
            Test::HasValue.holds(&n(0.0))
                && !Test::HasValue.holds(&unknown)
                && !Test::HasValue.holds(&gone)
        );
        assert!(
            Test::NoValue.holds(&unknown)
                && !Test::NoValue.holds(&n(0.0))
                && !Test::NoValue.holds(&gone)
        );
        assert!(
            Test::Unavailable.holds(&gone)
                && !Test::Unavailable.holds(&unknown)
                && !Test::Unavailable.holds(&n(0.0))
        );
    }

    #[test]
    fn a_rule_names_its_look() {
        let fixed = FeedbackRule::state(StateAddress::new("a.b", "s"), Test::HasValue).show("x");
        assert_eq!(
            fixed.look_for_state(Some(&Scalar::text("v"))),
            Some("x".into())
        );
        let mirror =
            FeedbackRule::state(StateAddress::new("a.b", "s"), Test::HasValue).show_from_value();
        assert_eq!(
            mirror.look_for_state(Some(&Scalar::text("running"))),
            Some("running".into())
        );
        assert_eq!(
            mirror.look_for_state(Some(&Scalar::Bool(true))),
            Some("true".into())
        );
        assert_eq!(mirror.look_for_state(None), None);
    }

    // -- the gate ------------------------------------------------------------

    fn defs(provider: &str, id: &str) -> Option<ObservableDefinition> {
        match (provider, id) {
            ("example.counter", "level") => Some(
                ObservableDefinition::state("example.counter", "level", "Counter level", "")
                    .one_of(&[
                        ("zero", "Empty", Some(Role::Inactive)),
                        ("high", "High", None),
                    ]),
            ),
            ("example.counter", "value") => Some(
                ObservableDefinition::state("example.counter", "value", "Counter value", "")
                    .number(),
            ),
            ("agent-control", "slot.status") => Some(
                ObservableDefinition::state("agent-control", "slot.status", "Track", "")
                    .param(
                        ParamDef::number("slot", "Track")
                            .range(0.0, 7.0)
                            .integer()
                            .required(),
                    )
                    .one_of(&[("idle", "Idle", None)]),
            ),
            _ => None,
        }
    }

    fn level_rule(op: Test) -> FeedbackRule {
        FeedbackRule::state(StateAddress::new("example.counter", "level"), op).show("warning")
    }

    fn messages(r: Result<Vec<FeedbackRule>, Vec<RuleIssue>>) -> Vec<String> {
        r.unwrap_err().into_iter().map(|i| i.message).collect()
    }

    #[test]
    fn good_rules_pass_and_arguments_are_completed() {
        let ok = vec![
            level_rule(Test::Equals {
                value: Scalar::text("high"),
            }),
            FeedbackRule::state(
                StateAddress::new("agent-control", "slot.status").with("slot", 2.0),
                Test::HasValue,
            )
            .show_from_value(),
            FeedbackRule::event("ci", "build.finished")
                .where_field("result", "failed")
                .show("failed")
                .hold(Hold::For(4000)),
        ];
        let out = prepare_rules(&ok, &[], &defs).unwrap();
        assert_eq!(out.len(), 3);
        let When::State(c) = &out[1].when else {
            panic!()
        };
        assert_eq!(c.state.args.get("slot"), Some(&Scalar::Num(2.0)));
    }

    #[test]
    fn exactly_one_look_and_the_right_kind_of_rule() {
        let none = FeedbackRule::state(
            StateAddress::new("example.counter", "level"),
            Test::HasValue,
        );
        assert_eq!(
            messages(prepare_rules(&[none], &[], &defs)),
            vec!["choose exactly one look to show"]
        );
        let both = level_rule(Test::HasValue).show_from_value();
        assert_eq!(
            messages(prepare_rules(&[both], &[], &defs)),
            vec!["choose exactly one look to show"]
        );
        let hold_on_state = level_rule(Test::HasValue).hold(Hold::Latch);
        assert!(
            messages(prepare_rules(&[hold_on_state], &[], &defs))[0].contains("only an event rule")
        );
        let from_field = FeedbackRule::state(
            StateAddress::new("example.counter", "level"),
            Test::HasValue,
        )
        .show_from_field("x");
        assert!(prepare_rules(&[from_field], &[], &defs).is_err());
        let from_value_on_event = FeedbackRule::event("ci", "x").show_from_value();
        assert!(prepare_rules(&[from_value_on_event], &[], &defs).is_err());
    }

    #[test]
    fn operators_must_fit_the_type_of_the_state() {
        let below = level_rule(Test::LessThan { value: 3.0 });
        assert!(messages(prepare_rules(&[below], &[], &defs))[0].contains("not a number"));
        let wrong = level_rule(Test::Equals {
            value: Scalar::Num(3.0),
        });
        assert!(messages(prepare_rules(&[wrong], &[], &defs))[0].contains("cannot be compared"));
        let unlisted = level_rule(Test::Equals {
            value: Scalar::text("overflowing"),
        });
        assert!(
            messages(prepare_rules(&[unlisted], &[], &defs))[0].contains("not one of the values")
        );
        let value = FeedbackRule::state(
            StateAddress::new("example.counter", "value"),
            Test::AtLeast { value: 5.0 },
        )
        .show("warning");
        assert!(prepare_rules(&[value], &[], &defs).is_ok());
        let value = FeedbackRule::state(
            StateAddress::new("example.counter", "value"),
            Test::Equals {
                value: Scalar::text("5"),
            },
        )
        .show("warning");
        assert!(prepare_rules(&[value], &[], &defs).is_err());
    }

    #[test]
    fn states_and_arguments_must_exist() {
        let missing =
            FeedbackRule::state(StateAddress::new("gone.plugin", "x"), Test::HasValue).show("a");
        assert!(messages(prepare_rules(&[missing], &[], &defs))[0]
            .contains("not a state anything offers"));
        let no_slot = FeedbackRule::state(
            StateAddress::new("agent-control", "slot.status"),
            Test::HasValue,
        )
        .show_from_value();
        assert!(prepare_rules(&[no_slot], &[], &defs).is_err());
        let slot9 = FeedbackRule::state(
            StateAddress::new("agent-control", "slot.status").with("slot", 9i64),
            Test::HasValue,
        )
        .show_from_value();
        assert!(prepare_rules(&[slot9], &[], &defs).is_err());
        let bad_id =
            FeedbackRule::state(StateAddress::new("Example", "x"), Test::HasValue).show("a");
        assert!(prepare_rules(&[bad_id], &[], &defs).is_err());
    }

    #[test]
    fn events_have_bounds() {
        let ok = |hold| FeedbackRule::event("ci", "done").show("ok").hold(hold);
        assert!(prepare_rules(&[ok(Hold::For(200))], &[], &defs).is_ok());
        assert!(prepare_rules(&[ok(Hold::For(60_000))], &[], &defs).is_ok());
        assert!(prepare_rules(&[ok(Hold::Latch)], &[], &defs).is_ok());
        assert!(prepare_rules(&[ok(Hold::For(199))], &[], &defs).is_err());
        assert!(prepare_rules(&[ok(Hold::For(60_001))], &[], &defs).is_err());
        let mut many = FeedbackRule::event("ci", "done").show("ok");
        for n in 0..=MAX_WHERE {
            many = many.where_field(&format!("f{n}"), "x");
        }
        assert!(prepare_rules(&[many], &[], &defs).is_err());
        let long_look = FeedbackRule::event("ci", "done").show(&"x".repeat(41));
        assert!(prepare_rules(&[long_look], &[], &defs).is_err());
    }

    #[test]
    fn a_look_from_the_value_needs_a_value() {
        let addr = || StateAddress::new("example.counter", "level");
        for test in [Test::NoValue, Test::Unavailable] {
            let r = FeedbackRule::state(addr(), test).show_from_value();
            assert!(messages(prepare_rules(&[r], &[], &defs))[0]
                .contains("needs a test that has a value"));
        }
        for test in [
            Test::HasValue,
            Test::NotEquals {
                value: Scalar::text("zero"),
            },
        ] {
            let r = FeedbackRule::state(addr(), test).show_from_value();
            assert!(prepare_rules(&[r], &[], &defs).is_ok());
        }
    }

    #[test]
    fn there_is_a_limit_on_the_number_of_rules() {
        let rule = level_rule(Test::HasValue);
        let rules = vec![rule; MAX_RULES + 1];
        assert!(prepare_rules(&rules, &[], &defs).is_err());
        let rules = vec![level_rule(Test::HasValue); MAX_RULES];
        assert!(prepare_rules(&rules, &[], &defs).is_ok());
    }

    /// Where each problem is, for one rule that has them.
    fn where_is(rules: &[FeedbackRule]) -> Vec<(usize, Option<IssueField>, Option<String>)> {
        prepare_rules(rules, &[], &defs)
            .unwrap_err()
            .into_iter()
            .map(|i| (i.rule, i.field, i.param))
            .collect()
    }

    fn only_field(rule: FeedbackRule) -> Option<IssueField> {
        let found = where_is(&[rule]);
        assert_eq!(found.len(), 1, "{found:?}");
        found[0].1
    }

    #[test]
    fn every_issue_says_which_part_of_the_rule_it_is_about() {
        use IssueField as F;
        let addr = || StateAddress::new("example.counter", "level");
        let unnamed = FeedbackRule::state(addr(), Test::HasValue);
        // The look.
        assert_eq!(only_field(unnamed), Some(F::Look));
        assert_eq!(
            only_field(level_rule(Test::HasValue).show_from_value()),
            Some(F::Look)
        );
        assert_eq!(
            only_field(FeedbackRule::event("ci", "done").show(&"x".repeat(41))),
            Some(F::Look)
        );
        assert_eq!(
            only_field(FeedbackRule::state(addr(), Test::HasValue).show_from_field("x")),
            Some(F::Look)
        );
        assert_eq!(
            only_field(FeedbackRule::state(addr(), Test::NoValue).show_from_value()),
            Some(F::Look)
        );
        assert_eq!(
            only_field(FeedbackRule::event("ci", "done").show_from_value()),
            Some(F::Look)
        );
        // The state.
        assert_eq!(
            only_field(
                FeedbackRule::state(StateAddress::new("gone.plugin", "x"), Test::HasValue)
                    .show("a")
            ),
            Some(F::State)
        );
        assert_eq!(
            only_field(
                FeedbackRule::state(StateAddress::new("Example", "x"), Test::HasValue).show("a")
            ),
            Some(F::State)
        );
        // The test and the value.
        assert_eq!(
            only_field(level_rule(Test::LessThan { value: 3.0 })),
            Some(F::Test)
        );
        assert_eq!(
            only_field(level_rule(Test::Equals {
                value: Scalar::Num(3.0)
            })),
            Some(F::Value)
        );
        assert_eq!(
            only_field(level_rule(Test::NotEquals {
                value: Scalar::text("overflowing")
            })),
            Some(F::Value)
        );
        // Hold, on a state rule and out of range on an event rule.
        assert_eq!(
            only_field(level_rule(Test::HasValue).hold(Hold::Latch)),
            Some(F::Hold)
        );
        let hold = |ms| {
            FeedbackRule::event("ci", "done")
                .show("ok")
                .hold(Hold::For(ms))
        };
        assert_eq!(only_field(hold(100)), Some(F::Hold));
        assert_eq!(only_field(hold(60_001)), Some(F::Hold));
        // The event and its conditions.
        assert_eq!(
            only_field(FeedbackRule::event("", "done").show("ok")),
            Some(F::Event)
        );
        assert_eq!(
            only_field(FeedbackRule::event("ci", &"x".repeat(65)).show("ok")),
            Some(F::Event)
        );
        let mut many = FeedbackRule::event("ci", "done").show("ok");
        for n in 0..=MAX_WHERE {
            many = many.where_field(&format!("f{n}"), "x");
        }
        assert_eq!(only_field(many), Some(F::Where));
        assert_eq!(
            only_field(
                FeedbackRule::event("ci", "done")
                    .show("ok")
                    .where_field("", "x")
            ),
            Some(F::Where)
        );
        assert_eq!(
            only_field(FeedbackRule::event("ci", "done").show_from_field("")),
            Some(F::Where)
        );
    }

    #[test]
    fn an_argument_problem_names_the_argument() {
        let slot = |args: StateAddress| FeedbackRule::state(args, Test::HasValue).show_from_value();
        let status = || StateAddress::new("agent-control", "slot.status");
        // Missing, out of range, and one the state does not take.
        for (address, param) in [
            (status(), "slot"),
            (status().with("slot", 9i64), "slot"),
            (status().with("slot", 2.0).with("extra", "x"), "extra"),
        ] {
            let found = where_is(&[slot(address)]);
            assert_eq!(found.len(), 1, "{found:?}");
            assert_eq!(found[0].1, Some(IssueField::Args));
            assert_eq!(found[0].2.as_deref(), Some(param));
        }
        // Several at once, each in its own place.
        let both = where_is(&[
            slot(status().with("slot", 9i64)),
            slot(status()),
            level_rule(Test::LessThan { value: 1.0 }),
        ]);
        assert_eq!(
            both,
            vec![
                (0, Some(IssueField::Args), Some("slot".into())),
                (1, Some(IssueField::Args), Some("slot".into())),
                (2, Some(IssueField::Test), None),
            ]
        );
    }

    #[test]
    fn too_many_rules_is_about_the_list_and_sits_after_the_last_rule() {
        let rules = vec![level_rule(Test::HasValue); MAX_RULES + 1];
        let found = prepare_rules(&rules, &[], &defs).unwrap_err();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].rule, MAX_RULES);
        assert_eq!(found[0].field, Some(IssueField::Rules));
        assert_eq!(
            found[0].message,
            format!("a control has at most {MAX_RULES} feedback rules")
        );
    }

    #[test]
    fn an_issue_is_written_in_camel_case_without_what_it_does_not_have() {
        let issue = RuleIssue {
            rule: 3,
            message: "m".into(),
            field: Some(IssueField::Where),
            param: None,
        };
        assert_eq!(
            serde_json::to_value(&issue).unwrap(),
            json!({ "rule": 3, "message": "m", "field": "where" })
        );
        let with_param = RuleIssue {
            field: Some(IssueField::Args),
            param: Some("slot".into()),
            ..issue.clone()
        };
        assert_eq!(
            serde_json::to_value(&with_param).unwrap(),
            json!({ "rule": 3, "message": "m", "field": "args", "param": "slot" })
        );
        let bare = RuleIssue {
            field: None,
            ..issue
        };
        assert_eq!(
            serde_json::to_value(&bare).unwrap(),
            json!({ "rule": 3, "message": "m" })
        );
        for (field, word) in [
            (IssueField::Look, "look"),
            (IssueField::State, "state"),
            (IssueField::Args, "args"),
            (IssueField::Test, "test"),
            (IssueField::Value, "value"),
            (IssueField::Event, "event"),
            (IssueField::Where, "where"),
            (IssueField::Hold, "hold"),
            (IssueField::Rules, "rules"),
        ] {
            assert_eq!(serde_json::to_value(field).unwrap(), json!(word));
        }
    }

    #[test]
    fn the_limits_are_the_constants_of_the_gate() {
        let limits = FeedbackLimits::default();
        assert_eq!(limits, FeedbackLimits::new());
        assert_eq!(
            serde_json::to_value(limits).unwrap(),
            json!({
                "maxRules": MAX_RULES,
                "maxLookName": MAX_LOOK_NAME,
                "maxWhere": MAX_WHERE,
                "defaultHoldMs": DEFAULT_HOLD_MS,
                "minHoldMs": MIN_HOLD_MS,
                "maxHoldMs": MAX_HOLD_MS,
            })
        );
    }

    #[test]
    fn the_shape_check_does_not_ask_for_the_provider() {
        let unregistered =
            FeedbackRule::state(StateAddress::new("gone.plugin", "x"), Test::HasValue).show("a");
        assert_eq!(check_rule_shape(0, &unregistered), vec![]);
        assert!(prepare_rules(&[unregistered], &[], &defs).is_err());
        let event = FeedbackRule::event("acme.notes", "saved")
            .where_field("result", "ok")
            .show_from_field("status")
            .hold(Hold::Latch);
        assert_eq!(check_rule_shape(0, &event), vec![]);
    }

    #[test]
    fn the_shape_check_refuses_a_short_hold_and_too_many_conditions() {
        let short = FeedbackRule::event("ci", "done")
            .show("ok")
            .hold(Hold::For(10));
        let found = check_rule_shape(4, &short);
        assert_eq!(found.len(), 1, "{found:?}");
        assert_eq!((found[0].rule, found[0].field), (4, Some(IssueField::Hold)));
        let mut nine = FeedbackRule::event("ci", "done").show("ok");
        for n in 0..9 {
            nine = nine.where_field(&format!("f{n}"), "x");
        }
        let found = check_rule_shape(0, &nine);
        assert_eq!(found.len(), 1, "{found:?}");
        assert_eq!(found[0].field, Some(IssueField::Where));
        let bad_address =
            FeedbackRule::state(StateAddress::new("Example", "x"), Test::HasValue).show("a");
        assert_eq!(
            check_rule_shape(0, &bad_address)[0].field,
            Some(IssueField::State)
        );
    }

    /// The shape check is the first part of the gate: for a rule whose state
    /// exists, the gate finds exactly what the shape check finds.
    #[test]
    fn the_gate_starts_with_the_shape_check() {
        let addr = || StateAddress::new("example.counter", "level");
        let rules = [
            FeedbackRule::state(addr(), Test::HasValue),
            FeedbackRule::state(addr(), Test::NoValue).show_from_value(),
            FeedbackRule::state(addr(), Test::HasValue)
                .show("a")
                .hold(Hold::Latch),
            FeedbackRule::event("", "done").show_from_value(),
            FeedbackRule::event("ci", "done")
                .show_from_field("")
                .hold(Hold::For(70_000)),
        ];
        for (i, rule) in rules.iter().enumerate() {
            let shape = check_rule_shape(i, rule);
            assert!(!shape.is_empty(), "{rule:?}");
            let gate = prepare_rules(std::slice::from_ref(rule), &[], &defs).unwrap_err();
            let gate: Vec<RuleIssue> = gate
                .into_iter()
                .map(|g| RuleIssue { rule: i, ..g })
                .collect();
            assert_eq!(gate, shape, "{rule:?}");
        }
    }

    #[test]
    fn a_saved_rule_survives_its_provider_and_only_changes_are_checked() {
        let orphan =
            FeedbackRule::state(StateAddress::new("gone.plugin", "x"), Test::HasValue).show("a");
        // Unchanged: kept, even though nothing offers the state any more.
        let kept = prepare_rules(
            std::slice::from_ref(&orphan),
            std::slice::from_ref(&orphan),
            &defs,
        )
        .unwrap();
        assert_eq!(kept, vec![orphan.clone()]);
        // Changed: checked.
        let edited = orphan.clone().show("b");
        assert!(prepare_rules(&[edited], &[orphan], &defs).is_err());
    }
}
