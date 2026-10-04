//! Which controls depend on what.
//!
//! The one optimisation this milestone makes, and the simplest that works:
//! three hash maps, built from the workspace and rebuilt whenever the
//! workspace changes (the cost is the number of rules).
//!
//! ```text
//!   state key   ──► controls with a state rule on exactly that state
//!   provider    ──► controls with a state rule on any state of it
//!   (source,type) ► controls with an event rule on that event
//! ```
//!
//! An unrelated event is one hash lookup. A state change evaluates only the
//! controls listed for it. Nothing else is cached.

use std::collections::{HashMap, HashSet};

use super::address::StateKey;
use super::feedback::When;
use super::hub::Changes;
use crate::core::model::Workspace;

/// A control, by the three ids that locate it.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ControlRef {
    pub surface: String,
    pub page: String,
    pub control: String,
}

impl ControlRef {
    /// The `surface/page/control` key the runtime and the UI use.
    pub fn key(&self) -> String {
        format!("{}/{}/{}", self.surface, self.page, self.control)
    }
}

#[derive(Default)]
pub struct FeedbackIndex {
    controls: Vec<ControlRef>,
    /// Positions in `controls` of those with at least one state rule.
    stateful: Vec<usize>,
    by_key: HashMap<StateKey, Vec<usize>>,
    by_provider: HashMap<String, Vec<usize>>,
    by_event: HashMap<(String, String), Vec<usize>>,
}

impl FeedbackIndex {
    pub fn build(ws: &Workspace) -> Self {
        let mut ix = FeedbackIndex::default();
        for surface in &ws.surfaces {
            for page in &surface.pages {
                for control in page.controls.iter().filter(|c| !c.feedback.is_empty()) {
                    let n = ix.controls.len();
                    ix.controls.push(ControlRef {
                        surface: surface.id.clone(),
                        page: page.id.clone(),
                        control: control.id.clone(),
                    });
                    if control.feedback.iter().any(|r| r.is_state()) {
                        ix.stateful.push(n);
                    }
                    for rule in &control.feedback {
                        match &rule.when {
                            When::State(c) => {
                                push_once(ix.by_key.entry(c.state.key()).or_default(), n);
                                push_once(
                                    ix.by_provider.entry(c.state.provider.clone()).or_default(),
                                    n,
                                );
                            }
                            When::Event(c) => push_once(
                                ix.by_event
                                    .entry((c.event.provider.clone(), c.event.id.clone()))
                                    .or_default(),
                                n,
                            ),
                        }
                    }
                }
            }
        }
        ix
    }

    /// Every control that has at least one rule, in workspace order.
    pub fn controls(&self) -> &[ControlRef] {
        &self.controls
    }

    /// Every control that has at least one state rule: the ones to evaluate
    /// when everything has to be evaluated (at start, after a reset).
    pub fn stateful(&self) -> impl Iterator<Item = &ControlRef> {
        self.stateful.iter().map(|&n| &self.controls[n])
    }

    /// Controls whose state rules can read differently after `changes`, each
    /// once.
    pub fn dependents(&self, changes: &Changes) -> Vec<&ControlRef> {
        let mut seen: HashSet<usize> = HashSet::new();
        let mut out = Vec::new();
        let mut add = |list: &Vec<usize>| {
            for &n in list {
                if seen.insert(n) {
                    out.push(&self.controls[n]);
                }
            }
        };
        for key in &changes.keys {
            if let Some(list) = self.by_key.get(key) {
                add(list);
            }
        }
        for provider in &changes.providers {
            if let Some(list) = self.by_provider.get(provider) {
                add(list);
            }
        }
        out
    }

    /// Controls with an event rule for this event.
    pub fn for_event(&self, source: &str, kind: &str) -> Vec<&ControlRef> {
        self.by_event
            .get(&(source.to_string(), kind.to_string()))
            .map(|list| list.iter().map(|&n| &self.controls[n]).collect())
            .unwrap_or_default()
    }
}

