//! `SimLaunchpad`: a Launchpad Mini MK3 or a Launchpad X as bytes, for the
//! device tests. Bind it to a `FakeMidi` output and it answers on the paired
//! input, as a unit would.
//!
//! Written from the Programmer's Reference Manuals (MINI-PRM and X-PRM, both
//! 2019-11-08) and the Mini's User Guide (MINI-UG v2.0), and independently of
//! `protocol.rs` and `layout.rs`: it reads every byte itself, so a test of the
//! device against it is not the codec agreeing with itself. Where a manual is
//! silent the model makes a choice, said where it is made. NOT HARDWARE
//! VERIFIED: nothing here was compared with a real unit.
//!
//! Modelled: the Universal Device Inquiry (application or bootloader), the
//! layouts and the Programmer/Live switch with their readbacks, LED SysEx
//! colourspecs of types 0-3 and channel 1-3 lighting in Programmer mode,
//! text scrolling, sleep, brightness and LED feedback with their readbacks,
//! the Programmer-mode bytes of a press and a release, a power cycle and a
//! DAW taking the unit over. Not modelled: DAW mode and the DAW interface,
//! Lighting Custom Modes, the musical layouts' own notes.
//!
//! `FakeClock` is the time a test gives a device, moved by hand.

use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use super::device::Clock;
use crate::core::model::ControlPos;
use crate::devices::midi::testkit::Responder;

/// `"F0 00 20 29"` as bytes, so that expectations read like the manuals.
pub(crate) fn hex(line: &str) -> Vec<u8> {
    line.split_whitespace()
        .map(|b| u8::from_str_radix(b, 16).unwrap())
        .collect()
}

/// A clock that stands still until the test moves it. Clones share it.
#[derive(Clone)]
pub(crate) struct FakeClock(Arc<Mutex<Instant>>);

impl FakeClock {
    pub fn new() -> Self {
        FakeClock(Arc::new(Mutex::new(Instant::now())))
    }

    pub fn advance(&self, by: Duration) {
        *self.0.lock().unwrap() += by;
    }

    /// The clock a device reads.
    pub fn clock(&self) -> Clock {
        let now = self.0.clone();
        Arc::new(move || *now.lock().unwrap())
    }
}

/// Novation's manufacturer id and the `02h` that starts every MK3 header
/// (MINI-PRM p6).
const NOVATION: [u8; 4] = [0x00, 0x20, 0x29, 0x02];
/// A layout byte (MINI-PRM p7).
const SESSION: u8 = 0x00;
const PROGRAMMER: u8 = 0x7F;
/// Command bytes (MINI-PRM p23).
const SELECT_LAYOUT: u8 = 0x00;
const LED_LIGHTING: u8 = 0x03;
const TEXT_SCROLLING: u8 = 0x07;
const BRIGHTNESS_LEVEL: u8 = 0x08;
const LED_SLEEP: u8 = 0x09;
const LED_FEEDBACK: u8 = 0x0A;
const PROGRAMMER_LIVE: u8 = 0x0E;
/// "Up to 81 <colourspec> entries" (MINI-PRM p14).
const MAX_COLOURSPECS: usize = 81;
/// LED indices run up to 99, the logo.
const LED_SLOTS: usize = 100;
const POWER_ON_BRIGHTNESS: u8 = 127;
const PRESSED: u8 = 0x7F;
const RELEASED: u8 = 0x00;

/// What one LED shows, in the units the device was given.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SimLed {
    /// A palette entry; 0 is off.
    Static(u8),
    /// Alternates between colour A and colour B (palette entries).
    Flash {
        b: u8,
        a: u8,
    },
    Pulse(u8),
    /// 7-bit channels.
    Rgb(u8, u8, u8),
}

const DARK: SimLed = SimLed::Static(0);

/// A text scroll as the device was given it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SimText {
    pub looping: bool,
    /// The speed byte: pads per second, `40h` and above negative.
    pub speed: u8,
    /// `00 <palette>` or `01 <r> <g> <b>`.
    pub colour: Vec<u8>,
    pub text: Vec<u8>,
}

