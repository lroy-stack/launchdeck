//! `FakeMidi`: an in-memory `MidiBackend` for tests. It lists the ports a test
//! sets, records every message sent as `(output id, bytes)`, delivers bytes a
//! test injects to the callback of an open input, can refuse opens (a busy
//! port) and can remove ports (an unplugged unit). A device model bound to an
//! output (a `Responder`) receives what is sent there and answers on its
//! input, synchronously, as the hardware would.

use std::sync::{Arc, Mutex};

use super::{MidiBackend, MidiLink, OnBytes, PortInfo, PortSnapshot};
use crate::devices::DeviceError;

/// What the fake says when an open is refused.
pub(crate) const BUSY: &str = "port is in use by another client";

/// A device behind a fake output: what it sends back for each message it
/// receives there.
pub(crate) trait Responder: Send {
    fn respond(&mut self, message: &[u8]) -> Vec<Vec<u8>>;
}

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
    bindings: Vec<Binding>,
}

type SharedResponder = Arc<Mutex<Box<dyn Responder>>>;

/// A responder behind an output, answering on an input.
struct Binding {
    output: String,
    input: String,
    device: SharedResponder,
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

    /// Replaces every listed port, in the system's order: inputs and outputs
    /// need not come in pairs.
    pub fn set_ports(&self, inputs: &[PortInfo], outputs: &[PortInfo]) {
        self.state().ports = PortSnapshot {
            inputs: inputs.to_vec(),
            outputs: outputs.to_vec(),
        };
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

    /// Puts a device behind the output with this id: every message sent
    /// there reaches it, and each of its answers arrives, before `send`
    /// returns, at the input with `input_id` if a link has it open.
    pub fn bind(&self, output_id: &str, input_id: &str, device: impl Responder + 'static) {
        self.state().bindings.push(Binding {
            output: output_id.into(),
            input: input_id.into(),
            device: Arc::new(Mutex::new(Box::new(device))),
        });
    }

    /// The device bound to this output and the input it answers on.
    fn bound(&self, output_id: &str) -> Option<(String, SharedResponder)> {
        self.state()
            .bindings
            .iter()
            .find(|b| b.output == output_id)
            .map(|b| (b.input.clone(), b.device.clone()))
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
        {
            let mut s = self.midi.state();
            if !s.ports.outputs.contains(&self.output) {
                return Err(DeviceError::Midi("device unplugged".into()));
            }
            s.sent.push((self.output.id.clone(), bytes.to_vec()));
        }
        // Answered without holding the fake's state, so that the input
        // callback may use the fake.
        if let Some((input, device)) = self.midi.bound(&self.output.id) {
            let answers = device.lock().unwrap().respond(bytes);
            for answer in answers {
                self.midi.inject(&input, &answer);
            }
        }
        Ok(())
    }

    fn close(self: Box<Self>) {
        self.midi
            .state()
            .inputs
            .retain(|(s, _, _)| *s != self.serial);
    }
}
