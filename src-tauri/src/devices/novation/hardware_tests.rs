//! The physical acceptance suite (LAUNCHPAD_MINI_MK3_PLAN.md section 15):
//! one ignored test for a real Launchpad Mini MK3, or else a Launchpad X,
//! on the system's MIDI ports, through the real port parser,
//! `NovationHardware` with the system's `Platform` and `Mk3Device`, with
//! the suite's own probes on the same link (`tap`).
//!
//! The owner answers on the unit: the top right `>` button (Side 0) is
//! yes, the bottom right one (Side 7, Stop/Solo/Mute) is no; a question not
//! answered within 60 s is skipped. Each step prints the bytes both ways,
//! its checks and the open questions of the plan it answers; the report is
//! written to `$LAUNCHDECK_ACCEPTANCE_REPORT`, by default a file in the
//! system's temporary folder. Without a unit it says NOT HARDWARE VERIFIED
//! and passes; with one it fails if a step failed. Run it with
//! `scripts/launchpad-acceptance.sh`, never from `npm run check` or CI.

mod controls;
mod lighting;
mod report;
mod steps;
mod taking;
mod tap;

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver};

use self::report::{hex, millis, Report, Status, Step, Verdict};
use self::tap::{Tap, TapBackend};
use super::hardware::NovationHardware;
use super::ports::{self, FoundUnit, Model};
use super::profile::ModelProfile;
use super::protocol::{parse_reply, query, Reply};
use super::{launchpad_mini_mk3, launchpad_x};
use crate::core::model::{ControlPos, Rgb};
use crate::devices::midi::{MidiBackend, SystemMidi};
use crate::devices::source::HardwareSource;
use crate::devices::{
    DeviceError, DeviceInput, InputKind, InputSender, Led, LedCell, ModeState, SurfaceDevice,
    SurfaceFrame,
};
use crate::platform::Platform;

const REPORT_ENV: &str = "LAUNCHDECK_ACCEPTANCE_REPORT";
const NO_UNIT: &str = "No Launchpad Mini MK3 or Launchpad X found: NOT HARDWARE VERIFIED";
/// The models the suite looks for, in order of preference.
const PROFILES: [&ModelProfile; 2] = [&launchpad_mini_mk3::PROFILE, &launchpad_x::PROFILE];
/// How long a question waits for the owner's answer.
const ANSWER_WAIT: Duration = Duration::from_secs(60);
/// How long a probe waits for the unit's answer: longer than the session's
/// 500 ms, so that a slow answer is measured rather than missed.
const REPLY_WAIT: Duration = Duration::from_secs(2);
/// The runtime's render interval.
const TICK: Duration = Duration::from_millis(40);
/// How long taking the unit may take before the step gives up.
const OWN_WAIT: Duration = Duration::from_secs(5);
const YES: ControlPos = ControlPos::Side { index: 0 };
const NO: ControlPos = ControlPos::Side { index: 7 };

#[test]
#[ignore = "needs a real Launchpad; run scripts/launchpad-acceptance.sh"]
fn launchpad_acceptance() {
    let mut report = Report::new(report_path());
    report.head(format!("Started: {}", utc_now()));
    report.head(format!("Report: {}", report.path().display()));
    let tap = Tap::new(Arc::new(SystemMidi));
    let backend: Arc<dyn MidiBackend> = Arc::new(TapBackend(tap.clone()));
    let platform = Platform::current();
    let mut hardware = NovationHardware::new(backend, platform);

    let mut detection = Step::new(1, "Detection", "Q1, Q16");
    let found = detect(&tap, &mut hardware, platform, &mut detection);
    report.add(detection);
    let Some((unit, naming)) = found else {
        report.head(NO_UNIT);
        report.write();
        println!("Report: {}", report.path().display());
        return;
    };
    report.head(format!("Unit: {} ({})", unit.name, unit.id));

    let mut suite = Suite::new(tap, hardware, unit, platform, naming);
    for (number, title, answers, run) in steps::STEPS {
        let mut step = Step::new(number, title, answers);
        let mark = suite.tap.mark();
        if let Err(e) = run(&mut suite, &mut step) {
            let detail = e.detail().map(|d| format!(" ({d})")).unwrap_or_default();
            step.check(format!("the step stopped: {e}{detail}"), Verdict::Fail);
        }
        step.set_transcript(suite.tap.since(mark));
        report.add(step);
    }
    suite.finish();
    summarise(&mut report, &suite.facts);
    report.write();
    let failed: Vec<u8> = report
        .steps()
        .iter()
        .filter(|s| s.status() == Status::Fail)
        .map(|s| s.number)
        .collect();
    println!("\nReport: {}", report.path().display());
    assert!(failed.is_empty(), "steps {failed:?} failed; see the report");
}

