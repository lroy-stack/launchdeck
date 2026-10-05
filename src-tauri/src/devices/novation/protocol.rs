//! The Novation Launchpad MK3 codec shared by the Launchpad Mini MK3 and the
//! Launchpad X: SysEx framing, the commands LaunchDeck uses and their
//! readback queries, the Universal Device Inquiry and the reply parser,
//! colourspecs, channel lighting and scrolling text. Pure: bytes in, bytes
//! out, no transport.
//!
//! Every byte comes from the Programmer's Reference Manuals (MINI-PRM and
//! X-PRM, both 2019-11-08), which agree on all of it apart from the product
//! byte. X-only commands (04h, 0Bh, 0Dh, 0Fh, 13h, 15h-17h) and the DAW-port
//! commands (01h, 10h, 12h, 14h) are deliberately absent. NOT HARDWARE
//! VERIFIED: no Launchpad was attached when this was written.

/// The product byte that ends a model's SysEx header `F0 00 20 29 02 <p>`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Product(pub u8);

impl Product {
    pub(crate) const MINI_MK3: Product = Product(0x0D);
    pub(crate) const X: Product = Product(0x0C);
}

const SYSEX_START: u8 = 0xF0;
const SYSEX_END: u8 = 0xF7;
/// Novation's manufacturer id, then the `02h` that starts every MK3 header.
const NOVATION: [u8; 4] = [0x00, 0x20, 0x29, 0x02];
const DATA_MAX: u8 = 0x7F;

// Command bytes, as sent after the header and echoed by readback replies.
pub(crate) const LAYOUT: u8 = 0x00;
pub(crate) const LED: u8 = 0x03;
pub(crate) const TEXT: u8 = 0x07;
pub(crate) const BRIGHTNESS: u8 = 0x08;
pub(crate) const SLEEP: u8 = 0x09;
pub(crate) const FEEDBACK: u8 = 0x0A;
pub(crate) const MODE: u8 = 0x0E;

/// The manuals allow up to 81 colourspecs per lighting message: one per LED.
pub(crate) const MAX_COLOURSPECS: usize = 81;

/// The Universal Device Inquiry request.
pub(crate) const INQUIRY: [u8; 6] = [0xF0, 0x7E, 0x7F, 0x06, 0x01, 0xF7];

/// Scrolling speeds the text command can carry, in pads per second.
const TEXT_SPEED_MIN: i8 = -64;
const TEXT_SPEED_MAX: i8 = 63;

/// A data byte: values above 127 are clamped, so that a message never
/// carries a status byte by accident.
fn data(v: u8) -> u8 {
    v.min(DATA_MAX)
}

/// `F0 00 20 29 02 <p> <command> <data…> F7`.
pub(crate) fn sysex(product: Product, command: u8, payload: &[u8]) -> Vec<u8> {
    let mut m = Vec::with_capacity(8 + payload.len());
    m.push(SYSEX_START);
    m.extend(NOVATION);
    m.push(product.0);
    m.push(command);
    m.extend(payload.iter().copied().map(data));
    m.push(SYSEX_END);
    m
}

/// The readback form of a command: the command with no data.
pub(crate) fn query(product: Product, command: u8) -> Vec<u8> {
    sysex(product, command, &[])
}

/// Programmer mode (`0Eh 01`) or Live mode (`0Eh 00`).
pub(crate) fn programmer_mode(product: Product, on: bool) -> Vec<u8> {
    sysex(product, MODE, &[u8::from(on)])
}

/// Selects a layout (`00h <layout>`); `7Fh` is Programmer mode.
pub(crate) fn select_layout(product: Product, layout: u8) -> Vec<u8> {
    sysex(product, LAYOUT, &[layout])
}

/// LED brightness, 0 (lowest) to 127 (highest).
pub(crate) fn brightness(product: Product, level: u8) -> Vec<u8> {
    sysex(product, BRIGHTNESS, &[level])
}

/// Turns the LEDs off (`09h 00`, sleep) or back on (`09h 01`).
pub(crate) fn sleep(product: Product, leds_on: bool) -> Vec<u8> {
    sysex(product, SLEEP, &[u8::from(leds_on)])
}

/// Internal and external LED feedback, each on or off.
pub(crate) fn feedback(product: Product, internal: bool, external: bool) -> Vec<u8> {
    sysex(product, FEEDBACK, &[u8::from(internal), u8::from(external)])
}

/// One LED of a lighting message. Colours are 7-bit; palette entries index
/// the device's fixed 128-colour palette.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Colourspec {
    Static {
        index: u8,
        palette: u8,
    },
    /// Alternates between `a` (the colour already set) and `b`.
    Flash {
        index: u8,
        b: u8,
        a: u8,
    },
    Pulse {
        index: u8,
        palette: u8,
    },
    Rgb {
        index: u8,
        r: u8,
        g: u8,
        b: u8,
    },
}

