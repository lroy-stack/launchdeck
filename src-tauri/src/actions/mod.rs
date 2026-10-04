//! Actions: the unified registry, the Action Library's data model and the
//! engine that executes an assignment.
//!
//! * [`definition`]: `ActionDefinition` (what a provider offers) and
//!   `Assignment` (what a control does).
//! * [`params`]: the declarative parameter schema and its validation.
//! * [`registry`]: the catalog and the single gate (`prepare`) every
//!   assignment passes before it is saved or run.
//! * [`builtin`]: the navigation, shell and HTTP providers.
//!
//! Navigation is performed by the runtime because it needs runtime state.
//! Everything else runs here, through the provider that owns it. Shell and
//! HTTP stay as constrained as before: they only run for a control a person
//! configured, or for the inspector's Test button, and no remote interface can
//! reach them.

pub mod builtin;
pub mod definition;
pub mod params;
pub mod registry;

use serde_json::Value;

use crate::events::EventBus;
use crate::plugins::{ActionError, PluginContext, PluginRegistry};
use crate::state::StateHub;

use definition::Assignment;
use registry::ActionRegistry;

#[derive(Clone)]
pub struct ActionEngine {
    registry: ActionRegistry,
    bus: EventBus,
    hub: StateHub,
}

impl ActionEngine {
    /// An engine with a hub of its own (for tests that need none).
    pub fn new(plugins: PluginRegistry, bus: EventBus) -> Self {
        Self::with_hub(plugins, bus, StateHub::new())
    }

    /// An engine whose plugins write their states into `hub`.
    pub fn with_hub(plugins: PluginRegistry, bus: EventBus, hub: StateHub) -> Self {
        Self {
            registry: ActionRegistry::new(plugins),
            bus,
            hub,
        }
    }

    pub fn registry(&self) -> &ActionRegistry {
        &self.registry
    }

    pub fn plugins(&self) -> &PluginRegistry {
        self.registry.plugins()
    }

    /// Execute an assignment. Nothing reaches a provider without passing
    /// [`ActionRegistry::prepare`], so a malformed assignment fails here, loudly,
    /// and never runs half-configured.
    pub async fn execute(&self, assignment: &Assignment) -> Result<Value, ActionError> {
        let prepared = self.registry.prepare(assignment)?;
        let plugin = self
            .registry
            .plugin(&prepared.provider)
            .ok_or_else(|| ActionError::UnknownPlugin(prepared.provider.clone()))?;
        let ctx = PluginContext::new(
            &prepared.provider,
            self.bus.clone(),
            tokio::runtime::Handle::current(),
        )
        .with_hub(self.hub.clone());
        // Providers are synchronous and may block (keyboard, process, network).
        tokio::task::spawn_blocking(move || plugin.invoke(&ctx, &prepared.action, &prepared.args))
            .await
            .map_err(|e| ActionError::Failed(format!("plugin task failed: {e}")))?
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugins::demo::DemoPlugin;
    use serde_json::json;
    use std::sync::Arc;

    fn engine() -> ActionEngine {
        let reg = PluginRegistry::default();
        reg.register(Arc::new(DemoPlugin::new()));
        reg.register(Arc::new(builtin::NavigationProvider));
        reg.register(Arc::new(builtin::ShellProvider));
        reg.register(Arc::new(builtin::HttpProvider));
        ActionEngine::new(reg, EventBus::new())
    }

    fn a(provider: &str, action: &str, args: Value) -> Assignment {
        Assignment::new(provider, action, args)
    }

    #[tokio::test]
    async fn assignments_route_by_provider_and_action() {
        let e = engine();
        let out = e
            .execute(&a("demo", "startTask", json!({ "agent": "alpha" })))
            .await
            .unwrap();
        assert_eq!(out["state"], "running");
        let err = e.execute(&a("nope", "x", json!({}))).await.unwrap_err();
        assert_eq!(err, ActionError::UnknownPlugin("nope".into()));
        let err = e.execute(&a("demo", "nope", json!({}))).await.unwrap_err();
        assert!(matches!(err, ActionError::UnknownAction { .. }));
    }

    #[tokio::test]
    async fn malformed_assignments_never_reach_the_provider() {
        let e = engine();
        // Missing a required setting, an unknown setting, and the wrong type.
        for args in [
            json!({}),
            json!({ "agent": "alpha", "extra": 1 }),
            json!({ "agent": 7 }),
        ] {
            let err = e.execute(&a("demo", "startTask", args)).await.unwrap_err();
            assert!(matches!(err, ActionError::InvalidArgs(_)), "{err:?}");
        }
    }

    #[cfg(unix)]
    #[tokio::test(flavor = "multi_thread")]
    async fn shell_runs_through_the_same_path() {
        let e = engine();
        let ok = e
            .execute(&a(
                "shell",
                "execute",
                json!({ "command": "echo launchdeck" }),
            ))
            .await
            .unwrap();
        assert_eq!(ok["stdout"], "launchdeck");
        let bad = e
            .execute(&a(
                "shell",
                "execute",
                json!({ "command": "echo nope >&2; exit 3" }),
            ))
            .await
            .unwrap_err();
        assert_eq!(bad, ActionError::Failed("exit 3: nope".into()));
        assert!(e
            .execute(&a("shell", "execute", json!({ "command": "  " })))
            .await
            .is_err());
    }

    #[tokio::test]
    async fn http_requests_are_validated_before_any_network_use() {
        let e = engine();
        assert!(e
            .execute(&a(
                "http",
                "request",
                json!({ "method": "TRACE", "url": "http://127.0.0.1" })
            ))
            .await
            .is_err());
        assert!(e
            .execute(&a(
                "http",
                "request",
                json!({ "method": "GET", "url": "file:///etc/passwd" })
            ))
            .await
            .is_err());
    }

    #[tokio::test]
    async fn navigation_is_not_executable_by_the_engine() {
        let e = engine();
        assert!(e
            .execute(&a("navigation", "openPage", json!({ "pageId": "x" })))
            .await
            .is_err());
    }
}
