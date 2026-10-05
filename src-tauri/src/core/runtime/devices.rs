//! Devices: drawing the active page on every connected device, and the
//! device list (connect, disconnect, rescan for hardware).

use serde_json::json;

use super::{control_key, Inner, Runtime};
use crate::devices::manager::{DeviceChange, DeviceInfo};
use crate::devices::DeviceError;
use crate::events::types;
use crate::renderer;

impl Runtime {
    pub fn render_tick(&self) {
        let t_ms = self.epoch.elapsed().as_millis() as u64;
        let mut changes = Vec::new();
        {
            let mut inner = self.lock();
            let Inner {
                workspace,
                active_surface,
                active_page,
                overlay,
                derived,
                devices,
                ..
            } = &mut *inner;
            let page = workspace
                .surface(active_surface)
                .and_then(|s| s.page(active_page));
            for (id, caps) in devices.connected_capabilities() {
                // A temporary look over the one the state rules give.
                let state_of = |cid: &str| {
                    let key = control_key(active_surface, active_page, cid);
                    overlay
                        .get(&key)
                        .map(|e| e.name.clone())
                        .or_else(|| derived.get(&key).cloned())
                };
                let frame = renderer::compose(page, &state_of, &caps, t_ms);
                if let Some(change) = devices.render(&id, &frame) {
                    changes.push(change);
                }
            }
        }
        self.announce(changes);
    }

    fn announce(&self, changes: Vec<DeviceChange>) {
        if changes.is_empty() {
            return;
        }
        for change in changes {
            match change {
                DeviceChange::Connected(id) => self.publish(
                    "runtime",
                    types::DEVICE_CONNECTED,
                    json!({ "deviceId": id }),
                ),
                DeviceChange::Disconnected(id) => self.publish(
                    "runtime",
                    types::DEVICE_DISCONNECTED,
                    json!({ "deviceId": id }),
                ),
                // Only the device list changed: the state below carries it.
                DeviceChange::Updated(_) => {}
            }
        }
        self.emit_state();
    }

    pub fn list_devices(&self) -> Vec<DeviceInfo> {
        self.lock().devices.list()
    }

    pub fn connect_device(&self, id: &str) -> Result<(), String> {
        let change = self.lock().devices.connect(id).map_err(sentence)?;
        self.announce(vec![change]);
        Ok(())
    }

    pub fn disconnect_device(&self, id: &str) -> Result<(), String> {
        let change = self.lock().devices.disconnect(id).map_err(sentence)?;
        self.announce(vec![change]);
        Ok(())
    }

    /// Look for Launchpad X hardware; connect units that are new or came back.
    /// A connect that fails is recorded by the device manager as the unit's
    /// problem.
    pub fn rescan(&self) {
        self.scan_hardware(false);
    }

    /// The person pressed Look again: also retry units whose connect failed
    /// or that turned LaunchDeck down.
    pub fn look_again(&self) {
        self.scan_hardware(true);
    }

    fn scan_hardware(&self, retry_failed: bool) {
        let mut changes = Vec::new();
        {
            let mut inner = self.lock();
            let rescan = inner.devices.rescan_hardware(retry_failed);
            changes.extend(rescan.changes);
            for id in rescan.to_connect {
                if let Ok(change) = inner.devices.connect(&id) {
                    changes.push(change);
                }
            }
        }
        // A newly listed, unplugged or failed device also changes the list.
        self.announce(changes);
        self.emit_state();
    }
}

