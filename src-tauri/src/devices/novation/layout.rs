//! The Programmer-mode layout both MK3 models share (MINI-PRM p10, p13;
//! X-PRM the same): LED indices of every position and the decoding of what
//! the device sends. Raw note and CC numbers never leave `devices::novation`.
//!
//! Grid: `note = 10 × (8 − row) + col + 1`, so 11 is the lower left pad, 81
//! the upper left and 18 the lower right (the manuals' own examples). Top
//! row CC 91-98 left to right, right column CC 89 down to 19, logo 99 (lit,
//! never pressed). The numbers are the manuals'; NOT HARDWARE VERIFIED.

use crate::core::model::ControlPos;
use crate::devices::InputKind;

const LOGO: u8 = 99;
const TOP_FIRST: u8 = 91;
const SIDE_FIRST: u8 = 89;
const PRESS_WITHOUT_VELOCITY: u8 = 127;

/// The LED index of a position, which is also its note or CC number.
pub(crate) fn led_index(pos: ControlPos) -> Option<u8> {
    match pos {
        ControlPos::Grid { col, row } if col < 8 && row < 8 => Some(10 * (8 - row) + col + 1),
        ControlPos::Top { index } if index < 8 => Some(TOP_FIRST + index),
        ControlPos::Side { index } if index < 8 => Some(SIDE_FIRST - 10 * index),
        ControlPos::Logo => Some(LOGO),
        _ => None,
    }
}

/// The pad that sends this note.
pub(crate) fn pos_from_note(note: u8) -> Option<ControlPos> {
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

/// The top or right-column button that sends this CC. The logo is output
/// only.
pub(crate) fn pos_from_cc(cc: u8) -> Option<ControlPos> {
    match cc {
        91..=98 => Some(ControlPos::Top {
            index: cc - TOP_FIRST,
        }),
        c if c % 10 == 9 && (1..=8).contains(&(c / 10)) => {
            Some(ControlPos::Side { index: 8 - c / 10 })
        }
        _ => None,
    }
}

/// Decodes a three-byte Programmer-mode message, on any channel. A value
/// above 0 is a press, 0 or a Note Off a release. With `velocity` false
/// (a model without velocity) every press reports 127. Pressure, clock,
/// unknown CCs and notes outside the layout are `None`.
pub(crate) fn decode(bytes: &[u8], velocity: bool) -> Option<(ControlPos, InputKind)> {
    let [status, number, value] = *bytes else {
        return None;
    };
    let press_or_release = |pos| {
        let kind = match (value, velocity) {
            (0, _) => InputKind::Released,
            (v, true) => InputKind::Pressed { velocity: v },
            (_, false) => InputKind::Pressed {
                velocity: PRESS_WITHOUT_VELOCITY,
            },
        };
        (pos, kind)
    };
    match status & 0xF0 {
        0x90 => pos_from_note(number).map(press_or_release),
        0x80 => Some((pos_from_note(number)?, InputKind::Released)),
        0xB0 => pos_from_cc(number).map(press_or_release),
        _ => None,
    }
}
