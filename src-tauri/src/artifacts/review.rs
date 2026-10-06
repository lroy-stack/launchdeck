//! Actions that run things (plan 7.15): which assignments a person reviews
//! before a file is shared or imported, and the text they review.
//!
//! Three kinds run something on the computer or the network: a shell
//! command, a request (`http.request`, and any other host definition marked
//! privileged), and any launch but one of a plain application name without
//! arguments, read with the runtime's own parser. The fourth,
//! an instruction to an agent, is shown and never needs a decision. Plugin
//! actions are never review items: a plugin cannot be privileged, and its
//! network access stays behind the grants a person gives.

use std::fmt::Write as _;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::env::Environment;
use super::format::{assignments, echo, AssignmentV1, ContentV1, Rejection, RejectionCode};
use super::json::segment;
use crate::actions::builtin::{
    http_request_of, request_destination, shell_command, HttpRequest, HTTP, HTTP_REQUEST, SHELL,
    SHELL_EXECUTE,
};
use crate::actions::definition::{Assignment, ProviderKind};
use crate::actions::params::hidden_char as hidden;
use crate::agents::plugin::{
    instruction_of, new_task_of, CONTINUE as AGENT_CONTINUE, NEW_TASK as AGENT_NEW_TASK,
    PLUGIN_ID as AGENT_CONTROL,
};
use crate::platform::Platform;
use crate::plugins::system;

/// Which of a control's two assignments.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Slot {
    Press,
    LongPress,
}

impl Slot {
    /// The member name of the slot in a file.
    pub fn member(self) -> &'static str {
        match self {
            Slot::Press => "press",
            Slot::LongPress => "longPress",
        }
    }
}

/// A press or a long press in a file: the surface by the id the file gives
/// it, then the page and the control by their own ids.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Location {
    pub surface_source_id: String,
    pub page_id: String,
    pub control_id: String,
    pub slot: Slot,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ReviewKind {
    RunsCommand,
    SendsRequest,
    LaunchesProgram,
    InstructsAgent,
}

/// One assignment a person looks at before sharing or importing: where it
/// is, the control's label, what kind of thing it runs and what exactly.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewItem {
    pub at: Location,
    pub label: String,
    pub kind: ReviewKind,
    pub text: String,
}

impl ReviewItem {
    pub fn new(at: Location, label: &str, kind: ReviewKind, assignment: &Assignment) -> ReviewItem {
        ReviewItem {
            at,
            label: visible(label, false),
            kind,
            text: review_text(kind, assignment),
        }
    }
}

/// Whether `assignment` runs something a person should review, and what.
pub fn classify(assignment: &Assignment, env: &Environment) -> Option<ReviewKind> {
    let (provider, action) = (assignment.provider.as_str(), assignment.action.as_str());
    match (provider, action) {
        (SHELL, SHELL_EXECUTE) => Some(ReviewKind::RunsCommand),
        (HTTP, HTTP_REQUEST) => Some(ReviewKind::SendsRequest),
        (system::ID, system::LAUNCH_APPLICATION) => {
            launches_program(&assignment.args, Platform::current())
                .then_some(ReviewKind::LaunchesProgram)
        }
        (AGENT_CONTROL, AGENT_NEW_TASK | AGENT_CONTINUE) => Some(ReviewKind::InstructsAgent),
        _ => {
            let privileged = env
                .provider(provider)
                .filter(|p| p.kind != ProviderKind::Plugin)
                .and_then(|p| p.actions.get(action))
                .is_some_and(|def| def.privileged);
            privileged.then_some(ReviewKind::SendsRequest)
        }
    }
}

/// Every launch but one of a plain application name with no argument, read
/// by the runtime's own parser: what the review sees is what would start.
/// A launch the runtime refuses is reviewed too. The answer is the same on
/// every system: what Windows alone refuses is never a plain name without
/// arguments.
fn launches_program(args: &Value, platform: Platform) -> bool {
    system::launch_spec(args, platform).map_or(true, |spec| {
        !plain_application(&spec.app) || !spec.args.is_empty()
    })
}

/// A name the system looks an application up by, never a path: ASCII
/// letters and digits, space, dot, underscore, hyphen and parentheses, not
/// starting with a dot. No separator of any system, no look-alike of one,
/// no home directory, nothing a command interpreter reads.
fn plain_application(app: &str) -> bool {
    !app.is_empty()
        && !app.starts_with('.')
        && app
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, ' ' | '.' | '_' | '-' | '(' | ')'))
}

