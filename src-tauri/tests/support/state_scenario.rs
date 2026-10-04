//! Shared helpers for the state and feedback tests: a real runtime, the real
//! virtual Launchpad, and pads described as JSON.
//!
//! A pad is written as JSON on purpose. The tests of this milestone start
//! before the feedback model exists in Rust, so they describe what a pad
//! *means* in both schemas at once: `bindings` is what workspace schema 3
//! understands, `feedback` is what schema 4 understands. Each runtime reads the
//! half it knows and ignores the other one, which lets the same test body run
//! against the old behaviour (and show it failing) and against the new one.

#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use launchdeck_lib::actions::definition::Assignment;
use launchdeck_lib::core::model::{Control, ControlPos, Rgb, Workspace};
use launchdeck_lib::core::runtime::{NullEmitter, Runtime, RuntimeConfig};
use launchdeck_lib::devices::Led;
use launchdeck_lib::events::Event;
use launchdeck_lib::persistence::Store;
use launchdeck_lib::plugins::keyboard::{Combo, Keyboard};
use launchdeck_lib::plugins::ActionError;
use launchdeck_lib::surfaces::default_workspace;
use serde_json::{json, Value};

pub struct NoKeyboard;
impl Keyboard for NoKeyboard {
    fn press(&self, _combo: &Combo) -> Result<(), ActionError> {
        Ok(())
    }
}

/// The resting colour of every test pad, and the looks they can show.
pub const REST: Rgb = Rgb::new(40, 110, 255);
pub const RED: Rgb = Rgb::new(255, 40, 50);
pub const GREEN: Rgb = Rgb::new(30, 220, 90);
pub const AMBER: Rgb = Rgb::new(255, 170, 0);
pub const PURPLE: Rgb = Rgb::new(170, 80, 255);
pub const WHITE: Rgb = Rgb::new(255, 255, 255);

pub fn examples() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../examples/plugins")
}

#[derive(Default)]
pub struct Options {
    pub workspace: Option<Workspace>,
    pub store: Option<Store>,
    pub plugins: bool,
    pub demo_pace: Option<f32>,
}

pub async fn start(options: Options) -> Arc<Runtime> {
    let rt = Runtime::start(RuntimeConfig {
        workspace: options.workspace.unwrap_or_else(default_workspace),
        store: options.store,
        emitter: Arc::new(NullEmitter),
        keyboard: Arc::new(NoKeyboard),
        scan_hardware: false,
        serve_external: false,
        demo_pace: options.demo_pace.unwrap_or(0.05),
        plugin_dirs: if options.plugins {
            vec![examples()]
        } else {
            vec![]
        },
        secrets: None,
        observer: None,
    })
    .await;
    wait("first frame", || {
        !rt.virtual_handle().frame().leds.is_empty()
    })
    .await;
    rt
}

