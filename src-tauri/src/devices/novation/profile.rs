//! What one Novation MK3 model is, as data: its names, its product byte, the
//! inquiry families it answers with, its USB-MIDI interfaces and what it can
//! do. Code that works for every model reads a profile; there is no
//! `if model == …` anywhere.

use super::protocol::Product;
use crate::devices::DeviceCapabilities;

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
}

impl ModelProfile {
    /// The role of USB-MIDI interface `n`, counted from 1.
    pub(crate) fn interface(&self, n: u32) -> Option<Role> {
        let index = usize::try_from(n.checked_sub(1)?).ok()?;
        self.interfaces.get(index).copied()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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
        assert_eq!(x.capabilities, launchpad_x::capabilities());
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
    }
}
