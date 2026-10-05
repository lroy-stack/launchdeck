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

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::env::Environment;
use super::format::is_bidi;
use crate::actions::builtin::{
    http_request_of, shell_command, HttpRequest, HTTP, HTTP_REQUEST, SHELL, SHELL_EXECUTE,
};
use crate::actions::definition::{Assignment, ProviderKind};
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
            label: label.to_string(),
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
/// the other; a word that is empty or holds a space, a quote or a control
/// character is quoted, so every argument shows where it starts and ends.
fn launch_text(args: &Value, platform: Platform) -> String {
    match system::launch_spec(args, platform) {
        Ok(spec) => std::iter::once(&spec.app)
            .chain(&spec.args)
            .map(|word| shown(word))
            .collect::<Vec<_>>()
            .join(" "),
        Err(refused) => format!(
            "{} (LaunchDeck will not start it: {})",
            shown(text(args, "app")),
            refused.message()
        ),
    }
}

fn shown(word: &str) -> String {
    let plain = !word.is_empty()
        && !word
            .chars()
            .any(|c| c.is_whitespace() || c.is_control() || c == '"' || is_bidi(c));
    if plain {
        word.to_string()
    } else {
        let quoted = Value::from(word).to_string();
        quoted.chars().map(escape_bidi).collect()
    }
}

/// A direction control written as an escape, so it cannot reorder what a
/// person reads.
fn escape_bidi(c: char) -> String {
    if is_bidi(c) {
        format!("\\u{:04x}", u32::from(c))
    } else {
        c.to_string()
    }
}

fn text<'a>(args: &'a Value, name: &str) -> &'a str {
    args.get(name).and_then(Value::as_str).unwrap_or_default()
}

