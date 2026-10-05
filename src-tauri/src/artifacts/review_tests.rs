//! The review: what runs things, what a person reads, and what a file may
//! not hide, each against the runtime's own readers.

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
        "POST https://example.com/hook (to example.com, port 443)\n\n{\"a\":1}"
    );
    assert_eq!(
        item(a(
            "http",
            "request",
            json!({ "url": "https://example.com" })
        )),
        "GET https://example.com (to example.com, port 443)"
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
        "POST https://example.com/h (to example.com, port 443)"
    );
    assert_eq!(
        item(
            "http",
            "request",
            json!({ "method": "", "url": "https://example.com/h" })
        ),
        "GET https://example.com/h (to example.com, port 443)"
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

/// Characters a person cannot see, or that reorder what they read.
const HIDDEN: [char; 17] = [
    // Direction controls, controls, separators.
    '\u{202E}',
    '\u{2066}',
    '\u{7}',
    '\r',
    '\u{1b}',
    '\u{85}',
    '\u{2028}',
    // Other default-ignorable characters: zero-width space and joiner,
    // byte order mark, soft hyphen, Arabic letter mark, word joiner,
    // variation selector, Hangul filler, a tag, a high variation selector.
    '\u{200B}',
    '\u{200D}',
    '\u{FEFF}',
    '\u{AD}',
    '\u{61C}',
    '\u{2060}',
    '\u{FE0F}',
    '\u{3164}',
    '\u{E0041}',
    '\u{E0100}',
];

/// Every kind of review text, for a value holding `c`: the assignment
/// and the argument it is in.
fn every_kind(c: char) -> Vec<(Assignment, &'static str)> {
    let v = format!("x{c}y");
    vec![
        (a("shell", "execute", json!({ "command": v })), "command"),
        (
            a(
                "http",
                "request",
                json!({ "url": format!("https://e.com/{v}") }),
            ),
            "url",
        ),
        (
            a(
                "http",
                "request",
                json!({ "url": "https://e.com", "body": v }),
            ),
            "body",
        ),
        (a("system", "launchApplication", json!({ "app": v })), "app"),
        (
            a(
                "system",
                "launchApplication",
                json!({ "app": "code", "args": [v] }),
            ),
            "args",
        ),
        (a("agent-control", "newTask", json!({ "task": v })), "task"),
        (
            a("agent-control", "newTask", json!({ "task": "t", "cwd": v })),
            "cwd",
        ),
        (a("agent-control", "continue", json!({ "text": v })), "text"),
    ]
}

/// One escaping for every review text: a control or direction character
/// is written as an escape wherever it is, so nothing hides or reorders
/// what a person reads. Line breaks and tabs of a multi-line text stay.
#[test]
fn no_review_text_holds_a_hidden_character() {
    for c in HIDDEN {
        for (assignment, arg) in every_kind(c) {
            let kind = kind(&assignment).unwrap();
            let at = Location {
                surface_source_id: "dev".into(),
                page_id: "home".into(),
                control_id: "g.0.0".into(),
                slot: Slot::Press,
            };
            let item = ReviewItem::new(at, &format!("Pad{c}"), kind, &assignment);
            for shown in [&item.text, &item.label] {
                assert!(
                    !shown.contains(c),
                    "{c:?} in {arg} of {}: {shown:?}",
                    assignment.definition_id()
                );
            }
        }
    }
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
            json!({ "command": "a\u{202E}b\r\n\tc\u{1b}[2J" })
        )),
        "a\\u202eb\\r\n\tc\\u001b[2J"
    );
    assert_eq!(
        item(a(
            "system",
            "launchApplication",
            json!({ "app": "code", "args": ["a\tb", "q\"\\"] })
        )),
        "code \"a\\tb\" \"q\\\"\\\\\""
    );
}

/// The format's direction controls are among the hidden characters.
#[test]
fn every_direction_control_is_hidden() {
    let all = (0..=0x10FFFF).filter_map(char::from_u32);
    assert!(all
        .filter(|&c| crate::artifacts::format::is_bidi(c))
        .all(hidden));
}

/// A host written in other scripts is also shown as the name the
/// request goes to, so a look-alike of a known name shows.
#[test]
fn an_address_in_other_scripts_shows_its_host_in_punycode() {
    let text = |url: &str| {
        let assignment = a("http", "request", json!({ "url": url }));
        let kind = kind(&assignment).unwrap();
        let at = Location {
            surface_source_id: "dev".into(),
            page_id: "home".into(),
            control_id: "g.0.0".into(),
            slot: Slot::Press,
        };
        ReviewItem::new(at, "Pad", kind, &assignment).text
    };
    let refused = "(LaunchDeck will not send it: The address is read differently by the HTTP \
                   client and by a browser.)";
    assert_eq!(
        text("https://\u{430}pple.com/login"),
        format!("GET https://\u{430}pple.com/login (host xn--pple-43d.com) {refused}")
    );
    assert_eq!(
        text("https://xn--pple-43d.com/"),
        "GET https://xn--pple-43d.com/ (to xn--pple-43d.com, port 443)"
    );
    // Only the host decides: a path in other scripts goes where it says.
    assert_eq!(
        text("https://example.com/caf\u{e9}"),
        "GET https://example.com/caf\u{e9} (to example.com, port 443)"
    );
}

