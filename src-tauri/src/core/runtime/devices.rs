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

    /// The person pressed Look again: also retry units whose connect failed.
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
