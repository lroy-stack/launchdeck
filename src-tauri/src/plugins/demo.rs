//! The Demo plugin: a minimal example plugin that simulates three generic
//! tasks and publishes their semantic state (`plugin.state.changed`).
//!
//! It is the smallest possible plugin, kept as the reference for plugin
//! authors and for custom pads. The Agent Control module has its own,
//! far richer simulation (`agents::demo`). Nothing here approves anything on
//! its own: an approval waits until someone presses the approve action.

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::{json, Value};

use super::{str_arg, ActionDef, ActionError, Plugin, PluginContext, PluginManifest, StateDef};
use crate::actions::definition::ProviderKind;
use crate::actions::params::ParamDef;

pub const ID: &str = "demo";
pub const AGENTS: [&str; 3] = ["alpha", "beta", "gamma"];
pub const STATES: [&str; 6] = [
    "idle",
    "running",
    "waiting",
    "approval-required",
    "success",
    "error",
];

#[derive(Default)]
struct Inner {
    states: BTreeMap<String, String>,
}

#[derive(Default, Clone)]
pub struct DemoPlugin {
    inner: Arc<Mutex<Inner>>,
    /// Bumped to cancel an in-flight scenario.
    generation: Arc<AtomicU64>,
}

impl DemoPlugin {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn state_of(&self, agent: &str) -> String {
        self.inner
            .lock()
            .unwrap()
            .states
            .get(agent)
            .cloned()
            .unwrap_or_else(|| "idle".into())
    }

    fn set(&self, ctx: &PluginContext, agent: &str, state: &str) {
        self.inner
            .lock()
            .unwrap()
            .states
            .insert(agent.to_string(), state.to_string());
        ctx.publish_state(agent, state);
    }

    fn agent_arg(args: &Value) -> Result<String, ActionError> {
        let agent = str_arg(args, "agent")?;
        if AGENTS.contains(&agent) {
            Ok(agent.to_string())
        } else {
            Err(ActionError::InvalidArgs(format!(
                "unknown agent `{agent}`; expected one of {}",
                AGENTS.join(", ")
            )))
        }
    }

    /// Approve every agent currently waiting for approval (or just `agent`).
    fn approve(&self, ctx: &PluginContext, only: Option<&str>) -> Vec<String> {
        let waiting: Vec<String> = AGENTS
            .iter()
            .filter(|a| only.is_none() || only == Some(**a))
            .filter(|a| self.state_of(a) == "approval-required")
            .map(|a| a.to_string())
            .collect();
        for agent in &waiting {
            self.set(ctx, agent, "running");
            let (me, c, a) = (self.clone(), ctx.clone(), agent.clone());
            ctx.spawn_after(Duration::from_millis(1500), move || {
                if me.state_of(&a) == "running" {
                    me.set(&c, &a, "success");
                }
            });
        }
        waiting
    }

    fn reset_all(&self, ctx: &PluginContext) {
        self.generation.fetch_add(1, Ordering::SeqCst);
        for a in AGENTS {
            self.set(ctx, a, "idle");
        }
    }

    /// A scripted ~15 second multi-agent run, cancellable and restartable.
    fn run_scenario(&self, ctx: &PluginContext) {
        self.reset_all(ctx);
        let gen = self.generation.load(Ordering::SeqCst);
        // (delay ms, agent, state). The approval step waits for the user.
        let steps: [(u64, &str, &str); 7] = [
            (300, "alpha", "running"),
            (1500, "beta", "running"),
            (2800, "gamma", "waiting"),
            (4200, "alpha", "approval-required"),
            (6500, "gamma", "running"),
            (9500, "beta", "success"),
            (11500, "gamma", "error"),
        ];
        for (delay, agent, state) in steps {
            let (me, c) = (self.clone(), ctx.clone());
            ctx.spawn_after(Duration::from_millis(delay), move || {
                if me.generation.load(Ordering::SeqCst) == gen {
                    me.set(&c, agent, state);
                }
            });
        }
    }
}

fn agent_param(required: bool) -> ParamDef {
    let choices: Vec<(&str, &str)> = vec![("alpha", "Alpha"), ("beta", "Beta"), ("gamma", "Gamma")];
    let p = ParamDef::choice("agent", "Agent", &choices);
    if required {
        p.required()
    } else {
        p.help("Leave empty for every agent that is waiting.")
    }
}

fn task_action(id: &str, label: &str, description: &str) -> ActionDef {
    ActionDef::new(id, label, description)
        .category("Demo")
        .keywords(&["simulated", "task", "sample", "fake"])
        .param(agent_param(true))
}

impl Plugin for DemoPlugin {
    fn manifest(&self) -> PluginManifest {
        PluginManifest {
            id: ID.into(),
            name: "Demo".into(),
            version: "0.2.0".into(),
            description:
                "Simulated agent tasks for demos and development. No network, no side effects."
                    .into(),
            kind: ProviderKind::Builtin,
            actions: vec![
                task_action("startTask", "Start task", "Set a simulated agent to running."),
                task_action("stopTask", "Stop task", "Set a simulated agent back to idle."),
                task_action(
                    "succeedTask",
                    "Succeed task",
                    "Mark a simulated agent's task as succeeded.",
                ),
                task_action(
                    "failTask",
                    "Fail task",
                    "Mark a simulated agent's task as failed.",
                ),
                task_action(
                    "requestApproval",
                    "Request approval",
                    "Make a simulated agent ask for approval.",
                ),
                ActionDef::new("approve", "Approve", "Approve simulated agents that are waiting.")
                    .category("Demo")
                    .keywords(&["simulated", "yes", "allow", "sample"])
                    .param(agent_param(false)),
                ActionDef::new(
                    "runScenario",
                    "Run agent demo",
                    "Play a scripted run of three simulated tasks. The approval step waits for you.",
                )
                .category("Demo")
                .keywords(&["simulated", "scenario", "sample", "play"]),
                ActionDef::new("resetAll", "Reset", "Return every simulated agent to idle.")
                    .category("Demo")
                    .keywords(&["clear", "stop", "sample"]),
            ],
            states: AGENTS
                .iter()
                .map(|a| StateDef {
                    key: (*a).into(),
                    description: format!("Task state of the simulated `{a}` agent."),
                    values: STATES.iter().map(|s| s.to_string()).collect(),
                })
                .collect(),
            permissions: vec![],
            external: None,
        }
    }

