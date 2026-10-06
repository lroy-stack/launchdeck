//! Measurements of the device layer (LAUNCHPAD_MINI_MK3_PLAN.md sections
//! 8.4 and 14.6), printed, never checked: what each way of lighting LEDs
//! puts on the wire and costs the host to encode and send over the fake
//! transport, how many renders a unit takes to be owned, and how long a
//! press takes from the input callback to a `DeviceInput`. Host-side only,
//! in whatever build the test runs: not device latency, which only a unit
//! can show (the acceptance suite). docs/contributing/hardware-testing.md
//! records the numbers. Run by name:
//! `cargo test --lib novation::measure_tests -- --ignored --nocapture`

use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::sync::mpsc::unbounded_channel;

use super::device::Mk3Device;
use super::layout::led_index;
use super::ports;
use super::profile::ModelProfile;
use super::protocol::{self, channel_lighting, ChannelLight, Colourspec, Product};
use super::testkit::{FakeClock, SimLaunchpad};
use super::{launchpad_mini_mk3, launchpad_x};
use crate::core::model::ControlPos;
use crate::devices::midi::testkit::{port, FakeMidi};
use crate::devices::midi::MidiBackend;
use crate::devices::{SurfaceDevice, SurfaceFrame};
use crate::platform::Platform;

/// Runs of each timed operation; the median is reported.
const RUNS: usize = 1000;
/// The runtime's render interval: the fake time one render stands for.
const TICK: Duration = Duration::from_millis(40);
/// More renders than any acquisition takes, timeouts included.
const MAX_TICKS: u32 = 100;
const MINI: &ModelProfile = &launchpad_mini_mk3::PROFILE;
/// The bytes one USB-MIDI event packet carries (USB MIDI 1.0, section 4):
/// a whole channel message, or three bytes of a SysEx.
const PACKET_BYTES: usize = 3;
const IN_ID: &str = "2001";
const OUT_ID: &str = "2002";

/// LEDs one render changes. The acceptance suite times the same cases
/// and strategies on a unit.
pub(super) struct Case {
    pub(super) name: &'static str,
    pub(super) positions: Vec<ControlPos>,
}

pub(super) fn cases() -> Vec<Case> {
    let grid = |rows: u8| -> Vec<ControlPos> {
        (0..rows)
            .flat_map(|row| (0..8).map(move |col| ControlPos::grid(col, row)))
            .collect()
    };
    vec![
        Case {
            name: "1 LED",
            positions: vec![ControlPos::grid(0, 0)],
        },
        Case {
            name: "8 LEDs (a row)",
            positions: grid(1),
        },
        Case {
            name: "64-pad page switch",
            positions: grid(8),
        },
        Case {
            name: "81-LED redraw",
            positions: MINI.capabilities.positions(),
        },
    ]
}

/// How the LEDs of one render are sent.
#[derive(Clone, Copy)]
pub(super) enum Strategy {
    /// One `03h` message, RGB colourspecs: what LaunchDeck sends.
    BatchRgb,
    /// One `03h` message, static palette colourspecs.
    BatchPalette,
    /// One `03h` message per LED, RGB.
    PerLedSysex,
    /// One channel-1 Note On per LED, a palette entry (MINI-PRM p13).
    PerLedNotes,
}

pub(super) const STRATEGIES: [(Strategy, &str); 4] = [
    (Strategy::BatchRgb, "(a) one 03h batch, RGB"),
    (Strategy::BatchPalette, "(b) one 03h batch, palette"),
    (Strategy::PerLedSysex, "(c) per-LED 03h, RGB"),
    (Strategy::PerLedNotes, "(d) per-LED channel notes"),
];

/// A colour of its own for the n-th LED.
fn rgb(n: u8) -> (u8, u8, u8) {
    (n % 128, 127 - n % 128, n.wrapping_mul(3) % 128)
}

fn palette(n: u8) -> u8 {
    n % 127 + 1
}

/// The messages that light these LED indices.
pub(super) fn encode(strategy: Strategy, product: Product, indices: &[u8]) -> Vec<Vec<u8>> {
    let numbered = indices.iter().zip(0u8..);
    match strategy {
        Strategy::BatchRgb | Strategy::PerLedSysex => {
            let specs: Vec<Colourspec> = numbered
                .map(|(&index, n)| {
                    let (r, g, b) = rgb(n);
                    Colourspec::Rgb { index, r, g, b }
                })
                .collect();
            if matches!(strategy, Strategy::BatchRgb) {
                protocol::lighting(product, &specs)
            } else {
                specs
                    .iter()
                    .flat_map(|spec| protocol::lighting(product, std::slice::from_ref(spec)))
                    .collect()
            }
        }
        Strategy::BatchPalette => {
            let specs: Vec<Colourspec> = numbered
                .map(|(&index, n)| Colourspec::Static {
                    index,
                    palette: palette(n),
                })
                .collect();
            protocol::lighting(product, &specs)
        }
        Strategy::PerLedNotes => numbered
            .map(|(&index, n)| channel_lighting(ChannelLight::Static, index, palette(n)).to_vec())
            .collect(),
    }
}

/// USB-MIDI event packets for these messages: one per channel message,
/// ⌈n/3⌉ for an n-byte SysEx.
fn packets(messages: &[Vec<u8>]) -> usize {
    messages
        .iter()
        .map(|m| {
            if m.first() == Some(&0xF0) {
                m.len().div_ceil(PACKET_BYTES)
            } else {
                1
            }
        })
        .sum()
}