/// The application and each argument as the runtime passes them, one after
/// the other, each a [`word`].
fn launch_text(args: &Value, platform: Platform) -> String {
    match system::launch_spec(args, platform) {
        Ok(spec) => std::iter::once(&spec.app)
            .chain(&spec.args)
            .map(|w| word(w))
            .collect::<Vec<_>>()
            .join(" "),
        Err(refused) => format!(
            "{} (LaunchDeck will not start it: {})",
            word(text(args, "app")),
            visible(&refused.message(), false)
        ),
    }
}

/// A launch word as it is passed: as it is when plain, otherwise quoted,
/// its quotes and backslashes escaped, so every argument shows where it
/// starts and ends, an empty one included.
fn word(w: &str) -> String {
    let plain = !w.is_empty()
        && !w
            .chars()
            .any(|c| c.is_whitespace() || c == '"' || hidden(c));
    if plain {
        return w.to_string();
    }
    let mut out = String::from("\"");
    for c in w.chars() {
        match c {
            '"' | '\\' => {
                out.push('\\');
                out.push(c);
            }
            c => push_visible(&mut out, c, false),
        }
    }
    out.push('"');
    out
}

/// The one escaping of every review text (SF-3): each [`hidden`] character
/// is written as an escape (`\r`, `\u001b`, `\u202e`), so nothing in a text
/// hides or reorders what a person reads. A multi-line text keeps its line
/// breaks and tabs, with long runs of blank lines [`counted`]; a one-line
/// text escapes them too.
fn visible(text: &str, multi_line: bool) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        push_visible(&mut out, c, multi_line);
    }
    if multi_line {
        counted(&out)
    } else {
        out
    }
}

/// Blank lines a review text shows as they are, one after another.
const SHOWN_BLANK_LINES: usize = 2;

/// `text` with each run of more than [`SHOWN_BLANK_LINES`] blank lines
/// (nothing but spaces and tabs) written as one line that says how many,
/// `[1899 blank lines]`: line breaks cannot push the rest of a command or
/// an instruction out of view below its first line.
fn counted(text: &str) -> String {
    let mut lines: Vec<String> = Vec::new();
    let mut blank: Vec<&str> = Vec::new();
    for line in text.split('\n') {
        if line.trim().is_empty() {
            blank.push(line);
            continue;
        }
        lines.extend(shown_blank(std::mem::take(&mut blank)));
        lines.push(line.to_string());
    }
    lines.extend(shown_blank(blank));
    lines.join("\n")
}

fn shown_blank(run: Vec<&str>) -> Vec<String> {
    if run.len() > SHOWN_BLANK_LINES {
        vec![format!("[{} blank lines]", run.len())]
    } else {
        run.into_iter().map(str::to_string).collect()
    }
}

fn push_visible(out: &mut String, c: char, multi_line: bool) {
    match c {
        '\n' | '\t' if multi_line => out.push(c),
        '\n' => out.push_str("\\n"),
        '\t' => out.push_str("\\t"),
        '\r' => out.push_str("\\r"),
        c if hidden(c) => {
            let _ = write!(out, "\\u{:04x}", u32::from(c));
        }
        c => out.push(c),
    }
}

fn text<'a>(args: &'a Value, name: &str) -> &'a str {
    args.get(name).and_then(Value::as_str).unwrap_or_default()
}

/// What exactly runs: the command, the request, the program and its
/// arguments, the instruction. In full: a person reviews all of it, every
/// hidden character escaped.
fn review_text(kind: ReviewKind, assignment: &Assignment) -> String {
    let args = &assignment.args;
    match kind {
        ReviewKind::RunsCommand => visible(shell_command(args), true),
        ReviewKind::SendsRequest
            if (assignment.provider.as_str(), assignment.action.as_str())
                == (HTTP, HTTP_REQUEST) =>
        {
            let HttpRequest { method, url, body } = http_request_of(args);
            let destination = match request_destination(&url) {
                Ok(to) => {
                    let goes = format!("goes to {}:{}", visible(&to.host, false), to.port);
                    match to.nearby() {
                        Some(place) => format!("{goes}, {place}"),
                        None => goes,
                    }
                }
                Err(refused) => format!(
                    "LaunchDeck will not send it: {}",
                    visible(&refused.message(), false)
                ),
            };
            let request = format!(
                "{} {} ({destination})",
                visible(&method, false),
                shown_address(&url)
            );
            match body {
                Some(body) => format!("{request}\n\n{}", visible(&body, true)),
                None => request,
            }
        }
        ReviewKind::SendsRequest => format!(
            "{} {}",
            visible(&assignment.definition_id(), false),
            visible(&assignment.args.to_string(), false)
        ),
        ReviewKind::LaunchesProgram => launch_text(args, Platform::current()),
        ReviewKind::InstructsAgent if assignment.action == AGENT_NEW_TASK => {
            match new_task_of(args) {
                Ok((task, Some(cwd))) => {
                    format!("{} (in {})", visible(&task, true), visible(&cwd, false))
                }
                Ok((task, None)) => visible(&task, true),
                Err(refused) => format!(
                    "LaunchDeck will not send it: {}",
                    visible(&refused.message(), false)
                ),
            }
        }
        ReviewKind::InstructsAgent => visible(instruction_of(args), true),
    }
}

