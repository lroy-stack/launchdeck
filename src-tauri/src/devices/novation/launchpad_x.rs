//! Novation Launchpad X adapter: its profile (port names, capabilities) and
//! the device. Its bytes are built and read by the shared MK3 codec
//! (`novation::protocol`, `novation::layout`) with the X's product byte.
//!
//! Protocol facts come from the official "Launchpad X Programmer's reference
//! manual" (see docs/devices/launchpad-x.md for what is verified and what is
//! not). Static colours always use RGB colourspecs, which need no palette.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use super::layout::{self, led_index};
use super::ports::FoundUnit;
use super::profile::{ModelProfile, Role};
use super::protocol::{self, to_7bit, Colourspec, Product};
#[cfg(test)]
use super::{
    layout::{pos_from_cc, pos_from_note},
    protocol::MAX_COLOURSPECS,
};
use crate::core::model::{ControlPos, Rgb};
use crate::devices::midi::{MidiBackend, MidiLink};
use crate::devices::{
    AnimationCaps, AuxCaps, DeviceCapabilities, DeviceError, DeviceIdentity, DeviceInput,
    DeviceKind, GridCaps, InputKind, InputSender, SurfaceDevice, SurfaceFrame,
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

/// The Launchpad X as profile data (X-PRM, 2019-11-08).
pub(crate) static PROFILE: ModelProfile = ModelProfile {
    model: "launchpad-x",
    name: "Launchpad X",
    product: PRODUCT,
    // X-PRM p6 prints 13 01, a copy of the Mini's family; Ableton's scripts
    // and a captured reply give 03 01. The readback product byte decides.
    families: &[[0x03, 0x01], [0x13, 0x01]],
    stem_tokens: &["lpx"],
    name_tokens: &["launchpad", "x"],
    // X-PRM p6: the DAW interface first, the MIDI interface second.
    interfaces: &[Role::Daw, Role::Midi],
    // X-PRM p7: Live mode outside a DAW is Note mode.
    live_layout: 0x01,
    capabilities: DeviceCapabilities {
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
    },
};

pub fn capabilities() -> DeviceCapabilities {
    PROFILE.capabilities
}

// ---------------------------------------------------------------------------
// Discovery + hardware adapter
// ---------------------------------------------------------------------------

pub struct LaunchpadX {
    unit: FoundUnit,
    midi: Arc<dyn MidiBackend>,
    link: Option<Box<dyn MidiLink>>,
    /// Shut until Programmer mode was asked for, so that what the unit sends
    /// before is not read as pad presses.
    input_open: Arc<AtomicBool>,
    /// Last colour sent per LED index, so only changes go over the wire.
    last: HashMap<u8, Rgb>,
}

impl LaunchpadX {
    pub(crate) fn new(unit: FoundUnit, midi: Arc<dyn MidiBackend>) -> Self {
        Self {
            unit,
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
}

impl SurfaceDevice for LaunchpadX {
    fn identity(&self) -> DeviceIdentity {
        DeviceIdentity {
            id: self.unit.id.clone(),
            name: self.unit.name.clone(),
            model: PROFILE.model.into(),
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
        self.input_open.store(false, Ordering::Release);
        let open = self.input_open.clone();
        let id = self.unit.id.clone();
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
        // Input and output together, or neither: the unit's own pair, by
        // name and id, so that identical units are not mixed up.
        let link = self
            .midi
            .open(&self.unit.input, &self.unit.output, on_bytes)?;
        self.link = Some(link);
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
    fn hardware_declares_synthesised_animations() {
        let caps = capabilities();
        assert!(caps.rgb);
        assert!(!caps.animations.pulse && !caps.animations.flash);
    }
}
