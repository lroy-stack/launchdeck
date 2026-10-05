//! Owns every `SurfaceDevice`, connects/disconnects them and fans frames out,
//! and says what state each one is in.

use std::collections::HashSet;
use std::sync::Arc;

use serde::Serialize;

use super::midi::SystemMidi;
use super::novation::NovationHardware;
use super::source::HardwareSource;
use super::{
    AnimationCaps, AuxCaps, DeviceCapabilities, DeviceDetails, DeviceError, DeviceIdentity,
    DeviceKind, InputSender, SurfaceDevice, SurfaceFrame,
};
use crate::platform::Platform;

/// What a person is told after a connect attempt failed. The operating
/// system's text goes to the log and to the error of the connect command.
pub(crate) const CONNECT_PROBLEM: &str = "Launchpad detected, but its control port is unavailable. Another program may be using it: close it and press Look again.";

struct Entry {
    identity: DeviceIdentity,
    capabilities: DeviceCapabilities,
    device: Box<dyn SurfaceDevice>,
    /// It connected and the person has not disconnected it since. A unit in
    /// use that goes missing is unplugged, and is connected again when found.
    in_use: bool,
    /// Missing from the last look at the hardware while in use.
    unplugged: bool,
    /// Why the last connect attempt failed or the device turned LaunchDeck
    /// down, until a connect succeeds.
    problem: Option<String>,
    /// The device's details as the runtime was last told them.
    announced: DeviceDetails,
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
        let details = self.device.details();
        DeviceInfo {
            identity: self.identity.clone(),
            capabilities: self.capabilities,
            connected: status == DeviceStatus::Connected,
            status,
            problem: (status == DeviceStatus::Failed)
                .then(|| self.problem.clone())
                .flatten(),
            notice: (status == DeviceStatus::Connected)
                .then(|| details.notice.clone())
                .flatten(),
            details,
        }
    }

    /// Whether the device's details changed since the runtime was last
    /// told them; they count as told from here on.
    fn details_changed(&mut self) -> bool {
        let details = self.device.details();
        if details == self.announced {
            return false;
        }
        self.announced = details;
        true
    }
}

/// The problem a person is shown for a failed connect or a refusal.
fn problem(error: &DeviceError) -> String {
    if error.is_refusal() {
        error.to_string()
    } else {
        CONNECT_PROBLEM.to_string()
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
    /// The last connect attempt failed, or the device turned LaunchDeck
    /// down (bootloader, another model, taken over); `problem` says why.
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
    /// Connected, with a limitation, in a sentence (`details.notice` while
    /// connected).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notice: Option<String>,
    pub details: DeviceDetails,
}

/// Device lifecycle changes the runtime turns into `device.*` events, and
/// changed details, which only change the device list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeviceChange {
    Connected(String),
    Disconnected(String),
    /// What LaunchDeck observed on the device changed (`DeviceDetails`).
    Updated(String),
}

/// What one look at the hardware changed.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Rescan {
    /// Units to connect: new ones, ones that came back, and failed ones when retried.
    pub to_connect: Vec<String>,
    /// Units that went away while connected.
    pub changes: Vec<DeviceChange>,
}

pub struct DeviceManager {
    entries: Vec<Entry>,
    /// Devices of other models from the last look, by name only.
    unsupported: Vec<DeviceIdentity>,
    hardware: Box<dyn HardwareSource>,
    input: InputSender,
}

impl DeviceManager {
    pub fn new(input: InputSender) -> Self {
        let hardware = NovationHardware::new(Arc::new(SystemMidi), Platform::current());
        Self::with_hardware(input, Box::new(hardware))
    }

