//! What one Novation MK3 model is, as data: its names, its product byte, the
//! inquiry families it answers with, its USB-MIDI interfaces and what it can
//! do. Code that works for every model reads a profile; there is no
//! `if model == …` anywhere.

use super::palette::{nearest, palette_rgb};
use super::protocol::Product;
use crate::core::model::{Animation, Rgb};
use crate::devices::virtual_device::VirtualModel;
use crate::devices::{DeviceCapabilities, DeviceFeatures, Led};

/// What one USB-MIDI interface of a model is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Role {
    /// DAW integration and Session mode: recognised, never opened.
    Daw,
    /// Custom modes and Programmer mode: the interface LaunchDeck opens.
    Midi,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct ModelProfile {
    /// The model id, `DeviceIdentity::model`: `launchpad-x`.
    pub model: &'static str,
    /// The name a person reads: `Launchpad X`.
    pub name: &'static str,
    /// The last byte of its SysEx header.
    pub product: Product,
    /// The Universal Device Inquiry families its application answers with.
    pub families: &'static [[u8; 2]],
    /// Lower-case port-name tokens that name the model on their own (`lpx`).
    pub stem_tokens: &'static [&'static str],
    /// Its USB product name as lower-case tokens (`launchpad`, `x`).
    pub name_tokens: &'static [&'static str],
    /// Its USB-MIDI interfaces in order: interface n is `interfaces[n - 1]`.
    pub interfaces: &'static [Role],
    /// The layout the unit shows when Programmer mode is left (Live mode).
    pub live_layout: u8,
    pub capabilities: DeviceCapabilities,
    /// What it can do beyond drawing frames.
    pub features: DeviceFeatures,
}

impl ModelProfile {
    /// The role of USB-MIDI interface `n`, counted from 1.
    pub(crate) fn interface(&self, n: u32) -> Option<Role> {
        let index = usize::try_from(n.checked_sub(1)?).ok()?;
        self.interfaces.get(index).copied()
    }
}

