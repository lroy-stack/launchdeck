//! Novation Launchpad X adapter. The ONLY place that knows MIDI bytes.
//!
//! Protocol facts come from the official "Launchpad X Programmer's reference
//! manual" (see docs/devices/launchpad-x.md for what is verified and what is
//! not). Static colours always use RGB colourspecs, which need no palette.

use std::collections::HashMap;

use midir::{Ignore, MidiInput, MidiInputConnection, MidiOutput, MidiOutputConnection};

use super::{
    AnimationCaps, AuxCaps, DeviceCapabilities, DeviceError, DeviceIdentity, DeviceInput,
    DeviceKind, GridCaps, InputKind, InputSender, SurfaceDevice, SurfaceFrame,
};
use crate::core::model::{ControlPos, Rgb};

// ---------------------------------------------------------------------------
// Protocol (pure functions, unit tested without hardware)
// ---------------------------------------------------------------------------

/// `F0 00 20 29 02 0C`: Novation manufacturer id + Launchpad X product id.
pub const SYSEX_HEADER: [u8; 6] = [0xF0, 0x00, 0x20, 0x29, 0x02, 0x0C];
const SYSEX_END: u8 = 0xF7;
/// The manual allows up to 81 colourspecs per lighting message.
pub const MAX_COLOURSPECS: usize = 81;

/// Select Programmer mode: `F0 00 20 29 02 0C 0E 01 F7`.
pub fn enter_programmer_mode() -> Vec<u8> {
    let mut m = SYSEX_HEADER.to_vec();
    m.extend([0x0E, 0x01, SYSEX_END]);
    m
}

/// Return to Live mode: `F0 00 20 29 02 0C 0E 00 F7`.
pub fn exit_programmer_mode() -> Vec<u8> {
    let mut m = SYSEX_HEADER.to_vec();
    m.extend([0x0E, 0x00, SYSEX_END]);
    m
}

/// Programmer-mode LED/note index for a logical position.
///
/// Grid: `note = 10 * (8 - row) + col + 1` (lower left 11, upper left 81,
/// lower right 18; all three are examples in the manual).
///
/// UNVERIFIED (the manual shows these only in diagrams): top row CC 91..=98,
/// side column CC 89 down to 19, logo 99. Auxiliary controls are therefore
/// experimental until confirmed on hardware.
pub fn led_index(pos: ControlPos) -> Option<u8> {
    match pos {
        ControlPos::Grid { col, row } if col < 8 && row < 8 => Some(10 * (8 - row) + col + 1),
        ControlPos::Top { index } if index < 8 => Some(91 + index),
        ControlPos::Side { index } if index < 8 => Some(89 - 10 * index),
        ControlPos::Logo => Some(99),
        _ => None,
    }
}

pub fn pos_from_note(note: u8) -> Option<ControlPos> {
    let (tens, units) = (note / 10, note % 10);
    if (1..=8).contains(&tens) && (1..=8).contains(&units) {
        Some(ControlPos::Grid {
            col: units - 1,
            row: 8 - tens,
        })
    } else {
        None
    }
}

pub fn pos_from_cc(cc: u8) -> Option<ControlPos> {
    match cc {
        91..=98 => Some(ControlPos::Top { index: cc - 91 }),
        c if c % 10 == 9 && (1..=8).contains(&(c / 10)) => {
            Some(ControlPos::Side { index: 8 - c / 10 })
        }
        _ => None, // 99 (logo) is output only
    }
}

/// Decode an incoming short MIDI message (Programmer mode).
pub fn decode_message(bytes: &[u8]) -> Option<(ControlPos, InputKind)> {
    let [status, data1, data2] = *bytes else {
        return None;
    };
    match status & 0xF0 {
        0x90 => {
            let pos = pos_from_note(data1)?;
            Some((
                pos,
                if data2 > 0 {
                    InputKind::Pressed { velocity: data2 }
                } else {
                    InputKind::Released
                },
            ))
        }
        0x80 => Some((pos_from_note(data1)?, InputKind::Released)),
        0xB0 => {
            let pos = pos_from_cc(data1)?;
            Some((
                pos,
                if data2 > 0 {
                    InputKind::Pressed { velocity: data2 }
                } else {
                    InputKind::Released
                },
            ))
        }
        _ => None,
    }
}

/// 0..=255 to the 0..=127 range the lighting SysEx expects.
pub fn to_7bit(v: u8) -> u8 {
    ((u16::from(v) * 127 + 127) / 255) as u8
}

/// Encode RGB updates (`03 <index> <r> <g> <b>` per LED) into one or more
/// SysEx messages of at most 81 colourspecs each.
pub fn encode_rgb_updates(changes: &[(u8, Rgb)]) -> Vec<Vec<u8>> {
    changes
        .chunks(MAX_COLOURSPECS)
        .map(|chunk| {
            let mut m = SYSEX_HEADER.to_vec();
            m.push(0x03);
            for (index, c) in chunk {
                m.extend([0x03, *index, to_7bit(c.r), to_7bit(c.g), to_7bit(c.b)]);
            }
            m.push(SYSEX_END);
            m
        })
        .collect()
}

