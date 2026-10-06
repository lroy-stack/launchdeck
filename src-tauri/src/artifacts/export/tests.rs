//! Export on whole workspaces: determinism and the golden files, what is
//! never written, legacy content, dependencies, surface references, the
//! errors that block a file, inputs, review, requirements and metadata.

use std::collections::BTreeMap;

use serde_json::{json, Value};

use super::select::artifact_ids;
use super::*;
use crate::actions::definition::{Assignment, ProviderKind, ProviderStatus};
use crate::actions::params::{ParamDef, RefTarget};
use crate::artifacts::env::{Environment, KnownAction, KnownDevice, KnownProvider};
use crate::artifacts::format::{
    compute_requirements, AppRefV1, ArtifactKind, ArtifactV1, Author, InputDef, InputType,
    InputValue, TestedKind, TestedWith,
};
use crate::artifacts::review::{Location, ReviewKind, Slot};
use crate::artifacts::testkit;
use crate::context::identity::{ApplicationIdentity, MACOS, WINDOWS, X11};
use crate::context::rules::{ContextRule, ContextSettings, Fallback};
use crate::core::model::{
    ContextHint, Control, ControlPos, ImportOrigin, Page, Preferences, Rgb, Surface, Workspace,
    SCHEMA_VERSION,
};
use crate::devices::{AnimationCaps, AuxCaps, DeviceCapabilities, DeviceKind, GridCaps};
use crate::platform::Platform;
use crate::state::feedback::{FeedbackRule, Hold, Test};
use crate::state::StateAddress;
use crate::surfaces::default_workspace_for;

use ExportCode as E;

// What the tests build on.

fn action(params: Vec<ParamDef>, privileged: bool) -> KnownAction {
    KnownAction { params, privileged }
}

fn provider(kind: ProviderKind, version: &str, actions: Vec<(&str, KnownAction)>) -> KnownProvider {
    KnownProvider {
        kind,
        name: "Provider".into(),
        version: version.into(),
        status: ProviderStatus::Ready,
        actions: actions
            .into_iter()
            .map(|(id, a)| (id.to_string(), a))
            .collect(),
    }
}

fn plugin(name: &str, version: &str, actions: Vec<(&str, KnownAction)>) -> KnownProvider {
    KnownProvider {
        name: name.into(),
        ..provider(ProviderKind::Plugin, version, actions)
    }
}

fn device(model: &str, kind: DeviceKind, connected: bool) -> KnownDevice {
    KnownDevice {
        name: format!("Device {model}"),
        model: model.into(),
        kind,
        connected,
        capabilities: DeviceCapabilities {
            grid: Some(GridCaps {
                width: 8,
                height: 8,
            }),
            aux: AuxCaps {
                top: 8,
                side: 8,
                logo: true,
            },
            rgb: true,
            velocity: true,
            pressure: false,
            animations: AnimationCaps {
                pulse: true,
                flash: true,
            },
        },
    }
}

/// An environment written by hand: the built-in definitions export reads,
/// three installed plugins and a few devices.
fn hand_env() -> Environment {
    let builtin = |actions| provider(ProviderKind::Builtin, "0.1.0", actions);
    let open_surface = action(
        vec![
            ParamDef::reference("surfaceId", "Surface", RefTarget::Surface).required(),
            ParamDef::reference("pageId", "Page", RefTarget::Page).scope("surfaceId"),
        ],
        false,
    );
    let providers = [
        ("navigation", builtin(vec![("openSurface", open_surface)])),
        (
            "shell",
            builtin(vec![(
                "execute",
                action(vec![ParamDef::string("command", "Command")], true),
            )]),
        ),
        ("http", builtin(vec![("request", action(vec![], true))])),
        (
            "system",
            builtin(vec![
                ("launchApplication", action(vec![], false)),
                ("openUrl", action(vec![], false)),
            ]),
        ),
        (
            "acme.notes",
            plugin(
                "Notes",
                "1.2.3",
                vec![
                    (
                        "mark",
                        action(vec![ParamDef::string("folder", "Folder")], false),
                    ),
                    (
                        "show",
                        action(
                            vec![ParamDef::reference("target", "Target", RefTarget::Surface)],
                            false,
                        ),
                    ),
                ],
            ),
        ),
        ("acme.timer", plugin("Timer", "0.4.0", vec![])),
        ("acme.ci", plugin("CI", "2.0.0", vec![])),
    ];
    Environment {
        app_version: "9.9.9".into(),
        namespace: Some(X11.into()),
        providers: providers
            .into_iter()
            .map(|(id, p)| (id.to_string(), p))
            .collect(),
        devices: vec![
            device("launchpad-x", DeviceKind::Virtual, true),
            device("launchpad-x", DeviceKind::Hardware, true),
            device("launchpad-x", DeviceKind::Hardware, true),
            device("launchpad-mini", DeviceKind::Hardware, false),
            device("Odd Model", DeviceKind::Hardware, true),
        ],
        recent_applications: Vec::new(),
        ..Environment::default()
    }
}

fn a(provider: &str, action: &str, args: Value) -> Option<Assignment> {
    Some(Assignment::new(provider, action, args))
}

fn pad(col: u8, label: &str, press: Option<Assignment>) -> Control {
    let mut c = Control::new(ControlPos::grid(col, 0), label, Rgb::new(200, 40, 40));
    c.press = press;
    c
}

fn surface(id: &str, controls: Vec<Control>) -> Surface {
    Surface {
        id: id.into(),
        name: format!("Surface {id}"),
        description: String::new(),
        icon: None,
        category: "custom".into(),
        pages: vec![Page {
            id: "home".into(),
            name: "Home".into(),
            controls,
        }],
        context: None,
        imported_from: None,
    }
}

fn workspace(surfaces: Vec<Surface>) -> Workspace {
    Workspace {
        schema_version: SCHEMA_VERSION,
        last_active_surface: surfaces.first().map(|s| s.id.clone()),
        surfaces,
        preferences: Preferences::default(),
        context: ContextSettings::default(),
    }
}

fn rule(namespace: &str, id: &str, name: &str, surface: &str, page: Option<&str>) -> ContextRule {
    ContextRule {
        application: ApplicationIdentity::new(namespace, id, name),
        surface_id: surface.into(),
        page_id: page.map(String::from),
    }
}

fn request(kind: ArtifactKind, ids: &[&str]) -> ExportRequest {
    ExportRequest {
        kind,
        surface_ids: ids.iter().map(|s| s.to_string()).collect(),
        metadata: ExportMetadata::default(),
        include_context_rules: false,
        inputs: Vec::new(),
        bindings: Vec::new(),
        rule_inputs: Vec::new(),
        extra_identities: Vec::new(),
    }
}

fn pack(ids: &[&str]) -> ExportRequest {
    request(ArtifactKind::Pack, ids)
}

fn location(surface: &str, control: &str, slot: Slot) -> Location {
    Location {
        surface_source_id: surface.into(),
        page_id: "home".into(),
        control_id: control.into(),
        slot,
    }
}

fn input(id: &str, kind: InputType) -> InputDef {
    InputDef {
        id: id.into(),
        kind,
        label: format!("Input {id}"),
        description: String::new(),
        required: true,
        default: None,
        options: Vec::new(),
        max_length: None,
    }
}

/// The file, read back as an importer would; panics with the errors when
/// the export produced none.
fn written(result: &ExportResult) -> (String, ArtifactV1) {
    let bytes = result
        .bytes
        .as_ref()
        .unwrap_or_else(|| panic!("no file: {:#?}", result.preview.errors));
    let text = String::from_utf8(bytes.clone()).unwrap();
    let artifact = testkit::read(&text).unwrap_or_else(|r| panic!("{r:?}\n{text}"));
    (text, artifact)
}