fn push_once(list: &mut Vec<usize>, n: usize) {
    if list.last() != Some(&n) {
        list.push(n);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::model::{Control, ControlPos, Page, Rgb, Surface};
    use crate::state::address::StateAddress;
    use crate::state::feedback::{FeedbackRule, Test};

    fn pad(col: u8, rules: Vec<FeedbackRule>) -> Control {
        let mut c = Control::new(ControlPos::grid(col, 0), "P", Rgb::new(1, 1, 1));
        c.feedback = rules;
        c
    }

    fn mirror(provider: &str, id: &str) -> FeedbackRule {
        FeedbackRule::state(StateAddress::new(provider, id), Test::HasValue).show_from_value()
    }

    fn workspace(controls: Vec<Control>) -> Workspace {
        Workspace {
            schema_version: 4,
            surfaces: vec![Surface {
                id: "s".into(),
                name: "S".into(),
                description: String::new(),
                icon: None,
                category: "custom".into(),
                pages: vec![Page {
                    id: "p".into(),
                    name: "P".into(),
                    controls,
                }],
                context: None,
            }],
            last_active_surface: None,
            preferences: Default::default(),
        }
    }

    fn changes(keys: &[StateAddress], providers: &[&str]) -> Changes {
        Changes {
            keys: keys.iter().map(StateAddress::key).collect(),
            providers: providers.iter().map(|p| p.to_string()).collect(),
        }
    }

    #[test]
    fn only_controls_with_rules_are_listed() {
        let ws = workspace(vec![pad(0, vec![]), pad(1, vec![mirror("demo", "alpha")])]);
        let ix = FeedbackIndex::build(&ws);
        assert_eq!(ix.controls().len(), 1);
        assert_eq!(ix.controls()[0].key(), "s/p/g.1.0");
    }

    #[test]
    fn a_state_change_finds_exactly_its_dependents() {
        let ws = workspace(vec![
            pad(0, vec![mirror("demo", "alpha")]),
            pad(1, vec![mirror("demo", "beta")]),
            pad(2, vec![mirror("demo", "alpha"), mirror("demo", "beta")]),
            pad(3, vec![mirror("other.one", "x")]),
        ]);
        let ix = FeedbackIndex::build(&ws);
        let ids = |c: &Changes| -> Vec<String> {
            let mut v: Vec<String> = ix.dependents(c).iter().map(|r| r.control.clone()).collect();
            v.sort();
            v
        };
        assert_eq!(
            ids(&changes(&[StateAddress::new("demo", "alpha")], &[])),
            ["g.0.0", "g.2.0"]
        );
        assert_eq!(
            ids(&changes(
                &[
                    StateAddress::new("demo", "alpha"),
                    StateAddress::new("demo", "beta")
                ],
                &[]
            )),
            ["g.0.0", "g.1.0", "g.2.0"],
            "a control with two matching rules is listed once"
        );
        assert!(ids(&changes(&[StateAddress::new("demo", "gamma")], &[])).is_empty());
        assert!(ids(&changes(&[], &[])).is_empty());
    }

    #[test]
    fn a_provider_change_finds_every_control_that_reads_it() {
        let ws = workspace(vec![
            pad(0, vec![mirror("demo", "alpha")]),
            pad(1, vec![mirror("demo", "beta")]),
            pad(2, vec![mirror("other.one", "x")]),
        ]);
        let ix = FeedbackIndex::build(&ws);
        let found = ix.dependents(&changes(&[], &["demo"]));
        assert_eq!(found.len(), 2);
        // A key and its provider changing together do not repeat a control.
        let found = ix.dependents(&changes(&[StateAddress::new("demo", "alpha")], &["demo"]));
        assert_eq!(found.len(), 2);
    }

    #[test]
    fn arguments_are_part_of_the_dependency() {
        let slot = |n: i64| {
            FeedbackRule::state(
                StateAddress::new("agent-control", "slot.status").with("slot", n),
                Test::HasValue,
            )
            .show_from_value()
        };
        let ws = workspace(vec![pad(0, vec![slot(1)]), pad(1, vec![slot(2)])]);
        let ix = FeedbackIndex::build(&ws);
        let c = changes(
            &[StateAddress::new("agent-control", "slot.status").with("slot", 2.0)],
            &[],
        );
        let found = ix.dependents(&c);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].control, "g.1.0");
    }

    #[test]
    fn events_are_found_by_source_and_type() {
        let ws = workspace(vec![
            pad(
                0,
                vec![FeedbackRule::event("ci", "build.finished").show("x")],
            ),
            pad(
                1,
                vec![FeedbackRule::event("ci", "build.started").show("y")],
            ),
            pad(2, vec![mirror("demo", "alpha")]),
        ]);
        let ix = FeedbackIndex::build(&ws);
        assert_eq!(ix.for_event("ci", "build.finished").len(), 1);
        assert!(ix.for_event("ci", "other").is_empty());
        assert!(ix.for_event("demo", "plugin.state.changed").is_empty());
    }
}
