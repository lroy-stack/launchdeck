//! What the unit does besides lighting: steps 4 (inputs), 9 (brightness),
//! 10 (scrolling text), 11 (sleep and wake) and 12 (LED feedback).

use std::time::{Duration, Instant};

use super::report::{hex, Step, Verdict};
use super::steps::{first, frame_of, grid_led, pattern, MAX_7BIT};
use super::{Suite, ANSWER_WAIT, TICK};
use crate::core::model::{ControlPos, Rgb};
use crate::devices::novation::layout;
use crate::devices::novation::protocol::{
    self, brightness, feedback, parse_reply, text, text_stop, Colourspec, Reply, TextColour,
    BRIGHTNESS, FEEDBACK, MODE, SLEEP,
};
use crate::devices::{DeviceError, DeviceSetting, InputKind, SurfaceFrame};

/// How long each brightness level is shown.
const SHOW: Duration = Duration::from_secs(2);
const SWEEP_SHOW: Duration = Duration::from_secs(1);

/// What one control sent.
struct Seen {
    pos: ControlPos,
    press: Option<Vec<u8>>,
    release: Option<Vec<u8>>,
}

/// Step 4: every pad and button pressed and released once, the raw bytes
/// of each, and whether each press reached LaunchDeck as input.
pub(super) fn inputs(s: &mut Suite, step: &mut Step) -> Result<(), DeviceError> {
    let positions: Vec<ControlPos> = s
        .profile()
        .capabilities
        .positions()
        .into_iter()
        .filter(|&p| p != ControlPos::Logo)
        .collect();
    let dim = Rgb::new(24, 24, 24);
    s.frame = frame_of(positions.iter().map(|&p| (p, dim)));
    s.render()?;
    step.say(format!(
        "press and release each of the {} lit pads and buttons once; each turns green once seen; {} s without a press ends the step",
        positions.len(),
        ANSWER_WAIT.as_secs()
    ));
    s.tap.clear_inbox();
    while s.rx.try_recv().is_ok() {}
    let mut seen: Vec<Seen> = positions
        .iter()
        .map(|&pos| Seen {
            pos,
            press: None,
            release: None,
        })
        .collect();
    let (mut raw_presses, mut delivered) = (0usize, 0usize);
    let mut all = Vec::new();
    let mut last = Instant::now();
    while seen.iter().any(|x| x.release.is_none()) && last.elapsed() < ANSWER_WAIT {
        std::thread::sleep(TICK);
        for message in s.tap.take_inbox().into_iter().filter(|m| m.is_channel()) {
            last = Instant::now();
            let Some((pos, kind)) = layout::decode(&message.bytes, true) else {
                step.say(format!("not a control: {}", hex(&message.bytes)));
                continue;
            };
            all.push((message.bytes.clone(), kind));
            let Some(entry) = seen.iter_mut().find(|x| x.pos == pos) else {
                continue;
            };
            match kind {
                InputKind::Pressed { .. } => {
                    raw_presses += 1;
                    entry.press.get_or_insert(message.bytes);
                }
                InputKind::Released if entry.press.is_some() && entry.release.is_none() => {
                    entry.release = Some(message.bytes);
                    s.frame = recolour(&s.frame, pos, Rgb::new(0, 255, 0));
                }
                InputKind::Released => {}
            }
        }
        while let Ok(input) = s.rx.try_recv() {
            if matches!(input.kind, InputKind::Pressed { .. }) {
                delivered += 1;
            }
        }
        s.render()?;
    }
    for x in &seen {
        let bytes = |b: &Option<Vec<u8>>| b.as_deref().map(hex).unwrap_or_else(|| "none".into());
        step.say(format!(
            "{}: press {}, release {}",
            x.pos.key(),
            bytes(&x.press),
            bytes(&x.release)
        ));
    }
    let complete = seen.iter().filter(|x| x.release.is_some()).count();
    let what = format!("{complete} of {} controls pressed and released", seen.len());
    if raw_presses == 0 {
        step.check(what, Verdict::Skipped);
    } else {
        step.pass_if(what, complete == seen.len());
        step.pass_if(
            format!("every press reached LaunchDeck as input ({delivered} of {raw_presses})"),
            delivered == raw_presses,
        );
        describe_inputs(step, &all);
    }
    s.blank()
}

