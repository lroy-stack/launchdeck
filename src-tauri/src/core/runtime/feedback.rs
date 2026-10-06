//! Feedback: the look of every control. State rules are evaluated from the hub
//! into the derived looks; event rules put temporary looks on; the index says
//! which controls read what. Also why a control looks as it does
//! (`explain_control`) and what a draft of its rules would do (`review_feedback`).

use std::collections::BTreeMap;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::Value;

use super::editing::{missing_control, missing_page, missing_surface};
use super::{control_key, ui, FeedbackStats, Inner, Origin, Runtime};
use crate::agents::service::is_projection;
use crate::core::explain::{self, ControlExplanation, OverlayExplanation, OverlaySource};
use crate::core::model::{Control, Workspace};
use crate::core::review::{self, FeedbackReview};
use crate::events::Event;
use crate::state::eval;
use crate::state::feedback::{Hold, When, IDLE};
use crate::state::index::{ControlRef, FeedbackIndex};
use crate::state::{Changes, Scalar, StateHub};

const RECENT_EVENTS: usize = 100;

fn find_control<'a>(ws: &'a Workspace, r: &ControlRef) -> Option<&'a Control> {
    ws.surface(&r.surface)?
        .page(&r.page)?
        .controls
        .iter()
        .find(|c| c.id == r.control)
}

/// The look every control shows: a temporary one over the one its state rules
/// give. A control at rest is absent.
pub(super) fn looks_of(inner: &Inner) -> BTreeMap<String, String> {
    let mut looks: BTreeMap<String, String> = inner
        .derived
        .iter()
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    for (k, e) in &inner.overlay {
        looks.insert(k.clone(), e.name.clone());
    }
    looks
}

/// An event rule's payload test: equal as values, so `2` equals `2.0`.
fn payload_matches(payload: &Value, test: &crate::state::feedback::FieldTest) -> bool {
    payload
        .get(&test.field)
        .and_then(|v| Scalar::from_json(v).ok())
        .is_some_and(|v| v.same_as(&test.equals))
}

/// The look and hold of the first event rule of `control` that `event`
/// matches, and the position of that rule.
fn event_look(control: &Control, event: &Event) -> Option<(usize, String, Hold)> {
    for (index, rule) in control.feedback.iter().enumerate() {
        let When::Event(c) = &rule.when else {
            continue;
        };
        if c.event.provider != event.source
            || c.event.id != event.kind
            || !c.where_.iter().all(|t| payload_matches(&event.payload, t))
        {
            continue;
        }
        let look = match (&rule.show, &rule.show_from_field) {
            (Some(name), _) => name.clone(),
            (None, Some(field)) => match event
                .payload
                .get(field)
                .and_then(|v| Scalar::from_json(v).ok())
            {
                Some(v) => v.look_name(),
                // The event lacks the field this rule names its look from.
                None => continue,
            },
            (None, None) => continue,
        };
        return Some((index, look, rule.hold_or_default()));
    }
    None
}

impl Runtime {
    /// The index follows the workspace: rebuilt after any edit of the rules.
    pub(super) fn rebuild_index(&self, inner: &mut Inner) {
        inner.index = FeedbackIndex::build(&inner.workspace);
        // What controls read may have changed, so what plugins report should too.
        self.plugin_host.refresh_all_soon();
    }

    /// Evaluate the state rules of every control that has any, from the values
    /// the hub holds now. Used at start and whenever the rules change as a
    /// whole.
    pub(super) fn evaluate_all(&self, inner: &mut Inner) {
        inner.derived.clear();
        let refs: Vec<ControlRef> = inner.index.stateful().cloned().collect();
        for r in &refs {
            self.derive_into(inner, r);
        }
    }

    /// Re-evaluate one control and store what its state rules give. Returns
    /// whether the derived look changed.
    pub(super) fn derive_into(&self, inner: &mut Inner, r: &ControlRef) -> bool {
        let key = r.key();
        // A control with only event rules has nothing to derive.
        if !find_control(&inner.workspace, r)
            .is_some_and(|c| c.feedback.iter().any(|r| r.is_state()))
        {
            return inner.derived.remove(&key).is_some();
        }
        let evaluation = find_control(&inner.workspace, r).map(|c| {
            eval::evaluate(&c.feedback, &|k| self.hub.read(k), &|name| {
                c.states.contains_key(name)
            })
        });
        self.counters.evaluations.fetch_add(1, Ordering::Relaxed);
        let Some(evaluation) = evaluation else {
            return inner.derived.remove(&key).is_some();
        };
        let before = inner.derived.get(&key).cloned();
        match &evaluation.look {
            Some(look) => {
                inner.derived.insert(key.clone(), look.clone());
            }
            None => {
                inner.derived.remove(&key);
            }
        }
        let changed = before != evaluation.look;
        if changed {
            for d in &evaluation.diagnostics {
                log::warn!("feedback of `{key}`: {d:?}");
            }
        }
        changed
    }