fn codes(issues: &[ExportIssue]) -> Vec<ExportCode> {
    issues.iter().map(|i| i.code).collect()
}

/// The errors of exporting `request`, and that no file came with them.
fn refused(ws: &Workspace, request: &ExportRequest) -> Vec<ExportIssue> {
    let result = export(ws, &hand_env(), request);
    assert!(result.bytes.is_none(), "{request:?}");
    assert!(!result.preview.errors.is_empty());
    result.preview.errors
}

/// The Linux starter workspace with two context rules: Visual Studio Code
/// shows Development (and gets its macOS and Windows ids on export), Firefox
/// shows the Git page.
fn starters_with_rules() -> Workspace {
    let mut ws = default_workspace_for(Platform::Linux);
    ws.context.rules = vec![
        rule(X11, "Code", "Visual Studio Code", "development", None),
        rule(X11, "firefox", "Firefox", "development", Some("git")),
    ];
    ws
}

/// The Home Assistant webhook of the Accessibility starter, asked of the
/// importer: a webhook id is a credential. (The Home starter's pads are the
/// Home Assistant example plugin's, with example entities and no webhook.)
fn webhook_inputs() -> (Vec<InputDef>, Vec<InputBinding>) {
    let hooks = [("lights", "Lights", vec![("accessibility", "g.4.0")])];
    let mut inputs = Vec::new();
    let mut bindings = Vec::new();
    for (name, label, pads) in hooks {
        let id = format!("{name}_webhook");
        inputs.push(InputDef {
            label: format!("{label} webhook"),
            description: "The address of your Home Assistant webhook for this pad.".into(),
            ..input(&id, InputType::Url)
        });
        for (surface, control) in pads {
            let page = if surface == "accessibility" {
                "help"
            } else {
                "home"
            };
            bindings.push(InputBinding {
                at: Location {
                    surface_source_id: surface.into(),
                    page_id: page.into(),
                    control_id: control.into(),
                    slot: Slot::Press,
                },
                arg: "url".into(),
                input: id.clone(),
            });
        }
    }
    (inputs, bindings)
}

fn starters_request() -> ExportRequest {
    let (inputs, bindings) = webhook_inputs();
    ExportRequest {
        inputs,
        bindings,
        metadata: ExportMetadata {
            id: String::new(),
            name: "Starters".into(),
            version: "1.0.0".into(),
            description: Some("The five starter surfaces of LaunchDeck.".into()),
            author: Some(Author {
                name: "LaunchDeck examples".into(),
                url: None,
            }),
            license: Some("MIT".into()),
        },
        include_context_rules: true,
        extra_identities: vec![
            ExtraIdentity {
                rule: 0,
                namespace: MACOS.into(),
                id: "com.microsoft.VSCode".into(),
            },
            ExtraIdentity {
                rule: 0,
                namespace: WINDOWS.into(),
                id: "Code.exe".into(),
            },
        ],
        ..pack(&[
            "agent-control",
            "development",
            "productivity",
            "smart-home",
            "accessibility",
        ])
    }
}

// The golden files.

fn golden_dir() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/artifacts/valid")
}

/// The two golden exports, with a fixed app version so a release does not
/// change them.
async fn goldens() -> Vec<(&'static str, ExportResult)> {
    let env = testkit::environment("9.9.9").await;
    let development = export(
        &default_workspace_for(Platform::Linux),
        &env,
        &request(ArtifactKind::Surface, &["development"]),
    );
    let starters = export(&starters_with_rules(), &env, &starters_request());
    vec![
        ("development.ldsurface", development),
        ("starters.ldpack", starters),
    ]
}

#[tokio::test]
async fn the_golden_files_are_what_export_writes() {
    for (name, result) in goldens().await {
        let (text, artifact) = written(&result);
        let golden = std::fs::read(golden_dir().join(name))
            .unwrap_or_else(|e| panic!("{name}: {e}; run regenerate_export_goldens"));
        assert!(
            golden == text.as_bytes(),
            "{name} differs from what export writes; if the change is intended, run \
             `cargo test --lib regenerate_export_goldens -- --ignored`"
        );
        assert_eq!(artifact.manifest.created_with.version, "9.9.9");
        // No device id or device name, and nothing of the runtime.
        for absent in [
            "virtual-launchpad-x",
            "Virtual Launchpad",
            "lastActiveSurface",
        ] {
            assert!(!text.contains(absent), "{name}: {absent}");
        }
    }
}

/// Rewrites the golden files from what export writes now. Run only for an
/// intended change of the format or of the starter content.
#[tokio::test]
#[ignore]
async fn regenerate_export_goldens() {
    std::fs::create_dir_all(golden_dir()).unwrap();
    for (name, result) in goldens().await {
        let (text, _) = written(&result);
        std::fs::write(golden_dir().join(name), text).unwrap();
    }
}

#[tokio::test]
async fn the_golden_pack_holds_the_five_starters_and_two_rules() {
    let results = goldens().await;
    let (_, artifact) = written(&results[1].1);
    let preview = &results[1].1.preview;
    assert_eq!(artifact.kind, ArtifactKind::Pack);
    assert_eq!(artifact.manifest.id, "launchdeck-examples.starters");
    assert_eq!(artifact.content.surfaces.len(), 5);
    assert_eq!(artifact.content.context_rules.len(), 2);
    let AppRefV1::Application(code) = &artifact.content.context_rules[0].application else {
        panic!("an application");
    };
    let namespaces: Vec<&str> = code
        .identities
        .iter()
        .map(|i| i.namespace.as_str())
        .collect();
    assert_eq!(namespaces, [X11, MACOS, WINDOWS]);
    assert_eq!(code.identities[2].id, "code.exe");
    assert_eq!(
        artifact.content.context_rules[1].page.as_deref(),
        Some("git")
    );
    assert_eq!(preview.file_name, "starters-1.0.0.ldpack");
    assert_eq!(
        artifact.manifest.tested_with,
        [TestedWith {
            model: "launchpad-x".into(),
            kind: TestedKind::Virtual
        }]
    );
    // Every surface switcher names a surface of the Pack.
    let refs = artifact
        .content
        .surfaces
        .iter()
        .flat_map(|s| &s.pages)
        .flat_map(|p| &p.controls)
        .filter_map(|c| c.press.as_ref())
        .filter(|a| a.action == "openSurface")
        .inspect(|a| assert_eq!(a.surface_refs, ["surfaceId"], "{a:?}"))
        .count();
    assert!(refs >= 20, "{refs}");
    assert!(!codes(&preview.warnings).contains(&E::ExternalSurfaceReference));
}

// Determinism.

#[test]
fn exporting_twice_gives_the_same_bytes() {
    let ws = starters_with_rules();
    let env = hand_env();
    let first = export(&ws, &env, &starters_request());
    let second = export(&ws, &env, &starters_request());
    assert!(first.bytes.is_some(), "{:#?}", first.preview.errors);
    assert_eq!(first, second);
    // The request's order of surfaces does not matter: the workspace's does.
    let mut reversed = starters_request();
    reversed.surface_ids.reverse();
    assert_eq!(export(&ws, &env, &reversed).bytes, first.bytes);
    // The preview tells the size and the digest of the bytes.
    let (text, artifact) = written(&first);
    assert_eq!(first.preview.size_bytes, text.len());
    assert_eq!(
        first.preview.content_digest,
        artifact.manifest.integrity.content
    );
    assert_eq!(first.preview.manifest, artifact.manifest);
}

#[test]
fn surfaces_get_the_id_they_prefer_once() {
    assert_eq!(artifact_ids(&["a", "b"]), ["a", "b"]);
    assert_eq!(artifact_ids(&["a", "a", "a"]), ["a", "a-2", "a-3"]);
    // An id another surface prefers is not given away.
    assert_eq!(artifact_ids(&["a", "a", "a-2"]), ["a", "a-3", "a-2"]);
    assert!(artifact_ids(&[]).is_empty());
}

