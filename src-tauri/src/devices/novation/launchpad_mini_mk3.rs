//! The Launchpad Mini MK3, as profile data (MINI-PRM, 2019-11-08). Its ports
//! are recognised and the unit is listed as not supported yet; LaunchDeck
//! drives it from package M6 of the Mini MK3 plan. NOT HARDWARE VERIFIED.

use super::profile::{ModelProfile, Role};
use super::protocol::Product;
use crate::devices::{AnimationCaps, AuxCaps, DeviceCapabilities, GridCaps};

pub(crate) static PROFILE: ModelProfile = ModelProfile {
    model: "launchpad-mini-mk3",
    name: "Launchpad Mini MK3",
    product: Product::MINI_MK3,
    // MINI-PRM p6; the bootloader answers 13 11.
    families: &[[0x13, 0x01]],
    stem_tokens: &["lpminimk3"],
    name_tokens: &["launchpad", "mini", "mk3"],
    // MINI-PRM p6: the DAW interface first, the MIDI interface second.
    interfaces: &[Role::Daw, Role::Midi],
    // MINI-PRM p7: Live mode outside a DAW is Custom mode 2 (Keys).
    live_layout: 0x05,
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
        // The Mini's pads send a fixed velocity and no aftertouch.
        velocity: false,
        pressure: false,
        animations: AnimationCaps {
            pulse: true,
            flash: true,
        },
    },
};
