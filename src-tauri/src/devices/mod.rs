//! Device abstraction.
//!
//! The rest of the runtime only ever sees `SurfaceDevice`, `DeviceCapabilities`,
//! `SurfaceFrame` and `DeviceInput`. Raw MIDI bytes live exclusively in
//! `novation`; the MIDI transport in `midi` (`midir` only in
//! `midi/system.rs`). The device manager finds hardware through a
//! `source::HardwareSource` and knows no model.

pub mod manager;
mod midi;
mod novation;
mod source;
pub mod virtual_device;

use serde::{Deserialize, Serialize};
use tokio::sync::mpsc::UnboundedSender;

use crate::core::model::{Animation, ControlPos, Rgb};

/// What went wrong with a device, in a sentence a person can read. The text
/// the operating system gave, if any, is the `detail`, for logs and bug reports.
#[derive(Debug, thiserror::Error)]
pub enum DeviceError {
    #[error("The device is not connected.")]
    NotConnected,
    #[error("The Launchpad could not be reached.")]
    Port(String),
    #[error("The Launchpad could not be reached.")]
    Midi(String),
    #[error("LaunchDeck does not know this device. Press Look again.")]
    Unknown(String),
    #[error("Not supported yet: LaunchDeck works with the Launchpad X.")]
    Unsupported(String),
}

impl DeviceError {
    /// The operating system's text or the device id behind the sentence.
    pub fn detail(&self) -> Option<&str> {
        match self {
            DeviceError::NotConnected => None,
            DeviceError::Port(d)
            | DeviceError::Midi(d)
            | DeviceError::Unknown(d)
            | DeviceError::Unsupported(d) => Some(d),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DeviceKind {
    Virtual,
    Hardware,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceIdentity {
    pub id: String,
    pub name: String,
    /// Device family, e.g. `launchpad-x`. Matches `compatibleDevices` in manifests.
    pub model: String,
    pub kind: DeviceKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct GridCaps {
    pub width: u8,
    pub height: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct AuxCaps {
    pub top: u8,
    pub side: u8,
    pub logo: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct AnimationCaps {
    pub pulse: bool,
    pub flash: bool,
}

/// What a device can do. The renderer adapts to this; the core never assumes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceCapabilities {
    pub grid: Option<GridCaps>,
    pub aux: AuxCaps,
    pub rgb: bool,
    pub velocity: bool,
    pub pressure: bool,
    /// Native (hardware or UI) animation support. When false, the renderer
    /// synthesises animation from timed frames.
    pub animations: AnimationCaps,
}

impl DeviceCapabilities {
    pub fn has_position(&self, pos: ControlPos) -> bool {
        match pos {
            ControlPos::Grid { col, row } => {
                self.grid.is_some_and(|g| col < g.width && row < g.height)
            }
            ControlPos::Top { index } => index < self.aux.top,
            ControlPos::Side { index } => index < self.aux.side,
            ControlPos::Logo => self.aux.logo,
        }
    }

    /// Every position this device has, in a stable order.
    pub fn positions(&self) -> Vec<ControlPos> {
        let mut out = Vec::new();
        if let Some(g) = self.grid {
            for row in 0..g.height {
                for col in 0..g.width {
                    out.push(ControlPos::Grid { col, row });
                }
            }
        }
        out.extend((0..self.aux.top).map(|index| ControlPos::Top { index }));
        out.extend((0..self.aux.side).map(|index| ControlPos::Side { index }));
        if self.aux.logo {
            out.push(ControlPos::Logo);
        }
        out
    }
}

/// Final LED state sent to a device.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Led {
    pub color: Rgb,
    pub animation: Animation,
}

impl Led {
    pub const OFF: Led = Led {
        color: Rgb::OFF,
        animation: Animation::None,
    };
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LedCell {
    pub pos: ControlPos,
    pub led: Led,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SurfaceFrame {
    pub leds: Vec<LedCell>,
}

impl SurfaceFrame {
    pub fn get(&self, pos: ControlPos) -> Option<Led> {
        self.leds.iter().find(|c| c.pos == pos).map(|c| c.led)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputKind {
    Pressed { velocity: u8 },
    Released,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceInput {
    pub device_id: String,
    pub pos: ControlPos,
    pub kind: InputKind,
}

pub type InputSender = UnboundedSender<DeviceInput>;

/// A physical (or virtual) control surface.
///
/// Differences from the initial sketch: input subscription is folded into
/// `connect` (a device that is not connected has no input), and
/// `is_connected` lets the manager detect unplugging.
pub trait SurfaceDevice: Send {
    fn identity(&self) -> DeviceIdentity;
    fn capabilities(&self) -> DeviceCapabilities;
    fn connect(&mut self, input: InputSender) -> Result<(), DeviceError>;
    fn disconnect(&mut self) -> Result<(), DeviceError>;
    fn is_connected(&self) -> bool;
    fn render(&mut self, frame: &SurfaceFrame) -> Result<(), DeviceError>;
    fn clear(&mut self) -> Result<(), DeviceError>;
}