    pub(crate) fn with_hardware(input: InputSender, hardware: Box<dyn HardwareSource>) -> Self {
        Self {
            entries: Vec::new(),
            unsupported: Vec::new(),
            hardware,
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
            announced: DeviceDetails::default(),
        });
    }

    pub fn list(&self) -> Vec<DeviceInfo> {
        let unsupported = self.unsupported.iter().map(|identity| DeviceInfo {
            identity: identity.clone(),
            capabilities: NO_CAPABILITIES,
            connected: false,
            status: DeviceStatus::Unsupported,
            problem: None,
            notice: None,
            details: DeviceDetails::default(),
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
                entry.announced = entry.device.details();
                Ok(DeviceChange::Connected(id.to_string()))
            }
            Err(e) => {
                log::warn!(
                    "could not connect `{id}`: {e} ({})",
                    e.detail().unwrap_or("no detail")
                );
                // A half-open connection would read as connected.
                let _ = entry.device.disconnect();
                entry.problem = Some(problem(&e));
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
    /// disconnected and reported so the runtime can emit `device.disconnected`;
    /// one that turned LaunchDeck down keeps its sentence as its problem
    /// until Look again. Changed details are reported as `Updated`.
    pub fn render(&mut self, id: &str, frame: &SurfaceFrame) -> Option<DeviceChange> {
        let entry = self.entries.iter_mut().find(|e| e.identity.id == id)?;
        match entry.device.render(frame) {
            Ok(()) => entry
                .details_changed()
                .then(|| DeviceChange::Updated(id.to_string())),
            Err(e) => {
                log::warn!(
                    "render to `{id}` failed: {e} ({})",
                    e.detail().unwrap_or("")
                );
                let _ = entry.device.disconnect();
                if e.is_refusal() {
                    entry.problem = Some(problem(&e));
                }
                Some(DeviceChange::Disconnected(id.to_string()))
            }
        }
    }

    /// Look at what is plugged in. New units and units in use that are
    /// found again are returned to connect; `retry_failed` adds the ones
    /// whose last attempt failed (the person pressed Look again). A unit in
    /// use that is missing stays listed as unplugged; one that was never in
    /// use is forgotten. Devices of other models are listed by name.
    pub fn rescan_hardware(&mut self, retry_failed: bool) -> Rescan {
        let scan = self.hardware.scan();
        self.unsupported = scan.unsupported;
        let found_ids: HashSet<String> = scan.units.iter().map(|u| u.id.clone()).collect();
        let mut rescan = Rescan::default();
        for unit in scan.units {
            let id = unit.id;
            let Some(entry) = self.entries.iter_mut().find(|e| e.identity.id == id) else {
                if let Some(device) = self.hardware.device(&id) {
                    log::info!("found {} as `{id}`", unit.name);
                    self.register(device);
                    rescan.to_connect.push(id);
                }
                continue;
            };
            // A unit in use that went missing connects again when it returns,
            // whatever its last attempt said: it was plugged in again. A
            // failed attempt or a refusal otherwise waits for Look again
            // instead of repeating every few seconds.
            let retry = if entry.problem.is_some() && !entry.unplugged {
                retry_failed
            } else {
                entry.in_use
            };
            if retry && !entry.device.is_connected() {
                // A fresh device: its ports may have changed while it was away.
                let Some(device) = self.hardware.device(&id) else {
                    continue;
                };
                entry.device = device;
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
        let mut m = DeviceManager::with_hardware(tx, Box::new(FakeHardware::default()));
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

    // ---- hardware, with made-up units --------------------------------------

    use crate::devices::source::{Found, HardwareScan, HardwareSource, UNSUPPORTED_MODEL};
    use crate::devices::{DeviceDetails, ModeState};
    use std::sync::{Arc, Mutex};

    const LPX_ID: &str = "launchpad-x:1";

    const GRID: DeviceCapabilities = DeviceCapabilities {
        grid: Some(crate::devices::GridCaps {
            width: 8,
            height: 8,
        }),
        ..NO_CAPABILITIES
    };

    /// Units the test plugs in, and devices whose connect succeeds unless the
    /// unit is marked busy. Records every unit a device was made for.
    #[derive(Default, Clone)]
    struct FakeHardware(Arc<Mutex<FakeState>>);

    #[derive(Default)]
    struct FakeState {
        plugged: Vec<String>,
        others: Vec<DeviceIdentity>,
        busy: bool,
        made_for: Vec<String>,
        /// What every unit reports it observed.
        details: DeviceDetails,
        /// Every unit turns LaunchDeck down at its next render.
        refuse: bool,
    }

    impl FakeHardware {
        fn plug(&self, ids: &[&str]) {
            self.0.lock().unwrap().plugged = ids.iter().map(|id| id.to_string()).collect();
        }
        /// Devices of other models, by name.
        fn others(&self, names: &[&str]) {
            self.0.lock().unwrap().others = names
                .iter()
                .map(|name| DeviceIdentity {
                    id: format!("unsupported:{}", name.to_lowercase().replace(' ', "-")),
                    name: name.to_string(),
                    model: UNSUPPORTED_MODEL.into(),
                    kind: DeviceKind::Hardware,
                })
                .collect();
        }
        fn busy(&self, busy: bool) {
            self.0.lock().unwrap().busy = busy;
        }
        fn details(&self, details: DeviceDetails) {
            self.0.lock().unwrap().details = details;
        }
        fn refuse(&self, refuse: bool) {
            self.0.lock().unwrap().refuse = refuse;
        }
        fn made_for(&self) -> Vec<String> {
            self.0.lock().unwrap().made_for.clone()
        }
        fn plugged(&self, id: &str) -> bool {
            self.0.lock().unwrap().plugged.iter().any(|p| p == id)
        }
    }

    impl HardwareSource for FakeHardware {
        fn scan(&mut self) -> HardwareScan {
            let state = self.0.lock().unwrap();
            HardwareScan {
                units: state
                    .plugged
                    .iter()
                    .map(|id| Found {
                        id: id.clone(),
                        name: "Launchpad X".into(),
                    })
                    .collect(),
                unsupported: state.others.clone(),
            }
        }
        fn device(&self, id: &str) -> Option<Box<dyn SurfaceDevice>> {
            if !self.plugged(id) {
                return None;
            }
            self.0.lock().unwrap().made_for.push(id.to_string());
            Some(Box::new(FakeUnit {
                id: id.to_string(),
                hardware: self.clone(),
                connected: false,
            }))
        }
    }

    struct FakeUnit {
        id: String,
        hardware: FakeHardware,
        connected: bool,
    }

    impl SurfaceDevice for FakeUnit {
        fn identity(&self) -> DeviceIdentity {
            DeviceIdentity {
                id: self.id.clone(),
                name: "Launchpad X".into(),
                model: "launchpad-x".into(),
                kind: DeviceKind::Hardware,
            }
        }
        fn capabilities(&self) -> DeviceCapabilities {
            GRID
        }
        fn connect(&mut self, _input: InputSender) -> Result<(), DeviceError> {
            if self.hardware.0.lock().unwrap().busy || !self.hardware.plugged(&self.id) {
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
            if self.connected && self.hardware.0.lock().unwrap().refuse {
                return Err(DeviceError::Bootloader(BOOTLOADER_DETAIL.into()));
            }
            match (self.connected, self.hardware.plugged(&self.id)) {
                (true, true) => Ok(()),
                (true, false) => Err(DeviceError::Midi("device unplugged".into())),
                (false, _) => Err(DeviceError::NotConnected),
            }
        }
        fn clear(&mut self) -> Result<(), DeviceError> {
            Ok(())
        }
        fn details(&self) -> DeviceDetails {
            self.hardware.0.lock().unwrap().details.clone()
        }
    }

    const BOOTLOADER_DETAIL: &str = "bootloader 0102";

    fn manager(hardware: &FakeHardware) -> DeviceManager {
        let (tx, _rx) = unbounded_channel();
        DeviceManager::with_hardware(tx, Box::new(hardware.clone()))
    }

    /// What the runtime does after a look at the hw.
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
        let hw = FakeHardware::default();
        let mut m = manager(&hw);
        hw.plug(&[LPX_ID]);
        assert_eq!(look(&mut m, false).to_connect, [LPX_ID]);
        assert_eq!(status_of(&m, LPX_ID), Some((DeviceStatus::Connected, None)));

        hw.plug(&[]);
        let gone = look(&mut m, false);
        assert_eq!(gone.changes, [DeviceChange::Disconnected(LPX_ID.into())]);
        assert_eq!(status_of(&m, LPX_ID), Some((DeviceStatus::Unplugged, None)));
        assert!(!m.list()[0].connected);
        assert!(look(&mut m, false).changes.is_empty(), "reported once");
        assert_eq!(status_of(&m, LPX_ID), Some((DeviceStatus::Unplugged, None)));

        hw.plug(&[LPX_ID]);
        assert_eq!(look(&mut m, false).to_connect, [LPX_ID]);
        assert_eq!(status_of(&m, LPX_ID), Some((DeviceStatus::Connected, None)));
        assert_eq!(m.list().len(), 1);
    }

    #[test]
    fn a_unit_that_failed_drawing_before_the_rescan_is_unplugged_too() {
        let hw = FakeHardware::default();
        let mut m = manager(&hw);
        hw.plug(&[LPX_ID]);
        look(&mut m, false);
        // Pulled out: the next frame fails before the next look at the hw.
        hw.plug(&[]);
        assert_eq!(
            m.render(LPX_ID, &SurfaceFrame::default()),
            Some(DeviceChange::Disconnected(LPX_ID.into()))
        );
        assert!(look(&mut m, false).changes.is_empty(), "already reported");
        assert_eq!(status_of(&m, LPX_ID), Some((DeviceStatus::Unplugged, None)));
        // Back, but busy: failed, and left alone until Look again.
        hw.plug(&[LPX_ID]);
        hw.busy(true);
        assert_eq!(look(&mut m, false).to_connect, [LPX_ID]);
        assert_eq!(status_of(&m, LPX_ID).unwrap().0, DeviceStatus::Failed);
        assert!(look(&mut m, false).to_connect.is_empty());
        hw.busy(false);
        look(&mut m, true);
        assert_eq!(status_of(&m, LPX_ID), Some((DeviceStatus::Connected, None)));
    }

    #[test]
    fn a_unit_the_person_disconnected_is_forgotten_when_it_goes() {
        let hw = FakeHardware::default();
        let mut m = manager(&hw);
        hw.plug(&[LPX_ID]);
        look(&mut m, false);
        m.disconnect(LPX_ID).unwrap();
        assert_eq!(status_of(&m, LPX_ID), Some((DeviceStatus::Available, None)));
        assert!(
            look(&mut m, false).to_connect.is_empty(),
            "stays disconnected"
        );
        hw.plug(&[]);
        look(&mut m, false);
        assert_eq!(status_of(&m, LPX_ID), None);
    }

    #[test]
    fn a_failed_connect_is_reported_with_a_problem_and_retried_on_look_again() {
        let hw = FakeHardware::default();
        let mut m = manager(&hw);
        hw.plug(&[LPX_ID]);
        hw.busy(true);
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
        hw.busy(false);
        assert_eq!(look(&mut m, true).to_connect, [LPX_ID]);
        assert_eq!(status_of(&m, LPX_ID), Some((DeviceStatus::Connected, None)));
    }

    #[test]
    fn other_launchpads_are_listed_by_name_and_never_opened() {
        let hw = FakeHardware::default();
        let mut m = manager(&hw);
        hw.plug(&[LPX_ID]);
        hw.others(&["Launchpad Mini MK3 LPMiniMK3 MIDI", "Launchpad Pro MK3"]);
        assert_eq!(look(&mut m, true).to_connect, [LPX_ID]);
        assert_eq!(hw.made_for(), [LPX_ID], "only the Launchpad X got a device");
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
            ]
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
        assert_eq!(hw.made_for(), [LPX_ID], "a connect request opens nothing");

        hw.others(&[]);
        look(&mut m, true);
        assert_eq!(m.list().len(), 1, "gone from the list when unplugged");
    }

    #[test]
    fn device_info_serializes_status_and_problem_in_camel_case() {
        let hw = FakeHardware::default();
        let mut m = manager(&hw);
        hw.plug(&[LPX_ID]);
        hw.others(&["Launchpad Pro MK3"]);
        hw.busy(true);
        look(&mut m, false);
        let json = serde_json::to_value(m.list()).unwrap();
        assert_eq!(json[0]["status"], "failed");
        assert_eq!(json[0]["connected"], false);
        assert_eq!(
            json[0]["problem"],
            "Launchpad detected, but its control port is unavailable. Another program may be using it: close it and press Look again."
        );
        assert_eq!(json[1]["status"], "unsupported");
        assert!(json[1].get("problem").is_none());
        assert!(json[1].get("notice").is_none());
        assert_eq!(json[1]["details"]["mode"], "none");
    }

    const NOTICE: &str = "Launchpad X connected, but Programmer Mode could not be confirmed.";

    #[test]
    fn a_change_of_details_is_reported_once_and_a_notice_only_while_connected() {
        let hw = FakeHardware::default();
        let mut m = manager(&hw);
        hw.plug(&[LPX_ID]);
        look(&mut m, false);
        let frame = SurfaceFrame::default();
        assert_eq!(m.render(LPX_ID, &frame), None, "nothing new");

        hw.details(DeviceDetails {
            firmware: Some("0238".into()),
            mode: ModeState::Unconfirmed,
            notice: Some(NOTICE.into()),
            ..DeviceDetails::default()
        });
        assert_eq!(
            m.render(LPX_ID, &frame),
            Some(DeviceChange::Updated(LPX_ID.into()))
        );
        assert_eq!(m.render(LPX_ID, &frame), None, "reported once");
        let info = m.list().remove(0);
        assert_eq!(info.notice.as_deref(), Some(NOTICE));
        assert_eq!(info.details.mode, ModeState::Unconfirmed);
        let json = serde_json::to_value(&info).unwrap();
        assert_eq!(json["status"], "connected");
        assert_eq!(json["notice"], NOTICE);
        assert_eq!(json["details"]["firmware"], "0238");
        assert_eq!(json["details"]["mode"], "unconfirmed");

        m.disconnect(LPX_ID).unwrap();
        let info = m.list().remove(0);
        assert_eq!(info.notice, None, "a notice is about a connected device");
        assert!(serde_json::to_value(&info).unwrap().get("notice").is_none());
    }

    #[test]
    fn a_refusal_while_drawing_fails_the_unit_with_its_sentence_until_look_again() {
        let hw = FakeHardware::default();
        let mut m = manager(&hw);
        hw.plug(&[LPX_ID]);
        look(&mut m, false);
        hw.refuse(true);
        assert_eq!(
            m.render(LPX_ID, &SurfaceFrame::default()),
            Some(DeviceChange::Disconnected(LPX_ID.into()))
        );
        let sentence = "The Launchpad is in its bootloader. Unplug it and plug it in again.";
        assert_eq!(
            status_of(&m, LPX_ID),
            Some((DeviceStatus::Failed, Some(sentence.into())))
        );
        assert!(
            look(&mut m, false).to_connect.is_empty(),
            "the periodic look leaves it alone"
        );
        hw.refuse(false);
        assert_eq!(look(&mut m, true).to_connect, [LPX_ID]);
        assert_eq!(status_of(&m, LPX_ID), Some((DeviceStatus::Connected, None)));
    }

    #[test]
    fn a_refused_unit_plugged_in_again_as_its_sentence_asks_is_tried_again() {
        let hw = FakeHardware::default();
        let mut m = manager(&hw);
        hw.plug(&[LPX_ID]);
        look(&mut m, false);
        hw.refuse(true);
        m.render(LPX_ID, &SurfaceFrame::default());
        assert_eq!(status_of(&m, LPX_ID).unwrap().0, DeviceStatus::Failed);

        hw.plug(&[]);
        look(&mut m, false);
        assert_eq!(status_of(&m, LPX_ID), Some((DeviceStatus::Unplugged, None)));
        hw.refuse(false);
        hw.plug(&[LPX_ID]);
        assert_eq!(
            look(&mut m, false).to_connect,
            [LPX_ID],
            "back: tried again"
        );
        assert_eq!(status_of(&m, LPX_ID), Some((DeviceStatus::Connected, None)));
    }

    #[test]
    fn refusals_and_missing_features_read_as_sentences() {
        let cases = [
            (
                DeviceError::Bootloader(BOOTLOADER_DETAIL.into()),
                "The Launchpad is in its bootloader. Unplug it and plug it in again.",
                Some(BOOTLOADER_DETAIL),
                true,
            ),
            (
                DeviceError::OtherModel("inquiry family 03 01".into()),
                "LaunchDeck could not confirm which Launchpad this is. Press Look again.",
                Some("inquiry family 03 01"),
                true,
            ),
            (
                DeviceError::TakenOver("Launchpad Mini MK3".into()),
                "Another program is using the Launchpad Mini MK3. Press Look again to use it here.",
                None,
                true,
            ),
            (
                DeviceError::Unavailable("brightness".into()),
                "This device cannot do that.",
                Some("brightness"),
                false,
            ),
            (
                DeviceError::Port("port is in use by another client".into()),
                "The Launchpad could not be reached.",
                Some("port is in use by another client"),
                false,
            ),
        ];
        for (error, sentence, detail, refusal) in cases {
            assert_eq!(error.to_string(), sentence);
            assert_eq!(error.detail(), detail, "{sentence}");
            assert_eq!(error.is_refusal(), refusal, "{sentence}");
        }
    }
}
