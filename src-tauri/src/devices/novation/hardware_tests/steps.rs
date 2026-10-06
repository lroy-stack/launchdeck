//! Steps 2 to 14 of the suite (plan section 15.2), in the order they run,
//! and what they share: `taking` takes the unit and gives it back,
//! `lighting` lights it, `controls` tries what else it does.

use std::time::Duration;

use super::controls::{brightness_levels, inputs, led_feedback, scrolling_text, sleep_and_wake};
use super::lighting::{batching, flash_and_pulse, palette, rgb};
use super::report::Step;
use super::taking::{inquiry, programmer_mode, replug, safe_release};
use super::Suite;
use crate::core::model::{Animation, ControlPos, Rgb};
use crate::devices::novation::layout::led_index;
use crate::devices::{DeviceError, Led, LedCell, SurfaceFrame};

type Run = fn(&mut Suite, &mut Step) -> Result<(), DeviceError>;

pub(super) const STEPS: [(u8, &str, &str, Run); 13] = [
    (2, "Inquiry on the MIDI interface", "Q3, Q15", inquiry),
    (3, "Automatic Programmer Mode", "Q4", programmer_mode),
    (4, "Inputs", "Q5, Q6", inputs),
    (5, "Full palette", "Q8, plan 8.3", palette),
    (6, "RGB", "Q8", rgb),
    (7, "Native flash and pulse", "Q8, plan 8.5", flash_and_pulse),
    (8, "Full frame and batching", "Q9, plan 8.4", batching),
    (9, "Brightness", "Q10", brightness_levels),
    (10, "Scrolling text", "Q11", scrolling_text),
    (11, "Sleep and wake", "Q12", sleep_and_wake),
    (12, "LED feedback", "Q13", led_feedback),
    (13, "Unplug and plug in again", "Q14, plan 10", replug),
    (14, "Safe release", "Q14", safe_release),
];

pub(super) const MAX_7BIT: u8 = 127;

pub(super) fn first(data: Option<Vec<u8>>) -> Option<u8> {
    data.and_then(|d| d.first().copied())
}

pub(super) fn median(mut times: Vec<Duration>) -> Option<Duration> {
    times.sort();
    times.get(times.len() / 2).copied()
}

/// The LED index of a grid pad, row 0 at the top.
pub(super) fn grid_led(col: u8, row: u8) -> u8 {
    led_index(ControlPos::grid(col, row)).expect("a pad of the 8×8 grid")
}

/// A layout byte with its name in the manuals (MINI-PRM p7, X-PRM p7).
pub(super) fn layout_name(layout: Option<u8>) -> String {
    const NAMES: [(u8, &str); 8] = [
        (0x00, "Session"),
        (0x01, "Note (X)"),
        (0x04, "Custom 1"),
        (0x05, "Custom 2"),
        (0x06, "Custom 3"),
        (0x07, "Custom 4 (X)"),
        (0x0D, "DAW Faders"),
        (0x7F, "Programmer"),
    ];
    match layout {
        None => "unknown (no answer)".into(),
        Some(l) => match NAMES.iter().find(|(b, _)| *b == l) {
            Some((_, name)) => format!("{l:02X}h, {name}"),
            None => format!("{l:02X}h"),
        },
    }
}

pub(super) fn frame_of(cells: impl IntoIterator<Item = (ControlPos, Rgb)>) -> SurfaceFrame {
    SurfaceFrame {
        leds: cells
            .into_iter()
            .map(|(pos, color)| LedCell {
                pos,
                led: Led {
                    color,
                    animation: Animation::None,
                },
            })
            .collect(),
    }
}

/// Diagonal stripes of eight colours over the grid, easy to recognise.
pub(super) fn pattern() -> SurfaceFrame {
    const COLOURS: [Rgb; 8] = [
        Rgb::new(255, 0, 0),
        Rgb::new(255, 128, 0),
        Rgb::new(255, 255, 0),
        Rgb::new(0, 255, 0),
        Rgb::new(0, 255, 255),
        Rgb::new(0, 0, 255),
        Rgb::new(128, 0, 255),
        Rgb::new(255, 0, 128),
    ];
    frame_of((0..64u8).map(|i| {
        let (col, row) = (i % 8, i / 8);
        (
            ControlPos::grid(col, row),
            COLOURS[usize::from((col + row) % 8)],
        )
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layouts_are_named_as_the_manuals_name_them() {
        assert_eq!(layout_name(Some(0x05)), "05h, Custom 2");
        assert_eq!(layout_name(Some(0x7F)), "7Fh, Programmer");
        assert_eq!(layout_name(Some(0x42)), "42h");
        assert_eq!(layout_name(None), "unknown (no answer)");
    }
}
