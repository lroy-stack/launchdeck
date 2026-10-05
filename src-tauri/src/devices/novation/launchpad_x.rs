//! The Launchpad X as profile data (X-PRM, 2019-11-08). `Mk3Device` drives
//! it with the shared MK3 codec and the X's product byte; its golden
//! transcript is in `device_tests.rs`. Static colours always use RGB
//! colourspecs, which need no palette, and pulse and flash are synthesised
//! by the renderer. See docs/devices/launchpad-x.md for what is verified
//! and what is not.

use super::profile::{ModelProfile, Role};
use super::protocol::Product;
use crate::devices::{AnimationCaps, AuxCaps, DeviceCapabilities, DeviceFeatures, GridCaps};

pub(crate) static PROFILE: ModelProfile = ModelProfile {
    model: "launchpad-x",
    name: "Launchpad X",
    product: Product::X,
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
    // The same commands as the Mini MK3 (X-PRM). Its palette is not used:
    // static colours are RGB and animation is synthesised.
    features: DeviceFeatures {
        host_mode: true,
        palette: false,
        batched_lighting: true,
        flash_period_ms: None,
        pulse_period_ms: None,
        brightness: true,
        sleep: true,
        scrolling_text: true,
        firmware_version: true,
    },
};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::model::ControlPos;
    use crate::devices::novation::layout::{decode, led_index, pos_from_cc, pos_from_note};
    use crate::devices::novation::protocol::MAX_COLOURSPECS;
    use crate::devices::InputKind;

    fn decode_message(bytes: &[u8]) -> Option<(ControlPos, InputKind)> {
        decode(bytes, PROFILE.capabilities.velocity)
    }

    #[test]
    fn every_position_roundtrips_through_index_and_decode() {
        let positions = PROFILE.capabilities.positions();
        for &pos in &positions {
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
        let mut idx: Vec<u8> = positions.into_iter().filter_map(led_index).collect();
        idx.sort_unstable();
        idx.dedup();
        assert_eq!(idx.len(), MAX_COLOURSPECS);
    }

    #[test]
    fn decodes_presses_with_their_velocity_and_releases() {
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
            "aftertouch is ignored"
        );
        assert_eq!(decode_message(&[0x90, 5, 100]), None, "outside the grid");
    }

    #[test]
    fn hardware_declares_synthesised_animations() {
        let caps = PROFILE.capabilities;
        assert!(caps.rgb && caps.velocity && !caps.pressure);
        assert!(!caps.animations.pulse && !caps.animations.flash);
    }
}
