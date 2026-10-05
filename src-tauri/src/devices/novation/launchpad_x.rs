//! Novation Launchpad X adapter: its port names, its capabilities and the
//! device. Its bytes are built and read by the shared MK3 codec
//! (`novation::protocol`, `novation::layout`) with the X's product byte.
//!
//! Protocol facts come from the official "Launchpad X Programmer's reference
//! manual" (see docs/devices/launchpad-x.md for what is verified and what is
//! not). Static colours always use RGB colourspecs, which need no palette.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use super::layout::{self, led_index};
use super::protocol::{self, to_7bit, Colourspec, Product};
#[cfg(test)]
use super::{
    layout::{pos_from_cc, pos_from_note},
    protocol::MAX_COLOURSPECS,
};
use crate::core::model::{ControlPos, Rgb};
use crate::devices::midi::{MidiBackend, MidiLink, PortInfo};
use crate::devices::{
    AnimationCaps, AuxCaps, DeviceCapabilities, DeviceError, DeviceIdentity, DeviceInput,
    DeviceKind, GridCaps, InputKind, InputSender, PortNames, PortSource, SurfaceDevice,
    SurfaceFrame,
};

// ---------------------------------------------------------------------------
// Protocol: the shared MK3 codec with the X's product byte
// ---------------------------------------------------------------------------

const PRODUCT: Product = Product::X;

/// Select Programmer mode: `F0 00 20 29 02 0C 0E 01 F7`.
pub fn enter_programmer_mode() -> Vec<u8> {
    protocol::programmer_mode(PRODUCT, true)
}

/// Return to Live mode: `F0 00 20 29 02 0C 0E 00 F7`.
pub fn exit_programmer_mode() -> Vec<u8> {
    protocol::programmer_mode(PRODUCT, false)
}

/// Decode an incoming short MIDI message (Programmer mode); the X reports
/// its pads' velocity.
pub fn decode_message(bytes: &[u8]) -> Option<(ControlPos, InputKind)> {
    layout::decode(bytes, capabilities().velocity)
}

/// Encode RGB updates (`03 <index> <r> <g> <b>` per LED) into one or more
/// SysEx messages of at most 81 colourspecs each.
pub fn encode_rgb_updates(changes: &[(u8, Rgb)]) -> Vec<Vec<u8>> {
    let specs: Vec<Colourspec> = changes
        .iter()
        .map(|&(index, c)| Colourspec::Rgb {
            index,
            r: to_7bit(c.r),
            g: to_7bit(c.g),
            b: to_7bit(c.b),
        })
        .collect();
    protocol::lighting(PRODUCT, &specs)
}

/// Hardware flashing colourspec: `01 <index> <colour B> <colour A>`. Palette
/// based. Implemented and byte-tested, but not used by the renderer until the
/// palette is verified on hardware (animations are synthesised from RGB frames).
#[cfg_attr(not(test), expect(dead_code, reason = "byte-tested, not yet rendered"))]
pub fn encode_flash(index: u8, colour_b: u8, colour_a: u8) -> Vec<u8> {
    let flash = Colourspec::Flash {
        index,
        b: colour_b,
        a: colour_a,
    };
    protocol::lighting(PRODUCT, &[flash]).concat()
}

