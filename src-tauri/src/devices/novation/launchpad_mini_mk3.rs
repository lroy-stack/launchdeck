//! The Launchpad Mini MK3, as profile data (MINI-PRM, 2019-11-08).
//! `Mk3Device` drives it with the shared MK3 codec and the Mini's product
//! byte. Static colours use RGB colourspecs; pulse and flash are the unit's
//! own, with the nearest entry of the approximate palette (`palette.rs`).
//! Its pads send no velocity, so a press reads as full. Tested against the
//! simulator in `device_tests.rs` only: NOT HARDWARE VERIFIED.

use super::profile::{ModelProfile, Role};
use super::protocol::Product;
use crate::devices::{AnimationCaps, AuxCaps, DeviceCapabilities, DeviceFeatures, GridCaps};

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
    features: DeviceFeatures {
        host_mode: true,
        palette: true,
        batched_lighting: true,
        // MINI-PRM p12: a flash lasts one beat and a pulse two, at 120 bpm
        // unless another program sent the unit MIDI clock.
        flash_period_ms: Some(500),
        pulse_period_ms: Some(1000),
        brightness: true,
        sleep: true,
        scrolling_text: true,
        firmware_version: true,
    },
};