/// The facts of one model the simulator needs.
#[derive(Debug, Clone, Copy)]
struct Model {
    product: u8,
    family: [u8; 2],
    firmware: [u8; 4],
    /// Where Programmer mode's Live switch lands outside DAW mode.
    live_layout: u8,
    /// The layouts SysEx may select outside DAW mode.
    layouts: &'static [u8],
}

/// MINI-PRM p6-7. The firmware digits are illustrative: the manual leaves
/// them open.
const MINI_MK3: Model = Model {
    product: 0x0D,
    family: [0x13, 0x01],
    firmware: [0, 5, 3, 1],
    live_layout: 0x05,
    layouts: &[0x04, 0x05, 0x06, PROGRAMMER],
};

/// X-PRM p6-8. The family and firmware are those of a captured reply
/// (community); the manual misprints the Mini's `13 01`.
const LAUNCHPAD_X: Model = Model {
    product: 0x0C,
    family: [0x03, 0x01],
    firmware: [0, 2, 3, 8],
    live_layout: 0x01,
    layouts: &[0x01, 0x04, 0x05, 0x06, 0x07, PROGRAMMER],
};

struct SimState {
    model: Model,
    /// Answers the inquiry as its bootloader and nothing else.
    bootloader: bool,
    /// Acts on what it receives but its answers never arrive, as through a
    /// driver that drops SysEx.
    silent: bool,
    /// Commands its firmware does not know: it neither obeys nor answers
    /// them, as an older firmware might.
    unknown: Vec<u8>,
    layout: u8,
    /// Internal and external LED feedback, 0 or 1.
    feedback: (u8, u8),
    awake: bool,
    brightness: u8,
    leds: [SimLed; LED_SLOTS],
    text: Option<SimText>,
}

/// A simulated unit. Clones share it: bind one to a `FakeMidi` output and
/// keep another to press pads and read what the unit shows.
#[derive(Clone)]
pub(crate) struct SimLaunchpad(Arc<Mutex<SimState>>);

impl SimLaunchpad {
    fn new(model: Model) -> Self {
        SimLaunchpad(Arc::new(Mutex::new(SimState::powered_on(model))))
    }

    /// A Launchpad Mini MK3 just powered on.
    pub fn mini() -> Self {
        Self::new(MINI_MK3)
    }

    /// A Launchpad X just powered on.
    pub fn x() -> Self {
        Self::new(LAUNCHPAD_X)
    }

