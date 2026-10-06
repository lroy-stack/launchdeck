//! The suite's report: printed as it goes, and written as Markdown after
//! every step, so that a run cut short still leaves one.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::time::Duration;

use super::tap::{Dir, Entry};

/// Transcript lines kept per step; the rest are counted.
const TRANSCRIPT_MAX: usize = 60;

/// What one check of a step came to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Verdict {
    Pass,
    Fail,
    /// Not answered in time, or not run.
    Skipped,
    /// An answer or an observation with nothing expected of it.
    Noted(String),
}

impl Verdict {
    fn label(&self) -> String {
        match self {
            Verdict::Pass => "pass".into(),
            Verdict::Fail => "FAIL".into(),
            Verdict::Skipped => "skipped".into(),
            Verdict::Noted(what) => format!("noted: {what}"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Status {
    Pass,
    Fail,
    Skipped,
}

impl Status {
    pub(super) fn label(self) -> &'static str {
        match self {
            Status::Pass => "pass",
            Status::Fail => "FAIL",
            Status::Skipped => "skipped",
        }
    }
}

#[derive(Debug)]
struct Check {
    what: String,
    verdict: Verdict,
}

/// One step of the suite: what it said, what it checked, the bytes.
#[derive(Debug)]
pub(super) struct Step {
    pub number: u8,
    pub title: &'static str,
    /// The open questions of the plan this step answers.
    answers: &'static str,
    lines: Vec<String>,
    checks: Vec<Check>,
    transcript: Vec<Entry>,
}

impl Step {
    pub(super) fn new(number: u8, title: &'static str, answers: &'static str) -> Step {
        println!("\n== Step {number}. {title} (answers {answers})");
        Step {
            number,
            title,
            answers,
            lines: Vec::new(),
            checks: Vec::new(),
            transcript: Vec::new(),
        }
    }

    pub(super) fn say(&mut self, line: impl Into<String>) {
        let line = line.into();
        println!("   {line}");
        self.lines.push(line);
    }

    pub(super) fn check(&mut self, what: impl Into<String>, verdict: Verdict) {
        let what = what.into();
        println!("   [{}] {what}", verdict.label());
        self.checks.push(Check { what, verdict });
    }

    pub(super) fn pass_if(&mut self, what: impl Into<String>, ok: bool) {
        self.check(what, if ok { Verdict::Pass } else { Verdict::Fail });
    }

    pub(super) fn note(&mut self, what: impl Into<String>, observed: impl Into<String>) {
        self.check(what, Verdict::Noted(observed.into()));
    }

    /// Fail if any check failed; skipped if nothing passed or was noted.
    pub(super) fn status(&self) -> Status {
        let verdicts = || self.checks.iter().map(|c| &c.verdict);
        if verdicts().any(|v| *v == Verdict::Fail) {
            Status::Fail
        } else if verdicts().all(|v| *v == Verdict::Skipped) {
            Status::Skipped
        } else {
            Status::Pass
        }
    }

    pub(super) fn set_transcript(&mut self, entries: Vec<Entry>) {
        for line in transcript_lines(&entries) {
            println!("   {line}");
        }
        self.transcript = entries;
    }

    fn markdown(&self, out: &mut String) {
        let _ = writeln!(
            out,
            "\n## Step {}. {} ({})\n\nAnswers: {}.\n",
            self.number,
            self.title,
            self.status().label(),
            self.answers
        );
        for line in &self.lines {
            let _ = writeln!(out, "    {line}");
        }
        if !self.checks.is_empty() {
            out.push('\n');
        }
        for check in &self.checks {
            let _ = writeln!(out, "- [{}] {}", check.verdict.label(), check.what);
        }
        if !self.transcript.is_empty() {
            let _ = writeln!(out, "\nTranscript (`->` sent, `<-` received):\n\n```");
            for line in transcript_lines(&self.transcript) {
                let _ = writeln!(out, "{line}");
            }
            out.push_str("```\n");
        }
    }
}

pub(super) struct Report {
    path: PathBuf,
    head: Vec<String>,
    steps: Vec<Step>,
    summary: Vec<String>,
}

impl Report {
    pub(super) fn new(path: PathBuf) -> Report {
        Report {
            path,
            head: Vec::new(),
            steps: Vec::new(),
            summary: Vec::new(),
        }
    }

    pub(super) fn path(&self) -> &Path {
        &self.path
    }

    pub(super) fn head(&mut self, line: impl Into<String>) {
        let line = line.into();
        println!("{line}");
        self.head.push(line);
    }

    pub(super) fn add(&mut self, step: Step) {
        println!("   -> step {}: {}", step.number, step.status().label());
        self.steps.push(step);
        self.write();
    }

    pub(super) fn steps(&self) -> &[Step] {
        &self.steps
    }

    pub(super) fn summary(&mut self, line: impl Into<String>) {
        let line = line.into();
        println!("{line}");
        self.summary.push(line);
    }

    /// Writes the report; a failure is printed, never fatal.
    pub(super) fn write(&self) {
        if let Err(e) = std::fs::write(&self.path, self.markdown()) {
            println!("could not write the report to {}: {e}", self.path.display());
        }
    }

    fn markdown(&self) -> String {
        let mut out = String::from("# LaunchDeck Launchpad acceptance report\n\n");
        for line in &self.head {
            let _ = writeln!(out, "{line}  ");
        }
        for step in &self.steps {
            step.markdown(&mut out);
        }
        if !self.summary.is_empty() {
            out.push_str("\n## Step 15. Summary\n\n");
            for line in &self.summary {
                let _ = writeln!(out, "{line}");
            }
        }
        out
    }
}

/// `+12.345 s -> F0 7E 7F 06 01 F7`, at most `TRANSCRIPT_MAX` lines.
fn transcript_lines(entries: &[Entry]) -> Vec<String> {
    let mut lines: Vec<String> = entries
        .iter()
        .take(TRANSCRIPT_MAX)
        .map(|e| {
            let arrow = match e.dir {
                Dir::Sent => "->",
                Dir::Received => "<-",
            };
            format!("{} {arrow} {}", seconds(e.at), hex(&e.bytes))
        })
        .collect();
    if entries.len() > TRANSCRIPT_MAX {
        lines.push(format!(
            "… and {} more messages",
            entries.len() - TRANSCRIPT_MAX
        ));
    }
    lines
}

fn seconds(d: Duration) -> String {
    format!("+{:.3} s", d.as_secs_f64())
}

/// Bytes as the manuals print them: `F0 00 20 29`.
pub(super) fn hex(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|b| format!("{b:02X}"))
        .collect::<Vec<_>>()
        .join(" ")
}

pub(super) fn millis(d: Duration) -> String {
    format!("{:.2} ms", d.as_secs_f64() * 1e3)
}