/// Hardware pulsing colourspec: `02 <index> <palette>`.
#[cfg_attr(not(test), expect(dead_code, reason = "byte-tested, not yet rendered"))]
pub fn encode_pulse(index: u8, palette: u8) -> Vec<u8> {
    protocol::lighting(PRODUCT, &[Colourspec::Pulse { index, palette }]).concat()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PortRole {
    /// "LPX MIDI": the interface that carries Programmer mode.
    Midi,
    /// "LPX DAW": DAW integration, unused here.
    Daw,
}

/// Classify an OS MIDI port name. Names differ per OS, for example
/// `LPX MIDI` (macOS), `Launchpad X:Launchpad X LPX MIDI 28:1` (ALSA),
/// `MIDIIN2 (LPX MIDI)` (Windows).
pub fn classify_port(name: &str) -> Option<PortRole> {
    let n = name.to_ascii_lowercase();
    if !names_launchpad_x(&n) {
        return None;
    }
    if n.contains("daw") {
        Some(PortRole::Daw)
    } else if n.contains("midi") {
        Some(PortRole::Midi)
    } else {
        None
    }
}

/// `lpx`, or `launchpad x` as a whole word (so not `Launchpad XL`).
fn names_launchpad_x(lowercase: &str) -> bool {
    lowercase.contains("lpx")
        || lowercase.match_indices("launchpad x").any(|(i, m)| {
            !lowercase[i + m.len()..].starts_with(|c: char| c.is_ascii_alphanumeric())
        })
}

/// A port of another Launchpad model: listed by name, never opened. Its DAW
/// port is left out so that one unit shows once.
fn is_other_launchpad(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    n.contains("launchpad") && !names_launchpad_x(&n) && !n.contains("daw")
}

pub fn capabilities() -> DeviceCapabilities {
    DeviceCapabilities {
        grid: Some(GridCaps {
            width: 8,
            height: 8,
        }),
        aux: AuxCaps {
            top: 8,
            side: 8,
            logo: true,
        },
        rgb: true,
        velocity: true,
        pressure: false,
        // Not native: the renderer synthesises pulse/flash from RGB frames.
        animations: AnimationCaps {
            pulse: false,
            flash: false,
        },
    }
}

// ---------------------------------------------------------------------------
// Discovery + hardware adapter
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoveredDevice {
    pub id: String,
    pub name: String,
    pub in_port: String,
    pub out_port: String,
}

fn slug(port: &str) -> String {
    let slug: String = port
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect();
    slug.trim_matches('-').to_string()
}

fn device_id(in_port: &str) -> String {
    format!("launchpad-x:{}", slug(in_port))
}

/// What one look at the system's ports found.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Discovery {
    /// Launchpad X units, by their MIDI interface.
    pub units: Vec<DiscoveredDevice>,
    /// Port names of other Launchpad models, once each.
    pub unsupported: Vec<String>,
}

/// Sort the system's ports into Launchpad X units and other Launchpads.
pub(crate) fn discover(ports: &PortNames) -> Discovery {
    let midi = |names: &[String]| -> Vec<String> {
        names
            .iter()
            .filter(|n| classify_port(n) == Some(PortRole::Midi))
            .cloned()
            .collect()
    };
    let units = midi(&ports.inputs)
        .into_iter()
        .zip(midi(&ports.outputs))
        .map(|(in_port, out_port)| DiscoveredDevice {
            id: device_id(&in_port),
            name: "Launchpad X".into(),
            in_port,
            out_port,
        })
        .collect();
    let mut unsupported: Vec<String> = Vec::new();
    for name in ports.inputs.iter().chain(&ports.outputs) {
        if is_other_launchpad(name) && !unsupported.contains(name) {
            unsupported.push(name.clone());
        }
    }
    Discovery { units, unsupported }
}

/// Id of a port of another Launchpad model, listed but never opened.
pub(crate) fn unsupported_id(port: &str) -> String {
    format!("unsupported:{}", slug(port))
}

/// The operating system's MIDI ports, through a `MidiBackend`.
pub(crate) struct SystemPorts {
    midi: Arc<dyn MidiBackend>,
}

impl SystemPorts {
    pub(crate) fn new(midi: Arc<dyn MidiBackend>) -> Self {
        Self { midi }
    }
}

impl PortSource for SystemPorts {
    fn list(&self) -> PortNames {
        let ports = self.midi.ports();
        let names = |list: Vec<PortInfo>| list.into_iter().map(|p| p.name).collect();
        PortNames {
            inputs: names(ports.inputs),
            outputs: names(ports.outputs),
        }
    }