/// The sentence for the person, then what the system said, until command
/// errors carry their detail separately.
fn sentence(e: DeviceError) -> String {
    match e.detail() {
        Some(detail) => format!("{e} Details: {detail}"),
        None => e.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    use serde_json::Value;

    use super::super::{ui, Runtime, RuntimeConfig, UiEmitter};
    use crate::devices::{
        AnimationCaps, AuxCaps, DeviceCapabilities, DeviceDetails, DeviceError, DeviceIdentity,
        DeviceKind, InputSender, ModeState, SurfaceDevice, SurfaceFrame,
    };
    use crate::events::types;
    use crate::plugins::keyboard::{Combo, Keyboard};
    use crate::plugins::ActionError;
    use crate::surfaces::default_workspace;

    const ID: &str = "noticing:1";

    /// A device whose details the test changes.
    struct Noticing {
        details: Arc<Mutex<DeviceDetails>>,
        connected: bool,
    }

    impl SurfaceDevice for Noticing {
        fn identity(&self) -> DeviceIdentity {
            DeviceIdentity {
                id: ID.into(),
                name: "Noticing".into(),
                model: "noticing".into(),
                kind: DeviceKind::Hardware,
            }
        }
        fn capabilities(&self) -> DeviceCapabilities {
            DeviceCapabilities {
                grid: None,
                aux: AuxCaps::default(),
                rgb: true,
                velocity: false,
                pressure: false,
                animations: AnimationCaps::default(),
            }
        }
        fn connect(&mut self, _input: InputSender) -> Result<(), DeviceError> {
            self.connected = true;
            Ok(())
        }
        fn disconnect(&mut self) -> Result<(), DeviceError> {
            self.connected = false;
            Ok(())
        }
        fn is_connected(&self) -> bool {
            self.connected
        }
        fn render(&mut self, _frame: &SurfaceFrame) -> Result<(), DeviceError> {
            Ok(())
        }
        fn clear(&mut self) -> Result<(), DeviceError> {
            Ok(())
        }
        fn details(&self) -> DeviceDetails {
            self.details.lock().unwrap().clone()
        }
    }

    /// Every state the runtime sends the UI.
    #[derive(Default)]
    struct States(Mutex<Vec<Value>>);

    impl UiEmitter for States {
        fn emit(&self, event: &str, payload: Value) {
            if event == ui::STATE {
                self.0.lock().unwrap().push(payload);
            }
        }
    }

    impl States {
        fn mode_of(&self, id: &str) -> Vec<Value> {
            self.0
                .lock()
                .unwrap()
                .iter()
                .flat_map(|s| s["devices"].as_array().cloned().unwrap_or_default())
                .filter(|d| d["identity"]["id"] == id)
                .map(|d| d["details"]["mode"].clone())
                .collect()
        }
    }

    struct NoKeyboard;
    impl Keyboard for NoKeyboard {
        fn press(&self, _combo: &Combo) -> Result<(), ActionError> {
            Ok(())
        }
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn changed_details_send_the_state_again_and_publish_no_event() {
        let states = Arc::new(States::default());
        let rt = Runtime::start(RuntimeConfig {
            workspace: default_workspace(),
            store: None,
            emitter: states.clone(),
            keyboard: Arc::new(NoKeyboard),
            scan_hardware: false,
            serve_external: false,
            demo_pace: 1.0,
            plugin_dirs: vec![],
            secrets: None,
            observer: None,
        })
        .await;
        let details = Arc::new(Mutex::new(DeviceDetails::default()));
        rt.lock().devices.register(Box::new(Noticing {
            details: details.clone(),
            connected: false,
        }));
        rt.connect_device(ID).unwrap();
        assert_eq!(states.mode_of(ID).last(), Some(&Value::from("none")));

        *details.lock().unwrap() = DeviceDetails {
            mode: ModeState::Confirmed,
            ..DeviceDetails::default()
        };
        // The render loop draws every 40 ms; the next tick sees the change.
        for _ in 0..100 {
            if states.mode_of(ID).last() == Some(&Value::from("confirmed")) {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        assert_eq!(states.mode_of(ID).last(), Some(&Value::from("confirmed")));
        let events: Vec<String> = rt
            .snapshot()
            .recent_events
            .into_iter()
            .filter(|e| e.payload["deviceId"] == ID)
            .map(|e| e.kind)
            .collect();
        assert_eq!(events, [types::DEVICE_CONNECTED], "no event of its own");
    }
}