    fn invoke(
        &self,
        ctx: &PluginContext,
        action: &str,
        args: &Value,
    ) -> Result<Value, ActionError> {
        let simple = |state: &str| -> Result<Value, ActionError> {
            let agent = Self::agent_arg(args)?;
            self.set(ctx, &agent, state);
            Ok(json!({ "agent": agent, "state": state }))
        };
        match action {
            "startTask" => simple("running"),
            "stopTask" => simple("idle"),
            "succeedTask" => simple("success"),
            "failTask" => simple("error"),
            "requestApproval" => simple("approval-required"),
            "approve" => {
                let only = args.get("agent").and_then(Value::as_str);
                Ok(json!({ "approved": self.approve(ctx, only) }))
            }
            "runScenario" => {
                self.run_scenario(ctx);
                Ok(Value::Null)
            }
            "resetAll" => {
                self.reset_all(ctx);
                Ok(Value::Null)
            }
            other => Err(ActionError::UnknownAction {
                plugin: ID.into(),
                action: other.into(),
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::events::{types, EventBus};

    fn setup() -> (
        DemoPlugin,
        PluginContext,
        tokio::sync::broadcast::Receiver<crate::events::Event>,
    ) {
        let bus = EventBus::new();
        let rx = bus.subscribe();
        (
            DemoPlugin::new(),
            PluginContext::new(ID, bus, tokio::runtime::Handle::current()),
            rx,
        )
    }

    #[tokio::test]
    async fn actions_publish_semantic_state_changes() {
        let (demo, ctx, mut rx) = setup();
        demo.invoke(&ctx, "startTask", &json!({ "agent": "alpha" }))
            .unwrap();
        let e = rx.recv().await.unwrap();
        assert_eq!(e.kind, types::PLUGIN_STATE_CHANGED);
        assert_eq!(e.source, "demo");
        assert_eq!(e.payload, json!({ "key": "alpha", "value": "running" }));
        demo.invoke(&ctx, "failTask", &json!({ "agent": "alpha" }))
            .unwrap();
        assert_eq!(demo.state_of("alpha"), "error");
        demo.invoke(&ctx, "stopTask", &json!({ "agent": "alpha" }))
            .unwrap();
        assert_eq!(demo.state_of("alpha"), "idle");
    }

    #[tokio::test]
    async fn rejects_unknown_agents_and_missing_args() {
        let (demo, ctx, _rx) = setup();
        assert!(demo
            .invoke(&ctx, "startTask", &json!({ "agent": "hal9000" }))
            .is_err());
        assert!(demo.invoke(&ctx, "startTask", &json!({})).is_err());
    }

    #[tokio::test(start_paused = true)]
    async fn approval_flow_runs_then_succeeds() {
        let (demo, ctx, _rx) = setup();
        demo.invoke(&ctx, "requestApproval", &json!({ "agent": "alpha" }))
            .unwrap();
        assert_eq!(demo.state_of("alpha"), "approval-required");
        let out = demo.invoke(&ctx, "approve", &json!({})).unwrap();
        assert_eq!(out["approved"], json!(["alpha"]));
        assert_eq!(demo.state_of("alpha"), "running");
        tokio::time::sleep(Duration::from_millis(1600)).await;
        assert_eq!(demo.state_of("alpha"), "success");
        // Nothing left to approve.
        assert_eq!(
            demo.invoke(&ctx, "approve", &json!({})).unwrap()["approved"],
            json!([])
        );
    }

    #[tokio::test(start_paused = true)]
    async fn scenario_cycles_through_the_documented_states_and_can_be_cancelled() {
        let (demo, ctx, _rx) = setup();
        demo.invoke(&ctx, "runScenario", &Value::Null).unwrap();
        tokio::time::sleep(Duration::from_millis(5000)).await;
        assert_eq!(demo.state_of("alpha"), "approval-required");
        assert_eq!(demo.state_of("beta"), "running");
        assert_eq!(demo.state_of("gamma"), "waiting");
        // Unattended, nothing is ever approved on its own.
        tokio::time::sleep(Duration::from_secs(60)).await;
        assert_eq!(demo.state_of("alpha"), "approval-required");
        assert_eq!(demo.state_of("beta"), "success");
        assert_eq!(demo.state_of("gamma"), "error");
        // Cancelling leaves no stale timers behind.
        demo.invoke(&ctx, "runScenario", &Value::Null).unwrap();
        demo.invoke(&ctx, "resetAll", &Value::Null).unwrap();
        tokio::time::sleep(Duration::from_millis(20000)).await;
        for a in AGENTS {
            assert_eq!(demo.state_of(a), "idle", "{a}");
        }
    }
}