/// Which channels, press values and release messages the unit used.
fn describe_inputs(step: &mut Step, all: &[(Vec<u8>, InputKind)]) {
    let mut channels: Vec<u8> = all.iter().map(|(b, _)| (b[0] & 0x0F) + 1).collect();
    channels.sort_unstable();
    channels.dedup();
    let mut presses: Vec<String> = all
        .iter()
        .filter(|(_, k)| matches!(k, InputKind::Pressed { .. }))
        .map(|(b, _)| format!("{:02X}h {}", b[0] & 0xF0, b[2]))
        .collect();
    presses.sort();
    presses.dedup();
    let mut releases: Vec<String> = all
        .iter()
        .filter(|(_, k)| *k == InputKind::Released)
        .map(|(b, _)| match b[0] & 0xF0 {
            0x80 => "note off".to_string(),
            0x90 => "note on, velocity 0".to_string(),
            _ => "CC value 0".to_string(),
        })
        .collect();
    releases.sort();
    releases.dedup();
    step.note("channels the unit sends on", format!("{channels:?}"));
    step.note("press values (status, value)", presses.join(", "));
    step.note("release messages", releases.join(", "));
}

/// Step 9: brightness 0, 64 and 127 with readback, then a sweep; the
/// unit's own level is given back.
pub(super) fn brightness_levels(s: &mut Suite, step: &mut Step) -> Result<(), DeviceError> {
    s.blank()?;
    let product = s.profile().product;
    let saved = first(s.readback(step, BRIGHTNESS)?);
    let white: Vec<Colourspec> = (0..64u8)
        .map(|i| Colourspec::Rgb {
            index: grid_led(i % 8, i / 8),
            r: 127,
            g: 127,
            b: 127,
        })
        .collect();
    s.tap.send_all(&protocol::lighting(product, &white))?;
    step.say(format!(
        "showing levels 0, 64 and 127 for {} s each",
        SHOW.as_secs()
    ));
    for level in [0u8, 64, MAX_7BIT] {
        s.tap.send(&brightness(product, level))?;
        let read = first(s.readback(step, BRIGHTNESS)?);
        step.pass_if(
            format!("brightness {level} reads back {read:?}"),
            read == Some(level),
        );
        std::thread::sleep(SHOW);
    }
    s.ask(
        step,
        "Were the three levels (0, 64, 127) visibly different?",
        true,
        true,
    )?;
    step.say(format!(
        "sweeping 0, 16, … 112, 127, {} s each",
        SWEEP_SHOW.as_secs()
    ));
    for level in (0..=112u8).step_by(16).chain([MAX_7BIT]) {
        s.tap.send(&brightness(product, level))?;
        std::thread::sleep(SWEEP_SHOW);
    }
    s.ask(
        step,
        "Did each of the nine steps look brighter than the one before? (no: some looked the same)",
        false,
        true,
    )?;
    match saved {
        Some(level) => s.tap.send(&brightness(product, level))?,
        None => step.say("the unit's own level is unknown: left at 127"),
    }
    s.blank()
}

/// Step 10: scrolling text, its direction, colour, stop and end, and long
/// texts beyond LaunchDeck's own 64-character limit.
pub(super) fn scrolling_text(s: &mut Suite, step: &mut Step) -> Result<(), DeviceError> {
    let product = s.profile().product;
    let hello = b"Hello World";
    s.frame = frame_of((0..64u8).map(|i| (ControlPos::grid(i % 8, i / 8), Rgb::new(0, 0, 60))));
    s.render()?;
    s.tap
        .send(&text(product, true, 7, TextColour::Palette(37), hello))?;
    s.ask(
        step,
        "Is a turquoise \"Hello World\" scrolling right to left, over and over?",
        true,
        true,
    )?;
    s.tap
        .send(&text(product, true, -7, TextColour::Palette(37), hello))?;
    s.ask(step, "Does it now scroll left to right?", true, true)?;
    s.tap
        .send(&text(product, true, 7, TextColour::Rgb(127, 40, 0), hello))?;
    s.ask(step, "Is it now orange (an RGB colour)?", true, true)?;
    s.tap.send(&text_stop(product))?;
    s.ask(
        step,
        "Has it stopped, with the dim blue pads back as before?",
        true,
        true,
    )?;
    s.tap.clear_inbox();
    s.tap
        .send(&text(product, false, 10, TextColour::Palette(21), b"Once"))?;
    s.ask(
        step,
        "Did \"Once\" scroll across once and stop by itself? (answer after it ends)",
        true,
        true,
    )?;
    let after: Vec<String> = s
        .tap
        .take_inbox()
        .into_iter()
        .filter(|m| m.is_sysex() && !heartbeat_answer(&m.bytes))
        .map(|m| hex(&m.bytes))
        .collect();
    step.note(
        "SysEx from the unit when a non-looping text ends",
        if after.is_empty() {
            "none".to_string()
        } else {
            after.join("; ")
        },
    );
    for (length, speed) in [(64usize, 20i8), (200, 40)] {
        s.tap.send(&text(
            product,
            false,
            speed,
            TextColour::Palette(13),
            &long_text(length),
        ))?;
        let question = format!("Did the {length}-character text scroll all the way to its last word, END? (answer after it ends)");
        s.ask(step, &question, true, true)?;
    }
    s.tap.send(&text_stop(product))?;
    s.blank()
}