/// What the summary needs from the steps.
#[derive(Debug, Default)]
struct Facts {
    unit: String,
    firmware: Option<String>,
    windows_naming: Option<&'static str>,
    /// The layout the unit showed before LaunchDeck took it.
    layout_before: Option<u8>,
    numbers: Vec<(String, String)>,
}

impl Facts {
    fn number(&mut self, what: impl Into<String>, value: impl Into<String>) {
        self.numbers.push((what.into(), value.into()));
    }
}

/// The unit under test and what drives it.
struct Suite {
    tap: Arc<Tap>,
    hardware: NovationHardware,
    unit: FoundUnit,
    /// The system whose port names the suite reads.
    platform: Platform,
    device: Option<Box<dyn SurfaceDevice>>,
    tx: InputSender,
    rx: UnboundedReceiver<DeviceInput>,
    /// What the device is drawing between questions.
    frame: SurfaceFrame,
    facts: Facts,
}

impl Suite {
    fn new(
        tap: Arc<Tap>,
        hardware: NovationHardware,
        unit: FoundUnit,
        platform: Platform,
        windows_naming: Option<&'static str>,
    ) -> Suite {
        let (tx, rx) = unbounded_channel();
        let facts = Facts {
            unit: format!("{} ({})", unit.name, unit.id),
            windows_naming,
            ..Facts::default()
        };
        Suite {
            tap,
            hardware,
            unit,
            platform,
            device: None,
            tx,
            rx,
            frame: SurfaceFrame::default(),
            facts,
        }
    }

