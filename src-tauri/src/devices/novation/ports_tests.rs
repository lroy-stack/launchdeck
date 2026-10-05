//! Port-name fixtures per system (LAUNCHPAD_MINI_MK3_PLAN.md section 14.3).
//!
//! Every fixture is derived from docs/community, not observed on this
//! system: names come from Novation's screenshots, Microsoft's naming rules,
//! the Linux kernel sources and community projects. No Launchpad, no Windows
//! machine and no Linux desktop were available when they were written.

use super::launchpad_mini_mk3;
use super::launchpad_x;
use super::ports::{parse, read, Model, Parsed};
use super::profile::{ModelProfile, Role};
use crate::devices::midi::{PortInfo, PortSnapshot};
use crate::platform::Platform::{self, Linux, MacOs, Windows};

const MINI: &str = "launchpad-mini-mk3";
const X: &str = "launchpad-x";

fn profiles() -> [&'static ModelProfile; 2] {
    [&launchpad_x::PROFILE, &launchpad_mini_mk3::PROFILE]
}

fn ports(list: &[(&str, &str)]) -> Vec<PortInfo> {
    list.iter()
        .map(|(name, id)| PortInfo {
            name: name.to_string(),
            id: id.to_string(),
        })
        .collect()
}

fn on(platform: Platform, inputs: &[(&str, &str)], outputs: &[(&str, &str)]) -> Parsed {
    let snapshot = PortSnapshot {
        inputs: ports(inputs),
        outputs: ports(outputs),
    };
    parse(&snapshot, platform, &profiles())
}

/// `(model, id, name, unit key, (input name, input id), (output name, output id))`.
type Unit<'a> = (
    &'a str,
    &'a str,
    &'a str,
    u32,
    (&'a str, &'a str),
    (&'a str, &'a str),
);

fn units(parsed: &Parsed) -> Vec<Unit<'_>> {
    parsed
        .units
        .iter()
        .map(|u| {
            (
                u.profile.model,
                u.id.as_str(),
                u.name.as_str(),
                u.key,
                (u.input.name.as_str(), u.input.id.as_str()),
                (u.output.name.as_str(), u.output.id.as_str()),
            )
        })
        .collect()
}

fn unsupported(parsed: &Parsed) -> Vec<(&str, &str)> {
    parsed
        .unsupported
        .iter()
        .map(|d| (d.id.as_str(), d.name.as_str()))
        .collect()
}

/// `(model, role, unit key)` of one name.
fn reads(platform: Platform, name: &str) -> Option<(&'static str, Option<Role>, u32)> {
    let reading = read(name, platform, &profiles())?;
    let model = match reading.model {
        Model::Known(profile) => profile.model,
        Model::Other(_) => "other",
    };
    Some((model, reading.role, reading.key))
}

// ---- macOS -----------------------------------------------------------------

const M_DAW_OUT: &str = "Launchpad Mini MK3 LPMiniMK3 DAW Out";
const M_MIDI_OUT: &str = "Launchpad Mini MK3 LPMiniMK3 MIDI Out";
const M_DAW_IN: &str = "Launchpad Mini MK3 LPMiniMK3 DAW In";
const M_MIDI_IN: &str = "Launchpad Mini MK3 LPMiniMK3 MIDI In";

#[test]
fn m1_macos_mini_opens_its_midi_pair() {
    let parsed = on(
        MacOs,
        &[(M_DAW_OUT, "101"), (M_MIDI_OUT, "102")],
        &[(M_DAW_IN, "103"), (M_MIDI_IN, "104")],
    );
    assert_eq!(
        units(&parsed),
        [(
            MINI,
            "launchpad-mini-mk3:1",
            "Launchpad Mini MK3",
            1,
            (M_MIDI_OUT, "102"),
            (M_MIDI_IN, "104")
        )]
    );
    assert!(parsed.unsupported.is_empty());
    assert_eq!(reads(MacOs, M_DAW_OUT), Some((MINI, Some(Role::Daw), 1)));
    assert_eq!(reads(MacOs, M_MIDI_IN), Some((MINI, Some(Role::Midi), 1)));
}

#[test]
fn m2_macos_mini_without_in_and_out() {
    let daw = "Launchpad Mini MK3 LPMiniMK3 DAW";
    let midi = "Launchpad Mini MK3 LPMiniMK3 MIDI";
    let parsed = on(
        MacOs,
        &[(daw, "101"), (midi, "102")],
        &[(daw, "103"), (midi, "104")],
    );
    assert_eq!(
        units(&parsed),
        [(
            MINI,
            "launchpad-mini-mk3:1",
            "Launchpad Mini MK3",
            1,
            (midi, "102"),
            (midi, "104")
        )]
    );
}