/// An address as a person reviews it: escaped, and when its host is written
/// in other scripts, followed by the host the request goes to in punycode
/// (SF-3b), so a look-alike of a known name shows.
fn shown_address(address: &str) -> String {
    let shown = visible(address, false);
    if address.is_ascii() {
        return shown;
    }
    let host = url::Url::parse(address)
        .ok()
        .and_then(|u| u.host_str().map(str::to_string))
        .filter(|host| !address.contains(host.as_str()));
    match host {
        Some(host) => format!("{shown} (host {})", visible(&host, false)),
        None => shown,
    }
}

/// The actions whose arguments say what runs: a shell command, a request,
/// a launch, an instruction to an agent.
fn runs_things(provider: &str, action: &str) -> bool {
    matches!(
        (provider, action),
        (SHELL, SHELL_EXECUTE)
            | (HTTP, HTTP_REQUEST)
            | (system::ID, system::LAUNCH_APPLICATION)
            | (AGENT_CONTROL, AGENT_NEW_TASK | AGENT_CONTINUE)
    )
}

/// A file in which an action that runs things holds a [`hidden`] character
/// in an argument, at any depth, is refused at that argument, naming the
/// character (SF-3): the review would have to show an escape where the
/// action runs the character.
/// So is a request whose address the runtime would send elsewhere than a
/// person is shown (SF-4).
pub(crate) fn check_visible(content: &ContentV1) -> Result<(), Rejection> {
    for (at, a) in assignments(content).filter(|(_, a)| runs_things(&a.provider, &a.action)) {
        let found = a
            .args
            .iter()
            .find_map(|(name, value)| hidden_in(value).map(|c| (name, c)));
        if let Some((name, c)) = found {
            return Err(Rejection::at(
                RejectionCode::InvalidContent,
                format!("{at}/args/{}", segment(name)),
                format!(
                    "An action that runs something holds, in \"{}\", the character U+{:04X}, \
                     which a person cannot see or which reorders the text around it.",
                    echo(name),
                    u32::from(c)
                ),
            ));
        }
        check_address(&at, a)?;
    }
    Ok(())
}

/// The address of a request written in the file: the runtime's own reading
/// must accept it, whatever its scheme, so no file carries an address the
/// runtime and the review would read apart. An address asked of the
/// importer is checked once it is given; a blank one (a pad not set up yet)
/// sends nothing.
fn check_address(at: &str, a: &AssignmentV1) -> Result<(), Rejection> {
    const URL: &str = "url";
    if (a.provider.as_str(), a.action.as_str()) != (HTTP, HTTP_REQUEST)
        || a.inputs.contains_key(URL)
    {
        return Ok(());
    }
    let Some(Value::String(url)) = a.args.get(URL) else {
        return Ok(());
    };
    if url.trim().is_empty() {
        return Ok(());
    }
    request_destination(url).map(|_| ()).map_err(|refused| {
        Rejection::at(
            RejectionCode::InvalidContent,
            format!("{at}/args/{URL}"),
            refused.message(),
        )
    })
}

/// The first [`hidden`] character of any text in `value`, at any depth.
fn hidden_in(value: &Value) -> Option<char> {
    match value {
        Value::String(s) => s.chars().find(|&c| hidden(c)),
        Value::Array(items) => items.iter().find_map(hidden_in),
        Value::Object(map) => map.values().find_map(hidden_in),
        _ => None,
    }
}

#[cfg(test)]
#[path = "review_tests.rs"]
mod tests;