    /// The hub says these states changed: re-evaluate the controls that read
    /// them, and only those.
    pub(super) fn apply_hub_changes(&self, changes: &Changes) {
        let changed = {
            let mut inner = self.lock();
            let refs: Vec<ControlRef> = inner
                .index
                .dependents(changes)
                .into_iter()
                .cloned()
                .collect();
            let mut any = false;
            for r in &refs {
                any |= self.derive_into(&mut inner, r);
            }
            any
        };
        if changed {
            self.emit_state();
        }
    }

    /// The event loop fell behind and the bus dropped events. States are not
    /// affected (they never travel on the bus); an event rule can have missed
    /// its event.
    pub(super) fn bus_lagged(&self, missed: u64) {
        self.counters
            .bus_lagged
            .fetch_add(missed, Ordering::Relaxed);
        log::warn!("the event loop missed {missed} events; states are unaffected");
    }

    /// What the feedback machinery has done so far.
    pub fn feedback_stats(&self) -> FeedbackStats {
        FeedbackStats {
            evaluations: self.counters.evaluations.load(Ordering::Relaxed),
            events_routed: self.counters.events_routed.load(Ordering::Relaxed),
            bus_lagged: self.counters.bus_lagged.load(Ordering::Relaxed),
        }
    }

    /// The current values every provider holds.
    pub fn state_hub(&self) -> &StateHub {
        &self.hub
    }

    /// Why a control looks the way it does: the temporary look if one is on,
    /// then the state rules rule by rule against the values now, with what
    /// each state is and why it may not answer.
    pub fn explain_control(
        &self,
        surface_id: &str,
        page_id: &str,
        control_id: &str,
    ) -> Result<ControlExplanation, String> {
        let sources = self.observable_sources();
        let inner = self.lock();
        let control = inner
            .workspace
            .surface(surface_id)
            .and_then(|s| s.page(page_id))
            .and_then(|p| p.controls.iter().find(|c| c.id == control_id))
            .ok_or_else(missing_control)?;
        let key = control_key(surface_id, page_id, control_id);
        let now = Instant::now();
        let overlay = inner.overlay.get(&key).map(|e| OverlayExplanation {
            look: e.name.clone(),
            source: e.origin.source,
            rule: e.origin.rule,
            remaining_ms: e
                .until
                .map(|t| t.saturating_duration_since(now).as_millis() as u64),
            latched: e.origin.latched,
        });
        Ok(explain::explain(
            (surface_id, page_id, control_id),
            control,
            overlay,
            inner.derived.get(&key).cloned(),
            &self.hub,
            &sources,
        ))
    }

    /// What a draft of a control's feedback comes to, without saving it: the
    /// issues `update_control` would refuse, what Rust can prove about the
    /// rules, the limits, and what the draft would explain right now. Reads the
    /// workspace and the hub and changes neither.
    pub fn review_feedback(
        &self,
        surface_id: &str,
        page_id: &str,
        control: &Control,
    ) -> Result<FeedbackReview, String> {
        let stored = {
            let inner = self.lock();
            let surface = inner
                .workspace
                .surface(surface_id)
                .ok_or_else(missing_surface)?;
            let page = surface.page(page_id).ok_or_else(missing_page)?;
            page.controls.iter().find(|c| c.id == control.id).cloned()
        };
        Ok(review::review(
            (surface_id, page_id),
            control,
            stored.as_ref(),
            &self.hub,
            &self.observable_sources(),
        ))
    }

    pub(super) fn on_event(self: &Arc<Self>, event: Event) {
        // Projection events are for event rules and external listeners; they
        // would drown the event log.
        if !is_projection(&event) {
            {
                let mut inner = self.lock();
                inner.recent.push_back(event.clone());
                while inner.recent.len() > RECENT_EVENTS {
                    inner.recent.pop_front();
                }
            }
            self.emitter.emit(
                ui::EVENT,
                serde_json::to_value(&event).unwrap_or(Value::Null),
            );
        }
        self.route_events(&event);
    }

    /// Event rules: the first matching rule of each control shows its look as
    /// a temporary overlay. The event is not stored anywhere else.
    fn route_events(self: &Arc<Self>, event: &Event) {
        // (key, rule, look, hold, the control has a look called `idle` of its own)
        let mut matched: Vec<(String, usize, String, Hold, bool)> = Vec::new();
        {
            let inner = self.lock();
            for r in inner.index.for_event(&event.source, &event.kind) {
                let Some(control) = find_control(&inner.workspace, r) else {
                    continue;
                };
                if let Some((rule, look, hold)) = event_look(control, event) {
                    matched.push((r.key(), rule, look, hold, control.states.contains_key(IDLE)));
                }
            }
        }
        if matched.is_empty() {
            return;
        }
        self.counters
            .events_routed
            .fetch_add(matched.len() as u64, Ordering::Relaxed);
        for (key, rule, look, hold, own_idle) in matched {
            let origin = Origin {
                source: OverlaySource::Event,
                rule: Some(rule),
                latched: matches!(hold, Hold::Latch),
            };
            // `idle` means "no temporary look" unless the control draws one.
            if look == IDLE && !own_idle {
                self.set_overlay(&key, None, None, origin);
                continue;
            }
            let after = match hold {
                Hold::For(ms) => Some(Duration::from_millis(u64::from(ms))),
                Hold::Latch => None,
            };
            self.set_overlay(&key, Some(&look), after, origin);
        }
    }
}