/// Step 11: LaunchDeck puts the LEDs to sleep; a press wakes the unit.
pub(super) fn sleep_and_wake(s: &mut Suite, step: &mut Step) -> Result<(), DeviceError> {
    s.frame = pattern();
    s.render()?;
    s.device()?.apply(DeviceSetting::Sleep(true))?;
    let asleep = first(s.readback(step, SLEEP)?);
    step.pass_if("09h readback 00: the LEDs sleep", asleep == Some(0));
    println!(
        "\n   The LEDs should be off now. Press any pad once to wake the unit ({} s).",
        ANSWER_WAIT.as_secs()
    );
    s.tap.clear_inbox();
    while s.rx.try_recv().is_ok() {}
    let pressed = s.tap.wait_for(
        |m| {
            matches!(
                layout::decode(m, true),
                Some((_, InputKind::Pressed { .. }))
            )
        },
        Instant::now() + ANSWER_WAIT,
    );
    let Some(press) = pressed else {
        step.check("a press wakes the unit", Verdict::Skipped);
        s.device()?.apply(DeviceSetting::Sleep(false))?;
        return s.render();
    };
    step.note("the waking press, as the unit sent it", hex(&press.bytes));
    std::thread::sleep(Duration::from_millis(300));
    let forwarded = std::iter::from_fn(|| s.rx.try_recv().ok()).count();
    step.pass_if(
        "LaunchDeck did not take the waking press as input",
        forwarded == 0,
    );
    s.ask(
        step,
        "Before LaunchDeck draws again: are the pads back as they were before the sleep?",
        false,
        false,
    )?;
    let awake = first(s.readback(step, SLEEP)?);
    step.note("09h readback after the press", format!("{awake:?}"));
    for _ in 0..3 {
        std::thread::sleep(TICK);
        s.render()?;
    }
    s.ask(
        step,
        "Are all the LEDs as before the sleep now?",
        true,
        true,
    )?;
    s.blank()
}

/// Step 12: external feedback off, lighting by SysEx and by note; internal
/// feedback off and on; the feedback found is given back.
pub(super) fn led_feedback(s: &mut Suite, step: &mut Step) -> Result<(), DeviceError> {
    s.blank()?;
    let product = s.profile().product;
    let saved = s.readback(step, FEEDBACK)?;
    s.tap.send(&feedback(product, true, false))?;
    let left = Colourspec::Rgb {
        index: grid_led(0, 0),
        r: 0,
        g: 127,
        b: 0,
    };
    s.tap.send_all(&protocol::lighting(product, &[left]))?;
    s.tap.send(&protocol::channel_lighting(
        protocol::ChannelLight::Static,
        grid_led(7, 0),
        21,
    ))?;
    s.ask(
        step,
        "External feedback off: is the top left pad green (lit by SysEx)?",
        false,
        true,
    )?;
    s.ask(
        step,
        "External feedback off: is the top right pad green (lit by a note)?",
        false,
        true,
    )?;
    for internal in [false, true] {
        s.tap.send(&feedback(product, internal, true))?;
        let state = if internal { "on" } else { "off" };
        let question = format!("Internal feedback {state}: hold a pad in the middle of the grid, then answer: did it light while held?");
        s.ask(step, &question, false, true)?;
    }
    match saved.as_deref() {
        Some(&[internal, external]) => {
            s.tap
                .send(&feedback(product, internal == 1, external == 1))?
        }
        _ => step.say("the feedback the unit had is unknown: left on"),
    }
    s.blank()
}

/// The answer to the session's heartbeat: Programmer mode or sleep.
fn heartbeat_answer(bytes: &[u8]) -> bool {
    matches!(
        parse_reply(bytes),
        Some(Reply::Readback {
            command: MODE | SLEEP,
            ..
        })
    )
}

/// The frame with one position in another colour.
fn recolour(frame: &SurfaceFrame, pos: ControlPos, color: Rgb) -> SurfaceFrame {
    frame_of(
        frame
            .leds
            .iter()
            .map(|c| (c.pos, if c.pos == pos { color } else { c.led.color })),
    )
}

/// `START`, dashes, then `END`: `length` characters.
fn long_text(length: usize) -> Vec<u8> {
    let dashes = length.saturating_sub("START  END".len());
    format!("START {} END", "-".repeat(dashes)).into_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn long_texts_have_the_length_asked_for() {
        for length in [64, 200] {
            let t = long_text(length);
            assert_eq!(t.len(), length);
            assert!(t.starts_with(b"START ") && t.ends_with(b" END"));
        }
    }
}