    fn state(&self) -> MutexGuard<'_, SimState> {
        self.0.lock().unwrap()
    }

    /// Answers SysEx with this product byte instead of its model's.
    pub fn with_product(self, product: u8) -> Self {
        self.state().model.product = product;
        self
    }

    /// Answers the inquiry with this family.
    pub fn with_family(self, family: [u8; 2]) -> Self {
        self.state().model.family = family;
        self
    }

    /// Answers the inquiry with these four digits.
    pub fn with_firmware(self, digits: [u8; 4]) -> Self {
        self.state().model.firmware = digits;
        self
    }

    /// Showing this layout, as a person may have chosen it before
    /// LaunchDeck took the unit.
    pub fn with_layout(self, layout: u8) -> Self {
        self.state().layout = layout;
        self
    }

    /// LED feedback set in its Setup menu: internal and external, 0 or 1.
    pub fn with_feedback(self, internal: u8, external: u8) -> Self {
        self.state().feedback = (internal, external);
        self
    }

    /// Its LEDs put to sleep (MINI-PRM p21).
    pub fn sleeping(self) -> Self {
        self.state().awake = false;
        self
    }

    /// Started in its bootloader: answers the inquiry with the family's
    /// second byte `11h` and the firmware digits as the bootloader's
    /// version (MINI-PRM p6), and ignores everything else.
    pub fn in_bootloader(self) -> Self {
        self.state().bootloader = true;
        self
    }

    /// Its answers never arrive; it still acts on what it receives.
    pub fn silent(self) -> Self {
        self.state().silent = true;
        self
    }

    /// A firmware that does not know this command: it is neither obeyed
    /// nor answered.
    pub fn without_command(self, command: u8) -> Self {
        self.state().unknown.push(command);
        self
    }

    /// From now on its answers never arrive (`true`), or arrive again.
    pub fn set_silent(&self, silent: bool) {
        self.state().silent = silent;
    }

    /// The layout shown now (MINI-PRM p7); `7Fh` is Programmer mode.
    pub fn layout(&self) -> u8 {
        self.state().layout
    }

    pub fn in_programmer_mode(&self) -> bool {
        self.layout() == PROGRAMMER
    }

    /// Internal and external LED feedback, 0 or 1 each.
    pub fn feedback(&self) -> (u8, u8) {
        self.state().feedback
    }

    pub fn awake(&self) -> bool {
        self.state().awake
    }

    pub fn brightness(&self) -> u8 {
        self.state().brightness
    }

    pub fn text(&self) -> Option<SimText> {
        self.state().text.clone()
    }

    /// What the LED of this position shows; `None` for a position the unit
    /// does not have.
    pub fn led_at(&self, pos: ControlPos) -> Option<SimLed> {
        let index = lit_index(pos)?;
        Some(self.state().leds[usize::from(index)])
    }

    /// What a press of this control sends: in Programmer mode `90 <note> 7F`
    /// for a pad and `B0 <cc> 7F` for a button (MINI-PRM p10; the value 7F
    /// is not in a manual). A press also wakes a sleeping unit (MINI-UG
    /// p19); whether that press is sent too is not documented, and here it
    /// is. Nothing in another layout, nothing for the logo.
    pub fn press(&self, pos: ControlPos) -> Vec<u8> {
        let mut s = self.state();
        if s.bootloader {
            return Vec::new();
        }
        s.awake = true;
        s.event(pos, PRESSED)
    }

    /// What a release sends: Note On with velocity 0 for a pad (the device
    /// never sends 80h, MINI-PRM p6), the CC with value 0 for a button.
    pub fn release(&self, pos: ControlPos) -> Vec<u8> {
        let s = self.state();
        if s.bootloader {
            return Vec::new();
        }
        s.event(pos, RELEASED)
    }

    /// Unplugged and plugged in again: Live mode (MINI-UG p20), feedback
    /// on, awake, LEDs dark, no text. That brightness returns to full is
    /// this model's choice; whether a SysEx level survives is undocumented.
    pub fn power_cycle(&self) {
        let mut s = self.state();
        *s = SimState {
            bootloader: s.bootloader,
            silent: s.silent,
            unknown: s.unknown.clone(),
            ..SimState::powered_on(s.model)
        };
    }

    /// Another program takes the unit as Ableton Live does: DAW mode, then
    /// the Session layout, out of Programmer mode.
    pub fn take_over(&self) {
        self.state().layout = SESSION;
    }
}

impl Responder for SimLaunchpad {
    fn respond(&mut self, message: &[u8]) -> Vec<Vec<u8>> {
        let mut s = self.state();
        let answers = s.receive(message);
        if s.silent {
            Vec::new()
        } else {
            answers
        }
    }
}

impl SimState {
    fn powered_on(model: Model) -> Self {
        SimState {
            model,
            bootloader: false,
            silent: false,
            unknown: Vec::new(),
            layout: model.live_layout,
            feedback: (1, 1),
            awake: true,
            brightness: POWER_ON_BRIGHTNESS,
            leds: [DARK; LED_SLOTS],
            text: None,
        }
    }

    fn receive(&mut self, message: &[u8]) -> Vec<Vec<u8>> {
        match message {
            [0xF0, 0x7E, _device, 0x06, 0x01, 0xF7] => vec![self.identity()],
            _ if self.bootloader => Vec::new(),
            [0xF0, inner @ .., 0xF7] => self.sysex(inner),
            _ => {
                self.channel_lighting(message);
                Vec::new()
            }
        }
    }

    /// `F0 7E 00 06 02 00 20 29 <family> 00 00 <four digits> F7`.
    fn identity(&self) -> Vec<u8> {
        let [first, second] = self.model.family;
        let family = if self.bootloader {
            [first, 0x11]
        } else {
            [first, second]
        };
        let mut m = vec![0xF0, 0x7E, 0x00, 0x06, 0x02, 0x00, 0x20, 0x29];
        m.extend(family);
        m.extend([0x00, 0x00]);
        m.extend(self.model.firmware);
        m.push(0xF7);
        m
    }

