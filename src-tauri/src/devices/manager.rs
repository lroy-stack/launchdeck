//! Owns every `SurfaceDevice`, connects/disconnects them and fans frames out.

use std::collections::HashSet;

use serde::Serialize;

use super::launchpad_x::{self, LaunchpadX};
use super::{
    DeviceCapabilities, DeviceError, DeviceIdentity, DeviceKind, InputSender, SurfaceDevice,
    SurfaceFrame,
};

struct Entry {
    identity: DeviceIdentity,
    capabilities: DeviceCapabilities,
    device: Box<dyn SurfaceDevice>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DeviceInfo {
    pub identity: DeviceIdentity,
    pub capabilities: DeviceCapabilities,
    pub connected: bool,
}

/// Device lifecycle changes the runtime turns into `device.*` events.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeviceChange {
    Connected(String),
    Disconnected(String),
}

pub struct DeviceManager {
    entries: Vec<Entry>,
    input: InputSender,
}

impl DeviceManager {
    pub fn new(input: InputSender) -> Self {
        Self {
            entries: Vec::new(),
            input,
        }
    }

    pub fn register(&mut self, device: Box<dyn SurfaceDevice>) {
        let identity = device.identity();
        if self.entries.iter().any(|e| e.identity.id == identity.id) {
            return;
        }
        let capabilities = device.capabilities();
        self.entries.push(Entry {
            identity,
            capabilities,
            device,
        });
    }

    pub fn list(&self) -> Vec<DeviceInfo> {
        self.entries
            .iter()
            .map(|e| DeviceInfo {
                identity: e.identity.clone(),
                capabilities: e.capabilities,
                connected: e.device.is_connected(),
            })
            .collect()
    }

    fn entry_mut(&mut self, id: &str) -> Result<&mut Entry, DeviceError> {
        self.entries
            .iter_mut()
            .find(|e| e.identity.id == id)
            .ok_or_else(|| DeviceError::Port(format!("unknown device `{id}`")))
    }

    pub fn connect(&mut self, id: &str) -> Result<DeviceChange, DeviceError> {
        let input = self.input.clone();
        let entry = self.entry_mut(id)?;
        entry.device.connect(input)?;
        Ok(DeviceChange::Connected(id.to_string()))
    }

    pub fn disconnect(&mut self, id: &str) -> Result<DeviceChange, DeviceError> {
        let entry = self.entry_mut(id)?;
        entry.device.disconnect()?;
        Ok(DeviceChange::Disconnected(id.to_string()))
    }

    /// Capabilities of every connected device (for the render loop).
    pub fn connected_capabilities(&self) -> Vec<(String, DeviceCapabilities)> {
        self.entries
            .iter()
            .filter(|e| e.device.is_connected())
            .map(|e| (e.identity.id.clone(), e.capabilities))
            .collect()
    }

    /// Render one frame to one device. A failing hardware device is
    /// disconnected and reported so the runtime can emit `device.disconnected`.
    pub fn render(&mut self, id: &str, frame: &SurfaceFrame) -> Option<DeviceChange> {
        let entry = self.entries.iter_mut().find(|e| e.identity.id == id)?;
        match entry.device.render(frame) {
            Ok(()) => None,
            Err(e) => {
                log::warn!("render to `{id}` failed: {e}");
                let _ = entry.device.disconnect();
                Some(DeviceChange::Disconnected(id.to_string()))
            }
        }
    }

    /// Find attached Launchpad X units and forget unplugged, unconnected ones.
    /// Returns ids of newly registered hardware devices.
    pub fn rescan_hardware(&mut self) -> (Vec<String>, Vec<DeviceChange>) {
        let found = launchpad_x::discover();
        let found_ids: HashSet<String> = found.iter().map(|d| d.id.clone()).collect();
        let mut added = Vec::new();
        for d in found {
            if !self.entries.iter().any(|e| e.identity.id == d.id) {
                added.push(d.id.clone());
                self.register(Box::new(LaunchpadX::new(d)));
            }
        }
        let mut changes = Vec::new();
        self.entries.retain_mut(|e| {
            let gone =
                e.identity.kind == DeviceKind::Hardware && !found_ids.contains(&e.identity.id);
            if gone && e.device.is_connected() {
                let _ = e.device.disconnect();
                changes.push(DeviceChange::Disconnected(e.identity.id.clone()));
            }
            !gone
        });
        (added, changes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::devices::virtual_device::{VirtualLaunchpadX, VIRTUAL_ID};
    use tokio::sync::mpsc::unbounded_channel;

    #[test]
    fn registers_connects_and_reports_devices() {
        let (tx, _rx) = unbounded_channel();
        let mut m = DeviceManager::new(tx);
        let (dev, _handle) = VirtualLaunchpadX::new();
        m.register(Box::new(dev));
        let (dup, _h2) = VirtualLaunchpadX::new();
        m.register(Box::new(dup));
        assert_eq!(m.list().len(), 1, "same id is registered once");
        assert!(!m.list()[0].connected);
        assert_eq!(
            m.connect(VIRTUAL_ID).unwrap(),
            DeviceChange::Connected(VIRTUAL_ID.into())
        );
        assert!(m.list()[0].connected);
        assert_eq!(m.connected_capabilities().len(), 1);
        m.disconnect(VIRTUAL_ID).unwrap();
        assert!(m.connect("nope").is_err());
    }

    #[test]
    fn render_failure_disconnects_the_device() {
        let (tx, _rx) = unbounded_channel();
        let mut m = DeviceManager::new(tx);
        let (dev, _handle) = VirtualLaunchpadX::new();
        m.register(Box::new(dev));
        // Not connected: rendering fails, which the manager reports as a disconnect.
        assert_eq!(
            m.render(VIRTUAL_ID, &SurfaceFrame::default()),
            Some(DeviceChange::Disconnected(VIRTUAL_ID.into()))
        );
    }
}