#[test]
fn m3_macos_two_identical_minis_pair_in_list_order() {
    let parsed = on(
        MacOs,
        &[
            (M_DAW_OUT, "101"),
            (M_MIDI_OUT, "102"),
            (M_DAW_OUT, "201"),
            (M_MIDI_OUT, "202"),
        ],
        &[
            (M_DAW_IN, "103"),
            (M_MIDI_IN, "104"),
            (M_DAW_IN, "203"),
            (M_MIDI_IN, "204"),
        ],
    );
    assert_eq!(
        units(&parsed),
        [
            (
                MINI,
                "launchpad-mini-mk3:1",
                "Launchpad Mini MK3",
                1,
                (M_MIDI_OUT, "102"),
                (M_MIDI_IN, "104")
            ),
            (
                MINI,
                "launchpad-mini-mk3:2",
                "Launchpad Mini MK3 (2)",
                1,
                (M_MIDI_OUT, "202"),
                (M_MIDI_IN, "204")
            ),
        ]
    );
}

#[test]
fn m4_macos_launchpad_x() {
    let parsed = on(
        MacOs,
        &[
            ("Launchpad X LPX DAW Out", "301"),
            ("Launchpad X LPX MIDI Out", "302"),
        ],
        &[
            ("Launchpad X LPX DAW In", "303"),
            ("Launchpad X LPX MIDI In", "304"),
        ],
    );
    assert_eq!(
        units(&parsed),
        [(
            X,
            "launchpad-x:1",
            "Launchpad X",
            1,
            ("Launchpad X LPX MIDI Out", "302"),
            ("Launchpad X LPX MIDI In", "304")
        )]
    );
}

#[test]
fn m5_macos_pro_mk3_is_listed_once_and_the_iac_bus_not_at_all() {
    let list = [
        ("IAC Driver Bus 1", "401"),
        ("Launchpad Pro MK3 LPProMK3 MIDI", "402"),
        ("Launchpad Pro MK3 LPProMK3 DAW", "403"),
        ("Launchpad Pro MK3 LPProMK3 DIN", "404"),
    ];
    let parsed = on(MacOs, &list, &list);
    assert!(parsed.units.is_empty());
    assert_eq!(
        unsupported(&parsed),
        [("unsupported:launchpad-pro-mk3", "Launchpad Pro MK3")]
    );
    assert_eq!(reads(MacOs, "IAC Driver Bus 1"), None);
}

#[test]
fn macos_two_pro_mk3_units_are_listed_twice() {
    let list = [
        ("Launchpad Pro MK3 LPProMK3 MIDI", "402"),
        ("Launchpad Pro MK3 LPProMK3 DAW", "403"),
        ("Launchpad Pro MK3 LPProMK3 MIDI", "502"),
        ("Launchpad Pro MK3 LPProMK3 DAW", "503"),
    ];
    assert_eq!(
        unsupported(&on(MacOs, &list, &list)),
        [
            ("unsupported:launchpad-pro-mk3", "Launchpad Pro MK3"),
            ("unsupported:launchpad-pro-mk3-2", "Launchpad Pro MK3 (2)"),
        ]
    );
}

// ---- Windows, classic (WinMM) names ------------------------------------------

/// The interface path midir gives as the id, the same for every port of a unit.
const UNIT_A: &str = r"\\?\usb#vid_1235&pid_0113#a";
const UNIT_B: &str = r"\\?\usb#vid_1235&pid_0113#b";

#[test]
fn w1_windows_mini_opens_the_second_interface() {
    let parsed = on(
        Windows,
        &[
            ("LPMiniMK3 MIDI", UNIT_A),
            ("MIDIIN2 (LPMiniMK3 MIDI)", UNIT_A),
        ],
        &[
            ("LPMiniMK3 MIDI", UNIT_A),
            ("MIDIOUT2 (LPMiniMK3 MIDI)", UNIT_A),
        ],
    );
    assert_eq!(
        units(&parsed),
        [(
            MINI,
            "launchpad-mini-mk3:1",
            "Launchpad Mini MK3",
            1,
            ("MIDIIN2 (LPMiniMK3 MIDI)", UNIT_A),
            ("MIDIOUT2 (LPMiniMK3 MIDI)", UNIT_A)
        )]
    );
    assert_eq!(
        reads(Windows, "LPMiniMK3 MIDI"),
        Some((MINI, Some(Role::Daw), 1)),
        "the bare classic name is the first interface, the DAW one"
    );
}

