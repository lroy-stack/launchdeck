//! `NovationHardware`: the Launchpads on the system's MIDI ports, as a
//! `HardwareSource` for the device manager. Which ports make a unit is
//! `ports`' answer for the system it runs on; which models are driven is
//! the list below.

use std::sync::Arc;

use super::launchpad_mini_mk3;
use super::launchpad_x::{self, LaunchpadX};
use super::ports::{self, FoundUnit};
use super::profile::ModelProfile;
use crate::devices::midi::MidiBackend;
use crate::devices::source::{Found, HardwareScan, HardwareSource};
use crate::devices::SurfaceDevice;
use crate::platform::Platform;

/// Every model recognised on the ports.
static KNOWN: [&ModelProfile; 2] = [&launchpad_x::PROFILE, &launchpad_mini_mk3::PROFILE];

/// The models LaunchDeck drives; the others are listed as not supported
/// yet. The Mini MK3 joins with package M6 of the Mini MK3 plan.
static DRIVEN: [&ModelProfile; 1] = [&launchpad_x::PROFILE];

pub(crate) struct NovationHardware {
    backend: Arc<dyn MidiBackend>,
    platform: Platform,
    /// The driven units of the last look, to build their devices from.
    last: Vec<FoundUnit>,
}

impl NovationHardware {
    pub(crate) fn new(backend: Arc<dyn MidiBackend>, platform: Platform) -> Self {
        Self {
            backend,
            platform,
            last: Vec::new(),
        }
    }
}

impl HardwareSource for NovationHardware {
    fn scan(&mut self) -> HardwareScan {
        let parsed = ports::parse(&self.backend.ports(), self.platform, &KNOWN);
        let (driven, waiting): (Vec<FoundUnit>, Vec<FoundUnit>) = parsed
            .units
            .into_iter()
            .partition(|u| DRIVEN.iter().any(|p| p.model == u.profile.model));
        let driven: Vec<FoundUnit> = driven.into_iter().map(e2_identity).collect();
        let unsupported = waiting
            .iter()
            .map(|u| ports::unsupported(u.profile.name, u.number))
            .chain(parsed.unsupported)
            .collect();
        for unit in driven.iter().filter(|u| !self.last.contains(u)) {
            log::info!(
                "{} `{}` on input `{}` and output `{}` (unit key {})",
                unit.name,
                unit.id,
                unit.input.name,
                unit.output.name,
                unit.key
            );
        }
        let units = driven
            .iter()
            .map(|u| Found {
                id: u.id.clone(),
                name: u.name.clone(),
            })
            .collect();
        self.last = driven;
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

/// E2's id (the model and the slug of the input's name) and the bare model
/// name, until stable unit ids replace them.
fn e2_identity(unit: FoundUnit) -> FoundUnit {
    FoundUnit {
        id: format!("{}:{}", unit.profile.model, ports::slug(&unit.input.name)),
        name: unit.profile.name.to_string(),
        ..unit
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
        manager_on(midi, Platform::MacOs)
    }

    fn manager_on(midi: &FakeMidi, platform: Platform) -> DeviceManager {
        let (tx, _rx) = unbounded_channel();
        let hardware = NovationHardware::new(Arc::new(midi.clone()), platform);
        DeviceManager::with_hardware(tx, Box::new(hardware))
    }

    /// What the runtime does after a look: connect every unit to connect.
    fn look(m: &mut DeviceManager) -> Vec<String> {
        let ids = m.rescan_hardware(true).to_connect;
        for id in &ids {
            m.connect(id).unwrap();
        }
        ids
    }

    /// The outputs that received bytes, once each.
    fn outputs_used(midi: &FakeMidi) -> Vec<String> {
        let mut used: Vec<String> = midi.take_sent().into_iter().map(|(id, _)| id).collect();
        used.dedup();
        used
    }

    fn listed(m: &DeviceManager) -> Vec<(String, String, DeviceStatus)> {
        m.list()
            .into_iter()
            .map(|d| (d.identity.id, d.identity.name, d.status))
            .collect()
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

    // Port names below are derived from docs and community projects, not
    // observed: no Windows machine, Linux desktop or Launchpad here.

    /// Windows interface paths: the DAW interface and the MIDI interface.
    const DAW_IF: &str = r"\\?\usb#vid_1235&pid_0103&mi_00#a";
    const MIDI_IF: &str = r"\\?\usb#vid_1235&pid_0103&mi_01#a";

    #[test]
    fn windows_classic_launchpad_x_is_one_unit_on_its_midi_interface() {
        // D-X1: `LPX MIDI` is the DAW interface; it was a second unit.
        let midi = FakeMidi::default();
        midi.set_ports(
            &[
                port("LPX MIDI", DAW_IF),
                port("MIDIIN2 (LPX MIDI)", MIDI_IF),
            ],
            &[
                port("LPX MIDI", DAW_IF),
                port("MIDIOUT2 (LPX MIDI)", MIDI_IF),
            ],
        );
        let mut m = manager_on(&midi, Platform::Windows);
        assert_eq!(look(&mut m).len(), 1);
        assert_eq!(outputs_used(&midi), [MIDI_IF]);
    }

    #[test]
    fn linux_before_5_13_launchpad_x_is_one_unit_on_midi_2() {
        // D-X2: both interfaces are named `MIDI n`; both were units.
        let midi = FakeMidi::default();
        let list = [
            port("Launchpad X:Launchpad X MIDI 1 28:0", "28:0"),
            port("Launchpad X:Launchpad X MIDI 2 28:1", "28:1"),
        ];
        midi.set_ports(&list, &list);
        let mut m = manager_on(&midi, Platform::Linux);
        assert_eq!(look(&mut m).len(), 1);
        assert_eq!(outputs_used(&midi), ["28:1"]);
    }

    #[test]
    fn a_mini_mk3_is_listed_once_and_never_opened() {
        // D-X4: on Linux it was listed per port, on Windows not at all.
        let mini = (
            "unsupported:launchpad-mini-mk3".to_string(),
            "Launchpad Mini MK3".to_string(),
            DeviceStatus::Unsupported,
        );
        let linux = [
            port(
                "Launchpad Mini MK3:Launchpad Mini MK3 LPMiniMK3 DA 20:0",
                "20:0",
            ),
            port(
                "Launchpad Mini MK3:Launchpad Mini MK3 LPMiniMK3 MI 20:1",
                "20:1",
            ),
        ];
        let windows = [
            port("LPMiniMK3 MIDI", DAW_IF),
            port("MIDIIN2 (LPMiniMK3 MIDI)", MIDI_IF),
        ];
        for (platform, list) in [(Platform::Linux, linux), (Platform::Windows, windows)] {
            let midi = FakeMidi::default();
            midi.set_ports(&list, &list);
            let mut m = manager_on(&midi, platform);
            assert!(look(&mut m).is_empty(), "{platform:?}");
            assert_eq!(listed(&m), std::slice::from_ref(&mini), "{platform:?}");
            assert_eq!(midi.open_links(), 0);
            assert!(midi.take_sent().is_empty());
        }
    }
}
