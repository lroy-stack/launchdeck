//! Lighting the unit: steps 5 (palette), 6 (RGB), 7 (flash and pulse) and
//! 8 (batching, timed with a readback barrier). Each blanks the device
//! first, so that its LED cache knows the LEDs are off, then lights the unit
//! through the tap; the device draws only the yes and no buttons meanwhile.

use std::time::Duration;

use super::report::{millis, Step, Verdict};
use super::steps::{grid_led, median, MAX_7BIT};
use super::{Suite, REPLY_WAIT};
use crate::devices::novation::layout::led_index;
use crate::devices::novation::measure_tests::{self, Strategy};
use crate::devices::novation::protocol::{self, parse_reply, query, Colourspec, Reply, MODE};
use crate::devices::DeviceError;

/// Runs of each timed barrier; the median is reported.
const REPEATS: usize = 15;
/// A barrier faster by more than this decides for notes (plan 8.4).
const NOTES_MARGIN: Duration = Duration::from_millis(1);

/// Step 5: the 128 palette entries, 64 at a time, row by row from the top
/// left.
pub(super) fn palette(s: &mut Suite, step: &mut Step) -> Result<(), DeviceError> {
    s.blank()?;
    let product = s.profile().product;
    for first in [0u8, 64] {
        let specs: Vec<Colourspec> = (0..64u8)
            .map(|i| Colourspec::Static {
                index: grid_led(i % 8, i / 8),
                palette: first + i,
            })
            .collect();
        s.tap.send_all(&protocol::lighting(product, &specs))?;
        let question = format!(
            "Palette entries {first} to {}: {first} top left, {} top right, row by row to {} bottom right. Do the colours match the palette chart of the Programmer's Reference Manual?",
            first + 63,
            first + 7,
            first + 63
        );
        s.ask(step, &question, true, true)?;
    }
    s.blank()
}

/// Step 6: a gradient per channel and white, then exact white, red, green
/// and blue.
pub(super) fn rgb(s: &mut Suite, step: &mut Step) -> Result<(), DeviceError> {
    s.blank()?;
    let mut specs = Vec::new();
    for (band, channel) in [(0u8, [1, 0, 0]), (2, [0, 1, 0]), (4, [0, 0, 1])] {
        for i in 0..16u8 {
            let v = i * MAX_7BIT / 15;
            let [r, g, b] = channel.map(|c: u8| c * v);
            specs.push(Colourspec::Rgb {
                index: grid_led(i % 8, band + i / 8),
                r,
                g,
                b,
            });
        }
    }
    for col in 0..8u8 {
        let v = col * MAX_7BIT / 7;
        specs.push(Colourspec::Rgb {
            index: grid_led(col, 6),
            r: v,
            g: v,
            b: v,
        });
    }
    for (col, [r, g, b]) in [[127, 127, 127], [127, 0, 0], [0, 127, 0], [0, 0, 127]]
        .into_iter()
        .enumerate()
    {
        specs.push(Colourspec::Rgb {
            index: grid_led(col as u8, 7),
            r,
            g,
            b,
        });
    }
    s.tap
        .send_all(&protocol::lighting(s.profile().product, &specs))?;
    s.ask(
        step,
        "Rows 1-2 red, 3-4 green, 5-6 blue, each from dark to full over 16 pads, row 7 dark to full white: are the gradients smooth?",
        true,
        true,
    )?;
    s.ask(
        step,
        "Bottom row: are the first four pads pure white, red, green and blue?",
        true,
        true,
    )?;
    s.blank()
}

/// Step 7: the unit's own flash and pulse, their periods, and whether a
/// static colour stops a flash (plan 8.5's open question).
pub(super) fn flash_and_pulse(s: &mut Suite, step: &mut Step) -> Result<(), DeviceError> {
    s.blank()?;
    let product = s.profile().product;
    let row = |r: u8, spec: fn(u8) -> Colourspec| (0..8u8).map(move |c| spec(grid_led(c, r)));
    let specs: Vec<Colourspec> = row(0, |index| Colourspec::Flash { index, b: 5, a: 0 })
        .chain(row(2, |index| Colourspec::Pulse { index, palette: 45 }))
        .collect();
    s.tap.send_all(&protocol::lighting(product, &specs))?;
    s.ask(
        step,
        "Top row: flashing red, about twice a second (500 ms)?",
        true,
        true,
    )?;
    s.ask(
        step,
        "Third row: pulsing blue, about once a second (1000 ms)?",
        true,
        true,
    )?;
    let still: Vec<Colourspec> = row(0, |index| Colourspec::Rgb {
        index,
        r: 0,
        g: 127,
        b: 0,
    })
    .collect();
    s.tap.send_all(&protocol::lighting(product, &still))?;
    let stopped = s.ask(
        step,
        "Top row now: steady green, no flashing left?",
        true,
        true,
    )?;
    if stopped == Some(false) {
        step.say("a static RGB colour does not stop a flash: the planner must send palette 0 first (plan 8.5)");
    }
    s.blank()
}