    /// `F0 00 20 29 02 <product> <command> <data> F7` (MINI-PRM p6-7).
    fn readback(&self, command: u8, data: &[u8]) -> Vec<u8> {
        let mut m = vec![0xF0];
        m.extend(NOVATION);
        m.extend([self.model.product, command]);
        m.extend(data);
        m.push(0xF7);
        m
    }

    /// A SysEx body: only its own header is obeyed, and a status byte
    /// inside makes the message void.
    fn sysex(&mut self, inner: &[u8]) -> Vec<Vec<u8>> {
        if inner.iter().any(|&b| b > 0x7F) {
            return Vec::new();
        }
        match inner {
            [0x00, 0x20, 0x29, 0x02, product, command, data @ ..]
                if *product == self.model.product && !self.unknown.contains(command) =>
            {
                self.command(*command, data)
            }
            _ => Vec::new(),
        }
    }

    fn command(&mut self, command: u8, data: &[u8]) -> Vec<Vec<u8>> {
        match (command, data) {
            (SELECT_LAYOUT, []) => return vec![self.readback(command, &[self.layout])],
            (SELECT_LAYOUT, [layout]) if self.model.layouts.contains(layout) => {
                self.layout = *layout;
            }
            (PROGRAMMER_LIVE, []) => {
                let on = u8::from(self.layout == PROGRAMMER);
                return vec![self.readback(command, &[on])];
            }
            (PROGRAMMER_LIVE, [0]) => self.layout = self.model.live_layout,
            (PROGRAMMER_LIVE, [1]) => self.layout = PROGRAMMER,
            (LED_LIGHTING, specs) if self.layout == PROGRAMMER => self.light(specs),
            (TEXT_SCROLLING, data) => self.scroll(data),
            (BRIGHTNESS_LEVEL, []) => return vec![self.readback(command, &[self.brightness])],
            (BRIGHTNESS_LEVEL, [level]) => self.brightness = *level,
            (LED_SLEEP, []) => return vec![self.readback(command, &[u8::from(self.awake)])],
            (LED_SLEEP, [mode @ (0 | 1)]) => self.awake = *mode == 1,
            (LED_FEEDBACK, []) => {
                let (internal, external) = self.feedback;
                return vec![self.readback(command, &[internal, external])];
            }
            (LED_FEEDBACK, [internal @ (0 | 1), external @ (0 | 1)]) => {
                self.feedback = (*internal, *external);
            }
            _ => {}
        }
        Vec::new()
    }

    /// Colourspecs `<type> <index> <data>` (MINI-PRM p14); a malformed one
    /// ends the message.
    fn light(&mut self, mut specs: &[u8]) {
        for _ in 0..MAX_COLOURSPECS {
            let [kind, index, rest @ ..] = specs else {
                return;
            };
            let (led, used) = match (*kind, rest) {
                (0, [palette, ..]) => (SimLed::Static(*palette), 1),
                (1, [b, a, ..]) => (SimLed::Flash { b: *b, a: *a }, 2),
                (2, [palette, ..]) => (SimLed::Pulse(*palette), 1),
                (3, [r, g, b, ..]) => (SimLed::Rgb(*r, *g, *b), 3),
                _ => return,
            };
            self.set_led(*index, led);
            specs = &rest[used..];
        }
    }

    /// Notes and CCs on channels 1-3 (MINI-PRM p12-13): static, flashing,
    /// pulsing palette colours, in Programmer mode while external feedback
    /// is on (MINI-UG p19). A flash's colour A is the static or pulsing
    /// colour already set; after an RGB colour this model takes palette 0.
    fn channel_lighting(&mut self, bytes: &[u8]) {
        if self.layout != PROGRAMMER || self.feedback.1 == 0 {
            return;
        }
        for message in bytes.chunks_exact(3) {
            let [status, index, value] = [message[0], message[1], message[2]];
            let led = match status {
                0x90 | 0xB0 => SimLed::Static(value),
                0x80 => DARK,
                0x91 | 0xB1 => SimLed::Flash {
                    b: value,
                    a: self.colour_a(index),
                },
                0x92 | 0xB2 => SimLed::Pulse(value),
                _ => continue,
            };
            self.set_led(index, led);
        }
    }

    fn colour_a(&self, index: u8) -> u8 {
        match self.leds.get(usize::from(index)) {
            Some(SimLed::Static(p) | SimLed::Pulse(p)) => *p,
            Some(SimLed::Flash { a, .. }) => *a,
            _ => 0,
        }
    }

