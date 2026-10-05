//! Devices: drawing the active page on every connected device, and the
//! device list (connect, disconnect, rescan for hardware).

use serde_json::json;

use super::{control_key, Inner, Runtime};
use crate::devices::manager::{DeviceChange, DeviceInfo};
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
        let change = self.lock().devices.connect(id).map_err(|e| e.to_string())?;
        self.announce(vec![change]);
        Ok(())
    }

    pub fn disconnect_device(&self, id: &str) -> Result<(), String> {
        let change = self
            .lock()
            .devices
            .disconnect(id)
            .map_err(|e| e.to_string())?;
        self.announce(vec![change]);
        Ok(())
    }

    /// Look for Launchpad X hardware; connect newly found units.
    pub fn rescan(&self) {
        let mut changes = Vec::new();
        {
            let mut inner = self.lock();
            let (added, removed) = inner.devices.rescan_hardware();
            changes.extend(removed);
            for id in added {
                match inner.devices.connect(&id) {
                    Ok(change) => changes.push(change),
                    Err(e) => log::warn!("found `{id}` but could not connect: {e}"),
                }
            }
        }
        // A newly registered but unconnected device also changes the list.
        self.announce(changes);
        self.emit_state();
    }
}
