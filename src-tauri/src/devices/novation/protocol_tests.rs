//! Byte tests of the shared MK3 codec, written from the hex lines of the
//! Launchpad Mini [MK3] Programmer's Reference Manual (MINI-PRM, 2019-11-08).
//! Each test names its page. The manual's decimal lines are not used (the
//! text example's is wrong), nor the `13 01` family the X manual prints for
//! the X (a copy error: a real X answers `03 01`).

use super::layout::{decode, led_index, pos_from_cc, pos_from_note};
use super::protocol::{
    brightness, channel_lighting, feedback, lighting, parse_reply, programmer_mode, query,
    select_layout, sleep, text, text_stop, to_7bit, ChannelLight, Colourspec, Product, Reply,
    TextColour, BRIGHTNESS, FEEDBACK, INQUIRY, LAYOUT, MAX_COLOURSPECS, MODE, SLEEP,
};
use crate::core::model::ControlPos;
use crate::devices::{AnimationCaps, AuxCaps, DeviceCapabilities, GridCaps, InputKind};

const MINI: Product = Product::MINI_MK3;
const X: Product = Product::X;

/// `"F0 00 20 29"` as bytes, so that expectations read like the manual.
fn hex(line: &str) -> Vec<u8> {
    line.split_whitespace()
        .map(|b| u8::from_str_radix(b, 16).unwrap())
        .collect()
}

// ---- identity (MINI-PRM p6) -------------------------------------------------

#[test]
fn inquiry_request_p6() {
    assert_eq!(INQUIRY.to_vec(), hex("F0 7E 7F 06 01 F7"));
}

#[test]
fn mini_application_reply_p6() {
    // The four version digits are illustrative; the manual leaves them open.
    assert_eq!(
        parse_reply(&hex("F0 7E 00 06 02 00 20 29 13 01 00 00 00 05 03 01 F7")),
        Some(Reply::Identity {
            family: [0x13, 0x01],
            member: [0x00, 0x00],
            version: [0, 5, 3, 1],
        })
    );
}

#[test]
fn mini_bootloader_reply_p6() {
    assert_eq!(
        parse_reply(&hex("F0 7E 00 06 02 00 20 29 13 11 00 00 00 01 00 02 F7")),
        Some(Reply::Identity {
            family: [0x13, 0x11],
            member: [0x00, 0x00],
            version: [0, 1, 0, 2],
        })
    );
}

#[test]
fn captured_launchpad_x_reply_community() {
    // ruohki/lunchpad, test `real_launchpad_x_reply`: not from a manual.
    assert_eq!(
        parse_reply(&hex("F0 7E 00 06 02 00 20 29 03 01 00 00 00 02 03 08 F7")),
        Some(Reply::Identity {
            family: [0x03, 0x01],
            member: [0x00, 0x00],
            version: [0, 2, 3, 8],
        })
    );
}

#[test]
fn malformed_replies_are_rejected_p6() {
    let bad = [
        (
            "16 bytes",
            "F0 7E 00 06 02 00 20 29 13 01 00 00 00 05 03 F7",
        ),
        (
            "digit 0A",
            "F0 7E 00 06 02 00 20 29 13 01 00 00 00 0A 03 01 F7",
        ),
        (
            "no F7",
            "F0 7E 00 06 02 00 20 29 13 01 00 00 00 05 03 01 00",
        ),
        (
            "manufacturer",
            "F0 7E 00 06 02 00 20 2A 13 01 00 00 00 05 03 01 F7",
        ),
        (
            "data byte 80",
            "F0 7E 00 06 02 00 20 29 13 81 00 00 00 05 03 01 F7",
        ),
        ("readback no F7", "F0 00 20 29 02 0D 0E 01 00"),
        ("readback 2A", "F0 00 20 2A 02 0D 0E 01 F7"),
        ("readback data 80", "F0 00 20 29 02 0D 0E 80 F7"),
        ("readback no data", "F0 00 20 29 02 0D 0E F7"),
        ("note", "90 0B 7F"),
        ("empty", ""),
    ];
    for (what, line) in bad {
        assert_eq!(parse_reply(&hex(line)), None, "{what}");
    }
}