    fn set_led(&mut self, index: u8, led: SimLed) {
        if is_led(index) {
            self.leds[usize::from(index)] = led;
        }
    }

    /// `[<loop> [<speed> [<colourspec> [<text>]]]]` (MINI-PRM p20): empty
    /// stops the scroll; without text it changes the current one.
    fn scroll(&mut self, data: &[u8]) {
        let [looping, rest @ ..] = data else {
            self.text = None;
            return;
        };
        let (speed, rest) = match rest {
            [speed, rest @ ..] => (Some(*speed), rest),
            [] => (None, rest),
        };
        let (colour, text) = match rest {
            [0, palette, text @ ..] => (Some(vec![0, *palette]), text),
            [1, r, g, b, text @ ..] => (Some(vec![1, *r, *g, *b]), text),
            [] => (None, rest),
            _ => return,
        };
        let looping = *looping == 1;
        match (speed, colour, text) {
            (Some(speed), Some(colour), text) if !text.is_empty() => {
                self.text = Some(SimText {
                    looping,
                    speed,
                    colour,
                    text: text.to_vec(),
                });
            }
            (speed, colour, _) => {
                if let Some(current) = self.text.as_mut() {
                    current.looping = looping;
                    current.speed = speed.unwrap_or(current.speed);
                    current.colour = colour.unwrap_or_else(|| current.colour.clone());
                }
            }
        }
    }

    fn event(&self, pos: ControlPos, value: u8) -> Vec<u8> {
        if self.layout != PROGRAMMER {
            return Vec::new();
        }
        match programmer_control(pos) {
            Some(Control::Note(note)) => vec![0x90, note, value],
            Some(Control::Cc(cc)) => vec![0xB0, cc, value],
            None => Vec::new(),
        }
    }
}

/// What a control sends in Programmer mode (MINI-PRM p10).
enum Control {
    Note(u8),
    Cc(u8),
}

/// The Programmer layout (MINI-PRM p10 and the examples of p13, MINI-UG
/// p23): pad notes `10 × r + c` with row `r` 1 at the bottom and column `c`
/// 1 at the left; top buttons CC 91-98 left to right; right buttons CC 89
/// down to 19. `ControlPos` counts rows from the top, from 0.
fn programmer_control(pos: ControlPos) -> Option<Control> {
    match pos {
        ControlPos::Grid { col, row } if col < 8 && row < 8 => {
            Some(Control::Note(10 * (8 - row) + (col + 1)))
        }
        ControlPos::Top { index } if index < 8 => Some(Control::Cc(91 + index)),
        ControlPos::Side { index } if index < 8 => Some(Control::Cc(10 * (8 - index) + 9)),
        _ => None,
    }
}

/// The LED of a position: its Programmer note or CC, and 99 for the logo.
fn lit_index(pos: ControlPos) -> Option<u8> {
    match (pos, programmer_control(pos)) {
        (ControlPos::Logo, _) => Some(99),
        (_, Some(Control::Note(n) | Control::Cc(n))) => Some(n),
        (_, None) => None,
    }
}

