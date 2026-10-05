//! The Launchpad X golden transcript: every byte the adapter sends and every
//! input it decodes, over `FakeMidi`. Only the commits that fix a defect of
//! LAUNCHPAD_MINI_MK3_PLAN.md section 2.3 may change it, each with a test of
//! the fix.

use std::sync::Arc;

use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver};

use super::NovationHardware;
use crate::core::model::{Animation, ControlPos, Rgb};
use crate::devices::midi::testkit::{port, FakeMidi};
use crate::devices::source::HardwareSource;
use crate::devices::{DeviceInput, InputKind, Led, LedCell, SurfaceDevice, SurfaceFrame};
use crate::platform::Platform;

/// macOS names: the unit's input is "… Out" and its output "… In".
const IN_NAME: &str = "Launchpad X LPX MIDI Out";
const OUT_NAME: &str = "Launchpad X LPX MIDI In";
const IN_ID: &str = "1001";
const OUT_ID: &str = "1002";
const DEVICE_ID: &str = "launchpad-x:1";

const ENTER: [u8; 9] = [0xF0, 0x00, 0x20, 0x29, 0x02, 0x0C, 0x0E, 0x01, 0xF7];
const EXIT: [u8; 9] = [0xF0, 0x00, 0x20, 0x29, 0x02, 0x0C, 0x0E, 0x00, 0xF7];

fn fake() -> FakeMidi {
    let midi = FakeMidi::default();
    midi.add(port(IN_NAME, IN_ID), port(OUT_NAME, OUT_ID));
    midi
}

/// The device the manager would get for the one unit on these ports.
fn device(midi: &FakeMidi) -> Box<dyn SurfaceDevice> {
    let mut hardware = NovationHardware::new(Arc::new(midi.clone()), Platform::MacOs);
    let found = hardware.scan().units;
    assert_eq!(found.len(), 1);
    hardware.device(&found[0].id).unwrap()
}

/// LED indices in `capabilities().positions()` order, written out: the grid
/// from the top row (81..88) down (11..18), the top row 91..98, the side
/// column 89 down to 19, the logo 99.
fn clear_indices() -> Vec<u8> {
    let grid = (1..=8u8)
        .rev()
        .flat_map(|t| (1..=8u8).map(move |u| 10 * t + u));
    let side = (1..=8u8).rev().map(|t| 10 * t + 9);
    grid.chain(91..=98).chain(side).chain([99]).collect()
}

fn clear_message() -> Vec<u8> {
    let mut m = vec![0xF0, 0x00, 0x20, 0x29, 0x02, 0x0C, 0x03];
    for index in clear_indices() {
        m.extend([0x03, index, 0x00, 0x00, 0x00]);
    }
    m.push(0xF7);
    m
}

fn to_output(messages: &[Vec<u8>]) -> Vec<(String, Vec<u8>)> {
    messages
        .iter()
        .map(|m| (OUT_ID.to_string(), m.clone()))
        .collect()
}

fn frame(pos: ControlPos, color: Rgb) -> SurfaceFrame {
    SurfaceFrame {
        leds: vec![LedCell {
            pos,
            led: Led {
                color,
                animation: Animation::None,
            },
        }],
    }
}

fn connected(midi: &FakeMidi) -> (Box<dyn SurfaceDevice>, UnboundedReceiver<DeviceInput>) {
    let mut dev = device(midi);
    let (tx, rx) = unbounded_channel();
    dev.connect(tx).unwrap();
    (dev, rx)
}

#[test]
fn the_clear_message_is_one_413_byte_message_in_position_order() {
    let clear = clear_message();
    assert_eq!(clear.len(), 413);
    assert_eq!(&clear[7..12], [0x03, 81, 0, 0, 0]);
    assert_eq!(&clear[clear.len() - 6..], [0x03, 99, 0, 0, 0, 0xF7]);
}

#[test]
fn connect_enters_programmer_mode_then_clears_every_led() {
    let midi = fake();
    let (dev, _rx) = connected(&midi);
    assert!(dev.is_connected());
    assert_eq!(dev.identity().id, DEVICE_ID);
    assert_eq!(
        midi.take_sent(),
        to_output(&[ENTER.to_vec(), clear_message()])
    );
    assert_eq!(midi.open_links(), 1);
}

#[test]
fn a_frame_sends_only_what_changed() {
    let midi = fake();
    let (mut dev, _rx) = connected(&midi);
    midi.take_sent();
    let f = frame(ControlPos::grid(0, 0), Rgb::new(255, 0, 128));
    dev.render(&f).unwrap();
    assert_eq!(
        midi.take_sent(),
        to_output(&[vec![
            0xF0, 0x00, 0x20, 0x29, 0x02, 0x0C, 0x03, 0x03, 0x51, 0x7F, 0x00, 0x40, 0xF7
        ]])
    );
    dev.render(&f).unwrap();
    assert_eq!(midi.take_sent(), [], "the same frame again sends nothing");
}

#[test]
fn input_decodes_pads_and_ignores_sysex() {
    let midi = fake();
    let (_dev, mut rx) = connected(&midi);
    midi.inject(IN_ID, &[0x90, 0x0B, 0x64]);
    assert_eq!(
        rx.try_recv().unwrap(),
        DeviceInput {
            device_id: DEVICE_ID.into(),
            pos: ControlPos::grid(0, 7),
            kind: InputKind::Pressed { velocity: 100 },
        }
    );
    midi.inject(IN_ID, &ENTER);
    assert!(
        rx.try_recv().is_err(),
        "SysEx reaches the adapter and is ignored"
    );
}

#[test]
fn disconnect_clears_leaves_programmer_mode_and_closes_the_link() {
    let midi = fake();
    let (mut dev, mut rx) = connected(&midi);
    midi.take_sent();
    dev.disconnect().unwrap();
    assert_eq!(
        midi.take_sent(),
        to_output(&[clear_message(), EXIT.to_vec()])
    );
    assert_eq!(midi.open_links(), 0);
    assert!(!dev.is_connected());
    midi.inject(IN_ID, &[0x90, 0x0B, 0x64]);
    assert!(rx.try_recv().is_err(), "no input once closed");
}

#[test]
fn a_refused_open_sends_nothing_and_leaves_the_unit_disconnected() {
    let midi = fake();
    let mut dev = device(&midi);
    midi.refuse_opens(true);
    let (tx, _rx) = unbounded_channel();
    assert!(dev.connect(tx).is_err());
    assert_eq!(midi.take_sent(), []);
    assert!(!dev.is_connected());
    assert_eq!(midi.open_links(), 0);
}

#[test]
fn a_missing_input_leaves_no_half_open_adapter() {
    // D-X11: the output used to be opened and 0E 01 sent before the input.
    let midi = fake();
    let mut dev = device(&midi);
    midi.remove(IN_ID);
    let (tx, _rx) = unbounded_channel();
    assert!(dev.connect(tx).is_err());
    assert_eq!(midi.take_sent(), [], "nothing sent without an input");
    assert!(!dev.is_connected());
    assert_eq!(midi.open_links(), 0);
}