    fn profile(&self) -> &'static ModelProfile {
        self.unit.profile
    }

    /// Takes the unit with a fresh `Mk3Device` from `NovationHardware`, as
    /// the app does after a look, rendering every tick until it is owned.
    fn take(&mut self, step: &mut Step) -> Result<Duration, DeviceError> {
        self.release()?;
        self.tap.close();
        self.hardware.scan();
        let mut device = self
            .hardware
            .device(&self.unit.id)
            .ok_or_else(|| DeviceError::Unknown(self.unit.id.clone()))?;
        let began = Instant::now();
        device.connect(self.tx.clone())?;
        let mut renders = 0;
        while device.details().mode == ModeState::Acquiring && began.elapsed() < OWN_WAIT {
            std::thread::sleep(TICK);
            device.render(&self.frame)?;
            renders += 1;
        }
        let took = began.elapsed();
        let details = device.details();
        step.say(format!(
            "taken in {} ({renders} renders): mode {:?}, firmware {}, notice {}",
            millis(took),
            details.mode,
            details.firmware.as_deref().unwrap_or("unknown"),
            details.notice.as_deref().unwrap_or("none"),
        ));
        if details.firmware.is_some() {
            self.facts.firmware = details.firmware;
        }
        self.device = Some(device);
        Ok(took)
    }

    /// Gives the unit back, if a device holds it.
    fn release(&mut self) -> Result<(), DeviceError> {
        match self.device.take() {
            Some(mut device) => device.disconnect(),
            None => Ok(()),
        }
    }

    fn device(&mut self) -> Result<&mut Box<dyn SurfaceDevice>, DeviceError> {
        self.device.as_mut().ok_or(DeviceError::NotConnected)
    }

    fn render(&mut self) -> Result<(), DeviceError> {
        let frame = self.frame.clone();
        self.device()?.render(&frame)
    }

    /// The device's LEDs all off, and nothing drawn until the next frame:
    /// the suite lights the unit itself.
    fn blank(&mut self) -> Result<(), DeviceError> {
        self.frame = SurfaceFrame::default();
        self.device()?.clear()
    }

    /// Asks the owner, who answers on the unit; `None` when 60 s pass.
    /// With `redraw`, the device renders every tick meanwhile, with the yes
    /// and no buttons lit; without, nothing is sent while waiting.
    fn ask(
        &mut self,
        step: &mut Step,
        question: &str,
        expect_yes: bool,
        redraw: bool,
    ) -> Result<Option<bool>, DeviceError> {
        println!("\n   QUESTION: {question}");
        println!(
            "   Press the top right > button for yes, the bottom right button for no ({} s).",
            ANSWER_WAIT.as_secs()
        );
        while self.rx.try_recv().is_ok() {}
        let hinted = with_hints(&self.frame);
        let deadline = Instant::now() + ANSWER_WAIT;
        let answer = loop {
            if redraw {
                self.device()?.render(&hinted)?;
            }
            if let Some(answer) = self.answer() {
                break Some(answer);
            }
            if Instant::now() >= deadline {
                break None;
            }
            std::thread::sleep(TICK);
        };
        let verdict = match (answer, expect_yes) {
            (None, _) => Verdict::Skipped,
            (Some(yes), true) => {
                if yes {
                    Verdict::Pass
                } else {
                    Verdict::Fail
                }
            }
            (Some(yes), false) => Verdict::Noted(if yes { "yes" } else { "no" }.into()),
        };
        step.check(question, verdict);
        Ok(answer)
    }

    /// A press of the yes or the no button since the last call.
    fn answer(&mut self) -> Option<bool> {
        while let Ok(input) = self.rx.try_recv() {
            if let InputKind::Pressed { .. } = input.kind {
                match input.pos {
                    YES => return Some(true),
                    NO => return Some(false),
                    _ => {}
                }
            }
        }
        None
    }

    /// Reads back `command` on whatever link is open, kept from the device.
    fn readback(&self, step: &mut Step, command: u8) -> Result<Option<Vec<u8>>, DeviceError> {
        let product = self.profile().product;
        let request = query(product, command);
        let answer = self.tap.probe(
            &request,
            |m| {
                matches!(parse_reply(m), Some(Reply::Readback { product: p, command: c, .. })
                    if p == product.0 && c == command)
            },
            REPLY_WAIT,
        )?;
        Ok(match answer {
            Some((bytes, took)) => {
                step.say(format!(
                    "readback {command:02X}h: {} -> {} in {}",
                    hex(&request),
                    hex(&bytes),
                    millis(took)
                ));
                match parse_reply(&bytes) {
                    Some(Reply::Readback { data, .. }) => Some(data),
                    _ => None,
                }
            }
            None => {
                step.say(format!(
                    "readback {command:02X}h: {} -> no answer in {}",
                    hex(&request),
                    millis(REPLY_WAIT)
                ));
                None
            }
        })
    }

    /// Opens the unit's pair for the suite alone, after giving it back.
    fn open_alone(&mut self) -> Result<(), DeviceError> {
        self.release()?;
        let unit = self.unit.clone();
        self.tap.open(&unit.input, &unit.output)
    }

    /// Gives the unit back and closes every link: the end of the run.
    fn finish(&mut self) {
        if let Err(e) = self.release() {
            println!("giving the unit back at the end: {e}");
        }
        self.tap.close();
    }
}

/// The frame with the yes button lit green and the no button red.
fn with_hints(frame: &SurfaceFrame) -> SurfaceFrame {
    let mut leds: Vec<LedCell> = frame
        .leds
        .iter()
        .filter(|c| c.pos != YES && c.pos != NO)
        .cloned()
        .collect();
    for (pos, color) in [(YES, Rgb::new(0, 255, 0)), (NO, Rgb::new(255, 0, 0))] {
        leds.push(LedCell {
            pos,
            led: Led {
                color,
                animation: Default::default(),
            },
        });
    }
    SurfaceFrame { leds }
}