#[test]
fn w2_windows_second_mini_has_a_leading_marker() {
    let parsed = on(
        Windows,
        &[
            ("LPMiniMK3 MIDI", UNIT_A),
            ("MIDIIN2 (LPMiniMK3 MIDI)", UNIT_A),
            ("2- LPMiniMK3 MIDI", UNIT_B),
            ("MIDIIN2 (2- LPMiniMK3 MIDI)", UNIT_B),
        ],
        &[
            ("LPMiniMK3 MIDI", UNIT_A),
            ("MIDIOUT2 (LPMiniMK3 MIDI)", UNIT_A),
            ("2- LPMiniMK3 MIDI", UNIT_B),
            ("MIDIOUT2 (2- LPMiniMK3 MIDI)", UNIT_B),
        ],
    );
    assert_eq!(
        units(&parsed),
        [
            (
                MINI,
                "launchpad-mini-mk3:1",
                "Launchpad Mini MK3",
                1,
                ("MIDIIN2 (LPMiniMK3 MIDI)", UNIT_A),
                ("MIDIOUT2 (LPMiniMK3 MIDI)", UNIT_A)
            ),
            (
                MINI,
                "launchpad-mini-mk3:2",
                "Launchpad Mini MK3 (2)",
                2,
                ("MIDIIN2 (2- LPMiniMK3 MIDI)", UNIT_B),
                ("MIDIOUT2 (2- LPMiniMK3 MIDI)", UNIT_B)
            ),
        ]
    );
    assert_eq!(
        reads(Windows, "2- LPMiniMK3 MIDI"),
        Some((MINI, Some(Role::Daw), 2))
    );
}

#[test]
fn w3_windows_identical_names_are_split_by_port_id() {
    // The outputs are listed in the other order: pairing by position would
    // join one unit's input with the other's output.
    let parsed = on(
        Windows,
        &[
            ("LPMiniMK3 MIDI", UNIT_A),
            ("MIDIIN2 (LPMiniMK3 MIDI)", UNIT_A),
            ("LPMiniMK3 MIDI", UNIT_B),
            ("MIDIIN2 (LPMiniMK3 MIDI)", UNIT_B),
        ],
        &[
            ("LPMiniMK3 MIDI", UNIT_B),
            ("MIDIOUT2 (LPMiniMK3 MIDI)", UNIT_B),
            ("LPMiniMK3 MIDI", UNIT_A),
            ("MIDIOUT2 (LPMiniMK3 MIDI)", UNIT_A),
        ],
    );
    assert_eq!(
        units(&parsed),
        [
            (
                MINI,
                "launchpad-mini-mk3:1",
                "Launchpad Mini MK3",
                1,
                ("MIDIIN2 (LPMiniMK3 MIDI)", UNIT_A),
                ("MIDIOUT2 (LPMiniMK3 MIDI)", UNIT_A)
            ),
            (
                MINI,
                "launchpad-mini-mk3:2",
                "Launchpad Mini MK3 (2)",
                1,
                ("MIDIIN2 (LPMiniMK3 MIDI)", UNIT_B),
                ("MIDIOUT2 (LPMiniMK3 MIDI)", UNIT_B)
            ),
        ]
    );
}

#[test]
fn w4_windows_launchpad_x_opens_the_second_interface() {
    let parsed = on(
        Windows,
        &[("LPX MIDI", UNIT_A), ("MIDIIN2 (LPX MIDI)", UNIT_A)],
        &[("LPX MIDI", UNIT_A), ("MIDIOUT2 (LPX MIDI)", UNIT_A)],
    );
    assert_eq!(
        units(&parsed),
        [(
            X,
            "launchpad-x:1",
            "Launchpad X",
            1,
            ("MIDIIN2 (LPX MIDI)", UNIT_A),
            ("MIDIOUT2 (LPX MIDI)", UNIT_A)
        )]
    );
    assert_eq!(
        reads(Windows, "LPX MIDI"),
        Some((X, Some(Role::Daw), 1)),
        "D-X1: `LPX MIDI` is the DAW interface on Windows"
    );
}

