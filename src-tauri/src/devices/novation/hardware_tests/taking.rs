//! Taking the unit and giving it back: steps 2 (inquiry), 3 (Programmer
//! Mode), 13 (unplug and plug in again) and 14 (safe release).

use std::time::{Duration, Instant};

use super::report::{hex, millis, Step, Verdict};
use super::steps::{first, layout_name, pattern};
use super::{Suite, ANSWER_WAIT, PROFILES, REPLY_WAIT};
use crate::devices::novation::layout;
use crate::devices::novation::ports::{self, Model};
use crate::devices::novation::profile::Role;
use crate::devices::novation::protocol::{parse_reply, Reply, INQUIRY, LAYOUT, MODE};
use crate::devices::novation::session::REPLY_TIMEOUT;
use crate::devices::source::HardwareSource;
use crate::devices::{DeviceError, ModeState, SurfaceFrame};

const DAW_ENV: &str = "LAUNCHDECK_ACCEPTANCE_DAW";
const PROGRAMMER_LAYOUT: u8 = 0x7F;

/// Step 2: who the unit says it is, on its MIDI interface, and the layout
/// it shows before LaunchDeck takes it; the DAW interface only on request.
pub(super) fn inquiry(s: &mut Suite, step: &mut Step) -> Result<(), DeviceError> {
    s.open_alone()?;
    match ask_identity(s, step)? {
        Some(Identity {
            family,
            version,
            took,
        }) => {
            let profile = s.profile();
            step.check("the unit answers the inquiry", Verdict::Pass);
            step.pass_if(
                format!(
                    "family {} is the {}'s application",
                    hex(&family),
                    profile.name
                ),
                profile.families.contains(&family),
            );
            step.pass_if(
                format!(
                    "answered within the session's {} ms",
                    REPLY_TIMEOUT.as_millis()
                ),
                took <= REPLY_TIMEOUT,
            );
            let firmware: String = version.iter().map(|d| d.to_string()).collect();
            step.note("firmware digits", firmware.clone());
            s.facts.firmware = Some(firmware);
            s.facts.number("inquiry reply time", millis(took));
        }
        None => step.check("the unit answers the inquiry", Verdict::Fail),
    }
    let layout = first(s.readback(step, LAYOUT)?);
    s.facts.layout_before = layout;
    step.note(
        "the layout before LaunchDeck takes the unit",
        layout_name(layout),
    );
    s.tap.close();
    if std::env::var(DAW_ENV).as_deref() == Ok("1") {
        daw_inquiry(s, step)?;
    } else {
        step.say(format!(
            "the DAW interface is not opened ({DAW_ENV}=1 asks it too)"
        ));
    }
    Ok(())
}

/// What the unit answered to the inquiry, and how fast.
struct Identity {
    family: [u8; 2],
    version: [u8; 4],
    took: Duration,
}

/// The inquiry on whatever link is open.
fn ask_identity(s: &Suite, step: &mut Step) -> Result<Option<Identity>, DeviceError> {
    let answer = s.tap.probe(
        &INQUIRY,
        |m| matches!(parse_reply(m), Some(Reply::Identity { .. })),
        REPLY_WAIT,
    )?;
    let Some((bytes, took)) = answer else {
        step.say(format!(
            "inquiry {} -> no answer in {}",
            hex(&INQUIRY),
            millis(REPLY_WAIT)
        ));
        return Ok(None);
    };
    step.say(format!(
        "inquiry {} -> {} in {}",
        hex(&INQUIRY),
        hex(&bytes),
        millis(took)
    ));
    Ok(match parse_reply(&bytes) {
        Some(Reply::Identity {
            family, version, ..
        }) => {
            step.say(format!(
                "family {}, version digits {version:?}",
                hex(&family)
            ));
            Some(Identity {
                family,
                version,
                took,
            })
        }
        _ => None,
    })
}

/// The inquiry on the unit's DAW interface: does it answer, and with what.
fn daw_inquiry(s: &mut Suite, step: &mut Step) -> Result<(), DeviceError> {
    let platform = s.platform;
    let snapshot = s.tap.ports();
    let daw = |list: &[crate::devices::midi::PortInfo]| {
        list.iter()
            .find(|p| {
                ports::read(&p.name, platform, &PROFILES).is_some_and(|r| {
                    matches!(r.model, Model::Known(m) if m.model == s.profile().model)
                        && r.key == s.unit.key
                        && r.role == Some(Role::Daw)
                })
            })
            .cloned()
    };
    let (Some(input), Some(output)) = (daw(&snapshot.inputs), daw(&snapshot.outputs)) else {
        step.note("inquiry on the DAW interface", "no DAW interface found");
        return Ok(());
    };
    step.say(format!(
        "DAW interface: input `{}`, output `{}`",
        input.name, output.name
    ));
    s.tap.open(&input, &output)?;
    let answer = ask_identity(s, step);
    s.tap.close();
    let observed = match answer? {
        Some(Identity {
            family,
            version,
            took,
        }) => format!(
            "answered: family {}, version {version:?}, in {}",
            hex(&family),
            millis(took)
        ),
        None => "no answer".into(),
    };
    step.note("inquiry on the DAW interface", observed);
    Ok(())
}