/// SF-4: the review states where a request goes, as the runtime would
/// send it, and a host definition marked privileged shows every argument.
#[test]
fn a_request_states_its_destination() {
    let text = |assignment: Assignment, env: &Environment| {
        let kind = classify(&assignment, env).unwrap();
        let at = Location {
            surface_source_id: "dev".into(),
            page_id: "home".into(),
            control_id: "g.0.0".into(),
            slot: Slot::Press,
        };
        ReviewItem::new(at, "Pad", kind, &assignment).text
    };
    let env = Environment::default();
    let request = |url: &str| a("http", "request", json!({ "url": url }));
    assert_eq!(
        text(request("https://evil.example#@trusted.example"), &env),
        "GET https://evil.example#@trusted.example (to evil.example, port 443)"
    );
    assert_eq!(
        text(request("https://trusted.example@evil.example/"), &env),
        "GET https://trusted.example@evil.example/ (LaunchDeck will not send it: The address \
         carries a user name or password, which LaunchDeck does not send.)"
    );
    // A destination on this computer or the local network says so (SF-5).
    assert_eq!(
        text(
            request("http://homeassistant.local:8123/api/webhook/x"),
            &env
        ),
        "GET http://homeassistant.local:8123/api/webhook/x (to homeassistant.local, port 8123, \
         on your local network)"
    );
    assert_eq!(
        text(request("http://127.0.0.1:9000/"), &env),
        "GET http://127.0.0.1:9000/ (to 127.0.0.1, port 9000, on this computer)"
    );
    let privileged = KnownProvider {
        kind: crate::actions::definition::ProviderKind::Builtin,
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
        providers: BTreeMap::from([("future".to_string(), privileged)]),
        ..Environment::default()
    };
    assert_eq!(
        text(
            a("future", "send", json!({ "to": "x", "secret\u{200B}": 1 })),
            &env
        ),
        "future.send {\"secret\\u200b\":1,\"to\":\"x\"}"
    );
}

/// SF-4: a file whose request would go elsewhere than the review shows is
/// refused at the address; an address asked of the importer, or one the
/// runtime refuses anyway (not http), is left to the import's own checks.
#[test]
fn a_request_address_read_two_ways_is_refused() {
    let file = |args: Value, inputs: &[(&str, &str)]| {
        let mut assignment =
            crate::artifacts::format::AssignmentV1::from_model(&a("http", "request", args));
        for (arg, input) in inputs {
            assignment.inputs.insert(arg.to_string(), input.to_string());
        }
        let content = json!({ "surfaces": [ { "id": "s", "name": "S", "category": "custom",
            "pages": [ { "id": "home", "name": "Home", "controls": [
                { "id": "g.0.0", "pos": { "kind": "grid", "col": 0, "row": 0 }, "label": "Pad",
                  "visual": { "color": "#ff0000" }, "press": assignment } ] } ] } ] });
        serde_json::from_value::<ContentV1>(content).unwrap()
    };
    let at = "/content/surfaces/0/pages/0/controls/0/press/args/url";
    for url in [
        "https://trusted.example@evil.example/",
        "https://trusted.example\\@evil.example/",
        "http://0x7f.1/",
        "https://\u{430}pple.com/",
    ] {
        let refused = check_visible(&file(json!({ "url": url }), &[])).unwrap_err();
        assert_eq!(
            (refused.code, refused.at.as_deref()),
            (crate::artifacts::RejectionCode::InvalidContent, Some(at)),
            "{url}"
        );
    }
    for (url, inputs) in [
        ("https://example.com/hook", &[][..]),
        ("", &[("url", "hook")][..]),
        ("", &[][..]),
        ("mailto:me@example.com", &[][..]),
    ] {
        assert_eq!(
            check_visible(&file(json!({ "url": url }), inputs)),
            Ok(()),
            "{url}"
        );
    }
}

/// A file whose action that runs things hides a character in an
/// argument is refused, at the argument; the same characters elsewhere
/// are the format's text rules' to judge.
#[test]
fn an_action_that_runs_things_hides_nothing() {
    let file = |assignment: &Assignment| {
        let mut control = crate::core::model::Control::new(
            crate::core::model::ControlPos::grid(0, 0),
            "Pad",
            crate::core::model::Rgb::new(1, 2, 3),
        );
        control.press = Some(assignment.clone());
        let surface = crate::core::model::Surface {
            id: "s".into(),
            name: "S".into(),
            description: String::new(),
            icon: None,
            category: "custom".into(),
            pages: vec![crate::core::model::Page {
                id: "home".into(),
                name: "Home".into(),
                controls: vec![control],
            }],
            context: None,
        };
        ContentV1 {
            surfaces: vec![crate::artifacts::format::SurfaceV1::from_model(&surface).unwrap()],
            context_rules: vec![],
        }
    };
    for c in HIDDEN {
        for (assignment, arg) in every_kind(c) {
            let refused = check_visible(&file(&assignment)).unwrap_err();
            assert_eq!(
                (refused.code, refused.at.as_deref()),
                (
                    crate::artifacts::RejectionCode::InvalidContent,
                    Some(
                        format!("/content/surfaces/0/pages/0/controls/0/press/args/{arg}").as_str()
                    )
                ),
                "{c:?} in {arg}"
            );
            assert!(!refused.message.contains(c));
            // The rejection names the character it found.
            let named = format!("U+{:04X}", u32::from(c));
            assert!(refused.message.contains(&named), "{}", refused.message);
        }
    }
    // Line breaks and tabs are fine; so is anything in an action that
    // runs nothing.
    for fine in [
        a("shell", "execute", json!({ "command": "git pull\n\tmake" })),
        a("agent-control", "newTask", json!({ "task": "one\ntwo" })),
        a(
            "system",
            "openUrl",
            json!({ "url": "https://e.com/\u{202E}" }),
        ),
        a("acme.notes", "mark", json!({ "text": "\u{7}" })),
    ] {
        assert_eq!(check_visible(&file(&fine)), Ok(()), "{fine:?}");
    }
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