/// What exactly runs: the command, the request, the program and its
/// arguments, the instruction. In full: a person reviews all of it.
fn review_text(kind: ReviewKind, assignment: &Assignment) -> String {
    let args = &assignment.args;
    match kind {
        ReviewKind::RunsCommand => shell_command(args).to_string(),
        ReviewKind::SendsRequest
            if (assignment.provider.as_str(), assignment.action.as_str())
                == (HTTP, HTTP_REQUEST) =>
        {
            let HttpRequest { method, url, body } = http_request_of(args);
            match body {
                Some(body) => format!("{method} {url}\n\n{body}"),
                None => format!("{method} {url}"),
            }
        }
        ReviewKind::SendsRequest => assignment.definition_id(),
        ReviewKind::LaunchesProgram => launch_text(args, Platform::current()),
        ReviewKind::InstructsAgent if assignment.action == AGENT_NEW_TASK => {
            match new_task_of(args) {
                Ok((task, Some(cwd))) => format!("{task} (in {cwd})"),
                Ok((task, None)) => task,
                Err(refused) => format!("LaunchDeck will not send it: {}", refused.message()),
            }
        }
        ReviewKind::InstructsAgent => instruction_of(args).to_string(),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use serde_json::json;

    use super::*;
    use crate::actions::definition::ProviderStatus;
    use crate::artifacts::env::{KnownAction, KnownProvider};
    use crate::artifacts::testkit;

    fn a(provider: &str, action: &str, args: Value) -> Assignment {
        Assignment::new(provider, action, args)
    }

    fn kind(assignment: &Assignment) -> Option<ReviewKind> {
        classify(assignment, &Environment::default())
    }

    /// The ids written here name real built-in actions, with the privileged
    /// flag and the parameters the classification reads; and every
    /// privileged definition of the catalog is a review item.
    #[tokio::test]
    async fn the_ids_are_the_real_catalogs() {
        let env = testkit::environment("9.9.9").await;
        let params = |provider: &str, action: &str| -> (bool, Vec<String>) {
            let def = env
                .action(provider, action)
                .unwrap_or_else(|| panic!("{provider}.{action} is not in the catalog"));
            let names = def.params.iter().map(|p| p.name.clone()).collect();
            (def.privileged, names)
        };
        assert_eq!(params(SHELL, SHELL_EXECUTE), (true, vec!["command".into()]));
        assert_eq!(
            params(HTTP, HTTP_REQUEST),
            (true, vec!["method".into(), "url".into(), "body".into()])
        );
        assert_eq!(
            params(system::ID, system::LAUNCH_APPLICATION),
            (false, vec!["app".into(), "args".into()])
        );
        assert_eq!(
            params(AGENT_CONTROL, AGENT_NEW_TASK),
            (false, vec!["task".into(), "cwd".into()])
        );
        assert_eq!(
            params(AGENT_CONTROL, AGENT_CONTINUE),
            (false, vec!["text".into()])
        );
        for (provider, known) in &env.providers {
            for (action, def) in &known.actions {
                let found = classify(&a(provider, action, json!({})), &env);
                // A launch without an application is one the runtime refuses,
                // and is reviewed as every launch it cannot show to be plain.
                let expected = if def.privileged {
                    true
                } else if provider == system::ID {
                    action == system::LAUNCH_APPLICATION
                } else {
                    provider == AGENT_CONTROL
                        && [AGENT_NEW_TASK, AGENT_CONTINUE].contains(&action.as_str())
                };
                assert_eq!(found.is_some(), expected, "{provider}.{action}");
            }
        }
    }

    #[test]
    fn what_runs_things() {
        use ReviewKind::*;
        assert_eq!(
            kind(&a("shell", "execute", json!({ "command": "ls" }))),
            Some(RunsCommand)
        );
        assert_eq!(
            kind(&a(
                "http",
                "request",
                json!({ "url": "https://example.com" })
            )),
            Some(SendsRequest)
        );
        assert_eq!(
            kind(&a("agent-control", "newTask", json!({ "task": "Test" }))),
            Some(InstructsAgent)
        );
        assert_eq!(
            kind(&a("agent-control", "continue", json!({}))),
            Some(InstructsAgent)
        );
        for quiet in [
            a("agent-control", "approve", json!({})),
            a("navigation", "openSurface", json!({ "surfaceId": "dev" })),
            a("system", "openUrl", json!({ "url": "https://example.com" })),
            a("shell", "other", json!({})),
            a("acme.shell", "execute", json!({ "command": "ls" })),
        ] {
            assert_eq!(kind(&quiet), None, "{quiet:?}");
        }
    }

    #[test]
    fn a_launch_is_reviewed_with_a_path_or_arguments() {
        let launch = |args: Value| kind(&a("system", "launchApplication", args));
        assert_eq!(launch(json!({ "app": "code" })), None);
        assert_eq!(launch(json!({ "app": "Visual Studio Code" })), None);
        assert_eq!(launch(json!({ "app": "code", "args": " \n " })), None);
        assert_eq!(launch(json!({ "app": "code", "args": [] })), None);
        for runs in [
            json!({ "app": "/usr/bin/code" }),
            json!({ "app": "C:\\Tools\\code.exe" }),
            json!({ "app": "code", "args": "--new-window" }),
            json!({ "app": "code", "args": ["--new-window"] }),
        ] {
            assert_eq!(
                launch(runs.clone()),
                Some(ReviewKind::LaunchesProgram),
                "{runs}"
            );
        }
    }

    /// Launches as the runtime would read them, fed to the runtime's parser
    /// and to the review alike: whatever the runtime would pass beyond a
    /// plain application name is reviewed, and the review shows it.
    #[test]
    fn a_launch_is_reviewed_unless_a_plain_name_without_arguments() {
        let launch = |args: &Value| kind(&a("system", "launchApplication", args.clone()));
        for quiet in [
            json!({ "app": "code" }),
            json!({ "app": "Visual Studio Code" }),
            json!({ "app": "x-terminal-emulator" }),
            json!({ "app": "Adobe Photoshop (Beta)" }),
            json!({ "app": "tool_v2.1" }),
            json!({ "app": "code", "args": " \n " }),
            json!({ "app": "code", "args": [] }),
            json!({ "app": "code", "args": null }),
        ] {
            let spec = system::launch_spec(&quiet, Platform::current()).unwrap();
            assert!(spec.args.is_empty(), "{quiet}");
            assert_eq!(launch(&quiet), None, "{quiet}");
        }
        let hostile = [
            // Arguments the runtime passes, however empty.
            json!({ "app": "code", "args": [""] }),
            json!({ "app": "code", "args": [" "] }),
            json!({ "app": "code", "args": ["", " "] }),
            json!({ "app": "code", "args": "--new-window" }),
            // Arguments the runtime refuses.
            json!({ "app": "code", "args": 1 }),
            json!({ "app": "code", "args": { "a": "b" } }),
            json!({ "app": "code", "args": true }),
            json!({ "app": "code", "args": ["--x", 1] }),
            // Names that are not a plain application name.
            json!({ "app": "calc & whoami" }),
            json!({ "app": "calc | more" }),
            json!({ "app": "calc ^& x" }),
            json!({ "app": "%COMSPEC%" }),
            json!({ "app": "\"calc\"" }),
            json!({ "app": "calc > out" }),
            json!({ "app": "calc\nwhoami" }),
            json!({ "app": "calc\u{0}x" }),
            json!({ "app": "~calc" }),
            json!({ "app": "~/bin/calc" }),
            json!({ "app": ".hidden" }),
            json!({ "app": ".." }),
            json!({ "app": "bin\u{2215}calc" }),
            json!({ "app": "bin\u{2044}calc" }),
            json!({ "app": "bin\u{FF0F}calc" }),
            json!({ "app": "bin\u{29F8}calc" }),
            json!({ "app": "/usr/bin/code" }),
            json!({ "app": "C:\\Tools\\code.exe" }),
            json!({ "app": "calc;whoami" }),
            json!({ "app": "$(whoami)" }),
            // No application the runtime would start.
            json!({ "app": " " }),
            json!({ "app": 7 }),
            json!({}),
        ];
        for args in &hostile {
            assert_eq!(
                launch(args),
                Some(ReviewKind::LaunchesProgram),
                "{args} is not reviewed"
            );
        }
        // Every system classifies alike, though Windows refuses more.
        let systems = [Platform::Linux, Platform::MacOs, Platform::Windows];
        for args in hostile.iter() {
            assert!(systems.iter().all(|&p| launches_program(args, p)), "{args}");
        }
        for args in [json!({ "app": "code" }), json!({ "app": "Notes (Old)" })] {
            assert!(
                !systems.iter().any(|&p| launches_program(&args, p)),
                "{args}"
            );
        }
    }

    /// The runtime finds a provider and an action by their exact ids, so a
    /// case variant of an action that runs things is no review item because
    /// it never runs: the one gate every run passes refuses it by its id.
    #[tokio::test]
    async fn a_case_variant_is_neither_reviewed_nor_run() {
        let rt = testkit::headless().await;
        for (provider, action, args) in [
            ("Shell", "execute", json!({ "command": "true" })),
            ("SHELL", "execute", json!({ "command": "true" })),
            ("shell", "Execute", json!({ "command": "true" })),
            (
                "SYSTEM",
                "launchApplication",
                json!({ "app": "/usr/bin/true" }),
            ),
            (
                "System",
                "launchapplication",
                json!({ "app": "/usr/bin/true" }),
            ),
            (
                "system",
                "LaunchApplication",
                json!({ "app": "/usr/bin/true" }),
            ),
            ("HTTP", "request", json!({ "url": "http://127.0.0.1:9" })),
            ("http", "REQUEST", json!({ "url": "http://127.0.0.1:9" })),
            ("Agent-Control", "newTask", json!({ "task": "x" })),
            ("agent-control", "NewTask", json!({ "task": "x" })),
        ] {
            let assignment = a(provider, action, args);
            assert_eq!(kind(&assignment), None, "{provider}.{action}");
            let issues = rt.validate_assignment(&assignment).unwrap_err();
            let message = &issues[0].message;
            let unknown = message.ends_with("is not installed or was removed")
                || message.contains(&format!("has no action `{action}`"));
            assert!(unknown, "{provider}.{action}: {message}");
        }
    }

    #[test]
    fn a_launch_is_reviewed_as_the_runtime_passes_it() {
        let text = |args: Value| {
            let assignment = a("system", "launchApplication", args);
            ReviewItem::new(
                Location {
                    surface_source_id: "dev".into(),
                    page_id: "home".into(),
                    control_id: "g.0.0".into(),
                    slot: Slot::Press,
                },
                "Pad",
                kind(&assignment).unwrap(),
                &assignment,
            )
            .text
        };
        // Every argument the runtime passes is visible, each on its own.
        assert_eq!(
            text(json!({ "app": "code", "args": ["", " ", "a b"] })),
            "code \"\" \" \" \"a b\""
        );
        assert_eq!(
            text(json!({ "app": "Visual Studio Code", "args": "--wait" })),
            "\"Visual Studio Code\" --wait"
        );
        // One the runtime refuses says so.
        assert_eq!(
            text(json!({ "app": "code", "args": 1 })),
            "code (LaunchDeck will not start it: `args` is text, one argument per line)"
        );
        assert_eq!(
            launch_text(&json!({ "app": "calc & whoami" }), Platform::Windows),
            "\"calc & whoami\" (LaunchDeck will not start it: This application name contains \
             characters LaunchDeck will not pass to the Windows command interpreter.)"
        );
        assert_eq!(
            launch_text(&json!({ "app": "calc & whoami" }), Platform::Linux),
            "\"calc & whoami\""
        );
        assert_eq!(
            launch_text(&json!({ "app": "c\u{202E}exe.txt" }), Platform::Linux),
            "\"c\\u202eexe.txt\""
        );
    }

    #[test]
    fn a_privileged_host_definition_sends_a_request_and_a_plugin_never_does() {
        let provider = |kind: ProviderKind| KnownProvider {
            kind,
            name: "Future".into(),
            version: "1.0.0".into(),
            status: ProviderStatus::Ready,
            actions: BTreeMap::from([(
                "send".to_string(),
                KnownAction {
                    params: vec![],
                    privileged: true,
                },
            )]),
        };
        let env = Environment {
            providers: BTreeMap::from([
                ("future".to_string(), provider(ProviderKind::Builtin)),
                ("acme.future".to_string(), provider(ProviderKind::Plugin)),
            ]),
            ..Environment::default()
        };
        assert_eq!(
            classify(&a("future", "send", json!({})), &env),
            Some(ReviewKind::SendsRequest)
        );
        assert_eq!(classify(&a("future", "other", json!({})), &env), None);
        assert_eq!(classify(&a("acme.future", "send", json!({})), &env), None);
    }

    #[test]
    fn the_text_is_what_runs() {
        let item = |assignment: Assignment| {
            let kind = kind(&assignment).unwrap();
            let at = Location {
                surface_source_id: "dev".into(),
                page_id: "home".into(),
                control_id: "g.0.0".into(),
                slot: Slot::Press,
            };
            ReviewItem::new(at, "Pad", kind, &assignment).text
        };
        assert_eq!(
            item(a(
                "shell",
                "execute",
                json!({ "command": "git pull\nmake" })
            )),
            "git pull\nmake"
        );
        assert_eq!(
            item(a(
                "http",
                "request",
                json!({ "method": "POST", "url": "https://example.com/hook", "body": "{\"a\":1}" })
            )),
            "POST https://example.com/hook\n\n{\"a\":1}"
        );
        assert_eq!(
            item(a(
                "http",
                "request",
                json!({ "url": "https://example.com" })
            )),
            "GET https://example.com"
        );
        assert_eq!(
            item(a(
                "system",
                "launchApplication",
                json!({ "app": "code", "args": "--new-window\n\n/tmp/x\n" })
            )),
            "code --new-window /tmp/x"
        );
        assert_eq!(
            item(a(
                "system",
                "launchApplication",
                json!({ "app": "/opt/x", "args": ["-a", "b"] })
            )),
            "/opt/x -a b"
        );
        assert_eq!(
            item(a(
                "agent-control",
                "newTask",
                json!({ "task": "Run the tests", "cwd": "/src" })
            )),
            "Run the tests (in /src)"
        );
        assert_eq!(
            item(a(
                "agent-control",
                "newTask",
                json!({ "task": "Run the tests" })
            )),
            "Run the tests"
        );
        assert_eq!(item(a("agent-control", "continue", json!({}))), "continue");
        assert_eq!(
            item(a("agent-control", "continue", json!({ "text": "go on" }))),
            "go on"
        );
    }

    /// The text comes from the functions the runtime reads the arguments
    /// with: what a person reviews is what would run, not the raw value.
    #[test]
    fn the_text_is_read_as_the_runtime_reads_it() {
        let item = |provider: &str, action: &str, args: Value| {
            let assignment = a(provider, action, args);
            let kind = kind(&assignment).unwrap();
            let at = Location {
                surface_source_id: "dev".into(),
                page_id: "home".into(),
                control_id: "g.0.0".into(),
                slot: Slot::Press,
            };
            ReviewItem::new(at, "Pad", kind, &assignment).text
        };
        // The shell runs the command trimmed.
        assert_eq!(
            item("shell", "execute", json!({ "command": "  make\n " })),
            "make"
        );
        // The method in capitals, GET when blank; a blank body is not sent.
        assert_eq!(
            item(
                "http",
                "request",
                json!({ "method": "post", "url": "https://example.com/h", "body": " " })
            ),
            "POST https://example.com/h"
        );
        assert_eq!(
            item(
                "http",
                "request",
                json!({ "method": "", "url": "https://example.com/h" })
            ),
            "GET https://example.com/h"
        );
        // A blank instruction continues; a blank folder is no folder.
        assert_eq!(
            item("agent-control", "continue", json!({ "text": "  " })),
            "continue"
        );
        assert_eq!(
            item(
                "agent-control",
                "newTask",
                json!({ "task": "Report", "cwd": " " })
            ),
            "Report"
        );
        assert_eq!(
            item("agent-control", "newTask", json!({ "cwd": "/src" })),
            "LaunchDeck will not send it: `task` (string) is required"
        );
    }

    #[test]
    fn a_review_item_is_written_in_camel_case() {
        let at = Location {
            surface_source_id: "dev".into(),
            page_id: "home".into(),
            control_id: "g.0.0".into(),
            slot: Slot::LongPress,
        };
        let item = ReviewItem::new(
            at.clone(),
            "Deploy",
            ReviewKind::RunsCommand,
            &a("shell", "execute", json!({ "command": "make deploy" })),
        );
        assert_eq!(
            serde_json::to_value(&item).unwrap(),
            json!({
                "at": { "surfaceSourceId": "dev", "pageId": "home", "controlId": "g.0.0", "slot": "longPress" },
                "label": "Deploy",
                "kind": "runsCommand",
                "text": "make deploy"
            })
        );
        let back: Location = serde_json::from_value(serde_json::to_value(&at).unwrap()).unwrap();
        assert_eq!(back, at);
        assert_eq!(Slot::LongPress.member(), "longPress");
        assert!(serde_json::from_value::<Location>(json!({
            "surfaceSourceId": "dev", "pageId": "home", "controlId": "c", "slot": "press", "extra": 1
        }))
        .is_err());
    }
}