fn median(mut times: Vec<Duration>) -> Duration {
    times.sort();
    times[times.len() / 2]
}

fn micros(d: Duration) -> String {
    format!("{:.1} µs", d.as_secs_f64() * 1e6)
}

/// A unit's MIDI-interface ports on the fake transport, as macOS names
/// them.
fn unit_ports(profile: &ModelProfile) -> FakeMidi {
    let (input, output) = if profile.model == MINI.model {
        (
            "Launchpad Mini MK3 LPMiniMK3 MIDI Out",
            "Launchpad Mini MK3 LPMiniMK3 MIDI In",
        )
    } else {
        ("Launchpad X LPX MIDI Out", "Launchpad X LPX MIDI In")
    };
    let midi = FakeMidi::default();
    midi.add(port(input, IN_ID), port(output, OUT_ID));
    midi
}

#[test]
#[ignore = "a measurement: run it by name with --ignored --nocapture"]
fn lighting_strategies_are_measured() {
    let midi = unit_ports(MINI);
    let snapshot = midi.ports();
    let mut link = midi
        .open(&snapshot.inputs[0], &snapshot.outputs[0], Box::new(|_| {}))
        .unwrap();
    println!("| LEDs | strategy | bytes | USB-MIDI packets | messages per tick | encode + send, median of {RUNS} |");
    println!("| --- | --- | ---: | ---: | ---: | ---: |");
    for case in cases() {
        let indices: Vec<u8> = case
            .positions
            .iter()
            .filter_map(|&p| led_index(p))
            .collect();
        assert_eq!(indices.len(), case.positions.len());
        for (strategy, label) in STRATEGIES {
            let messages = encode(strategy, MINI.product, &indices);
            let bytes: usize = messages.iter().map(Vec::len).sum();
            let times: Vec<Duration> = (0..RUNS)
                .map(|_| {
                    let began = Instant::now();
                    for message in encode(strategy, MINI.product, &indices) {
                        link.send(&message).unwrap();
                    }
                    let took = began.elapsed();
                    midi.take_sent();
                    took
                })
                .collect();
            println!(
                "| {} | {label} | {bytes} | {} | {} | {} |",
                case.name,
                packets(&messages),
                messages.len(),
                micros(median(times)),
            );
        }
    }
}

/// Renders, `TICK` of fake time apart, from connect until the unit is sent
/// its first lighting message (the clear frame: it is owned); and the mode
/// it reports then.
fn renders_to_owned(profile: &'static ModelProfile, sim: SimLaunchpad) -> (u32, String) {
    let midi = unit_ports(profile);
    midi.bind(OUT_ID, IN_ID, sim);
    let clock = FakeClock::new();
    let mut units = ports::parse(&midi.ports(), Platform::MacOs, &[profile]).units;
    let mut dev = Mk3Device::new(units.remove(0), Arc::new(midi.clone())).with_clock(clock.clock());
    let (tx, _rx) = unbounded_channel();
    dev.connect(tx).unwrap();
    let lighting = |m: &Vec<u8>| m.len() > 7 && m[0] == 0xF0 && m[6] == protocol::LED;
    for tick in 1..=MAX_TICKS {
        clock.advance(TICK);
        dev.render(&SurfaceFrame::default()).unwrap();
        if midi.take_sent().iter().any(|(_, m)| lighting(m)) {
            return (tick, format!("{:?}", dev.details().mode));
        }
    }
    panic!("not owned after {MAX_TICKS} renders");
}

#[test]
#[ignore = "a measurement: run it by name with --ignored --nocapture"]
fn connect_to_owned_is_measured() {
    for (what, profile, sim) in [
        ("Mini MK3, answering", MINI, SimLaunchpad::mini()),
        (
            "Launchpad X, answering",
            &launchpad_x::PROFILE,
            SimLaunchpad::x(),
        ),
        ("Mini MK3, silent", MINI, SimLaunchpad::mini().silent()),
    ] {
        let (ticks, mode) = renders_to_owned(profile, sim);
        println!(
            "connect to owned, {what}: {ticks} renders, {} ms of fake time at {} ms a render (mode {mode})",
            u128::from(ticks) * TICK.as_millis(),
            TICK.as_millis()
        );
    }
}

#[test]
#[ignore = "a measurement: run it by name with --ignored --nocapture"]
fn decode_to_device_input_is_measured() {
    let midi = unit_ports(MINI);
    let sim = SimLaunchpad::mini();
    midi.bind(OUT_ID, IN_ID, sim.clone());
    let mut units = ports::parse(&midi.ports(), Platform::MacOs, &[MINI]).units;
    let mut dev = Mk3Device::new(units.remove(0), Arc::new(midi.clone()));
    let (tx, mut rx) = unbounded_channel();
    dev.connect(tx).unwrap();
    for _ in 0..2 {
        dev.render(&SurfaceFrame::default()).unwrap();
    }
    assert!(sim.in_programmer_mode());
    let pad = ControlPos::grid(3, 4);
    let (press, release) = (sim.press(pad), sim.release(pad));
    let times: Vec<Duration> = (0..RUNS)
        .map(|_| {
            let began = Instant::now();
            midi.inject(IN_ID, &press);
            let input = rx.try_recv();
            let took = began.elapsed();
            assert_eq!(input.map(|i| i.pos), Ok(pad));
            midi.inject(IN_ID, &release);
            rx.try_recv().unwrap();
            took
        })
        .collect();
    println!(
        "decode to DeviceInput (callback bytes to the input channel), median of {RUNS}: {}",
        micros(median(times))
    );
}