/// Step 8: how long the unit takes to process each way of lighting LEDs,
/// timed with a readback barrier, and what that says for the batching rule.
pub(super) fn batching(s: &mut Suite, step: &mut Step) -> Result<(), DeviceError> {
    s.blank()?;
    let product = s.profile().product;
    let barrier = query(product, MODE);
    let answers = |m: &[u8]| matches!(parse_reply(m), Some(Reply::Readback { command: MODE, .. }));
    let mut alone = Vec::new();
    for _ in 0..REPEATS {
        alone.extend(s.tap.probe(&barrier, answers, REPLY_WAIT)?.map(|(_, t)| t));
    }
    let Some(base) = median(alone) else {
        step.check("the unit answers the readback barrier", Verdict::Fail);
        return s.blank();
    };
    step.say(format!(
        "the readback alone, median of {REPEATS}: {}",
        millis(base)
    ));
    s.facts.number("0Eh readback round trip", millis(base));
    step.say("| LEDs | strategy | messages | bytes | to the barrier's reply (median) | less the readback alone |");
    let mut results = Vec::new();
    let mut unanswered = 0;
    for case in measure_tests::cases() {
        let indices: Vec<u8> = case
            .positions
            .iter()
            .filter_map(|&p| led_index(p))
            .collect();
        for (strategy, label) in measure_tests::STRATEGIES {
            let messages = measure_tests::encode(strategy, product, &indices);
            let mut times = Vec::new();
            for _ in 0..REPEATS {
                match s
                    .tap
                    .probe_after(&messages, &barrier, answers, REPLY_WAIT)?
                {
                    Some((_, took)) => times.push(took),
                    None => unanswered += 1,
                }
            }
            let Some(m) = median(times) else { continue };
            let bytes: usize = messages.iter().map(Vec::len).sum();
            step.say(format!(
                "| {} | {label} | {} | {bytes} | {} | {} |",
                case.name,
                messages.len(),
                millis(m),
                millis(m.saturating_sub(base))
            ));
            s.facts
                .number(format!("barrier, {}, {label}", case.name), millis(m));
            results.push((indices.len(), strategy, m));
        }
    }
    step.pass_if(
        format!("every barrier was answered ({unanswered} not)"),
        unanswered == 0,
    );
    let k = notes_up_to(&results);
    let advice = match k {
        0 => "keep NOTE_LIGHTING_MAX_CHANGES = 0: one lighting SysEx per render".to_string(),
        k => format!("set NOTE_LIGHTING_MAX_CHANGES = {k}: notes are faster by more than 1 ms up to {k} LEDs"),
    };
    step.note("recommendation (plan 8.4)", advice.clone());
    s.facts.number("batching recommendation", advice);
    s.blank()
}

/// The most LEDs for which per-LED notes beat one palette batch by more
/// than `NOTES_MARGIN`, counting up from the smallest case.
fn notes_up_to(results: &[(usize, Strategy, Duration)]) -> usize {
    let time = |count: usize, wanted: fn(&Strategy) -> bool| {
        results
            .iter()
            .find(|(c, s, _)| *c == count && wanted(s))
            .map(|r| r.2)
    };
    let mut counts: Vec<usize> = results.iter().map(|r| r.0).collect();
    counts.sort_unstable();
    counts.dedup();
    let mut k = 0;
    for count in counts {
        let notes = time(count, |s| matches!(s, Strategy::PerLedNotes));
        let batch = time(count, |s| matches!(s, Strategy::BatchPalette));
        match (notes, batch) {
            (Some(n), Some(b)) if n + NOTES_MARGIN < b => k = count,
            _ => break,
        }
    }
    k
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notes_win_only_by_more_than_a_millisecond_from_the_smallest_case() {
        let ms = Duration::from_millis;
        let r = |c, s, t| (c, s, ms(t));
        let results = [
            r(1, Strategy::PerLedNotes, 2),
            r(1, Strategy::BatchPalette, 4),
            r(8, Strategy::PerLedNotes, 3),
            r(8, Strategy::BatchPalette, 5),
            r(64, Strategy::PerLedNotes, 9),
            r(64, Strategy::BatchPalette, 9),
            r(81, Strategy::PerLedNotes, 2),
            r(81, Strategy::BatchPalette, 9),
        ];
        assert_eq!(notes_up_to(&results), 8, "64 ties, so 81 does not count");
        let slow_first = [
            r(1, Strategy::PerLedNotes, 5),
            r(1, Strategy::BatchPalette, 4),
            r(81, Strategy::PerLedNotes, 2),
            r(81, Strategy::BatchPalette, 9),
        ];
        assert_eq!(
            notes_up_to(&slow_first),
            0,
            "counted from the smallest case"
        );
        let close = [
            r(1, Strategy::PerLedNotes, 3),
            r(1, Strategy::BatchPalette, 4),
        ];
        assert_eq!(notes_up_to(&close), 0, "exactly 1 ms is not more");
    }
}
