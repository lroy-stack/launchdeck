//! Owns every `SurfaceDevice`, connects/disconnects them and fans frames out,
//! and says what state each one is in.

use std::collections::HashSet;
use std::sync::Arc;

use serde::Serialize;

use super::midi::SystemMidi;
use super::novation::launchpad_x::{self, SystemPorts};
use super::{
    AnimationCaps, AuxCaps, DeviceCapabilities, DeviceError, DeviceIdentity, DeviceKind,
    InputSender, PortSource, SurfaceDevice, SurfaceFrame,
};

/// What a person is told after a connect attempt failed. The operating
/// system's text goes to the log and to the error of the connect command.
pub(crate) const CONNECT_PROBLEM: &str = "LaunchDeck could not open the Launchpad. Another program may be using it; close it and press Look again.";

struct Entry {
    identity: DeviceIdentity,
    capabilities: DeviceCapabilities,
    device: Box<dyn SurfaceDevice>,
    /// It connected and the person has not disconnected it since. A unit in
    /// use that goes missing is unplugged, and is connected again when found.
    in_use: bool,
    /// Missing from the last look at the ports while in use.
    unplugged: bool,
    /// Why the last connect attempt failed, until one succeeds.
    problem: Option<String>,
}

impl Entry {
    fn status(&self) -> DeviceStatus {
        if self.device.is_connected() {
            DeviceStatus::Connected
        } else if self.unplugged {
            DeviceStatus::Unplugged
        } else if self.problem.is_some() {
            DeviceStatus::Failed
        } else {
            DeviceStatus::Available
        }
    }

    fn info(&self) -> DeviceInfo {
        let status = self.status();
        DeviceInfo {
            identity: self.identity.clone(),
            capabilities: self.capabilities,
            connected: status == DeviceStatus::Connected,
            status,
            problem: (status == DeviceStatus::Failed)
                .then(|| self.problem.clone())
                .flatten(),
        }
    }
}

/// Where a device stands, for the person.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum DeviceStatus {
    Connected,
    /// Found, not connected.
    Available,
    /// Was in use and is no longer found; connects again when it returns.
    Unplugged,
    /// Another Launchpad model: listed by name, never opened.
    Unsupported,
    /// The last connect attempt failed; `problem` says why.
    Failed,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DeviceInfo {
    pub identity: DeviceIdentity,
    pub capabilities: DeviceCapabilities,
    /// `status == connected`, kept for older readers.
    pub connected: bool,
    pub status: DeviceStatus,
    /// A sentence for the person when something went wrong.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub problem: Option<String>,
}

/// Device lifecycle changes the runtime turns into `device.*` events.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeviceChange {
    Connected(String),
    Disconnected(String),
}

/// What one look at the ports changed.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Rescan {
    /// Units to connect: new ones, ones that came back, and failed ones when retried.
    pub to_connect: Vec<String>,
    /// Units that went away while connected.
    pub changes: Vec<DeviceChange>,
}

pub struct DeviceManager {
    entries: Vec<Entry>,
    /// Other Launchpad models from the last look, by name only.
    unsupported: Vec<DeviceIdentity>,
    ports: Box<dyn PortSource>,
    input: InputSender,
}

impl DeviceManager {
    pub fn new(input: InputSender) -> Self {
        Self::with_ports(input, Box::new(SystemPorts::new(Arc::new(SystemMidi))))
    }