pub async fn wait(what: &str, mut cond: impl FnMut() -> bool) {
    for _ in 0..600 {
        if cond() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("timed out waiting for: {what}");
}

/// True if the condition becomes true within `ms` (polled every few milliseconds).
pub async fn within(ms: u64, mut cond: impl FnMut() -> bool) -> bool {
    let end = tokio::time::Instant::now() + Duration::from_millis(ms);
    loop {
        if cond() {
            return true;
        }
        if tokio::time::Instant::now() >= end {
            return false;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
}

/// Assert that a condition stays false for `ms`. Used to prove that nothing
/// happens without an event.
pub async fn stays_false(what: &str, ms: u64, mut cond: impl FnMut() -> bool) {
    let end = tokio::time::Instant::now() + Duration::from_millis(ms);
    while tokio::time::Instant::now() < end {
        assert!(!cond(), "{what}");
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

pub fn led(rt: &Runtime, col: u8, row: u8) -> Led {
    rt.virtual_handle()
        .frame()
        .get(ControlPos::grid(col, row))
        .expect("the pad exists")
}

pub fn colour(rt: &Runtime, col: u8, row: u8) -> Rgb {
    led(rt, col, row).color
}

/// The colour if the device has drawn that pad, `None` while the frame is empty
/// (the device is unplugged).
pub fn colour_if_drawn(rt: &Runtime, col: u8, row: u8) -> Option<Rgb> {
    rt.virtual_handle()
        .frame()
        .get(ControlPos::grid(col, row))
        .map(|l| l.color)
}

/// A new surface with one empty page. Returns (surface id, page id).
pub fn new_surface(rt: &Runtime, name: &str) -> (String, String) {
    let s = rt.create_surface(name).unwrap();
    let p = rt.snapshot().workspace.surface(&s).unwrap().pages[0]
        .id
        .clone();
    (s, p)
}

pub fn seen(rt: &Runtime, kind: &str) -> bool {
    rt.snapshot().recent_events.iter().any(|e| e.kind == kind)
}

pub fn publish(rt: &Runtime, source: &str, kind: &str, payload: Value) {
    rt.publish_external(Event::new(source, kind, payload));
}

pub async fn demo(rt: &Arc<Runtime>, action: &str, agent: &str) {
    rt.run_action(Assignment::new("demo", action, json!({ "agent": agent })))
        .await
        .unwrap_or_else(|e| panic!("demo.{action}({agent}): {e}"));
}

// ---------------------------------------------------------------------------
// Pads, as JSON
// ---------------------------------------------------------------------------

fn solid(c: Rgb) -> Value {
    json!({ "color": c.to_hex() })
}

/// The looks a test pad owns. `idle` is deliberately absent: it is reserved and
/// means "rest".
fn looks() -> Value {
    json!({
        "running": solid(GREEN),
        "waiting": solid(PURPLE),
        "approval-required": solid(WHITE),
        "success": solid(GREEN),
        "error": solid(RED),
        "a": solid(AMBER),
        "b": solid(PURPLE),
        "failed": solid(RED),
        "ok": solid(GREEN),
        "warn": solid(AMBER),
        "other": solid(PURPLE),
    })
}

pub struct Pad {
    json: Value,
}

impl Pad {
    pub fn at(col: u8, row: u8) -> Pad {
        Pad {
            json: json!({
                "id": format!("g.{col}.{row}"),
                "pos": { "kind": "grid", "col": col, "row": row },
                "label": "Watcher",
                "visual": solid(REST),
                "states": looks(),
                "bindings": [],
                "feedback": [],
            }),
        }
    }

    /// Schema 3 half: a binding. Ignored by a runtime that reads schema 4.
    pub fn binding(mut self, binding: Value) -> Pad {
        self.json["bindings"].as_array_mut().unwrap().push(binding);
        self
    }

    /// Schema 4 half: a feedback rule. Ignored by a runtime that reads schema 3.
    pub fn rule(mut self, rule: Value) -> Pad {
        self.json["feedback"].as_array_mut().unwrap().push(rule);
        self
    }

    pub fn build(self) -> Control {
        serde_json::from_value(self.json).expect("a valid control")
    }
}

/// A pad that mirrors one simulated agent of the Demo plugin: the look named
/// after its current state (`running`, `waiting`, ...), resting when idle.
pub fn demo_pad(col: u8, row: u8, agent: &str) -> Control {
    Pad::at(col, row)
        .binding(json!({
            "source": "demo",
            "rules": [{
                "event": "plugin.state.changed",
                "when": [{ "field": "key", "equals": agent }],
                "setState": "$payload.value",
            }],
        }))
        .rule(json!({
            "when": {
                "state": { "provider": "demo", "id": agent },
                "is": { "op": "hasValue" },
            },
            "showFromValue": true,
        }))
        .build()
}

/// A pad with one rule per (agent, look): "when this agent is `running`, show
/// this look". In schema 3 each rule is its own binding, which is how that
/// schema expressed several sources.
pub fn demo_switch(col: u8, row: u8, rules: &[(&str, &str, &str)]) -> Control {
    let mut pad = Pad::at(col, row);
    for (agent, value, look) in rules {
        pad = pad
            .binding(json!({
                "source": "demo",
                "rules": [{
                    "event": "plugin.state.changed",
                    "when": [
                        { "field": "key", "equals": agent },
                        { "field": "value", "equals": value },
                    ],
                    "setState": look,
                }],
            }))
            .rule(json!({
                "when": {
                    "state": { "provider": "demo", "id": agent },
                    "is": { "op": "equals", "value": value },
                },
                "show": look,
            }));
    }
    pad.build()
}