fn imported(mut s: Surface, source_id: &str, instance: u32) -> Surface {
    s.imported_from = Some(ImportOrigin {
        format: "launchdeck.pack".into(),
        artifact_id: "acme.studio".into(),
        artifact_name: "Studio".into(),
        artifact_version: "1.0.0".into(),
        instance,
        source_surface_id: source_id.into(),
        content_digest: format!("sha256:{}", "c".repeat(64)),
        surface_digest: format!("sha256:{}", "d".repeat(64)),
    });
    s
}

/// A surface imported earlier is written with the id its file gave it, so
/// a file exported again keeps the ids of the one it came from; where two
/// imported surfaces came from the same id, each keeps its local id.
/// Where it came from is never written.
#[test]
fn an_imported_surface_is_exported_with_the_id_its_file_gave_it() {
    let ws = workspace(vec![
        surface("mixer", vec![]),
        imported(surface("mixer-2", vec![]), "mixer", 1),
        imported(surface("lights-2", vec![]), "lights", 1),
        imported(surface("lights-3", vec![]), "lights", 2),
    ]);
    let ids = |chosen: &[&str]| -> Vec<String> {
        let (_, artifact) = written(&export(&ws, &hand_env(), &pack(chosen)));
        artifact
            .content
            .surfaces
            .into_iter()
            .map(|s| s.id)
            .collect()
    };
    assert_eq!(ids(&["mixer-2"]), ["mixer"]);
    // The local surface that has the id first keeps it.
    assert_eq!(ids(&["mixer", "mixer-2"]), ["mixer", "mixer-2"]);
    assert_eq!(ids(&["lights-2", "lights-3"]), ["lights-2", "lights-3"]);
    assert_eq!(ids(&["lights-3"]), ["lights"]);

    let (text, _) = written(&export(&ws, &hand_env(), &pack(&["mixer-2", "lights-2"])));
    for absent in ["importedFrom", "acme.studio", "instance", &"c".repeat(64)] {
        assert!(!text.contains(absent), "{absent}");
    }
}

// What a file never carries.

#[test]
fn nothing_but_the_chosen_content_is_written() {
    let mut ws = starters_with_rules();
    ws.preferences.onboarding_done = true;
    ws.preferences.external_events_port = 50123;
    ws.context.auto = true;
    ws.context.fallback = Fallback::Surface {
        surface_id: "productivity".into(),
        page_id: None,
    };
    // A rule for a surface that is not exported stays home.
    ws.context
        .rules
        .push(rule(X11, "gimp", "GIMP", "smart-home", None));
    let dev = ws.surface_mut("development").unwrap();
    dev.context = Some(ContextHint {
        foreground_apps: vec!["code".into()],
    });
    dev.pages[0].controls[0].legacy_bindings = vec![json!({ "source": "old" }), json!({})];
    let request = ExportRequest {
        include_context_rules: true,
        ..pack(&["development", "productivity"])
    };
    let result = export(&ws, &hand_env(), &request);
    let (text, artifact) = written(&result);
    for absent in [
        "schemaVersion",
        "lastActiveSurface",
        "preferences",
        "onboardingDone",
        "externalEventsPort",
        "50123",
        "\"auto\"",
        "fallback",
        "legacyBindings",
        "\"source\": \"old\"",
        "foregroundApps",
        "importedFrom",
        "gimp",
        "Device launchpad-x",
    ] {
        assert!(!text.contains(absent), "{absent}");
    }
    assert_eq!(artifact.content.context_rules.len(), 2);
    assert_eq!(
        result.preview.stripped,
        Stripped {
            legacy_bindings: 2,
            legacy_actions: 0,
            context_hints: 1,
        }
    );
}

#[test]
fn legacy_actions_are_stripped_and_counted() {
    // A schema 2 action no later version knew, converted as a workspace
    // migration converts it.
    let mut doc = json!({ "surfaces": [ { "pages": [ { "controls": [
        { "press": { "type": "teleport", "where": "moon" },
          "longPress": { "type": "openUrl", "url": "https://example.com" } }
    ] } ] } ] });
    crate::persistence::legacy::upgrade_actions(&mut doc);
    let control = &doc["surfaces"][0]["pages"][0]["controls"][0];
    let legacy: Assignment = serde_json::from_value(control["press"].clone()).unwrap();
    let open: Assignment = serde_json::from_value(control["longPress"].clone()).unwrap();
    let mut both = pad(0, "Old", Some(legacy.clone()));
    both.long_press = Some(open);
    let ws = workspace(vec![surface(
        "old",
        vec![both, pad(1, "Older", Some(legacy))],
    )]);
    let result = export(&ws, &hand_env(), &pack(&["old"]));
    let (text, artifact) = written(&result);
    let controls = &artifact.content.surfaces[0].pages[0].controls;
    assert_eq!(controls[0].press, None);
    assert_eq!(controls[0].long_press.as_ref().unwrap().action, "openUrl");
    assert_eq!(controls[1].press, None);
    // The label, the look and the place stay.
    assert_eq!(controls[1].label, "Older");
    assert!(!text.contains("legacy") && !text.contains("teleport") && !text.contains("moon"));
    assert_eq!(result.preview.stripped.legacy_actions, 2);
}

// Dependencies.

#[test]
fn dependencies_follow_what_uses_them() {
    let mut watched = pad(
        0,
        "Watched",
        a("acme.notes", "mark", json!({ "folder": "notes" })),
    );
    watched.states.insert(
        "busy".into(),
        crate::core::model::Visual::solid(Rgb::new(0, 0, 255)),
    );
    watched.feedback = vec![
        FeedbackRule::state(StateAddress::new("acme.timer", "running"), Test::HasValue)
            .show("busy"),
        FeedbackRule::state(StateAddress::new("acme.weather", "rain"), Test::HasValue).show("busy"),
        FeedbackRule::event("acme.ci", "build.failed")
            .show("busy")
            .hold(Hold::Latch),
        FeedbackRule::event("build.server", "done")
            .show("busy")
            .hold(Hold::Latch),
        FeedbackRule::event("ci", "done")
            .show("busy")
            .hold(Hold::Latch),
    ];
    let ws = workspace(vec![surface(
        "work",
        vec![
            watched,
            pad(1, "Ghost", a("acme.ghost", "boo", json!({}))),
            pad(
                2,
                "Shell",
                a("shell", "execute", json!({ "command": "ls" })),
            ),
            pad(
                3,
                "Demo",
                a("demo", "startTask", json!({ "agent": "alpha" })),
            ),
        ],
    )]);
    let result = export(&ws, &hand_env(), &pack(&["work"]));
    let (_, artifact) = written(&result);
    let deps: Vec<(&str, Option<&str>, &str, bool)> = artifact
        .manifest
        .dependencies
        .iter()
        .map(|d| {
            (
                d.plugin_id.as_str(),
                d.name.as_deref(),
                d.version.as_str(),
                d.required,
            )
        })
        .collect();
    assert_eq!(
        deps,
        [
            ("acme.ci", Some("CI"), "^2.0.0", false),
            ("acme.ghost", None, "*", true),
            ("acme.notes", Some("Notes"), "^1.2.3", true),
            ("acme.timer", Some("Timer"), "^0.4.0", false),
            ("acme.weather", None, "*", false),
        ]
    );
    assert_eq!(result.preview.dependencies, artifact.manifest.dependencies);
    let not_installed: Vec<&ExportIssue> = result
        .preview
        .warnings
        .iter()
        .filter(|w| w.code == E::NotInstalled)
        .collect();
    assert_eq!(not_installed.len(), 2);
    assert!(not_installed[0].message.contains("acme.ghost"));
    assert_eq!(
        not_installed[0].at.as_deref(),
        Some("/content/surfaces/0/pages/0/controls/1/press/provider")
    );
    assert!(not_installed[1].message.contains("acme.weather"));
}