#[test]
fn w5_windows_synth_is_nothing() {
    let parsed = on(Windows, &[], &[("Microsoft GS Wavetable Synth", "0")]);
    assert_eq!(parsed, Parsed::default());
}

// ---- Windows MIDI Services, new-style names ------------------------------------

const N1_IN: [(&str, &str); 2] = [("LPMiniMK3 DAW Out", "n1"), ("LPMiniMK3 MIDI Out", "n2")];
const N1_OUT: [(&str, &str); 2] = [("LPMiniMK3 DAW In", "n3"), ("LPMiniMK3 MIDI In", "n4")];

#[test]
fn n1_new_style_mini() {
    let parsed = on(Windows, &N1_IN, &N1_OUT);
    assert_eq!(
        units(&parsed),
        [(
            MINI,
            "launchpad-mini-mk3:1",
            "Launchpad Mini MK3",
            1,
            ("LPMiniMK3 MIDI Out", "n2"),
            ("LPMiniMK3 MIDI In", "n4")
        )]
    );
    assert_eq!(
        reads(Windows, "LPMiniMK3 DAW Out"),
        Some((MINI, Some(Role::Daw), 1))
    );
}

#[test]
fn n2_new_style_second_mini_has_a_trailing_marker() {
    let inputs = [
        N1_IN[0],
        N1_IN[1],
        ("LPMiniMK3 DAW Out (2)", "n5"),
        ("LPMiniMK3 MIDI Out (2)", "n6"),
    ];
    let outputs = [
        N1_OUT[0],
        N1_OUT[1],
        ("LPMiniMK3 DAW In (2)", "n7"),
        ("LPMiniMK3 MIDI In (2)", "n8"),
    ];
    let parsed = on(Windows, &inputs, &outputs);
    assert_eq!(
        units(&parsed),
        [
            (
                MINI,
                "launchpad-mini-mk3:1",
                "Launchpad Mini MK3",
                1,
                ("LPMiniMK3 MIDI Out", "n2"),
                ("LPMiniMK3 MIDI In", "n4")
            ),
            (
                MINI,
                "launchpad-mini-mk3:2",
                "Launchpad Mini MK3 (2)",
                2,
                ("LPMiniMK3 MIDI Out (2)", "n6"),
                ("LPMiniMK3 MIDI In (2)", "n8")
            ),
        ]
    );
}

#[test]
fn n3_new_style_marker_after_the_device_name() {
    let parsed = on(
        Windows,
        &[
            ("Launchpad X LPX MIDI Out", "x1"),
            ("Launchpad X (2) LPX MIDI Out", "x3"),
        ],
        &[
            ("Launchpad X LPX MIDI In", "x2"),
            ("Launchpad X (2) LPX MIDI In", "x4"),
        ],
    );
    assert_eq!(
        units(&parsed),
        [
            (
                X,
                "launchpad-x:1",
                "Launchpad X",
                1,
                ("Launchpad X LPX MIDI Out", "x1"),
                ("Launchpad X LPX MIDI In", "x2")
            ),
            (
                X,
                "launchpad-x:2",
                "Launchpad X (2)",
                2,
                ("Launchpad X (2) LPX MIDI Out", "x3"),
                ("Launchpad X (2) LPX MIDI In", "x4")
            ),
        ]
    );
}

// ---- Linux (ALSA sequencer) ----------------------------------------------------

const L_DA: &str = "Launchpad Mini MK3:Launchpad Mini MK3 LPMiniMK3 DA 20:0";
const L_MI: &str = "Launchpad Mini MK3:Launchpad Mini MK3 LPMiniMK3 MI 20:1";

#[test]
fn l1_linux_mini_with_truncated_names() {
    let list = [(L_DA, "20:0"), (L_MI, "20:1")];
    let parsed = on(Linux, &list, &list);
    assert_eq!(
        units(&parsed),
        [(
            MINI,
            "launchpad-mini-mk3:1",
            "Launchpad Mini MK3",
            20,
            (L_MI, "20:1"),
            (L_MI, "20:1")
        )]
    );
    assert!(parsed.unsupported.is_empty(), "D-X4: no Mini listed twice");
    assert_eq!(reads(Linux, L_DA), Some((MINI, Some(Role::Daw), 20)));
}

