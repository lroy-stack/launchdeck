//! `FakeMidi`: an in-memory `MidiBackend` for tests. It lists the ports a test
//! sets, records every message sent as `(output id, bytes)`, delivers bytes a
//! test injects to the callback of an open input, can refuse opens (a busy
//! port) and can remove ports (an unplugged unit).

use std::sync::{Arc, Mutex};

use super::{MidiBackend, MidiLink, OnBytes, PortInfo, PortSnapshot};
use crate::devices::DeviceError;

/// What the fake says when an open is refused.
pub(crate) const BUSY: &str = "port is in use by another client";

#[derive(Clone, Default)]
pub(crate) struct FakeMidi(Arc<Mutex<FakeState>>);

#[derive(Default)]
struct FakeState {
    ports: PortSnapshot,
    busy: bool,
    sent: Vec<(String, Vec<u8>)>,
    /// Callbacks of the open inputs: (link serial, input id, callback).
    inputs: Vec<(u64, String, Arc<Mutex<OnBytes>>)>,
    next_serial: u64,
}

pub(crate) fn port(name: &str, id: &str) -> PortInfo {
    PortInfo {
        name: name.into(),
        id: id.into(),
    }
}

impl FakeMidi {
    fn state(&self) -> std::sync::MutexGuard<'_, FakeState> {
        self.0.lock().unwrap()
    }

    /// Adds one unit's pair of ports.
    pub fn add(&self, input: PortInfo, output: PortInfo) {
        let mut s = self.state();
        s.ports.inputs.push(input);
        s.ports.outputs.push(output);
    }

    /// Removes every port with this id, as when a unit is unplugged.
    pub fn remove(&self, id: &str) {
        let mut s = self.state();
        s.ports.inputs.retain(|p| p.id != id);
        s.ports.outputs.retain(|p| p.id != id);
    }

    /// Refuses every open while set, as a port another program holds.
    pub fn refuse_opens(&self, busy: bool) {
        self.state().busy = busy;
    }

    /// Every message sent since the last call, as `(output id, bytes)`.
    pub fn take_sent(&self) -> Vec<(String, Vec<u8>)> {
        std::mem::take(&mut self.state().sent)
    }

    /// Delivers bytes to the open input with this id, as the system would.
    pub fn inject(&self, input_id: &str, bytes: &[u8]) {
        let callbacks: Vec<Arc<Mutex<OnBytes>>> = self
            .state()
            .inputs
            .iter()
            .filter(|(_, id, _)| id == input_id)
            .map(|(_, _, cb)| cb.clone())
            .collect();
        for cb in callbacks {
            (cb.lock().unwrap())(bytes);
        }
    }

    /// How many links are open.
    pub fn open_links(&self) -> usize {
        self.state().inputs.len()
    }
}

impl MidiBackend for FakeMidi {
    fn ports(&self) -> PortSnapshot {
        self.state().ports.clone()
    }

    fn open(
        &self,
        input: &PortInfo,
        output: &PortInfo,
        on_bytes: OnBytes,
    ) -> Result<Box<dyn MidiLink>, DeviceError> {
        let mut s = self.state();
        if s.busy {
            return Err(DeviceError::Port(BUSY.into()));
        }
        if !s.ports.inputs.contains(input) {
            return Err(DeviceError::Port(format!(
                "input port `{}` not found",
                input.name
            )));
        }
        if !s.ports.outputs.contains(output) {
            return Err(DeviceError::Port(format!(
                "output port `{}` not found",
                output.name
            )));
        }
        let serial = s.next_serial;
        s.next_serial += 1;
        s.inputs
            .push((serial, input.id.clone(), Arc::new(Mutex::new(on_bytes))));
        Ok(Box::new(FakeLink {
            midi: self.clone(),
            output: output.clone(),
            serial,
        }))
    }
}

struct FakeLink {
    midi: FakeMidi,
    output: PortInfo,
    serial: u64,
}

impl MidiLink for FakeLink {
    fn send(&mut self, bytes: &[u8]) -> Result<(), DeviceError> {
        let mut s = self.midi.state();
        if !s.ports.outputs.contains(&self.output) {
            return Err(DeviceError::Midi("device unplugged".into()));
        }
        s.sent.push((self.output.id.clone(), bytes.to_vec()));
        Ok(())
    }

    fn close(self: Box<Self>) {
        self.midi
            .state()
            .inputs
            .retain(|(s, _, _)| *s != self.serial);
    }
}