// Surface references.

#[test]
fn references_to_exported_surfaces_are_marked() {
    let ws = workspace(vec![
        surface(
            "dev",
            vec![
                pad(
                    0,
                    "Music",
                    a(
                        "navigation",
                        "openSurface",
                        json!({ "surfaceId": "music", "pageId": "home" }),
                    ),
                ),
                pad(
                    1,
                    "Away",
                    a("navigation", "openSurface", json!({ "surfaceId": "away" })),
                ),
                pad(
                    2,
                    "Show",
                    a("acme.notes", "show", json!({ "target": "music" })),
                ),
                pad(
                    3,
                    "Unknown",
                    a("acme.ghost", "go", json!({ "surfaceId": "music" })),
                ),
            ],
        ),
        surface("music", vec![]),
        surface("away", vec![]),
    ]);
    let result = export(&ws, &hand_env(), &pack(&["dev", "music"]));
    let (_, artifact) = written(&result);
    let press = |c: usize| {
        artifact.content.surfaces[0].pages[0].controls[c]
            .press
            .clone()
            .unwrap()
    };
    assert_eq!(press(0).surface_refs, ["surfaceId"]);
    assert_eq!(press(0).args["surfaceId"], "music");
    assert_eq!(press(0).args["pageId"], "home");
    assert!(press(1).surface_refs.is_empty());
    assert_eq!(press(1).args["surfaceId"], "away");
    assert_eq!(press(2).surface_refs, ["target"]);
    // Without its definition, nothing is known to be a reference.
    assert!(press(3).surface_refs.is_empty());
    let external: Vec<&ExportIssue> = result
        .preview
        .warnings
        .iter()
        .filter(|w| w.code == E::ExternalSurfaceReference)
        .collect();
    assert_eq!(external.len(), 1);
    assert_eq!(
        external[0].at.as_deref(),
        Some("/content/surfaces/0/pages/0/controls/1/press/args/surfaceId")
    );
    // A surface reference is not offered as a parameter, and cannot be bound.
    assert!(!result
        .preview
        .parameters
        .iter()
        .any(|p| p.at.control_id == "g.0.0" && p.arg == "surfaceId"));
    let bound = ExportRequest {
        inputs: vec![input("where", InputType::String)],
        bindings: vec![InputBinding {
            at: location("dev", "g.0.0", Slot::Press),
            arg: "surfaceId".into(),
            input: "where".into(),
        }],
        ..pack(&["dev", "music"])
    };
    assert_eq!(codes(&refused(&ws, &bound)), [E::InvalidBinding]);
}

// What blocks a file.

#[test]
fn an_address_with_credentials_is_an_error() {
    let ws = workspace(vec![surface(
        "web",
        vec![
            pad(
                0,
                "Hook",
                a(
                    "http",
                    "request",
                    json!({ "url": "https://user:secret@example.com/hook" }),
                ),
            ),
            pad(
                1,
                "Nested",
                a(
                    "acme.notes",
                    "mark",
                    json!({ "list": ["https://token@example.com"] }),
                ),
            ),
            pad(
                2,
                "Mail",
                a(
                    "system",
                    "openUrl",
                    json!({ "url": "mailto:me@example.com" }),
                ),
            ),
            pad(
                3,
                "Query",
                a(
                    "system",
                    "openUrl",
                    json!({ "url": "https://example.com/?a=b@c" }),
                ),
            ),
        ],
    )]);
    let errors = refused(&ws, &pack(&["web"]));
    // The request's address is also one the runtime would not send (SF-4).
    assert_eq!(
        codes(&errors),
        [E::Credential, E::Credential, E::InvalidContent]
    );
    assert_eq!(
        errors[0].at.as_deref(),
        Some("/content/surfaces/0/pages/0/controls/0/press/args/url")
    );
    assert!(!errors[0].message.contains("secret"));
    assert_eq!(
        errors[1].at.as_deref(),
        Some("/content/surfaces/0/pages/0/controls/1/press/args/list")
    );
}

/// A file the importer would refuse is never written: an action that runs
/// things may not hide a character from the review (SF-3).
#[test]
fn an_action_that_runs_things_hides_nothing() {
    let ws = workspace(vec![surface(
        "dev",
        vec![pad(
            0,
            "Deploy",
            a(
                "shell",
                "execute",
                json!({ "command": "make \u{202E}lacol" }),
            ),
        )],
    )]);
    let errors = refused(&ws, &pack(&["dev"]));
    assert_eq!(codes(&errors), [E::InvalidContent]);
    assert_eq!(
        errors[0].at.as_deref(),
        Some("/content/surfaces/0/pages/0/controls/0/press/args/command")
    );
}

#[test]
fn seventeen_surfaces_are_too_many() {
    let ids: Vec<String> = (0..17).map(|i| format!("s{i}")).collect();
    let ws = workspace(ids.iter().map(|id| surface(id, vec![])).collect());
    let all: Vec<&str> = ids.iter().map(String::as_str).collect();
    let errors = refused(&ws, &pack(&all));
    assert_eq!(codes(&errors), [E::TooMany]);
    assert_eq!(errors[0].at.as_deref(), Some("/content/surfaces"));
    assert!(export(&ws, &hand_env(), &pack(&all[..16])).bytes.is_some());
}

#[test]
fn a_file_over_two_mebibytes_is_too_large() {
    // 128 pads, each with the longest command an argument may hold.
    let command = "x".repeat(crate::artifacts::limits::MAX_ARG_TEXT);
    let pads = (0..128u8)
        .map(|i| {
            let mut c = pad(
                0,
                "Run",
                a("shell", "execute", json!({ "command": command })),
            );
            c.pos = ControlPos::grid(i % 16, i / 16);
            c.id = c.pos.key();
            c
        })
        .collect();
    let ws = workspace(vec![surface("big", pads)]);
    let result = export(&ws, &hand_env(), &pack(&["big"]));
    assert!(result.bytes.is_none());
    assert_eq!(codes(&result.preview.errors), [E::TooLarge]);
    assert!(result.preview.size_bytes > crate::artifacts::limits::MAX_FILE_BYTES);
}

#[test]
fn a_surface_file_holds_one_surface_and_nothing_of_a_pack() {
    let mut ws = workspace(vec![surface("a", vec![]), surface("b", vec![])]);
    ws.context.rules = vec![rule(X11, "code", "Code", "a", None)];
    let surface_file = |ids: &[&str]| request(ArtifactKind::Surface, ids);
    assert_eq!(
        codes(&refused(&ws, &surface_file(&["a", "b"]))),
        [E::KindMismatch]
    );
    let with_rules = ExportRequest {
        include_context_rules: true,
        ..surface_file(&["a"])
    };
    assert_eq!(codes(&refused(&ws, &with_rules)), [E::KindMismatch]);
    let with_inputs = ExportRequest {
        inputs: vec![input("folder", InputType::Path)],
        ..surface_file(&["a"])
    };
    assert_eq!(codes(&refused(&ws, &with_inputs)), [E::KindMismatch]);
    let with_bindings = ExportRequest {
        bindings: vec![InputBinding {
            at: location("a", "g.0.0", Slot::Press),
            arg: "x".into(),
            input: "x".into(),
        }],
        ..surface_file(&["a"])
    };
    assert_eq!(codes(&refused(&ws, &with_bindings)), [E::KindMismatch]);
    // Asking for rules when none shows the surface leaves a valid Surface file.
    let no_rule_applies = ExportRequest {
        include_context_rules: true,
        ..surface_file(&["b"])
    };
    let result = export(&ws, &hand_env(), &no_rule_applies);
    let (_, artifact) = written(&result);
    assert_eq!(artifact.kind, ArtifactKind::Surface);
    assert!(result.preview.file_name.ends_with(".ldsurface"));
}