    fn launchpad_x(&self, found: DiscoveredDevice) -> Box<dyn SurfaceDevice> {
        Box::new(LaunchpadX::new(found, self.midi.clone()))
    }
}

pub struct LaunchpadX {
    info: DiscoveredDevice,
    midi: Arc<dyn MidiBackend>,
    link: Option<Box<dyn MidiLink>>,
    /// Shut until Programmer mode was asked for, so that what the unit sends
    /// before is not read as pad presses.
    input_open: Arc<AtomicBool>,
    /// Last colour sent per LED index, so only changes go over the wire.
    last: HashMap<u8, Rgb>,
}

impl LaunchpadX {
    pub(crate) fn new(info: DiscoveredDevice, midi: Arc<dyn MidiBackend>) -> Self {
        Self {
            info,
            midi,
            link: None,
            input_open: Arc::new(AtomicBool::new(false)),
            last: HashMap::new(),
        }
    }

    fn send(&mut self, bytes: &[u8]) -> Result<(), DeviceError> {
        let link = self.link.as_mut().ok_or(DeviceError::NotConnected)?;
        link.send(bytes)
    }

    /// The listed ports with this unit's names. Discovery knows names only.
    fn ports(&self) -> Result<(PortInfo, PortInfo), DeviceError> {
        let ports = self.midi.ports();
        let find = |list: Vec<PortInfo>, name: &str, role: &str| {
            list.into_iter()
                .find(|p| p.name == name)
                .ok_or_else(|| DeviceError::Port(format!("{role} port `{name}` not found")))
        };
        Ok((
            find(ports.inputs, &self.info.in_port, "input")?,
            find(ports.outputs, &self.info.out_port, "output")?,
        ))
    }
}

impl SurfaceDevice for LaunchpadX {
    fn identity(&self) -> DeviceIdentity {
        DeviceIdentity {
            id: self.info.id.clone(),
            name: self.info.name.clone(),
            model: "launchpad-x".into(),
            kind: DeviceKind::Hardware,
        }
    }

    fn capabilities(&self) -> DeviceCapabilities {
        capabilities()
    }

    fn connect(&mut self, sender: InputSender) -> Result<(), DeviceError> {
        if self.link.is_some() {
            return Ok(());
        }
        let (input, output) = self.ports()?;
        self.input_open.store(false, Ordering::Release);
        let open = self.input_open.clone();
        let id = self.info.id.clone();
        let on_bytes = Box::new(move |bytes: &[u8]| {
            if !open.load(Ordering::Acquire) {
                return;
            }
            if let Some((pos, kind)) = decode_message(bytes) {
                let _ = sender.send(DeviceInput {
                    device_id: id.clone(),
                    pos,
                    kind,
                });
            }
        });
        // Input and output together, or neither.
        self.link = Some(self.midi.open(&input, &output, on_bytes)?);
        self.last.clear();

        // Programmer mode first, then start listening.
        self.send(&enter_programmer_mode())?;
        self.input_open.store(true, Ordering::Release);
        self.clear()
    }

    fn disconnect(&mut self) -> Result<(), DeviceError> {
        if self.link.is_some() {
            let _ = self.clear();
            let _ = self.send(&exit_programmer_mode());
        }
        self.input_open.store(false, Ordering::Release);
        if let Some(link) = self.link.take() {
            link.close();
        }
        self.last.clear();
        Ok(())
    }

    fn is_connected(&self) -> bool {
        self.link.is_some()
    }

    fn render(&mut self, frame: &SurfaceFrame) -> Result<(), DeviceError> {
        if self.link.is_none() {
            return Err(DeviceError::NotConnected);
        }
        let mut changes = Vec::new();
        for cell in &frame.leds {
            let Some(index) = led_index(cell.pos) else {
                continue;
            };
            // Animations were already synthesised by the renderer.
            if self.last.get(&index) != Some(&cell.led.color) {
                changes.push((index, cell.led.color));
            }
        }
        for msg in encode_rgb_updates(&changes) {
            self.send(&msg)?;
        }
        for (index, color) in changes {
            self.last.insert(index, color);
        }
        Ok(())
    }

