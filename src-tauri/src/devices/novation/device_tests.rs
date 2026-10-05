//! `Mk3Device` over `FakeMidi`, most of it with a `SimLaunchpad` behind the
//! ports.
//!
//! First the Launchpad X golden transcript: every byte the device sends for
//! the X and every input it decodes. Only the commits that fix a defect of
//! LAUNCHPAD_MINI_MK3_PLAN.md section 2.3 may change it, each with a test of
//! the fix. Then the X against a simulated unit, and the same device driving
//! the Mini MK3's profile.

use std::sync::Arc;
use std::time::Duration;

use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver};

use super::device::Mk3Device;
use super::launchpad_mini_mk3;
use super::ports;
use super::protocol::to_7bit;
use super::session::REPLY_TIMEOUT;
use super::testkit::{hex, FakeClock, SimLaunchpad, SimLed};
use super::NovationHardware;
use crate::core::model::{Animation, ControlPos, Rgb};
use crate::devices::manager::{DeviceChange, DeviceManager, DeviceStatus};
use crate::devices::midi::testkit::{port, FakeMidi};
use crate::devices::midi::MidiBackend;
use crate::devices::source::HardwareSource;
use crate::devices::{
    DeviceError, DeviceInput, InputKind, Led, LedCell, ModeState, SurfaceDevice, SurfaceFrame,
};
use crate::platform::Platform;

/// macOS names: the unit's input is "… Out" and its output "… In".
const IN_NAME: &str = "Launchpad X LPX MIDI Out";
const OUT_NAME: &str = "Launchpad X LPX MIDI In";
const IN_ID: &str = "1001";
const OUT_ID: &str = "1002";
const DEVICE_ID: &str = "launchpad-x:1";

const INQUIRY: [u8; 6] = [0xF0, 0x7E, 0x7F, 0x06, 0x01, 0xF7];
const READ_LAYOUT: [u8; 8] = [0xF0, 0x00, 0x20, 0x29, 0x02, 0x0C, 0x00, 0xF7];
const ENTER: [u8; 9] = [0xF0, 0x00, 0x20, 0x29, 0x02, 0x0C, 0x0E, 0x01, 0xF7];
const READ_MODE: [u8; 8] = [0xF0, 0x00, 0x20, 0x29, 0x02, 0x0C, 0x0E, 0xF7];
const EXIT: [u8; 9] = [0xF0, 0x00, 0x20, 0x29, 0x02, 0x0C, 0x0E, 0x00, 0xF7];

/// The X's ports with this unit behind them.
fn fake_with(sim: SimLaunchpad) -> FakeMidi {
    let midi = FakeMidi::default();
    midi.add(port(IN_NAME, IN_ID), port(OUT_NAME, OUT_ID));
    midi.bind(OUT_ID, IN_ID, sim);
    midi
}