#[test]
fn surfaces_must_be_chosen_and_exist() {
    let ws = workspace(vec![surface("a", vec![])]);
    assert_eq!(codes(&refused(&ws, &pack(&[]))), [E::NoSurfaces]);
    let errors = refused(&ws, &pack(&["a", "gone"]));
    assert_eq!(codes(&errors), [E::UnknownSurface]);
    assert!(errors[0].message.contains("gone"));
}

// Inputs, bindings and identities.

/// A surface with a command and a notes pad, two context rules for it.
fn inputs_workspace() -> Workspace {
    let mut ws = workspace(vec![surface(
        "dev",
        vec![
            pad(
                0,
                "Build",
                a("shell", "execute", json!({ "command": "make" })),
            ),
            pad(
                1,
                "Notes",
                a(
                    "acme.notes",
                    "mark",
                    json!({ "folder": "/home/me/notes", "count": 2 }),
                ),
            ),
        ],
    )]);
    ws.context.rules = vec![
        rule(X11, "Code", "Visual Studio Code", "dev", None),
        rule(X11, "firefox", "Firefox", "dev", Some("home")),
    ];
    ws
}

fn inputs_request() -> ExportRequest {
    ExportRequest {
        include_context_rules: true,
        inputs: vec![
            input("folder", InputType::Path),
            input("editor", InputType::Application),
        ],
        bindings: vec![InputBinding {
            at: location("dev", "g.1.0", Slot::Press),
            arg: "folder".into(),
            input: "folder".into(),
        }],
        rule_inputs: vec![RuleInput {
            rule: 1,
            input: "editor".into(),
        }],
        extra_identities: vec![
            ExtraIdentity {
                rule: 0,
                namespace: MACOS.into(),
                id: "com.microsoft.VSCode".into(),
            },
            ExtraIdentity {
                rule: 0,
                namespace: WINDOWS.into(),
                id: "Code.EXE".into(),
            },
        ],
        ..pack(&["dev"])
    }
}

#[test]
fn inputs_bindings_and_identities_are_written() {
    let result = export(&inputs_workspace(), &hand_env(), &inputs_request());
    let (_, artifact) = written(&result);
    assert_eq!(artifact.manifest.inputs.len(), 2);
    let notes = artifact.content.surfaces[0].pages[0].controls[1]
        .press
        .as_ref()
        .unwrap();
    assert_eq!(
        notes.inputs,
        BTreeMap::from([("folder".to_string(), "folder".to_string())])
    );
    let rules = &artifact.content.context_rules;
    let AppRefV1::Application(code) = &rules[0].application else {
        panic!("an application");
    };
    let ids: Vec<(&str, &str)> = code
        .identities
        .iter()
        .map(|i| (i.namespace.as_str(), i.id.as_str()))
        .collect();
    assert_eq!(
        ids,
        [
            (X11, "Code"),
            (MACOS, "com.microsoft.VSCode"),
            (WINDOWS, "code.exe")
        ]
    );
    assert_eq!(
        rules[1].application,
        AppRefV1::Input(crate::artifacts::format::InputRef {
            input: "editor".into()
        })
    );
    let preview: Vec<usize> = result
        .preview
        .context_rules
        .iter()
        .map(|r| r.rule)
        .collect();
    assert_eq!(preview, [0, 1]);
}

#[test]
fn inputs_bindings_and_identities_are_checked() {
    let ws = inputs_workspace();
    let check = |change: &dyn Fn(&mut ExportRequest)| {
        let mut request = inputs_request();
        change(&mut request);
        codes(&refused(&ws, &request))
    };
    let bind = |r: &mut ExportRequest, control: &str, arg: &str, input: &str| {
        r.bindings = vec![InputBinding {
            at: location("dev", control, Slot::Press),
            arg: arg.into(),
            input: input.into(),
        }];
    };
    let binding = E::InvalidBinding;
    assert_eq!(check(&|r| bind(r, "g.1.0", "folder", "nothing")), [binding]);
    assert_eq!(check(&|r| bind(r, "g.1.0", "folder", "editor")), [binding]);
    assert_eq!(check(&|r| bind(r, "g.1.0", "count", "folder")), [binding]);
    assert_eq!(check(&|r| bind(r, "g.1.0", "missing", "folder")), [binding]);
    assert_eq!(check(&|r| bind(r, "g.9.9", "folder", "folder")), [binding]);
    assert_eq!(
        check(&|r| {
            let again = r.bindings[0].clone();
            r.bindings.push(again);
        }),
        [binding]
    );
    assert_eq!(
        check(&|r| r.rule_inputs[0].input = "folder".into()),
        [binding]
    );
    assert_eq!(check(&|r| r.rule_inputs[0].rule = 5), [binding]);
    assert_eq!(check(&|r| r.rule_inputs[0].rule = 0), [binding]);
    assert_eq!(
        check(&|r| {
            let again = r.rule_inputs[0].clone();
            r.rule_inputs.push(again);
        }),
        [binding]
    );
    assert_eq!(check(&|r| r.extra_identities[0].rule = 7), [binding]);
    assert_eq!(
        check(&|r| r.include_context_rules = false),
        [binding, binding, binding]
    );
    let identity = E::InvalidIdentity;
    assert_eq!(
        check(&|r| r.extra_identities[0].namespace = "Bad NS".into()),
        [identity]
    );
    assert_eq!(
        check(&|r| r.extra_identities[0].namespace = X11.into()),
        [identity]
    );
    assert_eq!(
        check(&|r| r.extra_identities[0].id = " ".into()),
        [identity]
    );
    // The inputs themselves are checked as the format checks them.
    assert_eq!(
        check(&|r| r.inputs[0].kind = InputType::Enum),
        [E::InvalidInput]
    );
    assert_eq!(
        check(&|r| r.inputs[0].label = String::new()),
        [E::InvalidInput]
    );
    let errors = refused(&ws, &{
        let mut r = inputs_request();
        r.inputs.push(input("Bad-Id", InputType::String));
        r
    });
    assert_eq!(codes(&errors), [E::InvalidInput]);
    assert_eq!(errors[0].at.as_deref(), Some("/manifest/inputs/2/id"));
}

/// A pad whose arguments hold what one person's computer knows: a webhook
/// with its id, a folder in a home directory.
fn local_workspace() -> Workspace {
    workspace(vec![surface(
        "home",
        vec![
            pad(
                0,
                "Lights",
                a(
                    "http",
                    "request",
                    json!({ "method": "POST", "url": "http://ha.local:8123/api/webhook/LOCAL-q7z-lights" }),
                ),
            ),
            pad(
                1,
                "Notes",
                a(
                    "acme.notes",
                    "mark",
                    json!({ "folder": "/home/me/LOCAL-q7z-notes", "count": 2 }),
                ),
            ),
            pad(
                2,
                "Build",
                a(
                    "shell",
                    "execute",
                    json!({ "command": "make LOCAL-q7z-target" }),
                ),
            ),
        ],
    )])
}

fn bound(control: &str, arg: &str, input: &str) -> InputBinding {
    InputBinding {
        at: location("home", control, Slot::Press),
        arg: arg.into(),
        input: input.into(),
    }
}