/// 81 LEDs: tens and units both 1-9.
fn is_led(index: u8) -> bool {
    (1..=9).contains(&(index / 10)) && (1..=9).contains(&(index % 10))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::devices::midi::testkit::{port, FakeMidi};
    use crate::devices::midi::MidiBackend;

    fn ask(sim: &SimLaunchpad, line: &str) -> Vec<Vec<u8>> {
        sim.clone().respond(&hex(line))
    }

    fn programmer(sim: &SimLaunchpad, product: &str) {
        ask(sim, &format!("F0 00 20 29 02 {product} 0E 01 F7"));
    }

    #[test]
    fn answers_the_inquiry_as_its_application_p6() {
        assert_eq!(
            ask(&SimLaunchpad::mini(), "F0 7E 7F 06 01 F7"),
            [hex("F0 7E 00 06 02 00 20 29 13 01 00 00 00 05 03 01 F7")]
        );
        assert_eq!(
            ask(&SimLaunchpad::x(), "F0 7E 7F 06 01 F7"),
            [hex("F0 7E 00 06 02 00 20 29 03 01 00 00 00 02 03 08 F7")]
        );
        let other = SimLaunchpad::x()
            .with_family([0x13, 0x01])
            .with_firmware([1, 2, 3, 4]);
        assert_eq!(
            ask(&other, "F0 7E 7F 06 01 F7"),
            [hex("F0 7E 00 06 02 00 20 29 13 01 00 00 01 02 03 04 F7")]
        );
    }

    #[test]
    fn a_bootloader_answers_only_the_inquiry_p6() {
        let sim = SimLaunchpad::mini()
            .with_firmware([0, 1, 0, 2])
            .in_bootloader();
        assert_eq!(
            ask(&sim, "F0 7E 7F 06 01 F7"),
            [hex("F0 7E 00 06 02 00 20 29 13 11 00 00 00 01 00 02 F7")]
        );
        programmer(&sim, "0D");
        assert!(!sim.in_programmer_mode());
        assert!(ask(&sim, "F0 00 20 29 02 0D 0E F7").is_empty());
        assert!(sim.press(ControlPos::grid(0, 0)).is_empty());
    }

    #[test]
    fn a_firmware_without_a_command_neither_obeys_nor_answers_it() {
        let sim = SimLaunchpad::mini().without_command(0x08);
        assert!(ask(&sim, "F0 00 20 29 02 0D 08 F7").is_empty());
        ask(&sim, "F0 00 20 29 02 0D 08 20 F7");
        assert_eq!(sim.brightness(), 0x7F);
        assert_eq!(
            ask(&sim, "F0 00 20 29 02 0D 09 F7"),
            [hex("F0 00 20 29 02 0D 09 01 F7")]
        );
    }

    #[test]
    fn a_silent_unit_acts_but_never_answers() {
        let sim = SimLaunchpad::mini().silent();
        assert!(ask(&sim, "F0 7E 7F 06 01 F7").is_empty());
        programmer(&sim, "0D");
        assert!(sim.in_programmer_mode());
        assert!(ask(&sim, "F0 00 20 29 02 0D 0E F7").is_empty());
    }

    #[test]
    fn programmer_and_live_with_readbacks_p7_p8() {
        let mini = SimLaunchpad::mini();
        assert_eq!(mini.layout(), 0x05, "powers on in Live mode, Keys");
        assert_eq!(
            ask(&mini, "F0 00 20 29 02 0D 00 F7"),
            [hex("F0 00 20 29 02 0D 00 05 F7")]
        );
        assert_eq!(
            ask(&mini, "F0 00 20 29 02 0D 0E F7"),
            [hex("F0 00 20 29 02 0D 0E 00 F7")]
        );
        programmer(&mini, "0D");
        assert_eq!(
            ask(&mini, "F0 00 20 29 02 0D 0E F7"),
            [hex("F0 00 20 29 02 0D 0E 01 F7")]
        );
        assert_eq!(
            ask(&mini, "F0 00 20 29 02 0D 00 F7"),
            [hex("F0 00 20 29 02 0D 00 7F F7")]
        );
        ask(&mini, "F0 00 20 29 02 0D 0E 00 F7");
        assert_eq!(mini.layout(), 0x05, "Live mode lands in Keys");

        let x = SimLaunchpad::x();
        assert_eq!(x.layout(), 0x01, "the X powers on in Note mode");
        programmer(&x, "0C");
        assert!(x.in_programmer_mode());
        ask(&x, "F0 00 20 29 02 0C 0E 00 F7");
        assert_eq!(x.layout(), 0x01, "Live mode lands in Note mode");
    }

    #[test]
    fn layouts_are_selected_as_the_manual_allows_p7() {
        let mini = SimLaunchpad::mini();
        ask(&mini, "F0 00 20 29 02 0D 00 04 F7");
        assert_eq!(mini.layout(), 0x04);
        ask(&mini, "F0 00 20 29 02 0D 00 00 F7");
        assert_eq!(mini.layout(), 0x04, "Session needs DAW mode");
        ask(&mini, "F0 00 20 29 02 0D 00 7F F7");
        assert!(mini.in_programmer_mode());
    }

    #[test]
    fn another_products_messages_are_ignored() {
        let mini = SimLaunchpad::mini();
        programmer(&mini, "0C");
        assert!(!mini.in_programmer_mode());
        assert!(ask(&mini, "F0 00 20 29 02 0C 0E F7").is_empty());
        let answers_as_x = SimLaunchpad::mini().with_product(0x0C);
        assert_eq!(
            ask(&answers_as_x, "F0 00 20 29 02 0C 0E F7"),
            [hex("F0 00 20 29 02 0C 0E 00 F7")]
        );
    }

    #[test]
    fn the_lighting_example_lights_three_pads_p14() {
        let sim = SimLaunchpad::mini();
        let example = "F0 00 20 29 02 0D 03 00 0B 0D 01 0C 15 17 02 0D 25 F7";
        ask(&sim, example);
        assert_eq!(
            sim.led_at(ControlPos::grid(0, 7)),
            Some(DARK),
            "not in Programmer mode"
        );
        programmer(&sim, "0D");
        ask(&sim, example);
        assert_eq!(sim.led_at(ControlPos::grid(0, 7)), Some(SimLed::Static(13)));
        assert_eq!(
            sim.led_at(ControlPos::grid(1, 7)),
            Some(SimLed::Flash { b: 21, a: 23 })
        );
        assert_eq!(sim.led_at(ControlPos::grid(2, 7)), Some(SimLed::Pulse(37)));
        ask(&sim, "F0 00 20 29 02 0D 03 03 63 7F 00 40 F7");
        assert_eq!(sim.led_at(ControlPos::Logo), Some(SimLed::Rgb(127, 0, 64)));
    }

    #[test]
    fn channel_lighting_follows_external_feedback_p12_p13() {
        let sim = SimLaunchpad::mini();
        programmer(&sim, "0D");
        for line in ["90 0B 05", "91 51 13", "92 12 2D"] {
            ask(&sim, line);
        }
        assert_eq!(sim.led_at(ControlPos::grid(0, 7)), Some(SimLed::Static(5)));
        assert_eq!(
            sim.led_at(ControlPos::grid(0, 0)),
            Some(SimLed::Flash { b: 0x13, a: 0 })
        );
        assert_eq!(
            sim.led_at(ControlPos::grid(7, 7)),
            Some(SimLed::Pulse(0x2D))
        );
        ask(&sim, "80 12 00");
        assert_eq!(sim.led_at(ControlPos::grid(7, 7)), Some(DARK));

        ask(&sim, "F0 00 20 29 02 0D 0A 01 00 F7");
        assert_eq!(sim.feedback(), (1, 0));
        ask(&sim, "90 0B 2D");
        assert_eq!(
            sim.led_at(ControlPos::grid(0, 7)),
            Some(SimLed::Static(5)),
            "notes are ignored with external feedback off"
        );
    }

    #[test]
    fn text_starts_changes_and_stops_p20() {
        let sim = SimLaunchpad::mini();
        ask(
            &sim,
            "F0 00 20 29 02 0D 07 01 07 00 25 48 65 6C 6C 6F 20 57 6F 72 6C 64 F7",
        );
        let hello = SimText {
            looping: true,
            speed: 7,
            colour: vec![0, 0x25],
            text: b"Hello World".to_vec(),
        };
        assert_eq!(sim.text(), Some(hello.clone()));
        ask(&sim, "F0 00 20 29 02 0D 07 00 79 F7");
        assert_eq!(
            sim.text(),
            Some(SimText {
                looping: false,
                speed: 0x79,
                ..hello
            })
        );
        ask(&sim, "F0 00 20 29 02 0D 07 F7");
        assert_eq!(sim.text(), None);
    }

    #[test]
    fn sleep_brightness_and_feedback_read_back_p21_p22() {
        let sim = SimLaunchpad::mini();
        assert_eq!(
            ask(&sim, "F0 00 20 29 02 0D 09 F7"),
            [hex("F0 00 20 29 02 0D 09 01 F7")]
        );
        ask(&sim, "F0 00 20 29 02 0D 09 00 F7");
        assert!(!sim.awake());
        assert_eq!(
            ask(&sim, "F0 00 20 29 02 0D 09 F7"),
            [hex("F0 00 20 29 02 0D 09 00 F7")]
        );
        assert_eq!(
            ask(&sim, "F0 00 20 29 02 0D 08 F7"),
            [hex("F0 00 20 29 02 0D 08 7F F7")]
        );
        ask(&sim, "F0 00 20 29 02 0D 08 20 F7");
        assert_eq!(sim.brightness(), 0x20);
        assert_eq!(
            ask(&sim, "F0 00 20 29 02 0D 0A F7"),
            [hex("F0 00 20 29 02 0D 0A 01 01 F7")]
        );
        sim.press(ControlPos::grid(0, 0));
        assert!(sim.awake(), "a press wakes it (MINI-UG p19)");
    }

    #[test]
    fn presses_and_releases_send_programmer_bytes_p10() {
        let sim = SimLaunchpad::mini();
        assert!(sim.press(ControlPos::grid(0, 7)).is_empty(), "Live mode");
        programmer(&sim, "0D");
        assert_eq!(sim.press(ControlPos::grid(0, 7)), hex("90 0B 7F"));
        assert_eq!(sim.release(ControlPos::grid(0, 7)), hex("90 0B 00"));
        assert_eq!(sim.press(ControlPos::grid(0, 0)), hex("90 51 7F"));
        assert_eq!(sim.press(ControlPos::grid(7, 7)), hex("90 12 7F"));
        assert_eq!(sim.press(ControlPos::Top { index: 0 }), hex("B0 5B 7F"));
        assert_eq!(sim.release(ControlPos::Side { index: 0 }), hex("B0 59 00"));
        assert_eq!(sim.press(ControlPos::Side { index: 7 }), hex("B0 13 7F"));
        assert!(
            sim.press(ControlPos::Logo).is_empty(),
            "the logo is no button"
        );
    }

    #[test]
    fn a_power_cycle_and_a_takeover_leave_programmer_mode() {
        let sim = SimLaunchpad::mini();
        programmer(&sim, "0D");
        ask(&sim, "F0 00 20 29 02 0D 03 03 0B 7F 7F 7F F7");
        ask(&sim, "F0 00 20 29 02 0D 0A 00 00 F7");
        sim.take_over();
        assert_eq!(sim.layout(), 0x00, "Session, as a DAW selects it");
        sim.power_cycle();
        assert_eq!(sim.layout(), 0x05);
        assert_eq!(sim.feedback(), (1, 1));
        assert_eq!(sim.led_at(ControlPos::grid(0, 7)), Some(DARK));
    }

    #[test]
    fn every_position_has_its_own_led() {
        let positions = (0..8)
            .flat_map(|row| (0..8).map(move |col| ControlPos::grid(col, row)))
            .chain((0..8).map(|index| ControlPos::Top { index }))
            .chain((0..8).map(|index| ControlPos::Side { index }))
            .chain([ControlPos::Logo]);
        let mut indices: Vec<u8> = positions.map(|p| lit_index(p).unwrap()).collect();
        indices.sort_unstable();
        indices.dedup();
        assert_eq!(indices.len(), 81);
        assert!(indices.iter().all(|&i| is_led(i)));
        assert_eq!(lit_index(ControlPos::grid(8, 0)), None);
    }

    #[test]
    fn bound_to_a_fake_output_it_answers_on_the_paired_input() {
        let midi = FakeMidi::default();
        let (input, output) = (port("sim out", "in-1"), port("sim in", "out-1"));
        midi.add(input.clone(), output.clone());
        let sim = SimLaunchpad::mini();
        midi.bind("out-1", "in-1", sim.clone());
        let heard = Arc::new(Mutex::new(Vec::new()));
        let log = heard.clone();
        let mut link = midi
            .open(
                &input,
                &output,
                Box::new(move |bytes: &[u8]| log.lock().unwrap().push(bytes.to_vec())),
            )
            .unwrap();

        link.send(&hex("F0 00 20 29 02 0D 0E 01 F7")).unwrap();
        link.send(&hex("F0 00 20 29 02 0D 0E F7")).unwrap();
        assert!(sim.in_programmer_mode());
        assert_eq!(
            *heard.lock().unwrap(),
            [hex("F0 00 20 29 02 0D 0E 01 F7")],
            "answered before send returned"
        );
        assert_eq!(midi.take_sent().len(), 2, "both messages are recorded");
        link.close();
    }
}