/// Step 1: every port in the system's order, how each name reads, the
/// units found and the one chosen: a Mini MK3 first, else an X.
fn detect(
    tap: &Tap,
    hardware: &mut NovationHardware,
    platform: Platform,
    step: &mut Step,
) -> Option<(FoundUnit, Option<&'static str>)> {
    let snapshot = tap.ports();
    step.say(format!("system: {platform:?}"));
    for (direction, list) in [
        ("inputs (the computer receives)", &snapshot.inputs),
        ("outputs (the computer sends)", &snapshot.outputs),
    ] {
        step.say(format!(
            "{} {direction}, in the system's order:",
            list.len()
        ));
        for (i, port) in list.iter().enumerate() {
            let reading = ports::read(&port.name, platform, &PROFILES)
                .map(|r| {
                    let model = match r.model {
                        Model::Known(p) => p.name.to_string(),
                        Model::Other(name) => format!("another Novation model, {name}"),
                    };
                    format!("{model}, unit key {}, role {:?}", r.key, r.role)
                })
                .unwrap_or_else(|| "not a Novation port".into());
            step.say(format!(
                "  {i}. `{}` (id `{}`): {reading}",
                port.name, port.id
            ));
        }
    }
    let parsed = ports::parse(&snapshot, platform, &PROFILES);
    for unit in &parsed.units {
        step.say(format!(
            "unit {} `{}`: input `{}` (id `{}`), output `{}` (id `{}`), unit key {}",
            unit.id,
            unit.name,
            unit.input.name,
            unit.input.id,
            unit.output.name,
            unit.output.id,
            unit.key
        ));
    }
    for other in &parsed.unsupported {
        step.say(format!("not supported: {} ({})", other.name, other.id));
    }
    let scanned: Vec<String> = hardware.scan().units.into_iter().map(|f| f.id).collect();
    step.say(format!("NovationHardware lists: {scanned:?}"));
    let chosen = PROFILES
        .iter()
        .find_map(|p| parsed.units.iter().find(|u| u.profile.model == p.model))
        .cloned();
    let Some(unit) = chosen else {
        step.check(
            "a Launchpad Mini MK3 or Launchpad X on the ports",
            Verdict::Skipped,
        );
        return None;
    };
    step.say(format!(
        "chosen: {} ({}), input `{}`, output `{}`",
        unit.name, unit.id, unit.input.name, unit.output.name
    ));
    step.check(
        format!("{} found on its MIDI interface", unit.name),
        Verdict::Pass,
    );
    step.pass_if(
        "NovationHardware lists the same unit",
        scanned.contains(&unit.id),
    );
    let naming = (platform == Platform::Windows).then(|| windows_naming(&unit));
    if let Some(naming) = naming {
        step.note("Windows port naming", naming);
    }
    Some((unit, naming))
}

/// Windows MIDI Services' new names end in `In` or `Out`; the classic
/// (WinMM) names do not.
fn windows_naming(unit: &FoundUnit) -> &'static str {
    let name = unit.input.name.to_lowercase();
    if name.ends_with(" in") || name.ends_with(" out") {
        "new style (Windows MIDI Services)"
    } else {
        "classic (WinMM)"
    }
}

/// Step 15: every step's result, the unit, the system and the numbers.
fn summarise(report: &mut Report, facts: &Facts) {
    let rows: Vec<String> = report
        .steps()
        .iter()
        .map(|s| format!("| {} | {} | {} |", s.number, s.title, s.status().label()))
        .collect();
    let count = |status| {
        report
            .steps()
            .iter()
            .filter(|s| s.status() == status)
            .count()
    };
    let totals = format!(
        "Steps: {} pass, {} failed, {} skipped.",
        count(Status::Pass),
        count(Status::Fail),
        count(Status::Skipped)
    );
    println!("\n== Step 15. Summary");
    report.summary("| Step | Title | Result |");
    report.summary("| --- | --- | --- |");
    for row in rows {
        report.summary(row);
    }
    report.summary("");
    report.summary(totals);
    report.summary("");
    report.summary(format!("- Unit: {}", facts.unit));
    report.summary(format!(
        "- Firmware (inquiry digits): {}",
        facts.firmware.as_deref().unwrap_or("unknown")
    ));
    report.summary(format!(
        "- System: {} {} ({:?}){}",
        std::env::consts::OS,
        std::env::consts::ARCH,
        Platform::current(),
        system_version()
    ));
    if let Some(naming) = facts.windows_naming {
        report.summary(format!("- Windows port naming: {naming}"));
    }
    for (what, value) in &facts.numbers {
        report.summary(format!("- {what}: {value}"));
    }
}

/// The macOS version or the Linux kernel, when the system says.
fn system_version() -> String {
    match Platform::current() {
        Platform::Linux => std::fs::read_to_string("/proc/sys/kernel/osrelease")
            .map(|v| format!(", kernel {}", v.trim()))
            .unwrap_or_default(),
        Platform::MacOs => {
            std::fs::read_to_string("/System/Library/CoreServices/SystemVersion.plist")
                .ok()
                .and_then(|plist| {
                    let after = plist.split("<key>ProductVersion</key>").nth(1)?;
                    let start = after.find("<string>")? + "<string>".len();
                    let end = after[start..].find("</string>")?;
                    Some(format!(", macOS {}", &after[start..start + end]))
                })
                .unwrap_or_default()
        }
        Platform::Windows => String::new(),
    }
}