fn local_request() -> ExportRequest {
    let mut hook = input("hook", InputType::Url);
    hook.default = Some(InputValue::Text {
        value: "https://example.com/hook".into(),
    });
    ExportRequest {
        inputs: vec![
            hook,
            input("folder", InputType::Path),
            input("target", InputType::String),
        ],
        bindings: vec![
            bound("g.0.0", "url", "hook"),
            bound("g.1.0", "folder", "folder"),
            bound("g.2.0", "command", "target"),
        ],
        ..pack(&["home"])
    }
}

/// SF-2: an argument bound to an input is written as the input's default,
/// or empty without one. The value the person asked the importer for stays
/// on this computer: not in the file, not in the preview, not in the review.
#[test]
fn a_bound_argument_leaves_its_local_value_home() {
    let result = export(&local_workspace(), &hand_env(), &local_request());
    let (text, artifact) = written(&result);
    let preview = serde_json::to_string(&result.preview).unwrap();
    for shown in [&text, &preview] {
        assert!(!shown.contains("LOCAL-q7z"), "{shown}");
    }
    let controls = &artifact.content.surfaces[0].pages[0].controls;
    let arg = |c: usize, name: &str| controls[c].press.as_ref().unwrap().args[name].clone();
    assert_eq!(arg(0, "url"), json!("https://example.com/hook"));
    assert_eq!(arg(1, "folder"), json!(""));
    assert_eq!(arg(1, "count"), json!(2));
    assert_eq!(arg(2, "command"), json!(""));
    // The preview names each bound argument as asked of the importer.
    let asked: Vec<(&str, Option<&str>, Option<&str>)> = result
        .preview
        .parameters
        .iter()
        .map(|p| (p.arg.as_str(), p.value.as_deref(), p.input.as_deref()))
        .collect();
    assert_eq!(
        asked,
        [
            ("method", Some("POST"), None),
            ("url", None, Some("hook")),
            ("folder", None, Some("folder")),
            ("command", None, Some("target")),
        ]
    );
    assert!(result.preview.suggestions.is_empty());
    let review: Vec<&str> = result
        .preview
        .review
        .iter()
        .map(|r| r.text.as_str())
        .collect();
    assert_eq!(
        review,
        [
            "POST https://example.com/hook (goes to example.com:443)",
            ""
        ]
    );
    // Determinism is unchanged.
    assert_eq!(
        export(&local_workspace(), &hand_env(), &local_request()),
        result
    );
}

/// SF-2: a value shaped like a credential is refused unless the person
/// asks the importer for it; the message names the pad, never the value.
#[test]
fn a_credential_is_refused_unless_bound() {
    let shapes = [
        json!({ "url": "https://user:LEAK@example.com/x" }),
        json!({ "url": "https://example.com/x?token=LEAK" }),
        json!({ "url": "https://example.com/x?a=1&Key=LEAK" }),
        json!({ "url": "https://example.com/x?APIKEY=LEAK" }),
        json!({ "url": "https://example.com/x?api_key=LEAK" }),
        json!({ "url": "https://example.com/x?access_token=LEAK" }),
        json!({ "url": "https://example.com/x?auth=LEAK" }),
        json!({ "url": "https://example.com/x?secret=LEAK" }),
        json!({ "url": "https://example.com/x?password=LEAK" }),
        json!({ "url": "https://example.com/x?sig=LEAK" }),
        json!({ "url": "https://example.com/x?Signature=LEAK" }),
        json!({ "url": "http://homeassistant.local:8123/api/webhook/LEAK" }),
        json!({ "url": "curl -s 'https://example.com/x?token=LEAK'" }),
        json!({ "url": "curl -H 'Authorization: Bearer LEAK' https://example.com" }),
        json!({ "url": "proxy-authorization: Basic LEAK" }),
        json!({ "url": "Cookie: session=LEAK" }),
        json!({ "url": "X-Api-Key: LEAK" }),
        json!({ "url": { "headers": { "Authorization": "Bearer LEAK" } } }),
        json!({ "url": ["x", { "x-api-key": "LEAK" }] }),
        // A plugin's JSON argument, which the preview does not list.
        json!({ "url": { "password": "LEAK" } }),
        json!({ "url": { "token": "LEAK" } }),
        json!({ "url": { "auth": { "user": "a", "pass": "LEAK" } } }),
        json!({ "url": [{ "api_key": "LEAK" }] }),
        // Secrets as services name them in an address.
        json!({ "url": "https://example.com/x?client_secret=LEAK" }),
        json!({ "url": "https://example.com/cb#access_token=LEAK" }),
        json!({ "url": "https://hooks.slack.com/services/T0/B0/LEAK" }),
        json!({ "url": "https://discord.com/api/webhooks/1/LEAK" }),
        json!({ "url": "http://ha.local:8123/api/%77ebhook/LEAK" }),
        // A request body or a command holding one.
        json!({ "url": "{\"api_key\": \"LEAK\"}" }),
        json!({ "url": "token=LEAK&x=1" }),
        json!({ "url": "psql postgresql://admin:LEAK@db/app" }),
    ];
    for args in shapes {
        let ws = workspace(vec![surface(
            "home",
            vec![pad(0, "Hook", a("acme.notes", "mark", args.clone()))],
        )]);
        let errors = refused(&ws, &pack(&["home"]));
        assert_eq!(codes(&errors), [E::Credential], "{args}");
        assert_eq!(
            errors[0].at.as_deref(),
            Some("/content/surfaces/0/pages/0/controls/0/press/args/url"),
            "{args}"
        );
        let message = &errors[0].message;
        assert!(
            message.contains("\"Hook\"") && message.contains("input"),
            "{message}"
        );
        assert!(!message.contains("LEAK"), "{message}");
        // Bound to an input with a clean default, it is written.
        if args["url"].is_string() {
            let mut hook = input("hook", InputType::String);
            hook.default = Some(InputValue::Text {
                value: "https://example.com/hook".into(),
            });
            let request = ExportRequest {
                inputs: vec![hook],
                bindings: vec![bound("g.0.0", "url", "hook")],
                ..pack(&["home"])
            };
            let (text, _) = written(&export(&ws, &hand_env(), &request));
            assert!(!text.contains("LEAK"), "{args}");
        }
    }
    // Plain addresses, hosts and paths are fine.
    for args in [
        json!({ "url": "https://example.com/a?page=2&keyboard=1&tokens=3" }),
        json!({ "url": "https://example.com/api/webhooks" }),
        json!({ "url": "https://example.com/x?token=" }),
        json!({ "url": "example.com" }),
        json!({ "url": "/home/me/notes" }),
        json!({ "url": "mailto:me@example.com" }),
        json!({ "url": "Authorization is needed" }),
        json!({ "url": { "headers": { "Accept": "text/plain" } } }),
        json!({ "url": { "key": "Enter", "keys": "mod+c", "token": "" } }),
    ] {
        let ws = workspace(vec![surface(
            "home",
            vec![pad(0, "Hook", a("acme.notes", "mark", args.clone()))],
        )]);
        written(&export(&ws, &hand_env(), &pack(&["home"])));
    }
    // An argument itself named for a secret.
    let ws = workspace(vec![surface(
        "home",
        vec![pad(
            0,
            "Hook",
            a("acme.notes", "mark", json!({ "password": "LEAK" })),
        )],
    )]);
    let errors = refused(&ws, &pack(&["home"]));
    assert_eq!(codes(&errors), [E::Credential]);
    assert_eq!(
        errors[0].at.as_deref(),
        Some("/content/surfaces/0/pages/0/controls/0/press/args/password")
    );
    assert!(!errors[0].message.contains("LEAK"));
}