#[test]
fn l2_linux_before_5_13_numbers_the_interfaces() {
    let daw = "Launchpad Mini MK3:Launchpad Mini MK3 MIDI 1 20:0";
    let midi = "Launchpad Mini MK3:Launchpad Mini MK3 MIDI 2 20:1";
    let list = [(daw, "20:0"), (midi, "20:1")];
    let parsed = on(Linux, &list, &list);
    assert_eq!(
        units(&parsed),
        [(
            MINI,
            "launchpad-mini-mk3:1",
            "Launchpad Mini MK3",
            20,
            (midi, "20:1"),
            (midi, "20:1")
        )]
    );
}

#[test]
fn l3_linux_two_minis_by_client() {
    let da_24 = "Launchpad Mini MK3:Launchpad Mini MK3 LPMiniMK3 DA 24:0";
    let mi_24 = "Launchpad Mini MK3:Launchpad Mini MK3 LPMiniMK3 MI 24:1";
    let list = [
        (L_DA, "20:0"),
        (L_MI, "20:1"),
        (da_24, "24:0"),
        (mi_24, "24:1"),
    ];
    let parsed = on(Linux, &list, &list);
    assert_eq!(
        units(&parsed),
        [
            (
                MINI,
                "launchpad-mini-mk3:1",
                "Launchpad Mini MK3",
                20,
                (L_MI, "20:1"),
                (L_MI, "20:1")
            ),
            (
                MINI,
                "launchpad-mini-mk3:2",
                "Launchpad Mini MK3 (2)",
                24,
                (mi_24, "24:1"),
                (mi_24, "24:1")
            ),
        ]
    );
}

#[test]
fn l4_linux_launchpad_x() {
    let daw = "Launchpad X:Launchpad X LPX DAW In 28:0";
    let midi = "Launchpad X:Launchpad X LPX MIDI In 28:1";
    let list = [(daw, "28:0"), (midi, "28:1")];
    let parsed = on(Linux, &list, &list);
    assert_eq!(
        units(&parsed),
        [(
            X,
            "launchpad-x:1",
            "Launchpad X",
            28,
            (midi, "28:1"),
            (midi, "28:1")
        )]
    );
}

#[test]
fn l5_linux_before_5_13_launchpad_x_opens_midi_2() {
    let daw = "Launchpad X:Launchpad X MIDI 1 28:0";
    let midi = "Launchpad X:Launchpad X MIDI 2 28:1";
    let list = [(daw, "28:0"), (midi, "28:1")];
    let parsed = on(Linux, &list, &list);
    assert_eq!(
        units(&parsed),
        [(
            X,
            "launchpad-x:1",
            "Launchpad X",
            28,
            (midi, "28:1"),
            (midi, "28:1")
        )]
    );
    assert_eq!(
        reads(Linux, daw),
        Some((X, Some(Role::Daw), 28)),
        "D-X2: `MIDI 1` is the DAW interface"
    );
}

#[test]
fn l6_linux_launchpad_xl_is_listed_and_midi_through_is_not() {
    let list = [
        ("Midi Through:Midi Through Port-0 14:0", "14:0"),
        ("Launchpad XL:Launchpad XL MIDI 1 24:0", "24:0"),
    ];
    let parsed = on(Linux, &list, &list);
    assert!(
        parsed.units.is_empty(),
        "a Launchpad XL is not a Launchpad X"
    );
    assert_eq!(
        unsupported(&parsed),
        [("unsupported:launchpad-xl", "Launchpad XL")]
    );
}

#[test]
fn linux_without_a_role_word_takes_the_interface_from_the_alsa_port() {
    assert_eq!(
        reads(Linux, "Launchpad X:Launchpad X 28:0"),
        Some((X, Some(Role::Daw), 28))
    );
    assert_eq!(
        reads(Linux, "Launchpad X:Launchpad X 28:1"),
        Some((X, Some(Role::Midi), 28))
    );
}

// ---- every system --------------------------------------------------------------

#[test]
fn a_unit_without_its_output_is_not_a_unit() {
    let parsed = on(MacOs, &[(M_MIDI_OUT, "102")], &[(M_DAW_IN, "103")]);
    assert!(parsed.units.is_empty());
}

#[test]
fn other_launchpads_on_windows_are_named_by_their_port_stem() {
    let inputs = [
        ("LPProMK3 MIDI", UNIT_A),
        ("MIDIIN2 (LPProMK3 MIDI)", UNIT_A),
        ("MIDIIN3 (LPProMK3 MIDI)", UNIT_A),
    ];
    assert_eq!(
        unsupported(&on(Windows, &inputs, &[])),
        [("unsupported:lppromk3", "LPProMK3")]
    );
}
