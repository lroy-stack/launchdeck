//! Semantic event bus.
//!
//! Everything that happens in the runtime is a small semantic event. The set
//! of built-in event types is deliberately short.

pub mod external;

use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::sync::broadcast;

/// Built-in event types emitted by the runtime itself.
pub mod types {
    pub const CONTROL_PRESSED: &str = "control.pressed";
    pub const ACTION_STARTED: &str = "action.started";
    pub const ACTION_COMPLETED: &str = "action.completed";
    pub const ACTION_FAILED: &str = "action.failed";
    pub const SURFACE_CHANGED: &str = "surface.changed";
    pub const PAGE_CHANGED: &str = "page.changed";
    pub const PLUGIN_STATE_CHANGED: &str = "plugin.state.changed";
    pub const DEVICE_CONNECTED: &str = "device.connected";
    pub const DEVICE_DISCONNECTED: &str = "device.disconnected";
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Event {
    pub source: String,
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(default)]
    pub payload: Value,
    #[serde(default, rename = "ts")]
    pub ts_ms: u64,
}

impl Event {
    pub fn new(source: impl Into<String>, kind: impl Into<String>, payload: Value) -> Self {
        Self {
            source: source.into(),
            kind: kind.into(),
            payload,
            ts_ms: now_ms(),
        }
    }

    /// Plugin state change: `{ key, value }` published by a plugin.
    pub fn plugin_state(plugin: &str, key: &str, value: &str) -> Self {
        Self::new(
            plugin,
            types::PLUGIN_STATE_CHANGED,
            json!({ "key": key, "value": value }),
        )
    }
}

pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

#[derive(Clone)]
pub struct EventBus {
    tx: broadcast::Sender<Event>,
}

impl Default for EventBus {
    fn default() -> Self {
        Self::new()
    }
}

impl EventBus {
    pub fn new() -> Self {
        let (tx, _) = broadcast::channel(1024);
        Self { tx }
    }

    pub fn publish(&self, event: Event) {
        // No subscribers is fine.
        let _ = self.tx.send(event);
    }

    pub fn subscribe(&self) -> broadcast::Receiver<Event> {
        self.tx.subscribe()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn publish_reaches_all_subscribers() {
        let bus = EventBus::new();
        let mut a = bus.subscribe();
        let mut b = bus.subscribe();
        bus.publish(Event::new("codex", "task.started", json!({})));
        assert_eq!(a.recv().await.unwrap().kind, "task.started");
        assert_eq!(b.recv().await.unwrap().source, "codex");
    }

    #[test]
    fn event_json_shape_matches_wire_format() {
        let e: Event = serde_json::from_str(
            r#"{"source":"codex","type":"task.completed","payload":{"success":true}}"#,
        )
        .unwrap();
        assert_eq!(e.kind, "task.completed");
        assert_eq!(e.payload["success"], true);
    }
}
