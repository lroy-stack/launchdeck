//! The MIDI transport seam: port listings and an open input+output pair,
//! with no protocol knowledge. `system` is the only file that uses `midir`;
//! `framing` turns what an input delivers into whole messages; `testkit` is
//! an in-memory backend for tests.

mod framing;
mod system;
#[cfg(test)]
pub(crate) mod testkit;

pub(crate) use framing::MessageFramer;
pub(crate) use system::SystemMidi;

use super::DeviceError;

/// One port as the operating system lists it: midir's `port_name()` and `id()`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PortInfo {
    pub name: String,
    pub id: String,
}

/// The ports listed right now, in the operating system's order.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct PortSnapshot {
    pub inputs: Vec<PortInfo>,
    pub outputs: Vec<PortInfo>,
}

/// Called with the bytes the input receives: whole messages on most
/// systems, but a call may hold several or part of one (`MessageFramer`).
pub(crate) type OnBytes = Box<dyn FnMut(&[u8]) + Send + 'static>;

pub(crate) trait MidiBackend: Send + Sync {
    fn ports(&self) -> PortSnapshot;
    /// Opens the input first (SysEx delivered), then the output. Both or
    /// neither: an output failure closes the input.
    fn open(
        &self,
        input: &PortInfo,
        output: &PortInfo,
        on_bytes: OnBytes,
    ) -> Result<Box<dyn MidiLink>, DeviceError>;
}

/// An open input+output pair. Closing it stops the input callback.
pub(crate) trait MidiLink: Send {
    fn send(&mut self, bytes: &[u8]) -> Result<(), DeviceError>;
    fn close(self: Box<Self>);
}