/// Step 3: LaunchDeck takes the unit as the app does, then the suite reads
/// back the mode and the layout.
pub(super) fn programmer_mode(s: &mut Suite, step: &mut Step) -> Result<(), DeviceError> {
    let took = s.take(step)?;
    s.facts.number("connect to owned", millis(took));
    let mode = s.device()?.details().mode;
    step.pass_if(
        format!("the session confirmed Programmer mode ({mode:?})"),
        mode == ModeState::Confirmed,
    );
    let readback = first(s.readback(step, MODE)?);
    step.pass_if("0Eh readback 01: Programmer mode", readback == Some(1));
    let layout = first(s.readback(step, LAYOUT)?);
    step.note("layout readback in Programmer mode", layout_name(layout));
    Ok(())
}

/// Step 13: the unit unplugged while held, then plugged in again: its
/// mode on power-up, how long until LaunchDeck holds it, the frame again.
pub(super) fn replug(s: &mut Suite, step: &mut Step) -> Result<(), DeviceError> {
    s.frame = pattern();
    s.render()?;
    println!(
        "\n   Unplug the Launchpad now ({} s).",
        ANSWER_WAIT.as_secs()
    );
    let id = s.unit.id.clone();
    if wait_until(|| !s.hardware.scan().units.iter().any(|u| u.id == id)).is_none() {
        step.check("the unit was unplugged", Verdict::Skipped);
        return Ok(());
    }
    let after = s.render();
    step.note("a render after the unplug", format!("{after:?}"));
    s.release()?;
    s.tap.close();
    println!("\n   Plug it in again ({} s).", ANSWER_WAIT.as_secs());
    if wait_until(|| s.hardware.scan().units.iter().any(|u| u.id == id)).is_none() {
        step.check("the unit was plugged in again", Verdict::Skipped);
        return Ok(());
    }
    let back = Instant::now();
    if let Some(unit) = ports::parse(&s.tap.ports(), s.platform, &PROFILES)
        .units
        .into_iter()
        .find(|u| u.id == id)
    {
        s.unit = unit;
    }
    s.open_alone()?;
    let mut mode = None;
    for _ in 0..3 {
        mode = first(s.readback(step, MODE)?);
        if mode.is_some() {
            break;
        }
    }
    step.pass_if(
        "0Eh readback 00: the unit came back in Live mode",
        mode == Some(0),
    );
    let took = s.take(step)?;
    let total = back.elapsed();
    step.say(format!(
        "from the ports' return to owned: {} (the readback included)",
        millis(total)
    ));
    s.facts.number("replug: ports back to owned", millis(total));
    s.facts.number("replug: connect to owned", millis(took));
    s.ask(
        step,
        "Is the pattern from before the unplug back?",
        true,
        true,
    )?;
    Ok(())
}

/// Step 14: LaunchDeck gives the unit back; the owner checks it in its
/// normal mode, then answers once LaunchDeck has taken it again.
pub(super) fn safe_release(s: &mut Suite, step: &mut Step) -> Result<(), DeviceError> {
    if s.device.is_none() {
        s.take(step)?;
    }
    s.frame = pattern();
    s.render()?;
    s.open_alone()?;
    check_given_back(s, step)?;
    println!(
        "\n   LaunchDeck gave the unit back. Check it: is it in its normal mode, and does holding Session open Setup? Leave Setup, then press any pad to go on (or wait {} s).",
        ANSWER_WAIT.as_secs()
    );
    s.tap.clear_inbox();
    let pressed = s.tap.wait_for(
        |m| layout::decode(m, true).is_some(),
        Instant::now() + ANSWER_WAIT,
    );
    step.note(
        "a press after the release",
        pressed
            .map(|m| hex(&m.bytes))
            .unwrap_or_else(|| "none".into()),
    );
    s.tap.close();
    s.frame = SurfaceFrame::default();
    s.take(step)?;
    s.ask(
        step,
        "Was the unit in its normal mode, and did holding Session open Setup?",
        true,
        true,
    )?;
    s.open_alone()?;
    check_given_back(s, step)?;
    s.tap.close();
    Ok(())
}

/// After a release: Live mode, and the layout the unit had before.
fn check_given_back(s: &mut Suite, step: &mut Step) -> Result<(), DeviceError> {
    let mode = first(s.readback(step, MODE)?);
    step.pass_if("0Eh readback 00: given back in Live mode", mode == Some(0));
    let expected = match s.facts.layout_before {
        Some(layout) if layout != PROGRAMMER_LAYOUT => layout,
        _ => s.profile().live_layout,
    };
    let layout = first(s.readback(step, LAYOUT)?);
    step.pass_if(
        format!(
            "layout {} restored (read {})",
            layout_name(Some(expected)),
            layout_name(layout)
        ),
        layout == Some(expected),
    );
    Ok(())
}

/// Waits up to `ANSWER_WAIT` for `done`, polling every 250 ms.
fn wait_until(mut done: impl FnMut() -> bool) -> Option<Duration> {
    let began = Instant::now();
    while began.elapsed() < ANSWER_WAIT {
        if done() {
            return Some(began.elapsed());
        }
        std::thread::sleep(Duration::from_millis(250));
    }
    None
}