/// Hardware flashing colourspec: `01 <index> <colour B> <colour A>`. Palette
/// based. Implemented and byte-tested, but not used by the renderer until the
/// palette is verified on hardware (animations are synthesised from RGB frames).
pub fn encode_flash(index: u8, colour_b: u8, colour_a: u8) -> Vec<u8> {
    let mut m = SYSEX_HEADER.to_vec();
    m.extend([0x03, 0x01, index, colour_b, colour_a, SYSEX_END]);
    m
}

/// Hardware pulsing colourspec: `02 <index> <palette>`.
pub fn encode_pulse(index: u8, palette: u8) -> Vec<u8> {
    let mut m = SYSEX_HEADER.to_vec();
    m.extend([0x03, 0x02, index, palette, SYSEX_END]);
    m
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
    if !(n.contains("lpx") || n.contains("launchpad x")) {
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

fn device_id(in_port: &str) -> String {
    let slug: String = in_port
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect();
    format!("launchpad-x:{}", slug.trim_matches('-'))
}

/// List attached Launchpad X units (MIDI interface only).
pub fn discover() -> Vec<DiscoveredDevice> {
    let (Ok(input), Ok(output)) = (
        MidiInput::new("launchdeck-scan-in"),
        MidiOutput::new("launchdeck-scan-out"),
    ) else {
        return Vec::new();
    };
    let in_names: Vec<String> = input
        .ports()
        .iter()
        .filter_map(|p| input.port_name(p).ok())
        .filter(|n| classify_port(n) == Some(PortRole::Midi))
        .collect();
    let out_names: Vec<String> = output
        .ports()
        .iter()
        .filter_map(|p| output.port_name(p).ok())
        .filter(|n| classify_port(n) == Some(PortRole::Midi))
        .collect();
    in_names
        .into_iter()
        .zip(out_names)
        .map(|(in_port, out_port)| DiscoveredDevice {
            id: device_id(&in_port),
            name: "Launchpad X".into(),
            in_port,
            out_port,
        })
        .collect()
}

pub struct LaunchpadX {
    info: DiscoveredDevice,
    out: Option<MidiOutputConnection>,
    input: Option<MidiInputConnection<()>>,
    /// Last colour sent per LED index, so only changes go over the wire.
    last: HashMap<u8, Rgb>,
}

impl LaunchpadX {
    pub fn new(info: DiscoveredDevice) -> Self {
        Self {
            info,
            out: None,
            input: None,
            last: HashMap::new(),
        }
    }

    fn send(&mut self, bytes: &[u8]) -> Result<(), DeviceError> {
        let out = self.out.as_mut().ok_or(DeviceError::NotConnected)?;
        out.send(bytes)
            .map_err(|e| DeviceError::Midi(e.to_string()))
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
        if self.out.is_some() {
            return Ok(());
        }
        let output =
            MidiOutput::new("launchdeck-out").map_err(|e| DeviceError::Midi(e.to_string()))?;
        let out_port = output
            .ports()
            .into_iter()
            .find(|p| output.port_name(p).is_ok_and(|n| n == self.info.out_port))
            .ok_or_else(|| {
                DeviceError::Port(format!("output port `{}` not found", self.info.out_port))
            })?;
        let conn = output
            .connect(&out_port, "launchdeck")
            .map_err(|e| DeviceError::Port(e.to_string()))?;
        self.out = Some(conn);
        self.last.clear();

        // Programmer mode first, then start listening.
        self.send(&enter_programmer_mode())?;

        let mut midi_in =
            MidiInput::new("launchdeck-in").map_err(|e| DeviceError::Midi(e.to_string()))?;
        midi_in.ignore(Ignore::All);
        let in_port = midi_in
            .ports()
            .into_iter()
            .find(|p| midi_in.port_name(p).is_ok_and(|n| n == self.info.in_port))
            .ok_or_else(|| {
                DeviceError::Port(format!("input port `{}` not found", self.info.in_port))
            })?;
        let id = self.info.id.clone();
        let conn = midi_in
            .connect(
                &in_port,
                "launchdeck",
                move |_ts, bytes, _| {
                    if let Some((pos, kind)) = decode_message(bytes) {
                        let _ = sender.send(DeviceInput {
                            device_id: id.clone(),
                            pos,
                            kind,
                        });
                    }
                },
                (),
            )
            .map_err(|e| DeviceError::Port(e.to_string()))?;
        self.input = Some(conn);
        self.clear()
    }

    fn disconnect(&mut self) -> Result<(), DeviceError> {
        if self.out.is_some() {
            let _ = self.clear();
            let _ = self.send(&exit_programmer_mode());
        }
        if let Some(conn) = self.input.take() {
            conn.close();
        }
        if let Some(conn) = self.out.take() {
            conn.close();
        }
        self.last.clear();
        Ok(())
    }

    fn is_connected(&self) -> bool {
        self.out.is_some()
    }

    fn render(&mut self, frame: &SurfaceFrame) -> Result<(), DeviceError> {
        if self.out.is_none() {
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
    }

    #[test]
    fn hardware_declares_synthesised_animations() {
        let caps = capabilities();
        assert!(caps.rgb);
        assert!(!caps.animations.pulse && !caps.animations.flash);
    }
}