/// The X's ports with a Launchpad X just powered on behind them.
fn fake() -> FakeMidi {
    fake_with(SimLaunchpad::x())
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

/// Connected and taken: the unit answered, one render entered Programmer
/// mode and the next took the unit. What was sent until then is dropped.
fn acquired(midi: &FakeMidi) -> (Box<dyn SurfaceDevice>, UnboundedReceiver<DeviceInput>) {
    let (mut dev, rx) = connected(midi);
    for _ in 0..2 {
        dev.render(&SurfaceFrame::default()).unwrap();
    }
    midi.take_sent();
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
fn connect_asks_who_the_unit_is_and_the_next_render_takes_it() {
    // D-X5: Programmer mode used to be sent blind, before anything was known.
    let midi = fake();
    let (mut dev, _rx) = connected(&midi);
    assert!(dev.is_connected());
    assert_eq!(dev.identity().id, DEVICE_ID);
    // D-X7: the layout the unit shows is read, to give it back on release.
    assert_eq!(
        midi.take_sent(),
        to_output(&[INQUIRY.to_vec(), READ_LAYOUT.to_vec()])
    );
    assert_eq!(midi.open_links(), 1);
    dev.render(&SurfaceFrame::default()).unwrap();
    // D-X6: Programmer mode used to be assumed, never read back.
    assert_eq!(
        midi.take_sent(),
        to_output(&[ENTER.to_vec(), READ_MODE.to_vec()])
    );
    dev.render(&SurfaceFrame::default()).unwrap();
    assert_eq!(midi.take_sent(), to_output(&[clear_message()]));
    let details = dev.details();
    assert_eq!(details.firmware.as_deref(), Some("0238"));
    assert_eq!((details.mode, details.notice), (ModeState::Confirmed, None));
}

#[test]
fn a_frame_sends_only_what_changed() {
    let midi = fake();
    let (mut dev, _rx) = acquired(&midi);
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
    let (_dev, mut rx) = acquired(&midi);
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
fn input_is_read_as_whole_messages_whatever_the_callbacks_hold() {
    let midi = fake();
    let (_dev, mut rx) = acquired(&midi);
    let press = |pos, velocity| (pos, InputKind::Pressed { velocity });
    // Two messages and a stray data pair (running status) in one callback.
    midi.inject(IN_ID, &[0x90, 0x0B, 0x64, 0xB0, 0x5B, 0x7F, 0x0B, 0x00]);
    // One message over two callbacks, with a clock byte inside.
    midi.inject(IN_ID, &[0x90, 0x51]);
    midi.inject(IN_ID, &[0xF8, 0x20]);
    let read: Vec<_> = std::iter::from_fn(|| rx.try_recv().ok())
        .map(|i| (i.pos, i.kind))
        .collect();
    assert_eq!(
        read,
        [
            press(ControlPos::grid(0, 7), 100),
            press(ControlPos::Top { index: 0 }, 127),
            press(ControlPos::grid(0, 0), 32),
        ]
    );
}

#[test]
fn disconnect_clears_leaves_programmer_mode_and_closes_the_link() {
    let midi = fake();
    let (mut dev, mut rx) = acquired(&midi);
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

// ---- the X against a simulated unit -----------------------------------------

/// An X taken by LaunchDeck, with a simulated unit behind its ports.
fn on_sim() -> (
    FakeMidi,
    SimLaunchpad,
    Box<dyn SurfaceDevice>,
    UnboundedReceiver<DeviceInput>,
) {
    let sim = SimLaunchpad::x();
    let midi = fake_with(sim.clone());
    let (dev, rx) = acquired(&midi);
    (midi, sim, dev, rx)
}

/// A different colour on every position the device has.
fn rainbow(dev: &dyn SurfaceDevice) -> SurfaceFrame {
    let leds = dev
        .capabilities()
        .positions()
        .into_iter()
        .zip(0u8..)
        .map(|(pos, i)| LedCell {
            pos,
            led: Led {
                color: Rgb::new(i * 3, 255 - i * 3, i),
                animation: Animation::None,
            },
        })
        .collect();
    SurfaceFrame { leds }
}

fn shown(color: Rgb) -> SimLed {
    SimLed::Rgb(to_7bit(color.r), to_7bit(color.g), to_7bit(color.b))
}

#[test]
fn the_unit_shows_each_frame_and_is_given_back_dark_in_live_mode() {
    let (_midi, sim, mut dev, _rx) = on_sim();
    assert!(sim.in_programmer_mode());
    let positions = dev.capabilities().positions();
    for &pos in &positions {
        assert_eq!(sim.led_at(pos), Some(SimLed::Rgb(0, 0, 0)), "{pos:?}");
    }

    let mut frame = rainbow(dev.as_ref());
    dev.render(&frame).unwrap();
    for cell in &frame.leds {
        assert_eq!(
            sim.led_at(cell.pos),
            Some(shown(cell.led.color)),
            "{:?}",
            cell.pos
        );
    }
    frame.leds[5].led.color = Rgb::new(255, 255, 255);
    dev.render(&frame).unwrap();
    for cell in &frame.leds {
        assert_eq!(
            sim.led_at(cell.pos),
            Some(shown(cell.led.color)),
            "{:?}",
            cell.pos
        );
    }

    dev.disconnect().unwrap();
    assert_eq!(sim.layout(), 0x01, "Live mode, Note layout");
    for &pos in &positions {
        assert_eq!(sim.led_at(pos), Some(SimLed::Rgb(0, 0, 0)), "{pos:?}");
    }
}

#[test]
fn every_button_and_pad_of_the_unit_is_read_in_order() {
    let (midi, sim, dev, mut rx) = on_sim();
    let buttons: Vec<ControlPos> = dev
        .capabilities()
        .positions()
        .into_iter()
        .filter(|p| *p != ControlPos::Logo)
        .collect();
    assert_eq!(buttons.len(), 64 + 8 + 8);
    for &pos in &buttons {
        midi.inject(IN_ID, &sim.press(pos));
        midi.inject(IN_ID, &sim.release(pos));
    }
    for &pos in &buttons {
        let press = rx.try_recv().unwrap();
        assert_eq!(
            (press.pos, press.kind),
            (pos, InputKind::Pressed { velocity: 127 })
        );
        let release = rx.try_recv().unwrap();
        assert_eq!((release.pos, release.kind), (pos, InputKind::Released));
    }
    assert!(sim.press(ControlPos::Logo).is_empty());
    assert!(rx.try_recv().is_err());
}

#[test]
fn pressure_unknown_controls_and_notes_off_the_layout_are_not_input() {
    let (midi, _sim, _dev, mut rx) = on_sim();
    for bytes in [
        [0xA0, 0x0B, 0x40].as_slice(),
        &[0xD0, 0x40],
        &[0xB0, 0x63, 0x7F],
        &[0x90, 0x05, 0x7F],
    ] {
        midi.inject(IN_ID, bytes);
        assert!(rx.try_recv().is_err(), "{bytes:02X?}");
    }
}

#[test]
fn a_whole_frame_is_one_message() {
    let (midi, _sim, mut dev, _rx) = on_sim();
    dev.render(&rainbow(dev.as_ref())).unwrap();
    let sent = midi.take_sent();
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].1.len(), 413, "81 RGB colourspecs");
}

#[test]
fn replies_from_the_unit_do_not_disturb_drawing() {
    let (midi, sim, mut dev, mut rx) = on_sim();
    let f = frame(ControlPos::Logo, Rgb::new(0, 255, 0));
    dev.render(&f).unwrap();
    midi.take_sent();
    midi.inject(IN_ID, &hex("F0 00 20 29 02 0C 0E 01 F7"));
    midi.inject(
        IN_ID,
        &hex("F0 7E 00 06 02 00 20 29 03 01 00 00 00 02 03 08 F7"),
    );
    dev.render(&f).unwrap();
    assert_eq!(midi.take_sent(), [], "nothing changed");
    assert!(rx.try_recv().is_err(), "replies are not input");
    assert_eq!(sim.led_at(ControlPos::Logo), Some(SimLed::Rgb(0, 127, 0)));
}

// ---- the same device for the Mini MK3 ---------------------------------------

const MINI_IN: &str = "Launchpad Mini MK3 LPMiniMK3 MIDI Out";
const MINI_OUT: &str = "Launchpad Mini MK3 LPMiniMK3 MIDI In";
const MINI_IN_ID: &str = "2001";
const MINI_OUT_ID: &str = "2002";

/// A Mini MK3 device on the Mini's ports with this unit behind them, on the
/// test's clock, connected; the hardware source does not drive the Mini yet
/// (package M6), so the unit is found by the parser.
fn mini_with(
    sim: &SimLaunchpad,
    clock: &FakeClock,
) -> (FakeMidi, Mk3Device, UnboundedReceiver<DeviceInput>) {
    let midi = FakeMidi::default();
    midi.add(port(MINI_IN, MINI_IN_ID), port(MINI_OUT, MINI_OUT_ID));
    midi.bind(MINI_OUT_ID, MINI_IN_ID, sim.clone());
    let profiles = [&launchpad_mini_mk3::PROFILE];
    let mut units = ports::parse(&midi.ports(), Platform::MacOs, &profiles).units;
    assert_eq!(units.len(), 1);
    let mut dev = Mk3Device::new(units.remove(0), Arc::new(midi.clone())).with_clock(clock.clock());
    let (tx, rx) = unbounded_channel();
    dev.connect(tx).unwrap();
    (midi, dev, rx)
}

/// Every message sent since the last call, without the output id.
fn sent(midi: &FakeMidi) -> Vec<Vec<u8>> {
    midi.take_sent().into_iter().map(|(_, m)| m).collect()
}

/// The Mini's clear frame: the X's with the Mini's product byte.
fn mini_clear() -> Vec<u8> {
    let mut clear = clear_message();
    clear[5] = 0x0D;
    clear
}

const MINI_FRAME: &str = "F0 00 20 29 02 0D 03 03 0B 7F 00 40 F7";

/// What connecting sends a Mini MK3: who are you, which layout.
fn mini_probe() -> Vec<Vec<u8>> {
    vec![hex("F0 7E 7F 06 01 F7"), hex("F0 00 20 29 02 0D 00 F7")]
}

/// Connected, identified, confirmed and taken; what was sent is dropped.
fn mini_acquired(
    sim: &SimLaunchpad,
    clock: &FakeClock,
) -> (FakeMidi, Mk3Device, UnboundedReceiver<DeviceInput>) {
    let (midi, mut dev, rx) = mini_with(sim, clock);
    for _ in 0..2 {
        dev.render(&SurfaceFrame::default()).unwrap();
    }
    assert_eq!(dev.details().mode, ModeState::Confirmed);
    sent(&midi);
    (midi, dev, rx)
}

fn lower_left_pink() -> SurfaceFrame {
    frame(ControlPos::grid(0, 7), Rgb::new(255, 0, 128))
}

#[test]
fn the_mini_mk3_is_identified_before_it_is_taken() {
    let sim = SimLaunchpad::mini();
    let (midi, mut dev, _rx) = mini_with(&sim, &FakeClock::new());
    assert_eq!(dev.identity().id, "launchpad-mini-mk3:1");
    assert_eq!(sent(&midi), mini_probe());
    assert_eq!(dev.details().mode, ModeState::Acquiring);
    assert!(!sim.in_programmer_mode());

    dev.render(&lower_left_pink()).unwrap();
    assert_eq!(
        sent(&midi),
        [
            hex("F0 00 20 29 02 0D 0E 01 F7"),
            hex("F0 00 20 29 02 0D 0E F7")
        ]
    );
    assert!(sim.in_programmer_mode());
    assert_eq!(dev.details().mode, ModeState::Acquiring);
    dev.render(&lower_left_pink()).unwrap();
    assert_eq!(sent(&midi), [mini_clear(), hex(MINI_FRAME)]);
    let details = dev.details();
    assert_eq!(details.firmware.as_deref(), Some("0531"));
    assert_eq!((details.mode, details.notice), (ModeState::Confirmed, None));
}

#[test]
fn a_unit_in_its_bootloader_is_sent_nothing_more() {
    let sim = SimLaunchpad::mini().in_bootloader();
    let (midi, mut dev, _rx) = mini_with(&sim, &FakeClock::new());
    assert_eq!(sent(&midi), mini_probe());
    for _ in 0..2 {
        let error = dev.render(&lower_left_pink()).unwrap_err();
        assert!(matches!(error, DeviceError::Bootloader(_)), "{error:?}");
        assert_eq!(
            error.to_string(),
            "The Launchpad is in its bootloader. Unplug it and plug it in again."
        );
        assert_eq!(sent(&midi), Vec::<Vec<u8>>::new());
    }
    dev.disconnect().unwrap();
    assert_eq!(sent(&midi), Vec::<Vec<u8>>::new(), "nothing to give back");
    assert_eq!(midi.open_links(), 0);
}

#[test]
fn another_model_on_the_mini_mk3_ports_is_turned_down() {
    let sim = SimLaunchpad::mini().with_family([0x03, 0x01]);
    let (midi, mut dev, _rx) = mini_with(&sim, &FakeClock::new());
    sent(&midi);
    let error = dev.render(&lower_left_pink()).unwrap_err();
    assert!(matches!(error, DeviceError::OtherModel(_)), "{error:?}");
    assert_eq!(
        error.to_string(),
        "LaunchDeck could not confirm which Launchpad this is. Press Look again."
    );
    assert_eq!(sent(&midi), Vec::<Vec<u8>>::new());
    assert!(!sim.in_programmer_mode());
}

#[test]
fn a_launchpad_x_answering_with_the_manuals_misprinted_family_is_taken() {
    let sim = SimLaunchpad::x().with_family([0x13, 0x01]);
    let midi = fake_with(sim.clone());
    let (dev, _rx) = acquired(&midi);
    assert!(sim.in_programmer_mode());
    assert_eq!(dev.details().mode, ModeState::Confirmed);
}

#[test]
fn a_unit_that_does_not_answer_is_still_drawn_with_a_notice() {
    let sim = SimLaunchpad::mini().silent();
    let clock = FakeClock::new();
    let (midi, mut dev, mut rx) = mini_with(&sim, &clock);
    assert_eq!(sent(&midi), mini_probe());
    dev.render(&lower_left_pink()).unwrap();
    clock.advance(REPLY_TIMEOUT - Duration::from_millis(1));
    dev.render(&lower_left_pink()).unwrap();
    assert_eq!(sent(&midi), Vec::<Vec<u8>>::new(), "still waiting");

    clock.advance(Duration::from_millis(1));
    dev.render(&lower_left_pink()).unwrap();
    assert_eq!(
        sent(&midi),
        [
            hex("F0 00 20 29 02 0D 0E 01 F7"),
            hex("F0 00 20 29 02 0D 0E F7")
        ]
    );
    // An answer for another model's product byte confirms nothing.
    midi.inject(MINI_IN_ID, &hex("F0 00 20 29 02 0C 0E 01 F7"));
    clock.advance(REPLY_TIMEOUT - Duration::from_millis(1));
    dev.render(&lower_left_pink()).unwrap();
    assert_eq!(sent(&midi), Vec::<Vec<u8>>::new(), "still waiting");

    clock.advance(Duration::from_millis(1));
    dev.render(&lower_left_pink()).unwrap();
    assert_eq!(sent(&midi), [mini_clear(), hex(MINI_FRAME)]);
    let details = dev.details();
    assert_eq!(details.firmware, None);
    assert_eq!(details.mode, ModeState::Unconfirmed);
    assert_eq!(
        details.notice.as_deref(),
        Some("Launchpad Mini MK3 connected, but Programmer Mode could not be confirmed.")
    );
    midi.inject(MINI_IN_ID, &sim.press(ControlPos::grid(0, 7)));
    assert_eq!(rx.try_recv().unwrap().pos, ControlPos::grid(0, 7));
}

#[test]
fn input_before_the_unit_is_taken_is_not_a_press() {
    // D-X5: Keys, the Mini's Live layout, sends notes in the pads' range.
    let sim = SimLaunchpad::mini();
    let (midi, mut dev, mut rx) = mini_with(&sim, &FakeClock::new());
    midi.inject(MINI_IN_ID, &hex("90 30 7F"));
    assert!(rx.try_recv().is_err());
    dev.render(&SurfaceFrame::default()).unwrap();
    midi.inject(MINI_IN_ID, &hex("90 30 7F"));
    assert!(rx.try_recv().is_err(), "Programmer mode not confirmed yet");
    dev.render(&SurfaceFrame::default()).unwrap();
    midi.inject(MINI_IN_ID, &hex("90 30 7F"));
    assert_eq!(rx.try_recv().unwrap().pos, ControlPos::grid(7, 4));
}

#[test]
fn the_mini_mk3_profile_runs_on_the_same_device() {
    let sim = SimLaunchpad::mini();
    let (midi, mut dev, mut rx) = mini_acquired(&sim, &FakeClock::new());

    dev.render(&lower_left_pink()).unwrap();
    assert_eq!(
        midi.take_sent(),
        [(MINI_OUT_ID.to_string(), hex(MINI_FRAME))]
    );
    assert_eq!(
        sim.led_at(ControlPos::grid(0, 7)),
        Some(SimLed::Rgb(127, 0, 64))
    );

    midi.inject(MINI_IN_ID, &hex("90 0B 05"));
    assert_eq!(
        rx.try_recv().unwrap().kind,
        InputKind::Pressed { velocity: 127 },
        "the Mini has no velocity"
    );

    dev.disconnect().unwrap();
    assert_eq!(
        sent(&midi),
        [mini_clear(), hex("F0 00 20 29 02 0D 0E 00 F7")],
        "Live mode lands in Keys, the layout the unit showed"
    );
    assert_eq!(sim.layout(), 0x05, "Live mode, Keys");
    assert_eq!(
        sim.led_at(ControlPos::grid(0, 7)),
        Some(SimLed::Rgb(0, 0, 0))
    );
    assert_eq!(midi.open_links(), 0);
}

#[test]
fn release_gives_the_person_the_layout_the_unit_showed() {
    // D-X7: Live mode lands in Keys, whatever layout the person had.
    let sim = SimLaunchpad::mini().with_layout(0x04);
    let (midi, mut dev, _rx) = mini_acquired(&sim, &FakeClock::new());
    dev.render(&lower_left_pink()).unwrap();
    sent(&midi);
    dev.disconnect().unwrap();
    assert_eq!(
        sent(&midi),
        [
            mini_clear(),
            hex("F0 00 20 29 02 0D 0E 00 F7"),
            hex("F0 00 20 29 02 0D 00 04 F7"),
        ]
    );
    assert_eq!(sim.layout(), 0x04, "Custom mode 1, Drums");
}

// ---- refusals through the device manager --------------------------------------

#[test]
fn the_manager_reports_a_unit_in_its_bootloader_as_failed_with_the_reason() {
    let midi = fake_with(SimLaunchpad::x().in_bootloader());
    let (tx, _rx) = unbounded_channel();
    let hardware = NovationHardware::new(Arc::new(midi.clone()), Platform::MacOs);
    let mut m = DeviceManager::with_hardware(tx, Box::new(hardware));
    let rescan = m.rescan_hardware(false);
    assert_eq!(rescan.to_connect, [DEVICE_ID]);
    assert_eq!(
        m.connect(DEVICE_ID).unwrap(),
        DeviceChange::Connected(DEVICE_ID.into()),
        "connect waits for nothing"
    );
    assert_eq!(
        m.render(DEVICE_ID, &SurfaceFrame::default()),
        Some(DeviceChange::Disconnected(DEVICE_ID.into()))
    );
    let info = m.list().remove(0);
    assert_eq!(info.status, DeviceStatus::Failed);
    assert_eq!(
        info.problem.as_deref(),
        Some("The Launchpad is in its bootloader. Unplug it and plug it in again.")
    );
    assert_eq!(
        midi.take_sent(),
        to_output(&[INQUIRY.to_vec(), READ_LAYOUT.to_vec()])
    );
    assert_eq!(midi.open_links(), 0);
    assert!(
        m.rescan_hardware(false).to_connect.is_empty(),
        "only Look again tries it again"
    );
}