// ---- mode and layout (MINI-PRM p7-8) ----------------------------------------

#[test]
fn programmer_and_live_mode_p7_p8() {
    assert_eq!(
        programmer_mode(MINI, true),
        hex("F0 00 20 29 02 0D 0E 01 F7")
    );
    assert_eq!(
        programmer_mode(MINI, false),
        hex("F0 00 20 29 02 0D 0E 00 F7")
    );
    assert_eq!(query(MINI, MODE), hex("F0 00 20 29 02 0D 0E F7"));
    assert_eq!(
        parse_reply(&hex("F0 00 20 29 02 0D 0E 01 F7")),
        Some(Reply::Readback {
            product: 0x0D,
            command: 0x0E,
            data: vec![0x01],
        })
    );
}

#[test]
fn layout_select_and_query_p7() {
    assert_eq!(select_layout(MINI, 0x7F), hex("F0 00 20 29 02 0D 00 7F F7"));
    assert_eq!(select_layout(MINI, 0x05), hex("F0 00 20 29 02 0D 00 05 F7"));
    assert_eq!(query(MINI, LAYOUT), hex("F0 00 20 29 02 0D 00 F7"));
}

// ---- lighting (MINI-PRM p12-14) ---------------------------------------------

#[test]
fn led_sysex_example_p14() {
    let specs = [
        Colourspec::Static {
            index: 11,
            palette: 13,
        },
        Colourspec::Flash {
            index: 12,
            b: 21,
            a: 23,
        },
        Colourspec::Pulse {
            index: 13,
            palette: 37,
        },
    ];
    assert_eq!(
        lighting(MINI, &specs),
        [hex("F0 00 20 29 02 0D 03 00 0B 0D 01 0C 15 17 02 0D 25 F7")]
    );
}

#[test]
fn rgb_colourspec_p14() {
    let rgb = Colourspec::Rgb {
        index: 11,
        r: 127,
        g: 0,
        b: 64,
    };
    assert_eq!(
        lighting(MINI, &[rgb]),
        [hex("F0 00 20 29 02 0D 03 03 0B 7F 00 40 F7")]
    );
    assert_eq!(to_7bit(0), 0);
    assert_eq!(to_7bit(128), 64);
    assert_eq!(to_7bit(255), 127);
}

#[test]
fn lighting_is_chunked_at_81_colourspecs_p14() {
    let statics: Vec<Colourspec> = (0..82)
        .map(|i| Colourspec::Static {
            index: i,
            palette: 1,
        })
        .collect();
    let msgs = lighting(MINI, &statics);
    assert_eq!(msgs.len(), 2, "81 + 1");
    assert_eq!(msgs[0].len(), 7 + 81 * 3 + 1);
    assert_eq!(msgs[1], hex("F0 00 20 29 02 0D 03 00 51 01 F7"));

    let rgb: Vec<Colourspec> = (0..MAX_COLOURSPECS as u8)
        .map(|i| Colourspec::Rgb {
            index: i,
            r: 0,
            g: 0,
            b: 0,
        })
        .collect();
    let msgs = lighting(MINI, &rgb);
    assert_eq!(msgs.len(), 1);
    assert_eq!(msgs[0].len(), 413, "a full RGB frame is one message");
    assert_eq!(lighting(MINI, &[]), Vec::<Vec<u8>>::new());
}

#[test]
fn channel_lighting_p13() {
    assert_eq!(
        channel_lighting(ChannelLight::Static, 0x0B, 0x05),
        [0x90, 0x0B, 0x05]
    );
    assert_eq!(
        channel_lighting(ChannelLight::Flash, 0x51, 0x13),
        [0x91, 0x51, 0x13]
    );
    assert_eq!(
        channel_lighting(ChannelLight::Pulse, 0x12, 0x2D),
        [0x92, 0x12, 0x2D]
    );
    assert_eq!(
        channel_lighting(ChannelLight::Static, 0x12, 0x00),
        [0x90, 0x12, 0x00],
        "off"
    );
}