/// A software unit of `profile`: its capabilities, and the features whose
/// state a simulator can hold (no mode to take, no firmware, no messages
/// to batch). Its LEDs show what the hardware is sent: a pulse or a flash
/// in the nearest palette colour (`leds.rs` sends the same entry), a static
/// colour exactly, an animated LED that is off as off. The renderer passes
/// pulse and flash only to a model that does them natively.
#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "the app registers no virtual Mini yet (plan section 13)"
    )
)]
pub(crate) fn virtual_model(profile: &ModelProfile) -> VirtualModel {
    fn display(led: Led) -> Led {
        if led.color == Rgb::OFF {
            return Led::OFF;
        }
        match led.animation {
            Animation::Pulse | Animation::Flash => Led {
                color: palette_rgb(nearest(led.color)),
                animation: led.animation,
            },
            Animation::None => led,
        }
    }
    VirtualModel {
        id: format!("virtual-{}", profile.model),
        name: format!("Virtual {}", profile.name),
        model: profile.model.into(),
        capabilities: profile.capabilities,
        features: DeviceFeatures {
            host_mode: false,
            batched_lighting: false,
            firmware_version: false,
            ..profile.features
        },
        display,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::devices::novation::palette::{nearest, palette_rgb};
    use crate::devices::novation::{launchpad_mini_mk3, launchpad_x};

    #[test]
    fn interfaces_are_counted_from_one() {
        let x = &launchpad_x::PROFILE;
        assert_eq!(x.interface(0), None);
        assert_eq!(x.interface(1), Some(Role::Daw));
        assert_eq!(x.interface(2), Some(Role::Midi));
        assert_eq!(x.interface(3), None);
    }

    #[test]
    fn the_launchpad_x_profile_holds_its_manual_data() {
        let x = &launchpad_x::PROFILE;
        assert_eq!((x.model, x.name), ("launchpad-x", "Launchpad X"));
        assert_eq!(x.product, Product::X);
        assert_eq!(x.families, [[0x03, 0x01], [0x13, 0x01]]);
        assert_eq!(x.stem_tokens, ["lpx"]);
        assert_eq!(x.name_tokens, ["launchpad", "x"]);
        assert_eq!(x.interfaces, [Role::Daw, Role::Midi]);
        assert_eq!(x.live_layout, 0x01, "Note mode (X-PRM p7)");
        let caps = x.capabilities;
        assert_eq!(caps.grid.map(|g| (g.width, g.height)), Some((8, 8)));
        assert_eq!((caps.aux.top, caps.aux.side, caps.aux.logo), (8, 8, true));
        assert!(caps.rgb && caps.velocity && !caps.pressure);
        assert!(!caps.animations.pulse && !caps.animations.flash);
        assert_eq!(
            x.features,
            DeviceFeatures {
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
            "pulse and flash are synthesised, the palette unused"
        );
    }

    #[test]
    fn the_mini_mk3_profile_holds_its_manual_data() {
        let mini = &launchpad_mini_mk3::PROFILE;
        assert_eq!(
            (mini.model, mini.name),
            ("launchpad-mini-mk3", "Launchpad Mini MK3")
        );
        assert_eq!(mini.product, Product::MINI_MK3);
        assert_eq!(mini.families, [[0x13, 0x01]]);
        assert_eq!(mini.stem_tokens, ["lpminimk3"]);
        assert_eq!(mini.name_tokens, ["launchpad", "mini", "mk3"]);
        assert_eq!(mini.interfaces, [Role::Daw, Role::Midi]);
        assert_eq!(mini.live_layout, 0x05, "Custom mode 2, Keys (MINI-PRM p7)");
        let caps = mini.capabilities;
        assert_eq!(caps.grid.map(|g| (g.width, g.height)), Some((8, 8)));
        assert_eq!((caps.aux.top, caps.aux.side, caps.aux.logo), (8, 8, true));
        assert!(caps.rgb && !caps.velocity && !caps.pressure);
        assert!(caps.animations.pulse && caps.animations.flash);
        assert_eq!(
            mini.features,
            DeviceFeatures {
                host_mode: true,
                palette: true,
                batched_lighting: true,
                flash_period_ms: Some(500),
                pulse_period_ms: Some(1000),
                brightness: true,
                sleep: true,
                scrolling_text: true,
                firmware_version: true,
            }
        );
    }

    #[test]
    fn the_virtual_mini_is_the_mini_without_a_mode_firmware_or_batches() {
        let mini = &launchpad_mini_mk3::PROFILE;
        let model = virtual_model(mini);
        assert_eq!(
            (model.id.as_str(), model.name.as_str(), model.model.as_str()),
            (
                "virtual-launchpad-mini-mk3",
                "Virtual Launchpad Mini MK3",
                "launchpad-mini-mk3"
            )
        );
        assert_eq!(model.capabilities, mini.capabilities);
        assert!(!model.capabilities.velocity, "presses report 127");
        assert_eq!(
            model.features,
            DeviceFeatures {
                host_mode: false,
                palette: true,
                batched_lighting: false,
                flash_period_ms: Some(500),
                pulse_period_ms: Some(1000),
                brightness: true,
                sleep: true,
                scrolling_text: true,
                firmware_version: false,
            }
        );
    }

    #[test]
    fn the_virtual_mini_shows_what_the_hardware_is_sent() {
        let display = virtual_model(&launchpad_mini_mk3::PROFILE).display;
        let blue = Rgb::new(40, 110, 255);
        let entry = palette_rgb(nearest(blue));
        for animation in [Animation::Pulse, Animation::Flash] {
            assert_eq!(
                display(Led {
                    color: blue,
                    animation
                }),
                Led {
                    color: entry,
                    animation
                },
                "{animation:?}: the nearest palette entry, still animated"
            );
        }
        let still = Led {
            color: blue,
            animation: Animation::None,
        };
        assert_eq!(display(still), still, "a static colour is exact");
        let off_pulse = Led {
            color: Rgb::OFF,
            animation: Animation::Pulse,
        };
        assert_eq!(display(off_pulse), Led::OFF);
    }
}
