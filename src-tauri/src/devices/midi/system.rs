//! The operating system's MIDI ports, through `midir`. The only file that uses it.

use midir::{
    Ignore, MidiInput, MidiInputConnection, MidiInputPort, MidiOutput, MidiOutputConnection,
    MidiOutputPort,
};

use super::{MidiBackend, MidiLink, OnBytes, PortInfo, PortSnapshot};
use crate::devices::DeviceError;

pub(crate) struct SystemMidi;

impl MidiBackend for SystemMidi {
    /// Fresh clients per call, so that the listing is current.
    fn ports(&self) -> PortSnapshot {
        let (Ok(input), Ok(output)) = (
            MidiInput::new("launchdeck-scan-in"),
            MidiOutput::new("launchdeck-scan-out"),
        ) else {
            return PortSnapshot::default();
        };
        PortSnapshot {
            inputs: input
                .ports()
                .iter()
                .filter_map(|p| {
                    let name = input.port_name(p).ok()?;
                    Some(PortInfo { name, id: p.id() })
                })
                .collect(),
            outputs: output
                .ports()
                .iter()
                .filter_map(|p| {
                    let name = output.port_name(p).ok()?;
                    Some(PortInfo { name, id: p.id() })
                })
                .collect(),
        }
    }

    fn open(
        &self,
        input: &PortInfo,
        output: &PortInfo,
        mut on_bytes: OnBytes,
    ) -> Result<Box<dyn MidiLink>, DeviceError> {
        let mut midi_in =
            MidiInput::new("launchdeck-in").map_err(|e| DeviceError::Midi(e.to_string()))?;
        midi_in.ignore(Ignore::TimeAndActiveSense);
        let in_port = input_port(&midi_in, input)?;
        let input_conn = midi_in
            .connect(
                &in_port,
                "launchdeck",
                move |_ts, bytes, _| on_bytes(bytes),
                (),
            )
            .map_err(|e| DeviceError::Port(e.to_string()))?;
        match open_output(output) {
            Ok(output_conn) => Ok(Box::new(SystemLink {
                input: input_conn,
                output: output_conn,
            })),
            Err(e) => {
                input_conn.close();
                Err(e)
            }
        }
    }
}

/// The input port with this name and id; never the first one of that name.
fn input_port(midi_in: &MidiInput, info: &PortInfo) -> Result<MidiInputPort, DeviceError> {
    midi_in
        .find_port_by_id(&info.id)
        .filter(|p| midi_in.port_name(p).is_ok_and(|n| n == info.name))
        .ok_or_else(|| DeviceError::Port(format!("input port `{}` not found", info.name)))
}

fn output_port(midi_out: &MidiOutput, info: &PortInfo) -> Result<MidiOutputPort, DeviceError> {
    midi_out
        .find_port_by_id(&info.id)
        .filter(|p| midi_out.port_name(p).is_ok_and(|n| n == info.name))
        .ok_or_else(|| DeviceError::Port(format!("output port `{}` not found", info.name)))
}

fn open_output(info: &PortInfo) -> Result<MidiOutputConnection, DeviceError> {
    let midi_out =
        MidiOutput::new("launchdeck-out").map_err(|e| DeviceError::Midi(e.to_string()))?;
    let port = output_port(&midi_out, info)?;
    midi_out
        .connect(&port, "launchdeck")
        .map_err(|e| DeviceError::Port(e.to_string()))
}

struct SystemLink {
    input: MidiInputConnection<()>,
    output: MidiOutputConnection,
}

impl MidiLink for SystemLink {
    fn send(&mut self, bytes: &[u8]) -> Result<(), DeviceError> {
        self.output
            .send(bytes)
            .map_err(|e| DeviceError::Midi(e.to_string()))
    }

    fn close(self: Box<Self>) {
        self.input.close();
        self.output.close();
    }
}