    pub(crate) fn with_ports(input: InputSender, ports: Box<dyn PortSource>) -> Self {
        Self {
            entries: Vec::new(),
            unsupported: Vec::new(),
            ports,
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
            in_use: false,
            unplugged: false,
            problem: None,
        });
    }

    pub fn list(&self) -> Vec<DeviceInfo> {
        let unsupported = self.unsupported.iter().map(|identity| DeviceInfo {
            identity: identity.clone(),
            capabilities: NO_CAPABILITIES,
            connected: false,
            status: DeviceStatus::Unsupported,
            problem: None,
        });
        self.entries
            .iter()
            .map(Entry::info)
            .chain(unsupported)
            .collect()
    }

    fn entry_mut(&mut self, id: &str) -> Result<&mut Entry, DeviceError> {
        if self.unsupported.iter().any(|u| u.id == id) {
            return Err(DeviceError::Unsupported(id.to_string()));
        }
        self.entries
            .iter_mut()
            .find(|e| e.identity.id == id)
            .ok_or_else(|| DeviceError::Unknown(id.to_string()))
    }

    /// Connect a device. A failure is remembered as the device's problem
    /// until a later attempt succeeds or the person disconnects it.
    pub fn connect(&mut self, id: &str) -> Result<DeviceChange, DeviceError> {
        let input = self.input.clone();
        let entry = self.entry_mut(id)?;
        match entry.device.connect(input) {
            Ok(()) => {
                entry.in_use = true;
                entry.unplugged = false;
                entry.problem = None;
                Ok(DeviceChange::Connected(id.to_string()))
            }
            Err(e) => {
                log::warn!(
                    "could not connect `{id}`: {e} ({})",
                    e.detail().unwrap_or("no detail")
                );
                // A half-open connection would read as connected.
                let _ = entry.device.disconnect();
                entry.problem = Some(CONNECT_PROBLEM.to_string());
                Err(e)
            }
        }
    }

    pub fn disconnect(&mut self, id: &str) -> Result<DeviceChange, DeviceError> {
        let entry = self.entry_mut(id)?;
        entry.device.disconnect()?;
        entry.in_use = false;
        entry.problem = None;
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
                log::warn!(
                    "render to `{id}` failed: {e} ({})",
                    e.detail().unwrap_or("")
                );
                let _ = entry.device.disconnect();
                Some(DeviceChange::Disconnected(id.to_string()))
            }
        }
    }

    /// Look at the system's ports. New Launchpad X units and units in use
    /// that are found again are returned to connect; `retry_failed` adds the
    /// ones whose last attempt failed (the person pressed Look again). A unit
    /// in use that is missing stays listed as unplugged; one that was never
    /// in use is forgotten. Other Launchpad models are listed by name.
    pub fn rescan_hardware(&mut self, retry_failed: bool) -> Rescan {
        let found = launchpad_x::discover(&self.ports.list());
        self.unsupported = found
            .unsupported
            .iter()
            .map(|name| DeviceIdentity {
                id: launchpad_x::unsupported_id(name),
                name: name.clone(),
                model: UNSUPPORTED_MODEL.into(),
                kind: DeviceKind::Hardware,
            })
            .collect();
        let found_ids: HashSet<String> = found.units.iter().map(|d| d.id.clone()).collect();
        let mut rescan = Rescan::default();
        for unit in found.units {
            let id = unit.id.clone();
            let Some(entry) = self.entries.iter_mut().find(|e| e.identity.id == id) else {
                self.register(self.ports.launchpad_x(unit));
                rescan.to_connect.push(id);
                continue;
            };
            // A failed attempt waits for Look again instead of repeating every few seconds.
            let retry = if entry.problem.is_some() {
                retry_failed
            } else {
                entry.in_use
            };
            if retry && !entry.device.is_connected() {
                // A fresh device: the ports may have changed while it was away.
                entry.device = self.ports.launchpad_x(unit);
                entry.unplugged = false;
                rescan.to_connect.push(id);
            }
        }
        self.entries.retain_mut(|e| {
            if e.identity.kind != DeviceKind::Hardware || found_ids.contains(&e.identity.id) {
                return true;
            }
            if e.device.is_connected() {
                let _ = e.device.disconnect();
                rescan
                    .changes
                    .push(DeviceChange::Disconnected(e.identity.id.clone()));
            }
            e.unplugged = e.in_use;
            e.in_use
        });
        rescan
    }
}