fn report_path() -> PathBuf {
    std::env::var_os(REPORT_ENV)
        .filter(|p| !p.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            std::env::temp_dir().join(format!("launchdeck-acceptance-{}.md", unix_seconds()))
        })
}

fn unix_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or_default()
}

fn utc_now() -> String {
    utc(unix_seconds())
}

/// `2026-10-06 12:34:56 UTC`, from the days since 1970 (H. Hinnant's
/// civil-from-days).
fn utc(secs: u64) -> String {
    let (days, rest) = (secs / 86_400, secs % 86_400);
    let z = days as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02} {:02}:{:02}:{:02} UTC",
        rest / 3600,
        rest % 3600 / 60,
        rest % 60
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::devices::midi::testkit::{port, FakeMidi};
    use crate::devices::novation::testkit::SimLaunchpad;

    /// The steps that ask nothing, on a simulated Mini MK3 behind fake
    /// ports: the plumbing of the suite, not the hardware.
    #[test]
    fn the_steps_without_questions_run_on_a_simulated_mini() {
        let midi = FakeMidi::default();
        midi.add(
            port("Launchpad Mini MK3 LPMiniMK3 MIDI Out", "2001"),
            port("Launchpad Mini MK3 LPMiniMK3 MIDI In", "2002"),
        );
        let sim = SimLaunchpad::mini().with_layout(0x06);
        midi.bind("2002", "2001", sim.clone());
        let tap = Tap::new(Arc::new(midi.clone()));
        let backend = Arc::new(TapBackend(tap.clone()));
        let mut hardware = NovationHardware::new(backend, Platform::MacOs);
        let mut detection = Step::new(1, "Detection", "Q1, Q16");
        let (unit, naming) = detect(&tap, &mut hardware, Platform::MacOs, &mut detection)
            .expect("the simulated Mini is found");
        assert_eq!((unit.id.as_str(), naming), ("launchpad-mini-mk3:1", None));
        assert_eq!(detection.status(), Status::Pass);

        let mut suite = Suite::new(tap, hardware, unit, Platform::MacOs, None);
        for (number, title, answers, run) in steps::STEPS {
            if ![2, 3, 8].contains(&number) {
                continue;
            }
            let mut step = Step::new(number, title, answers);
            run(&mut suite, &mut step).unwrap();
            assert_eq!(step.status(), Status::Pass, "step {number}");
            assert!(midi.open_links() <= 1, "one link at a time");
        }
        assert!(sim.in_programmer_mode());
        assert_eq!(suite.facts.layout_before, Some(0x06));
        assert_eq!(suite.facts.firmware.as_deref(), Some("0531"));
        suite.finish();
        assert!(!sim.in_programmer_mode(), "given back");
        assert_eq!(sim.layout(), 0x06, "in the layout it showed");
        assert_eq!(midi.open_links(), 0);
    }

    #[test]
    fn dates_are_written_in_utc() {
        assert_eq!(utc(0), "1970-01-01 00:00:00 UTC");
        assert_eq!(utc(951_827_696), "2000-02-29 12:34:56 UTC");
        assert_eq!(utc(1_791_244_800), "2026-10-06 00:00:00 UTC");
    }

    #[test]
    fn the_yes_and_no_buttons_are_lit_over_the_frame() {
        let red = Led {
            color: Rgb::new(255, 0, 0),
            animation: Default::default(),
        };
        let frame = SurfaceFrame {
            leds: vec![
                LedCell { pos: YES, led: red },
                LedCell {
                    pos: ControlPos::grid(0, 0),
                    led: red,
                },
            ],
        };
        let hinted = with_hints(&frame);
        assert_eq!(hinted.leds.len(), 3);
        assert_eq!(hinted.get(YES).map(|l| l.color), Some(Rgb::new(0, 255, 0)));
        assert_eq!(hinted.get(NO).map(|l| l.color), Some(Rgb::new(255, 0, 0)));
        assert_eq!(hinted.get(ControlPos::grid(0, 0)), Some(red));
    }
}