#[test]
fn data_bytes_never_reach_the_status_range() {
    let rgb = Colourspec::Rgb {
        index: 11,
        r: 255,
        g: 128,
        b: 200,
    };
    assert_eq!(
        lighting(MINI, &[rgb]),
        [hex("F0 00 20 29 02 0D 03 03 0B 7F 7F 7F F7")],
        "out-of-range values are clamped to 127"
    );
    assert_eq!(brightness(MINI, 200), hex("F0 00 20 29 02 0D 08 7F F7"));
    assert_eq!(
        channel_lighting(ChannelLight::Static, 11, 200),
        [0x90, 0x0B, 0x7F]
    );
}

// ---- text (MINI-PRM p20) ----------------------------------------------------

#[test]
fn scrolling_text_example_p20() {
    assert_eq!(
        text(MINI, true, 7, TextColour::Palette(37), b"Hello World"),
        hex("F0 00 20 29 02 0D 07 01 07 00 25 48 65 6C 6C 6F 20 57 6F 72 6C 64 F7")
    );
}

#[test]
fn scrolling_text_stop_colour_and_direction_p20() {
    assert_eq!(text_stop(MINI), hex("F0 00 20 29 02 0D 07 F7"));
    assert_eq!(
        text(MINI, false, 7, TextColour::Rgb(127, 127, 127), b"A"),
        hex("F0 00 20 29 02 0D 07 00 07 01 7F 7F 7F 41 F7"),
        "RGB white"
    );
    assert_eq!(
        text(MINI, false, -7, TextColour::Palette(5), b"A"),
        hex("F0 00 20 29 02 0D 07 00 79 00 05 41 F7"),
        "negative speed scrolls left to right: 80h - 7"
    );
    assert_eq!(
        text(MINI, false, 7, TextColour::Palette(5), "é".as_bytes()),
        hex("F0 00 20 29 02 0D 07 00 07 00 05 3F 3F F7"),
        "bytes outside ASCII become '?'"
    );
}

// ---- power and configuration (MINI-PRM p21-22) ------------------------------

#[test]
fn brightness_p22() {
    assert_eq!(brightness(MINI, 127), hex("F0 00 20 29 02 0D 08 7F F7"));
    assert_eq!(query(MINI, BRIGHTNESS), hex("F0 00 20 29 02 0D 08 F7"));
}

#[test]
fn sleep_and_wake_p21() {
    assert_eq!(
        sleep(MINI, false),
        hex("F0 00 20 29 02 0D 09 00 F7"),
        "LEDs off"
    );
    assert_eq!(
        sleep(MINI, true),
        hex("F0 00 20 29 02 0D 09 01 F7"),
        "LEDs on"
    );
    assert_eq!(query(MINI, SLEEP), hex("F0 00 20 29 02 0D 09 F7"));
}

#[test]
fn led_feedback_p22() {
    assert_eq!(
        feedback(MINI, false, true),
        hex("F0 00 20 29 02 0D 0A 00 01 F7")
    );
    assert_eq!(query(MINI, FEEDBACK), hex("F0 00 20 29 02 0D 0A F7"));
}

// ---- the Launchpad X product byte -------------------------------------------

#[test]
fn launchpad_x_bytes_equal_the_existing_x_tests() {
    assert_eq!(programmer_mode(X, true), hex("F0 00 20 29 02 0C 0E 01 F7"));
    assert_eq!(programmer_mode(X, false), hex("F0 00 20 29 02 0C 0E 00 F7"));
    let rgb = Colourspec::Rgb {
        index: 11,
        r: to_7bit(255),
        g: to_7bit(0),
        b: to_7bit(128),
    };
    assert_eq!(
        lighting(X, &[rgb]),
        [hex("F0 00 20 29 02 0C 03 03 0B 7F 00 40 F7")]
    );
    let flash = Colourspec::Flash {
        index: 11,
        b: 3,
        a: 5,
    };
    assert_eq!(
        lighting(X, &[flash]),
        [hex("F0 00 20 29 02 0C 03 01 0B 03 05 F7")]
    );
    let pulse = Colourspec::Pulse {
        index: 11,
        palette: 5,
    };
    assert_eq!(
        lighting(X, &[pulse]),
        [hex("F0 00 20 29 02 0C 03 02 0B 05 F7")]
    );
}