/// The model of a Launchpad LaunchDeck does not drive.
const UNSUPPORTED_MODEL: &str = "unsupported";

const NO_CAPABILITIES: DeviceCapabilities = DeviceCapabilities {
    grid: None,
    aux: AuxCaps {
        top: 0,
        side: 0,
        logo: false,
    },
    rgb: false,
    velocity: false,
    pressure: false,
    animations: AnimationCaps {
        pulse: false,
        flash: false,
    },
};

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

    #[test]
    fn the_virtual_device_reads_connected_and_unknown_ids_are_refused_in_a_sentence() {
        let (tx, _rx) = unbounded_channel();
        let mut m = DeviceManager::with_ports(tx, Box::new(FakePorts::default()));
        let (dev, _handle) = VirtualLaunchpadX::new();
        m.register(Box::new(dev));
        m.connect(VIRTUAL_ID).unwrap();
        let info = &m.list()[0];
        assert_eq!(info.identity.kind, DeviceKind::Virtual);
        assert_eq!(
            (info.status, info.connected),
            (DeviceStatus::Connected, true)
        );
        assert_eq!(info.problem, None);
        let err = m.connect("nope").unwrap_err();
        assert_eq!(
            err.to_string(),
            "LaunchDeck does not know this device. Press Look again."
        );
        assert_eq!(err.detail(), Some("nope"));
        // The virtual device is never part of a hardware rescan.
        m.rescan_hardware(true);
        assert_eq!(m.list().len(), 1);
    }

    // ---- hardware, with made-up ports --------------------------------------

    use crate::devices::novation::launchpad_x::DiscoveredDevice;
    use crate::devices::{PortNames, PortSource};
    use std::sync::{Arc, Mutex};

    const LPX: &str = "Launchpad X LPX MIDI";
    const LPX_ID: &str = "launchpad-x:launchpad-x-lpx-midi";

    /// Ports the test sets, and devices whose connect succeeds unless the
    /// port is marked busy. Records every port a device was made for.
    #[derive(Default, Clone)]
    struct FakePorts(Arc<Mutex<FakeState>>);

    #[derive(Default)]
    struct FakeState {
        names: Vec<String>,
        busy: bool,
        made_for: Vec<String>,
    }

    impl FakePorts {
        fn set(&self, names: &[&str]) {
            self.0.lock().unwrap().names = names.iter().map(|n| n.to_string()).collect();
        }
        fn busy(&self, busy: bool) {
            self.0.lock().unwrap().busy = busy;
        }
        fn made_for(&self) -> Vec<String> {
            self.0.lock().unwrap().made_for.clone()
        }
    }

    impl PortSource for FakePorts {
        fn list(&self) -> PortNames {
            let names = self.0.lock().unwrap().names.clone();
            PortNames {
                inputs: names.clone(),
                outputs: names,
            }
        }
        fn launchpad_x(&self, found: DiscoveredDevice) -> Box<dyn SurfaceDevice> {
            self.0.lock().unwrap().made_for.push(found.in_port.clone());
            Box::new(FakeUnit {
                found,
                ports: self.clone(),
                connected: false,
            })
        }
    }

    struct FakeUnit {
        found: DiscoveredDevice,
        ports: FakePorts,
        connected: bool,
    }

    impl SurfaceDevice for FakeUnit {
        fn identity(&self) -> DeviceIdentity {
            DeviceIdentity {
                id: self.found.id.clone(),
                name: self.found.name.clone(),
                model: "launchpad-x".into(),
                kind: DeviceKind::Hardware,
            }
        }
        fn capabilities(&self) -> DeviceCapabilities {
            launchpad_x::capabilities()
        }
        fn connect(&mut self, _input: InputSender) -> Result<(), DeviceError> {
            let state = self.ports.0.lock().unwrap();
            if state.busy || !state.names.contains(&self.found.out_port) {
                return Err(DeviceError::Port("port is in use by another client".into()));
            }
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
            let present = self
                .ports
                .0
                .lock()
                .unwrap()
                .names
                .contains(&self.found.out_port);
            match (self.connected, present) {
                (true, true) => Ok(()),
                (true, false) => Err(DeviceError::Midi("device unplugged".into())),
                (false, _) => Err(DeviceError::NotConnected),
            }
        }
        fn clear(&mut self) -> Result<(), DeviceError> {
            Ok(())
        }
    }

    fn manager(ports: &FakePorts) -> DeviceManager {
        let (tx, _rx) = unbounded_channel();
        DeviceManager::with_ports(tx, Box::new(ports.clone()))
    }

    /// What the runtime does after a look at the ports.
    fn look(m: &mut DeviceManager, retry_failed: bool) -> Rescan {
        let rescan = m.rescan_hardware(retry_failed);
        for id in &rescan.to_connect {
            let _ = m.connect(id);
        }
        rescan
    }

    fn status_of(m: &DeviceManager, id: &str) -> Option<(DeviceStatus, Option<String>)> {
        m.list()
            .into_iter()
            .find(|d| d.identity.id == id)
            .map(|d| (d.status, d.problem))
    }

    #[test]
    fn a_found_unit_connects_and_an_unplugged_one_stays_listed_until_it_returns() {
        let ports = FakePorts::default();
        let mut m = manager(&ports);
        ports.set(&[LPX]);
        assert_eq!(look(&mut m, false).to_connect, [LPX_ID]);
        assert_eq!(status_of(&m, LPX_ID), Some((DeviceStatus::Connected, None)));

        ports.set(&[]);
        let gone = look(&mut m, false);
        assert_eq!(gone.changes, [DeviceChange::Disconnected(LPX_ID.into())]);
        assert_eq!(status_of(&m, LPX_ID), Some((DeviceStatus::Unplugged, None)));
        assert!(!m.list()[0].connected);
        assert!(look(&mut m, false).changes.is_empty(), "reported once");
        assert_eq!(status_of(&m, LPX_ID), Some((DeviceStatus::Unplugged, None)));

        ports.set(&[LPX]);
        assert_eq!(look(&mut m, false).to_connect, [LPX_ID]);
        assert_eq!(status_of(&m, LPX_ID), Some((DeviceStatus::Connected, None)));
        assert_eq!(m.list().len(), 1);
    }

    #[test]
    fn a_unit_that_failed_drawing_before_the_rescan_is_unplugged_too() {
        let ports = FakePorts::default();
        let mut m = manager(&ports);
        ports.set(&[LPX]);
        look(&mut m, false);
        // Pulled out: the next frame fails before the next look at the ports.
        ports.set(&[]);
        assert_eq!(
            m.render(LPX_ID, &SurfaceFrame::default()),
            Some(DeviceChange::Disconnected(LPX_ID.into()))
        );
        assert!(look(&mut m, false).changes.is_empty(), "already reported");
        assert_eq!(status_of(&m, LPX_ID), Some((DeviceStatus::Unplugged, None)));
        // Back, but busy: failed, and left alone until Look again.
        ports.set(&[LPX]);
        ports.busy(true);
        assert_eq!(look(&mut m, false).to_connect, [LPX_ID]);
        assert_eq!(status_of(&m, LPX_ID).unwrap().0, DeviceStatus::Failed);
        assert!(look(&mut m, false).to_connect.is_empty());
        ports.busy(false);
        look(&mut m, true);
        assert_eq!(status_of(&m, LPX_ID), Some((DeviceStatus::Connected, None)));
    }

    #[test]
    fn a_unit_the_person_disconnected_is_forgotten_when_it_goes() {
        let ports = FakePorts::default();
        let mut m = manager(&ports);
        ports.set(&[LPX]);
        look(&mut m, false);
        m.disconnect(LPX_ID).unwrap();
        assert_eq!(status_of(&m, LPX_ID), Some((DeviceStatus::Available, None)));
        assert!(
            look(&mut m, false).to_connect.is_empty(),
            "stays disconnected"
        );
        ports.set(&[]);
        look(&mut m, false);
        assert_eq!(status_of(&m, LPX_ID), None);
    }

    #[test]
    fn a_failed_connect_is_reported_with_a_problem_and_retried_on_look_again() {
        let ports = FakePorts::default();
        let mut m = manager(&ports);
        ports.set(&[LPX]);
        ports.busy(true);
        let rescan = m.rescan_hardware(false);
        let err = m.connect(&rescan.to_connect[0]).unwrap_err();
        assert_eq!(err.to_string(), "The Launchpad could not be reached.");
        assert_eq!(err.detail(), Some("port is in use by another client"));
        assert_eq!(
            status_of(&m, LPX_ID),
            Some((DeviceStatus::Failed, Some(CONNECT_PROBLEM.into())))
        );
        let info = m.list().remove(0);
        assert!(!info.connected);
        assert!(!info.problem.unwrap().to_lowercase().contains("midi"));

        // The periodic look does not hammer a busy unit; Look again retries it.
        assert!(look(&mut m, false).to_connect.is_empty());
        ports.busy(false);
        assert_eq!(look(&mut m, true).to_connect, [LPX_ID]);
        assert_eq!(status_of(&m, LPX_ID), Some((DeviceStatus::Connected, None)));
    }

    #[test]
    fn other_launchpads_are_listed_by_name_and_never_opened() {
        let ports = FakePorts::default();
        let mut m = manager(&ports);
        ports.set(&[
            LPX,
            "Launchpad Mini MK3 LPMiniMK3 MIDI",
            "Launchpad Mini MK3 LPMiniMK3 DAW",
            "Launchpad Pro MK3",
            "IAC Driver Bus 1",
        ]);
        assert_eq!(look(&mut m, true).to_connect, [LPX_ID]);
        assert_eq!(ports.made_for(), [LPX], "only the Launchpad X got a device");
        let list = m.list();
        let names: Vec<(&str, DeviceStatus)> = list
            .iter()
            .map(|d| (d.identity.name.as_str(), d.status))
            .collect();
        assert_eq!(
            names,
            [
                ("Launchpad X", DeviceStatus::Connected),
                (
                    "Launchpad Mini MK3 LPMiniMK3 MIDI",
                    DeviceStatus::Unsupported
                ),
                ("Launchpad Pro MK3", DeviceStatus::Unsupported),
            ],
            "the IAC bus is not a Launchpad"
        );
        assert!(list[1..].iter().all(|d| !d.connected
            && d.capabilities.grid.is_none()
            && d.identity.kind == DeviceKind::Hardware));

        let pro = "unsupported:launchpad-pro-mk3";
        let err = m.connect(pro).unwrap_err();
        assert_eq!(
            err.to_string(),
            "Not supported yet: LaunchDeck works with the Launchpad X."
        );
        assert!(m.disconnect(pro).is_err());
        assert_eq!(ports.made_for(), [LPX], "a connect request opens nothing");

        ports.set(&[LPX]);
        look(&mut m, true);
        assert_eq!(m.list().len(), 1, "gone from the list when unplugged");
    }

    #[test]
    fn device_info_serializes_status_and_problem_in_camel_case() {
        let ports = FakePorts::default();
        let mut m = manager(&ports);
        ports.set(&[LPX, "Launchpad Pro MK3"]);
        ports.busy(true);
        look(&mut m, false);
        let json = serde_json::to_value(m.list()).unwrap();
        assert_eq!(json[0]["status"], "failed");
        assert_eq!(json[0]["connected"], false);
        assert_eq!(json[0]["problem"], CONNECT_PROBLEM);
        assert_eq!(json[1]["status"], "unsupported");
        assert!(json[1].get("problem").is_none());
    }
}