impl Colourspec {
    /// `<type> <index> <data>`; flash data is colour B, then colour A.
    fn encode_into(self, out: &mut Vec<u8>) {
        match self {
            Colourspec::Static { index, palette } => out.extend([0x00, index, palette]),
            Colourspec::Flash { index, b, a } => out.extend([0x01, index, b, a]),
            Colourspec::Pulse { index, palette } => out.extend([0x02, index, palette]),
            Colourspec::Rgb { index, r, g, b } => out.extend([0x03, index, r, g, b]),
        }
    }
}

/// LED SysEx messages (`03h`) for these colourspecs, at most 81 per message.
pub(crate) fn lighting(product: Product, specs: &[Colourspec]) -> Vec<Vec<u8>> {
    specs
        .chunks(MAX_COLOURSPECS)
        .map(|chunk| {
            let mut payload = Vec::with_capacity(chunk.len() * 5);
            for spec in chunk {
                spec.encode_into(&mut payload);
            }
            sysex(product, LED, &payload)
        })
        .collect()
}

/// How a channel-lit LED shows its palette colour: notes on channel 1
/// (static), 2 (flashing) or 3 (pulsing).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ChannelLight {
    Static,
    Flash,
    Pulse,
}

/// A Note On that lights one LED; palette entry 0 turns it off.
pub(crate) fn channel_lighting(kind: ChannelLight, index: u8, palette: u8) -> [u8; 3] {
    let status = match kind {
        ChannelLight::Static => 0x90,
        ChannelLight::Flash => 0x91,
        ChannelLight::Pulse => 0x92,
    };
    [status, data(index), data(palette)]
}

/// The colour of scrolling text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TextColour {
    Palette(u8),
    Rgb(u8, u8, u8),
}

/// Scrolls `ascii` across the pads and the right column (`07h`). `speed` is
/// in pads per second, clamped to -64..=63; a negative speed scrolls left to
/// right and is sent as `80h + speed`. Bytes outside ASCII are sent as `?`.
pub(crate) fn text(
    product: Product,
    looping: bool,
    speed: i8,
    colour: TextColour,
    ascii: &[u8],
) -> Vec<u8> {
    let speed = speed.clamp(TEXT_SPEED_MIN, TEXT_SPEED_MAX);
    let speed = if speed < 0 {
        (0x80 + i16::from(speed)) as u8
    } else {
        speed as u8
    };
    let mut payload = vec![u8::from(looping), speed];
    match colour {
        TextColour::Palette(p) => payload.extend([0x00, p]),
        TextColour::Rgb(r, g, b) => payload.extend([0x01, r, g, b]),
    }
    payload.extend(ascii.iter().map(|&c| if c.is_ascii() { c } else { b'?' }));
    sysex(product, TEXT, &payload)
}

/// Stops any scrolling text: the text command with no data.
pub(crate) fn text_stop(product: Product) -> Vec<u8> {
    query(product, TEXT)
}

/// A message from the device that answers a request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Reply {
    /// The inquiry reply. `family` is `13 01` for a Mini MK3 application,
    /// `13 11` for its bootloader, `03 01` for a Launchpad X; `version` holds
    /// four digits 0-9.
    Identity {
        family: [u8; 2],
        member: [u8; 2],
        version: [u8; 4],
    },
    /// `F0 00 20 29 02 <product> <command> <data…> F7`.
    Readback {
        product: u8,
        command: u8,
        data: Vec<u8>,
    },
}

/// Reads an inquiry or readback reply; anything else, or anything malformed,
/// is `None`.
pub(crate) fn parse_reply(bytes: &[u8]) -> Option<Reply> {
    let [SYSEX_START, inner @ .., SYSEX_END] = bytes else {
        return None;
    };
    if inner.iter().any(|&b| b > DATA_MAX) {
        return None;
    }
    parse_identity(inner).or_else(|| parse_readback(inner))
}

/// `7E <device> 06 02 00 20 29 <family 2> <member 2> <version 4>`.
fn parse_identity(inner: &[u8]) -> Option<Reply> {
    let [0x7E, _device, 0x06, 0x02, 0x00, 0x20, 0x29, f0, f1, m0, m1, v0, v1, v2, v3] = *inner
    else {
        return None;
    };
    let version = [v0, v1, v2, v3];
    if version.iter().any(|&d| d > 9) {
        return None;
    }
    Some(Reply::Identity {
        family: [f0, f1],
        member: [m0, m1],
        version,
    })
}

/// `00 20 29 02 <product> <command> <data, at least one byte>`.
fn parse_readback(inner: &[u8]) -> Option<Reply> {
    match inner {
        [0x00, 0x20, 0x29, 0x02, product, command, data @ ..] if !data.is_empty() => {
            Some(Reply::Readback {
                product: *product,
                command: *command,
                data: data.to_vec(),
            })
        }
        _ => None,
    }
}

/// 0..=255 to the 0..=127 the lighting messages expect.
pub(crate) fn to_7bit(v: u8) -> u8 {
    ((u16::from(v) * 127 + 127) / 255) as u8
}