// ---- Programmer layout (MINI-PRM p10, p13) ----------------------------------

fn full_surface() -> DeviceCapabilities {
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
        velocity: false,
        pressure: false,
        animations: AnimationCaps::default(),
    }
}

#[test]
fn layout_map_p10_p13() {
    assert_eq!(led_index(ControlPos::grid(0, 7)), Some(11), "lower left");
    assert_eq!(led_index(ControlPos::grid(0, 0)), Some(81), "upper left");
    assert_eq!(led_index(ControlPos::grid(7, 7)), Some(18), "lower right");
    let top: Vec<_> = (0..8)
        .map(|index| led_index(ControlPos::Top { index }).unwrap())
        .collect();
    assert_eq!(top, [91, 92, 93, 94, 95, 96, 97, 98]);
    let side: Vec<_> = (0..8)
        .map(|index| led_index(ControlPos::Side { index }).unwrap())
        .collect();
    assert_eq!(side, [89, 79, 69, 59, 49, 39, 29, 19]);
    assert_eq!(led_index(ControlPos::Logo), Some(99));
    assert_eq!(led_index(ControlPos::grid(8, 0)), None);
    assert_eq!(led_index(ControlPos::Top { index: 8 }), None);

    let mut all: Vec<u8> = full_surface()
        .positions()
        .into_iter()
        .map(|p| led_index(p).unwrap())
        .collect();
    all.sort_unstable();
    all.dedup();
    assert_eq!(all.len(), MAX_COLOURSPECS, "81 distinct indices");
}

#[test]
fn every_button_maps_back_to_its_position_p10() {
    for pos in full_surface().positions() {
        let index = led_index(pos).unwrap();
        match pos {
            ControlPos::Grid { .. } => assert_eq!(pos_from_note(index), Some(pos)),
            ControlPos::Top { .. } | ControlPos::Side { .. } => {
                assert_eq!(pos_from_cc(index), Some(pos))
            }
            ControlPos::Logo => {
                assert_eq!(pos_from_cc(index), None, "the logo is not a button");
                assert_eq!(pos_from_note(index), None);
            }
        }
    }
}

// ---- input (MINI-PRM p6, p10) -----------------------------------------------

#[test]
fn input_presses_and_releases_p6_p10() {
    let lower_left = ControlPos::grid(0, 7);
    assert_eq!(
        decode(&hex("90 0B 7F"), true),
        Some((lower_left, InputKind::Pressed { velocity: 127 }))
    );
    assert_eq!(
        decode(&hex("90 0B 00"), true),
        Some((lower_left, InputKind::Released))
    );
    assert_eq!(
        decode(&hex("80 0B 00"), true),
        Some((lower_left, InputKind::Released))
    );
    assert_eq!(
        decode(&hex("B0 5B 7F"), true),
        Some((
            ControlPos::Top { index: 0 },
            InputKind::Pressed { velocity: 127 }
        ))
    );
    assert_eq!(
        decode(&hex("B0 13 00"), true),
        Some((ControlPos::Side { index: 7 }, InputKind::Released))
    );
    assert_eq!(
        decode(&hex("9F 0B 7F"), true),
        Some((lower_left, InputKind::Pressed { velocity: 127 })),
        "the channel is masked"
    );
}

#[test]
fn input_ignores_what_is_not_a_button_p10() {
    for line in [
        "B0 63 7F", "A0 0B 40", "D0 40", "F8", "90 05 7F", "90 0B", "",
    ] {
        assert_eq!(decode(&hex(line), true), None, "{line}");
        assert_eq!(decode(&hex(line), false), None, "{line}");
    }
}

#[test]
fn velocity_is_reported_only_when_the_model_has_it() {
    let lower_left = ControlPos::grid(0, 7);
    assert_eq!(
        decode(&hex("90 0B 05"), false),
        Some((lower_left, InputKind::Pressed { velocity: 127 })),
        "Mini: no velocity, every press is 127"
    );
    assert_eq!(
        decode(&hex("90 0B 05"), true),
        Some((lower_left, InputKind::Pressed { velocity: 5 })),
        "X: the velocity is passed through"
    );
}
