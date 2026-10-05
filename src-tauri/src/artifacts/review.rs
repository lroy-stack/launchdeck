//! Actions that run things (plan 7.15): which assignments a person reviews
//! before a file is shared or imported, and the text they review.
//!
//! Three kinds run something on the computer or the network: a shell
//! command, a request (`http.request`, and any other host definition marked
//! privileged), a program launched by path or with arguments. The fourth,
//! an instruction to an agent, is shown and never needs a decision. Plugin
//! actions are never review items: a plugin cannot be privileged, and its
//! network access stays behind the grants a person gives.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::env::Environment;
use crate::actions::definition::{Assignment, ProviderKind};
use crate::plugins::system;

// The built-in providers that run things. `artifacts` may not import their
// owners (`actions::builtin`, `agents`); a test holds these to the real catalog.
const SHELL: &str = "shell";
const SHELL_EXECUTE: &str = "execute";
const HTTP: &str = "http";
const HTTP_REQUEST: &str = "request";
const AGENT_CONTROL: &str = "agent-control";
const AGENT_NEW_TASK: &str = "newTask";
const AGENT_CONTINUE: &str = "continue";

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
            launches_program(&assignment.args).then_some(ReviewKind::LaunchesProgram)
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

/// A launch by path (`/` or `\` in the application) or with arguments.
fn launches_program(args: &Value) -> bool {
    let by_path = text(args, "app").contains(['/', '\\']);
    by_path || !launch_args(args).is_empty()
}

/// The launch arguments, one per line or as a list, without empty ones.
fn launch_args(args: &Value) -> Vec<String> {
    match args.get("args") {
        Some(Value::String(lines)) => lines
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .map(String::from)
            .collect(),
        Some(Value::Array(list)) => list
            .iter()
            .filter_map(Value::as_str)
            .filter(|a| !a.trim().is_empty())
            .map(String::from)
            .collect(),
        _ => Vec::new(),
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
        ReviewKind::RunsCommand => text(args, "command").to_string(),
        ReviewKind::SendsRequest if assignment.provider == HTTP => {
            let method = Some(text(args, "method"))
                .filter(|m| !m.is_empty())
                .unwrap_or("GET");
            let request = format!("{method} {}", text(args, "url"));
            match text(args, "body").chars().count() {
                0 => request,
                n => format!("{request} (a body of {n} characters)"),
            }
        }
        ReviewKind::SendsRequest => assignment.definition_id(),
        ReviewKind::LaunchesProgram => std::iter::once(text(args, "app").to_string())
            .chain(launch_args(args))
            .collect::<Vec<_>>()
            .join(" "),
        ReviewKind::InstructsAgent if assignment.action == AGENT_NEW_TASK => {
            match text(args, "cwd") {
                "" => text(args, "task").to_string(),
                cwd => format!("{} (in {cwd})", text(args, "task")),
            }
        }
        ReviewKind::InstructsAgent => match text(args, "text") {
            "" => AGENT_CONTINUE.to_string(),
            instruction => instruction.to_string(),
        },
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
                let expected = if def.privileged {
                    true
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
            "POST https://example.com/hook (a body of 7 characters)"
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
