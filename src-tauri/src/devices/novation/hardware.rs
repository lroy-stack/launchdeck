//! `NovationHardware`: the Launchpads on the system's MIDI ports, as a
//! `HardwareSource` for the device manager.

use std::sync::Arc;

use super::launchpad_x::{self, DiscoveredDevice, LaunchpadX};
use crate::devices::midi::MidiBackend;
use crate::devices::source::{Found, HardwareScan, HardwareSource, UNSUPPORTED_MODEL};
use crate::devices::{DeviceIdentity, DeviceKind, SurfaceDevice};

pub(crate) struct NovationHardware {
    backend: Arc<dyn MidiBackend>,
    /// The units of the last look, to build their devices from.
    last: Vec<DiscoveredDevice>,
}

impl NovationHardware {
    pub(crate) fn new(backend: Arc<dyn MidiBackend>) -> Self {
        Self {
            backend,
            last: Vec::new(),
        }
    }
}

impl HardwareSource for NovationHardware {
    fn scan(&mut self) -> HardwareScan {
        let found = launchpad_x::discover(&self.backend.ports());
        let unsupported = found
            .unsupported
            .iter()
            .map(|name| DeviceIdentity {
                id: launchpad_x::unsupported_id(name),
                name: name.clone(),
                model: UNSUPPORTED_MODEL.into(),
                kind: DeviceKind::Hardware,
            })
            .collect();
        let units = found
            .units
            .iter()
            .map(|unit| Found {
                id: unit.id.clone(),
                name: unit.name.clone(),
            })
            .collect();
        self.last = found.units;
        HardwareScan { units, unsupported }
    }

    fn device(&self, id: &str) -> Option<Box<dyn SurfaceDevice>> {
        let unit = self.last.iter().find(|u| u.id == id)?;
        Some(Box::new(LaunchpadX::new(
            unit.clone(),
            self.backend.clone(),
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::devices::manager::{DeviceChange, DeviceManager, DeviceStatus};
    use crate::devices::midi::testkit::{port, FakeMidi};
    use tokio::sync::mpsc::unbounded_channel;

    const ENTER: [u8; 9] = [0xF0, 0x00, 0x20, 0x29, 0x02, 0x0C, 0x0E, 0x01, 0xF7];

    fn manager(midi: &FakeMidi) -> DeviceManager {
        let (tx, _rx) = unbounded_channel();
        let hardware = NovationHardware::new(Arc::new(midi.clone()));
        DeviceManager::with_hardware(tx, Box::new(hardware))
    }

    fn statuses(m: &DeviceManager) -> Vec<(String, DeviceStatus)> {
        m.list()
            .into_iter()
            .map(|d| (d.identity.id, d.status))
            .collect()
    }

    #[test]
    fn the_manager_finds_connects_and_loses_a_launchpad_x_on_fake_ports() {
        let midi = FakeMidi::default();
        midi.add(
            port("Launchpad X LPX MIDI Out", "1001"),
            port("Launchpad X LPX MIDI In", "1002"),
        );
        let mut m = manager(&midi);
        let id = "launchpad-x:launchpad-x-lpx-midi-out";

        let rescan = m.rescan_hardware(false);
        assert_eq!(rescan.to_connect, [id]);
        assert_eq!(m.connect(id).unwrap(), DeviceChange::Connected(id.into()));
        assert_eq!(statuses(&m), [(id.to_string(), DeviceStatus::Connected)]);
        assert_eq!(
            midi.take_sent().first(),
            Some(&("1002".to_string(), ENTER.to_vec())),
            "Programmer mode on the unit's output"
        );

        midi.remove("1001");
        midi.remove("1002");
        let gone = m.rescan_hardware(false);
        assert_eq!(gone.changes, [DeviceChange::Disconnected(id.into())]);
        assert_eq!(statuses(&m), [(id.to_string(), DeviceStatus::Unplugged)]);
    }
}