/// A feedback rule is written as it is: a credential in a state's
/// arguments or in an event's condition blocks the file too, at the rule.
#[test]
fn a_feedback_rule_carries_no_credential() {
    let mut address = StateAddress::new("acme.timer", "running");
    address
        .args
        .insert("feed".into(), "https://e.com/x?token=LEAK".into());
    let rules = [
        FeedbackRule::state(address, Test::HasValue).show("busy"),
        FeedbackRule::event("acme.ci", "done")
            .where_field("hook", "http://ha.local:8123/api/webhook/LEAK")
            .show("busy")
            .hold(Hold::Latch),
    ];
    for rule in rules {
        let mut watched = pad(
            0,
            "Watched",
            a("acme.notes", "mark", json!({ "folder": "notes" })),
        );
        watched.states.insert(
            "busy".into(),
            crate::core::model::Visual::solid(Rgb::new(0, 0, 255)),
        );
        watched.feedback = vec![
            FeedbackRule::state(StateAddress::new("acme.timer", "running"), Test::HasValue)
                .show("busy"),
            rule,
        ];
        let ws = workspace(vec![surface("work", vec![watched])]);
        let errors = refused(&ws, &pack(&["work"]));
        assert_eq!(codes(&errors), [E::Credential]);
        assert_eq!(
            errors[0].at.as_deref(),
            Some("/content/surfaces/0/pages/0/controls/0/feedback/1")
        );
        assert!(errors[0].message.contains("\"Watched\""));
        assert!(!errors[0].message.contains("LEAK"));
    }
}

/// SF-2: an input's default may not carry a credential either.
#[test]
fn a_default_carries_no_credential() {
    let ws = local_workspace();
    for value in [
        "https://example.com/x?token=LEAK",
        "http://ha.local:8123/api/webhook/LEAK",
    ] {
        let mut request = local_request();
        request.inputs[0].default = Some(InputValue::Text {
            value: value.into(),
        });
        let errors = refused(&ws, &request);
        assert_eq!(codes(&errors), [E::Credential], "{value}");
        assert_eq!(errors[0].at.as_deref(), Some("/manifest/inputs/0/default"));
        assert!(!errors[0].message.contains("LEAK"));
    }
}

// Review, suggestions and parameters.

#[test]
fn pads_that_run_things_are_listed_for_review() {
    let ws = default_workspace_for(Platform::Linux);
    let result = export(
        &ws,
        &hand_env(),
        &request(ArtifactKind::Surface, &["development"]),
    );
    let (_, artifact) = written(&result);
    let review = &result.preview.review;
    // Every shell command, and no launch by name without arguments.
    assert_eq!(review.len(), 10);
    assert!(review.iter().all(|r| r.kind == ReviewKind::RunsCommand));
    let tests = review.iter().find(|r| r.label == "Tests").unwrap();
    assert_eq!(tests.text, "sleep 2 && echo 'tests passed'");
    assert_eq!(tests.at, location("development", "g.0.1", Slot::Press));
    let warned = result
        .preview
        .warnings
        .iter()
        .filter(|w| w.code == E::ReviewBeforeSharing)
        .count();
    assert_eq!(warned, review.len());
    assert!(artifact.content.surfaces[0].pages[1].controls[0]
        .press
        .is_some());

    let ws = workspace(vec![surface(
        "mixed",
        vec![
            pad(
                0,
                "Launch",
                a(
                    "system",
                    "launchApplication",
                    json!({ "app": "code", "args": "--wait" }),
                ),
            ),
            pad(
                1,
                "Hook",
                a(
                    "http",
                    "request",
                    json!({ "method": "POST", "url": "https://example.com/h" }),
                ),
            ),
            pad(
                2,
                "Task",
                a("agent-control", "newTask", json!({ "task": "Report" })),
            ),
        ],
    )]);
    let result = export(&ws, &hand_env(), &pack(&["mixed"]));
    let kinds: Vec<(ReviewKind, &str)> = result
        .preview
        .review
        .iter()
        .map(|r| (r.kind, r.text.as_str()))
        .collect();
    assert_eq!(
        kinds,
        [
            (ReviewKind::LaunchesProgram, "code --wait"),
            (
                ReviewKind::SendsRequest,
                "POST https://example.com/h (goes to example.com:443)"
            ),
            (ReviewKind::InstructsAgent, "Report"),
        ]
    );
    let warning = &result.preview.warnings[0];
    assert_eq!(warning.code, E::ReviewBeforeSharing);
    assert_eq!(
        warning.message,
        "The pad \"Launch\" starts a program: check it holds no password or token before sharing."
    );
    assert_eq!(
        warning.at.as_deref(),
        Some("/content/surfaces/0/pages/0/controls/0/press")
    );
}

#[test]
fn text_arguments_are_offered_and_paths_suggested() {
    let ws = workspace(vec![surface(
        "dev",
        vec![
            pad(
                0,
                "Make",
                a("shell", "execute", json!({ "command": "/usr/bin/make" })),
            ),
            pad(
                1,
                "Notes",
                a(
                    "acme.notes",
                    "mark",
                    json!({ "folder": "~/notes", "count": 2 }),
                ),
            ),
            pad(
                2,
                "Tool",
                a(
                    "system",
                    "launchApplication",
                    json!({ "app": "C:\\Tools\\x.exe" }),
                ),
            ),
            pad(
                3,
                "Web",
                a("system", "openUrl", json!({ "url": "https://example.com" })),
            ),
            pad(
                4,
                "Self",
                a("navigation", "openSurface", json!({ "surfaceId": "dev" })),
            ),
        ],
    )]);
    let result = export(&ws, &hand_env(), &pack(&["dev"]));
    written(&result);
    let parameters: Vec<(&str, Option<&str>)> = result
        .preview
        .parameters
        .iter()
        .map(|p| (p.arg.as_str(), p.value.as_deref()))
        .collect();
    assert_eq!(
        parameters,
        [
            ("command", Some("/usr/bin/make")),
            ("folder", Some("~/notes")),
            ("app", Some("C:\\Tools\\x.exe")),
            ("url", Some("https://example.com")),
        ]
    );
    let suggested: Vec<(&str, SuggestedType)> = result
        .preview
        .suggestions
        .iter()
        .map(|s| (s.arg.as_str(), s.suggested_type))
        .collect();
    assert_eq!(
        suggested,
        [
            ("command", SuggestedType::Path),
            ("folder", SuggestedType::Path),
            ("app", SuggestedType::Path),
        ]
    );
    assert_eq!(
        result.preview.suggestions[1].at,
        location("dev", "g.1.0", Slot::Press)
    );
    let local = codes(&result.preview.warnings)
        .into_iter()
        .filter(|c| *c == E::LocalPath)
        .count();
    assert_eq!(local, 3);
}

/// A folder in someone's home folder inside a longer text carries their
/// user name: a warning, with no suggestion to ask for the whole text.
#[test]
fn a_home_folder_inside_a_text_is_a_warning() {
    let texts = [
        "cd /Users/me/proj && make",
        "--file=/home/me/x",
        "Read /Users/me/notes/todo.md first",
        "type C:\\Users\\me\\x.txt",
    ];
    let quiet = ["make -C build", "ls /Users and /home", "cat /usr/share/x"];
    let pads: Vec<Control> = texts
        .iter()
        .chain(&quiet)
        .enumerate()
        .map(|(i, text)| {
            let command = a("shell", "execute", json!({ "command": text }));
            pad(i as u8, "Run", command)
        })
        .collect();
    let ws = workspace(vec![surface("dev", pads)]);
    let result = export(&ws, &hand_env(), &pack(&["dev"]));
    written(&result);
    let local: Vec<&str> = result
        .preview
        .warnings
        .iter()
        .filter(|w| w.code == E::LocalPath)
        .map(|w| w.at.as_deref().unwrap_or_default())
        .collect();
    let expected: Vec<String> = (0..texts.len())
        .map(|i| format!("/content/surfaces/0/pages/0/controls/{i}/press/args/command"))
        .collect();
    assert_eq!(local, expected);
    assert!(result.preview.suggestions.is_empty());
}

