//! What the editor offers and checks: the Action Library catalog, an
//! assignment checked without running it, the Observable Catalog and the
//! value of one state. Also telling the UI what the hub changed, and asking
//! plugins to refresh the states some control reads.

use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;

use serde_json::Value;

use super::{ui, Runtime};
use crate::actions::definition::{Catalog, Issue};
use crate::core::model::Assignment;
use crate::core::observables as native;
use crate::state::catalog::{self, ObservableCatalog, ObservablesDelta, Source};
use crate::state::feedback::When;
use crate::state::Changes;

/// Changes to the Observable Catalog that arrive within this window reach the
/// UI as one message. Feedback is not delayed by it: pads are re-evaluated as
/// soon as the hub reports.
const OBSERVABLES_COALESCE: Duration = Duration::from_millis(50);

impl Runtime {
    /// Check an assignment the way saving and running do, without doing either.
    pub fn validate_assignment(&self, assignment: &Assignment) -> Result<Assignment, Vec<Issue>> {
        self.engine
            .registry()
            .prepare(assignment)
            .map_err(|e| e.issues())
    }

    /// The Action Library's catalog, evaluated now.
    pub fn catalog(&self) -> Catalog {
        self.engine.registry().catalog()
    }

    /// Who offers what to observe: the app itself first, then every registered
    /// provider (built-in, Agent Control, installed plugins).
    pub(super) fn observable_sources(&self) -> Vec<Source> {
        let mut sources = native::sources();
        sources.extend(self.engine.registry().observable_sources());
        sources
    }

    /// Everything a control can observe, states and events, with the values the
    /// hub holds now. The one list the editor picks from.
    pub fn observables(&self) -> ObservableCatalog {
        catalog::build(self.observable_sources(), &self.hub)
    }

    /// What one state reads now, for an address the editor has built. The
    /// arguments are checked against the definition, so `2` and `2.0` read the
    /// same instance and a wrong argument is an error, not an `unknown`.
    pub fn read_state(
        &self,
        provider: &str,
        id: &str,
        args: &Value,
    ) -> Result<crate::state::Observed, String> {
        let def = self
            .hub
            .definition(provider, id)
            .ok_or_else(|| format!("`{provider}.{id}` is not a state anything offers right now"))?;
        let address = def.address(args)?;
        Ok(self.hub.read_address(&address))
    }

    /// Tell the UI what the hub reported, as one message for a burst. The
    /// window starts at the first change; whatever arrives within it joins.
    pub(super) fn announce_observables(self: &Arc<Self>, changes: Changes) {
        self.unannounced
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .merge(changes);
        if self.announce_scheduled.swap(true, Ordering::SeqCst) {
            return;
        }
        let rt = self.clone();
        self.handle.spawn(async move {
            tokio::time::sleep(OBSERVABLES_COALESCE).await;
            // Cleared before the take: a change that lands after it schedules
            // the next message and is never lost.
            rt.announce_scheduled.store(false, Ordering::SeqCst);
            let changes =
                std::mem::take(&mut *rt.unannounced.lock().unwrap_or_else(|p| p.into_inner()));
            let delta: ObservablesDelta =
                catalog::delta(&rt.observable_sources(), &rt.hub, &changes);
            if !delta.is_empty() {
                rt.emitter.emit(
                    ui::OBSERVABLES,
                    serde_json::to_value(delta).unwrap_or(Value::Null),
                );
            }
        });
    }

    /// The instances of a provider's states that some control's rules read,
    /// anywhere in the workspace.
    fn watched_states(&self, provider: &str) -> Vec<crate::state::StateAddress> {
        let inner = self.lock();
        let mut seen = std::collections::HashSet::new();
        let mut out = Vec::new();
        for surface in &inner.workspace.surfaces {
            for page in &surface.pages {
                for control in &page.controls {
                    for rule in &control.feedback {
                        if let When::State(c) = &rule.when {
                            if c.state.provider == provider && seen.insert(c.state.key()) {
                                out.push(c.state.clone());
                            }
                        }
                    }
                }
            }
        }
        out
    }

    /// Start the refresh of every plugin that is due. Each runs on a blocking
    /// thread, like an action; none is started on top of one still running.
    pub(super) fn refresh_due(self: &Arc<Self>) {
        for id in self.plugin_host.due_refreshes() {
            let watching = self.watched_states(&id);
            let rt = self.clone();
            self.handle.spawn(async move {
                let host = rt.plugin_host.clone();
                let ctx = crate::plugins::PluginContext::new(
                    &id,
                    rt.bus.clone(),
                    tokio::runtime::Handle::current(),
                )
                .with_hub(rt.hub.clone());
                let _ =
                    tokio::task::spawn_blocking(move || host.refresh(&id, &ctx, &watching)).await;
            });
        }
    }
}
