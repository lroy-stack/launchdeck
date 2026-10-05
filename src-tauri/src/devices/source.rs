//! Where hardware comes from, for the device manager: one look at what is
//! plugged in, and a device for a unit that look found. Nothing here knows a
//! model; `novation::NovationHardware` is the source for the Launchpads, and
//! tests give the manager made-up units.

use super::{DeviceIdentity, SurfaceDevice};

/// The model of a device LaunchDeck lists but does not drive.
pub(crate) const UNSUPPORTED_MODEL: &str = "unsupported";

/// A unit LaunchDeck can drive, as one look found it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Found {
    pub id: String,
    pub name: String,
}

/// What one look found.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct HardwareScan {
    pub units: Vec<Found>,
    /// Devices of models LaunchDeck does not drive: listed, never opened.
    pub unsupported: Vec<DeviceIdentity>,
}

pub(crate) trait HardwareSource: Send {
    /// Looks at what is plugged in now.
    fn scan(&mut self) -> HardwareScan;
    /// A device for a unit of the last look; `None` for an id it did not
    /// find. Nothing is opened until `connect`.
    fn device(&self, id: &str) -> Option<Box<dyn SurfaceDevice>>;
}