// Requirements, devices and metadata.

#[test]
fn requirements_and_tested_devices() {
    let mut side = Control::new(ControlPos::Side { index: 2 }, "Side", Rgb::new(9, 9, 9));
    side.id = "s.2".into();
    let mut wide = pad(3, "Wide", None);
    wide.pos = ControlPos::grid(3, 4);
    wide.span = crate::core::model::Span { w: 2, h: 1 };
    let ws = workspace(vec![surface("dev", vec![side, wide])]);
    let result = export(&ws, &hand_env(), &pack(&["dev"]));
    let (_, artifact) = written(&result);
    let needed = compute_requirements(&artifact.content);
    assert_eq!(artifact.manifest.requirements, needed);
    assert_eq!(result.preview.requirements, needed);
    let grid = needed.grid.unwrap();
    assert_eq!((grid.min_width, grid.min_height), (5, 5));
    assert_eq!((needed.aux.top, needed.aux.side, needed.rgb), (0, 3, true));
    // Connected devices only, each model and kind once, hardware first.
    assert_eq!(
        result.preview.tested_with,
        [
            TestedWith {
                model: "launchpad-x".into(),
                kind: TestedKind::Hardware
            },
            TestedWith {
                model: "launchpad-x".into(),
                kind: TestedKind::Virtual
            },
        ]
    );
    assert_eq!(artifact.manifest.tested_with, result.preview.tested_with);
}

#[test]
fn metadata_left_empty_is_proposed() {
    let ws = default_workspace_for(Platform::Linux);
    let env = hand_env();
    let manifest = |request: &ExportRequest| {
        let result = export(&ws, &env, request);
        let preview = result.preview;
        assert!(preview.errors.is_empty(), "{:#?}", preview.errors);
        (
            preview.manifest.id,
            preview.manifest.name,
            preview.manifest.version,
            preview.file_name,
        )
    };
    let words = |id: &str, name: &str, version: &str, file: &str| {
        (
            id.to_string(),
            name.to_string(),
            version.to_string(),
            file.to_string(),
        )
    };
    assert_eq!(
        manifest(&request(ArtifactKind::Surface, &["development"])),
        words(
            "local.development",
            "Development",
            "1.0.0",
            "development-1.0.0.ldsurface"
        )
    );
    let named = |name: &str, author: Option<&str>, version: &str| ExportRequest {
        metadata: ExportMetadata {
            name: name.into(),
            version: version.into(),
            author: author.map(|a| Author {
                name: a.into(),
                url: None,
            }),
            ..ExportMetadata::default()
        },
        ..pack(&["development", "productivity"])
    };
    assert_eq!(
        manifest(&named("Video Editing!", Some("Acme Corp"), "")),
        words(
            "acme-corp.video-editing",
            "Video Editing!",
            "1.0.0",
            "video-editing-1.0.0.ldpack"
        )
    );
    // The reserved prefix is never proposed; a name may start with a digit.
    assert_eq!(
        manifest(&named("3D Tools", Some("LaunchDeck"), "2.0.0-Beta.1")),
        words(
            "local.pack-3d-tools",
            "3D Tools",
            "2.0.0-Beta.1",
            "3d-tools-2.0.0-beta.1.ldpack"
        )
    );
    assert_eq!(manifest(&named("  ", None, " ")).1, "Development");
    let (id, _, _, file) = manifest(&named("Long name ".repeat(6).trim(), None, ""));
    assert!(id.len() <= 64 && file.len() <= 64, "{id} {file}");
    assert!(file.ends_with("-1.0.0.ldpack"), "{file}");
}

#[test]
fn metadata_given_is_checked() {
    let ws = default_workspace_for(Platform::Linux);
    let check = |metadata: ExportMetadata| {
        let request = ExportRequest {
            metadata,
            ..pack(&["development"])
        };
        let errors = refused(&ws, &request);
        (errors[0].code, errors[0].at.clone().unwrap_or_default())
    };
    let given = |id: &str, name: &str, version: &str| ExportMetadata {
        id: id.into(),
        name: name.into(),
        version: version.into(),
        ..ExportMetadata::default()
    };
    assert_eq!(
        check(given("", "", "1.0")),
        (E::InvalidMetadata, "/manifest/version".into())
    );
    assert_eq!(
        check(given("Acme.Pack", "", "")),
        (E::InvalidMetadata, "/manifest/id".into())
    );
    assert_eq!(
        check(given("", &"x".repeat(61), "")),
        (E::InvalidMetadata, "/manifest/name".into())
    );
    assert_eq!(
        check(ExportMetadata {
            author: Some(Author {
                name: "Acme".into(),
                url: Some("http://example.com".into()),
            }),
            ..ExportMetadata::default()
        }),
        (E::InvalidMetadata, "/manifest/author/url".into())
    );
}

// The request and the preview as the interface carries them.

#[test]
fn the_request_and_the_preview_are_camel_case() {
    let request: ExportRequest = serde_json::from_value(json!({
        "kind": "pack",
        "surfaceIds": ["dev"],
        "metadata": { "id": "", "name": "Dev", "version": "1.0.0", "license": "MIT" },
        "includeContextRules": true,
        "inputs": [ { "id": "folder", "type": "path", "label": "Folder" },
                    { "id": "editor", "type": "application", "label": "Editor" } ],
        "bindings": [ { "at": { "surfaceSourceId": "dev", "pageId": "home", "controlId": "g.1.0", "slot": "press" },
                        "arg": "folder", "input": "folder" } ],
        "ruleInputs": [ { "rule": 1, "input": "editor" } ],
        "extraIdentities": [ { "rule": 0, "namespace": "macos", "id": "com.microsoft.VSCode" } ]
    }))
    .unwrap();
    assert_eq!(request.kind, ArtifactKind::Pack);
    assert_eq!(request.rule_inputs[0].rule, 1);
    // Only kind and surfaceIds are needed; nothing unknown is accepted.
    serde_json::from_value::<ExportRequest>(json!({ "kind": "surface", "surfaceIds": [] }))
        .unwrap();
    assert!(serde_json::from_value::<ExportRequest>(
        json!({ "kind": "pack", "surfaceIds": [], "path": "/tmp" })
    )
    .is_err());

    let result = export(&inputs_workspace(), &hand_env(), &request);
    assert!(result.bytes.is_some(), "{:#?}", result.preview.errors);
    let preview = serde_json::to_value(&result.preview).unwrap();
    let keys: Vec<&str> = preview
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    assert_eq!(
        keys,
        [
            "contentDigest",
            "contextRules",
            "dependencies",
            "errors",
            "fileName",
            "manifest",
            "parameters",
            "requirements",
            "review",
            "sizeBytes",
            "stripped",
            "suggestions",
            "surfaces",
            "testedWith",
            "warnings",
        ]
    );
    assert_eq!(
        preview["surfaces"][0],
        json!({ "localId": "dev", "artifactId": "dev", "name": "Surface dev", "pages": 1, "controls": 2 })
    );
    assert_eq!(
        preview["stripped"],
        json!({ "legacyBindings": 0, "legacyActions": 0, "contextHints": 0 })
    );
    assert_eq!(
        preview["contextRules"][1]["application"],
        json!({ "input": "editor" })
    );
    // The bound folder is asked of the importer: no value, no suggestion.
    assert_eq!(
        preview["parameters"][1],
        json!({
            "at": { "surfaceSourceId": "dev", "pageId": "home", "controlId": "g.1.0", "slot": "press" },
            "arg": "folder",
            "input": "folder"
        })
    );
    assert_eq!(preview["parameters"][0]["value"], "make");
    assert_eq!(preview["suggestions"], json!([]));
    assert_eq!(preview["warnings"][0]["code"], "reviewBeforeSharing");
}