    fn clear(&mut self) -> Result<(), DeviceError> {
        let all: Vec<(u8, Rgb)> = capabilities()
            .positions()
            .into_iter()
            .filter_map(led_index)
            .map(|i| (i, Rgb::OFF))
            .collect();
        for msg in encode_rgb_updates(&all) {
            self.send(&msg)?;
        }
        self.last = all.into_iter().collect();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn programmer_mode_messages_match_the_manual() {
        assert_eq!(
            enter_programmer_mode(),
            [0xF0, 0x00, 0x20, 0x29, 0x02, 0x0C, 0x0E, 0x01, 0xF7]
        );
        assert_eq!(
            exit_programmer_mode(),
            [0xF0, 0x00, 0x20, 0x29, 0x02, 0x0C, 0x0E, 0x00, 0xF7]
        );
    }

    #[test]
    fn grid_corners_match_the_manual_examples() {
        assert_eq!(led_index(ControlPos::grid(0, 7)), Some(11), "lower left");
        assert_eq!(led_index(ControlPos::grid(0, 0)), Some(81), "upper left");
        assert_eq!(led_index(ControlPos::grid(7, 7)), Some(18), "lower right");
        assert_eq!(led_index(ControlPos::grid(8, 0)), None);
    }

    #[test]
    fn every_position_roundtrips_through_index_and_decode() {
        for pos in capabilities().positions() {
            let idx = led_index(pos).unwrap();
            match pos {
                ControlPos::Grid { .. } => assert_eq!(pos_from_note(idx), Some(pos)),
                ControlPos::Top { .. } | ControlPos::Side { .. } => {
                    assert_eq!(pos_from_cc(idx), Some(pos))
                }
                ControlPos::Logo => assert_eq!(pos_from_cc(idx), None, "logo is output only"),
            }
        }
        // 81 distinct LED indices, exactly one full lighting message.
        let mut idx: Vec<u8> = capabilities()
            .positions()
            .into_iter()
            .filter_map(led_index)
            .collect();
        idx.sort_unstable();
        idx.dedup();
        assert_eq!(idx.len(), MAX_COLOURSPECS);
    }

    #[test]
    fn decodes_presses_and_releases() {
        assert_eq!(
            decode_message(&[0x90, 11, 100]),
            Some((ControlPos::grid(0, 7), InputKind::Pressed { velocity: 100 }))
        );
        assert_eq!(
            decode_message(&[0x90, 11, 0]),
            Some((ControlPos::grid(0, 7), InputKind::Released))
        );
        assert_eq!(
            decode_message(&[0x80, 88, 64]),
            Some((ControlPos::grid(7, 0), InputKind::Released))
        );
        assert_eq!(
            decode_message(&[0xB0, 91, 127]),
            Some((
                ControlPos::Top { index: 0 },
                InputKind::Pressed { velocity: 127 }
            ))
        );
        assert_eq!(
            decode_message(&[0xB0, 89, 0]),
            Some((ControlPos::Side { index: 0 }, InputKind::Released))
        );
        assert_eq!(
            decode_message(&[0xA0, 11, 50]),
            None,
            "aftertouch is ignored in v0.1"
        );
        assert_eq!(decode_message(&[0x90, 5, 100]), None, "outside the grid");
    }

    #[test]
    fn rgb_lighting_message_layout() {
        let msgs = encode_rgb_updates(&[(11, Rgb::new(255, 0, 128))]);
        assert_eq!(msgs.len(), 1);
        assert_eq!(
            msgs[0],
            [0xF0, 0x00, 0x20, 0x29, 0x02, 0x0C, 0x03, 0x03, 11, 127, 0, 64, 0xF7]
        );
        assert_eq!(to_7bit(0), 0);
        assert_eq!(to_7bit(255), 127);
    }

    #[test]
    fn lighting_messages_are_chunked_at_81_colourspecs() {
        let changes: Vec<(u8, Rgb)> = (0..100)
            .map(|i| (11 + (i % 70) as u8, Rgb::new(1, 2, 3)))
            .collect();
        let msgs = encode_rgb_updates(&changes);
        assert_eq!(msgs.len(), 2);
        assert_eq!(msgs[0].len(), 7 + 81 * 5 + 1);
        assert_eq!(msgs[1].len(), 7 + 19 * 5 + 1);
    }

    #[test]
    fn hardware_flash_and_pulse_colourspecs_are_encoded_per_manual() {
        assert_eq!(
            encode_pulse(11, 5),
            [0xF0, 0x00, 0x20, 0x29, 0x02, 0x0C, 0x03, 0x02, 11, 5, 0xF7]
        );
        assert_eq!(
            encode_flash(11, 3, 5),
            [0xF0, 0x00, 0x20, 0x29, 0x02, 0x0C, 0x03, 0x01, 11, 3, 5, 0xF7]
        );
    }

    #[test]
    fn port_names_are_classified_across_operating_systems() {
        assert_eq!(classify_port("LPX MIDI"), Some(PortRole::Midi));
        assert_eq!(classify_port("LPX DAW"), Some(PortRole::Daw));
        assert_eq!(
            classify_port("Launchpad X:Launchpad X LPX MIDI 28:1"),
            Some(PortRole::Midi)
        );
        assert_eq!(
            classify_port("Launchpad X:Launchpad X LPX DAW 28:0"),
            Some(PortRole::Daw)
        );
        assert_eq!(classify_port("MIDIIN2 (LPX MIDI)"), Some(PortRole::Midi));
        assert_eq!(classify_port("IAC Driver Bus 1"), None);
        assert_eq!(
            classify_port("Launchpad XL:Launchpad XL MIDI 1 24:0"),
            None,
            "a Launchpad XL is not a Launchpad X"
        );
    }

    fn names(list: &[&str]) -> PortNames {
        let v: Vec<String> = list.iter().map(|s| s.to_string()).collect();
        PortNames {
            inputs: v.clone(),
            outputs: v,
        }
    }

    #[test]
    fn discovery_lists_the_launchpad_x_and_names_other_launchpads() {
        let found = discover(&names(&[
            "Launchpad X LPX DAW",
            "Launchpad X LPX MIDI",
            "Launchpad Mini MK3 LPMiniMK3 DAW",
            "Launchpad Mini MK3 LPMiniMK3 MIDI",
            "Launchpad Pro MK3",
            "Launchpad XL",
            "IAC Driver Bus 1",
        ]));
        assert_eq!(
            found.units,
            vec![DiscoveredDevice {
                id: "launchpad-x:launchpad-x-lpx-midi".into(),
                name: "Launchpad X".into(),
                in_port: "Launchpad X LPX MIDI".into(),
                out_port: "Launchpad X LPX MIDI".into(),
            }]
        );
        assert_eq!(
            found.unsupported,
            [
                "Launchpad Mini MK3 LPMiniMK3 MIDI",
                "Launchpad Pro MK3",
                "Launchpad XL"
            ],
            "once each, without DAW ports, and nothing that is not a Launchpad"
        );
        assert_eq!(
            unsupported_id("Launchpad Pro MK3"),
            "unsupported:launchpad-pro-mk3"
        );
        assert_eq!(
            discover(&names(&["IAC Driver Bus 1"])),
            Discovery::default()
        );
    }

    #[test]
    fn hardware_declares_synthesised_animations() {
        let caps = capabilities();
        assert!(caps.rgb);
        assert!(!caps.animations.pulse && !caps.animations.flash);
    }
}
