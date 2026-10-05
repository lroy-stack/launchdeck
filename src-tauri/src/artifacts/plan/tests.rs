//! The import plan on whole files and workspaces: ids and names (conflicts A
//! and B), earlier imports (C), context rules (D), duplicate ids (E), the
//! digest, inputs, pads that run things, dependencies, providers, devices
//! and the size of the workspace.

use std::collections::BTreeMap;
use std::sync::Arc;

use serde_json::{json, Value};

use super::inputs::{bound, check_value};
use super::instances::surface_digest;
use super::*;
use crate::actions::definition::{Assignment, Issue, ProviderKind, ProviderStatus};
use crate::actions::params::{ParamDef, RefTarget};
use crate::artifacts::env::{Environment, KnownAction, KnownDevice, KnownProvider, PlanHost};
use crate::artifacts::format::{
    compute_requirements, AnimationV1, AppRefV1, ArtifactKind, ArtifactV1, AssignmentV1, Color,
    ContentV1, ContextRuleV1, ControlV1, CreatedWith, DependencyV1, InputDef, InputOption,
    InputRef, InputType, InputValue, Integrity, ManifestV1, PageV1, Pos, RejectionCode, SurfaceV1,
    VisualV1,
};
use crate::artifacts::inspect::{inspect, ParsedArtifact};
use crate::artifacts::review::{Location, ReviewKind, Slot};
use crate::artifacts::testkit;
use crate::context::identity::{ApplicationIdentity, MACOS, WINDOWS, X11};
use crate::context::portable::{PortableApplication, PortableIdentity};
use crate::context::rules::{ContextRule, MAX_RULES};
use crate::core::model::{ImportOrigin, Rgb, Surface, Workspace, SCHEMA_VERSION};
use crate::devices::{AnimationCaps, AuxCaps, DeviceCapabilities, DeviceKind, GridCaps};
use crate::platform::Platform;
use crate::state::definition::ObservableDefinition;
use crate::state::feedback::{FeedbackRule, Test};
use crate::state::StateAddress;
use crate::surfaces::{blank_surface, default_workspace_for};

use BlockerCode as B;
use PlanWarningCode as W;

// What the tests build on.

fn a(provider: &str, action: &str, args: Value) -> AssignmentV1 {
    AssignmentV1 {
        provider: provider.into(),
        action: action.into(),
        args: args.as_object().cloned().unwrap_or_default(),
        inputs: BTreeMap::new(),
        surface_refs: Vec::new(),
    }
}

fn bind(mut assignment: AssignmentV1, arg: &str, input: &str) -> AssignmentV1 {
    assignment.inputs.insert(arg.into(), input.into());
    assignment
}

fn pad(col: u8, label: &str, press: Option<AssignmentV1>) -> ControlV1 {
    ControlV1 {
        id: format!("g.{col}.0"),
        pos: Pos::Grid { col, row: 0 },
        span: None,
        label: label.into(),
        description: String::new(),
        icon: None,
        visual: VisualV1 {
            color: Color(Rgb::new(200, 40, 40)),
            animation: AnimationV1::None,
        },
        looks: BTreeMap::new(),
        press,
        long_press: None,
        feedback: Vec::new(),
    }
}

fn surface(id: &str, name: &str, controls: Vec<ControlV1>) -> SurfaceV1 {
    SurfaceV1 {
        id: id.into(),
        name: name.into(),
        description: String::new(),
        icon: None,
        category: "custom".into(),
        pages: vec![PageV1 {
            id: "home".into(),
            name: "Home".into(),
            controls,
        }],
    }
}

fn manifest(version: &str) -> ManifestV1 {
    ManifestV1 {
        id: "acme.studio".into(),
        name: "Studio".into(),
        version: version.into(),
        description: String::new(),
        author: None,
        license: String::new(),
        created_with: CreatedWith {
            app: "launchdeck".into(),
            version: "0.3.0".into(),
        },
        requirements: compute_requirements(&ContentV1 {
            surfaces: vec![],
            context_rules: vec![],
        }),
        tested_with: Vec::new(),
        dependencies: Vec::new(),
        inputs: Vec::new(),
        integrity: Integrity {
            content: String::new(),
        },
    }
}

/// The file as `inspect` reads it: requirements and digest filled in,
/// written canonically, read back. Panics when the file is refused.
fn read(kind: ArtifactKind, mut manifest: ManifestV1, content: ContentV1) -> ParsedArtifact {
    manifest.requirements = compute_requirements(&content);
    manifest.integrity.content = content.digest().unwrap();
    let text = ArtifactV1 {
        kind,
        manifest,
        content,
    }
    .to_canonical()
    .unwrap();
    let name = format!("studio.{}", kind.extension());
    inspect(text.as_bytes(), &name).unwrap_or_else(|r| panic!("{r:?}\n{text}"))
}

fn pack_of(
    manifest: ManifestV1,
    surfaces: Vec<SurfaceV1>,
    rules: Vec<ContextRuleV1>,
) -> ParsedArtifact {
    read(
        ArtifactKind::Pack,
        manifest,
        ContentV1 {
            surfaces,
            context_rules: rules,
        },
    )
}

/// Studio 1.0.0: a mixer and lights, one pad each.
fn studio(version: &str) -> ParsedArtifact {
    pack_of(
        manifest(version),
        vec![
            surface("mixer", "Mixer", vec![pad(0, "Mute", None)]),
            surface("lights", "Lights", vec![pad(0, "On", None)]),
        ],
        vec![],
    )
}

fn workspace(surfaces: Vec<Surface>) -> Workspace {
    Workspace {
        schema_version: SCHEMA_VERSION,
        last_active_surface: None,
        surfaces,
        preferences: Default::default(),
        context: Default::default(),
    }
}

fn local(id: &str, name: &str) -> Surface {
    blank_surface(id, name)
}

/// A surface of `file` as an import of it wrote it.
fn imported(file: &ParsedArtifact, source: &str, local_id: &str, instance: u32) -> Surface {
    let v1 = file
        .content
        .surfaces
        .iter()
        .find(|s| s.id == source)
        .unwrap();
    let mut s = v1.to_model();
    s.id = local_id.into();
    s.imported_from = Some(ImportOrigin {
        format: file.kind.format().into(),
        artifact_id: file.manifest.id.clone(),
        artifact_name: file.manifest.name.clone(),
        artifact_version: file.manifest.version.clone(),
        instance,
        source_surface_id: source.into(),
        content_digest: file.content_digest.clone(),
        surface_digest: surface_digest(&s).unwrap(),
    });
    s
}

fn action(params: Vec<ParamDef>, privileged: bool) -> KnownAction {
    KnownAction { params, privileged }
}

fn provider(
    kind: ProviderKind,
    version: &str,
    status: ProviderStatus,
    actions: Vec<(&str, KnownAction)>,
) -> KnownProvider {
    KnownProvider {
        kind,
        name: "Provider".into(),
        version: version.into(),
        status,
        actions: actions
            .into_iter()
            .map(|(id, a)| (id.to_string(), a))
            .collect(),
    }
}

fn caps(width: u8, rgb: bool) -> DeviceCapabilities {
    DeviceCapabilities {
        grid: Some(GridCaps {
            width,
            height: width,
        }),
        aux: AuxCaps {
            top: 8,
            side: 8,
            logo: true,
        },
        rgb,
        velocity: true,
        pressure: false,
        animations: AnimationCaps {
            pulse: true,
            flash: true,
        },
    }
}

/// An installation written by hand: the built-in providers the tests use,
/// two plugins (Notes, ready; Switch, off), what offers states and events,
/// two devices, X11.
fn env() -> Environment {
    let ready = ProviderStatus::Ready;
    let builtin = |actions| provider(ProviderKind::Builtin, "0.3.0", ready, actions);
    let providers = [
        (
            "navigation",
            builtin(vec![(
                "openSurface",
                action(
                    vec![ParamDef::reference(
                        "surfaceId",
                        "Surface",
                        RefTarget::Surface,
                    )],
                    false,
                ),
            )]),
        ),
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
            "agent-control",
            provider(
                ProviderKind::Module,
                "0.3.0",
                ready,
                vec![("newTask", action(vec![], false))],
            ),
        ),
        (
            "acme.notes",
            provider(
                ProviderKind::Plugin,
                "1.2.3",
                ready,
                vec![(
                    "mark",
                    action(vec![ParamDef::string("folder", "Folder")], false),
                )],
            ),
        ),
        (
            "acme.switch",
            provider(
                ProviderKind::Plugin,
                "0.1.0",
                ProviderStatus::Disabled,
                vec![("toggle", action(vec![], false))],
            ),
        ),
    ];
    Environment {
        app_version: "0.3.0".into(),
        namespace: Some(X11.into()),
        providers: providers
            .into_iter()
            .map(|(id, p)| (id.to_string(), p))
            .collect(),
        observable_sources: ["runtime", "device", "agent-control", "acme.notes"]
            .into_iter()
            .map(String::from)
            .collect(),
        devices: vec![
            KnownDevice {
                name: "Virtual Launchpad X".into(),
                model: "launchpad-x".into(),
                kind: DeviceKind::Virtual,
                connected: true,
                capabilities: caps(8, true),
            },
            KnownDevice {
                name: "Small pad".into(),
                model: "small-pad".into(),
                kind: DeviceKind::Hardware,
                connected: true,
                capabilities: caps(2, false),
            },
        ],
        recent_applications: Vec::new(),
        host: None,
    }
}

/// The runtime's checks, as a test wants them: an argument value an
/// action refuses, an action refused as a whole, and the states offered.
#[derive(Default)]
struct Host {
    /// (`provider.action`, parameter, refused value).
    refused_values: Vec<(String, String, Value)>,
    /// `provider.action` refused with an issue about no parameter.
    refused_actions: Vec<String>,
    states: Vec<ObservableDefinition>,
}

impl PlanHost for Host {
    fn validate_assignment(&self, a: &Assignment) -> Result<Assignment, Vec<Issue>> {
        let id = a.definition_id();
        let mut issues: Vec<Issue> = self
            .refused_values
            .iter()
            .filter(|(action, param, value)| *action == id && a.args.get(param) == Some(value))
            .map(|(_, param, _)| Issue::param(param, format!("`{param}` is refused here")))
            .collect();
        if self.refused_actions.contains(&id) {
            issues.push(Issue::new("this action needs a newer plugin"));
        }
        if issues.is_empty() {
            Ok(a.clone())
        } else {
            Err(issues)
        }
    }

    fn state_definition(&self, provider: &str, id: &str) -> Option<ObservableDefinition> {
        self.states
            .iter()
            .find(|d| d.provider == provider && d.local == id)
            .cloned()
    }
}

fn with_host(host: Host) -> Environment {
    Environment {
        host: Some(Arc::new(host)),
        ..env()
    }
}

fn plan_of(file: &ParsedArtifact, ws: &Workspace, decisions: &ImportDecisions) -> ImportPlan {
    plan(file, ws, &env(), decisions)
}

fn blockers(p: &ImportPlan) -> Vec<BlockerCode> {
    p.blockers.iter().map(|b| b.code).collect()
}

fn warnings(p: &ImportPlan) -> Vec<PlanWarningCode> {
    p.warnings.iter().map(|w| w.code).collect()
}

fn local_ids(p: &ImportPlan) -> Vec<&str> {
    p.surfaces.iter().map(|s| s.local_id.as_str()).collect()
}

fn as_new() -> ImportDecisions {
    ImportDecisions {
        resolution: Some(Resolution::ImportAsNew),
        ..ImportDecisions::default()
    }
}

fn replace(instance: u32) -> ImportDecisions {
    ImportDecisions {
        resolution: Some(Resolution::Replace { instance }),
        ..ImportDecisions::default()
    }
}

// A first import.

#[test]
fn a_first_import_adds_every_surface_and_changes_nothing_else() {
    let file = studio("1.0.0");
    let ws = workspace(vec![local("desk", "Desk")]);
    let p = plan_of(&file, &ws, &ImportDecisions::default());
    assert!(p.ready, "{:?}", p.blockers);
    assert!(p.existing.is_empty());
    assert_eq!(p.effects.instance, 1);
    assert_eq!(
        p.effects.add_surfaces,
        [
            AddedSurface {
                source_id: "mixer".into(),
                local_id: "mixer".into(),
                name: "Mixer".into()
            },
            AddedSurface {
                source_id: "lights".into(),
                local_id: "lights".into(),
                name: "Lights".into()
            },
        ]
    );
    assert!(p.effects.replace_surfaces.is_empty());
    assert!(p.effects.add_rules.is_empty() && p.effects.removed_slots.is_empty());
    assert_eq!(p.surfaces[0].action, SurfaceAction::Add);
    assert_eq!((p.surfaces[0].pages, p.surfaces[0].controls), (1, 1));
    assert_eq!(p.suggested, ImportDecisions::default());
    assert_eq!(p.plan_id.len(), 32);
    assert!(p.plan_id.bytes().all(|b| b.is_ascii_hexdigit()));
    assert!(p.plan_digest.starts_with("sha256:"));
    assert_eq!(p.source.file_name, "studio.ldpack");
    assert_eq!(p.source.content_digest, file.content_digest);
    assert_eq!(
        (p.artifact.id.as_str(), p.artifact.version.as_str()),
        ("acme.studio", "1.0.0")
    );
    // Nothing in the plan refers to an outside system.
    assert!(p.warnings.is_empty(), "{:?}", p.warnings);
}

// Conflict A: the same surface id is remapped, not a choice.

#[test]
fn a_surface_id_taken_here_gets_the_next_free_one() {
    let cases: [(&[&str], &[&str], &[&str]); 4] = [
        (&[], &["mixer", "lights"], &["mixer", "lights"]),
        (&["mixer"], &["mixer", "lights"], &["mixer-2", "lights"]),
        (
            &["mixer", "mixer-2"],
            &["mixer", "lights"],
            &["mixer-3", "lights"],
        ),
        // An id given earlier in the table is taken too.
        (&["mixer"], &["mixer", "mixer-2"], &["mixer-2", "mixer-2-2"]),
    ];
    for (here, incoming, expected) in cases {
        let ws = workspace(here.iter().map(|id| local(id, "Here")).collect());
        let file = pack_of(
            manifest("1.0.0"),
            incoming
                .iter()
                .map(|id| surface(id, "There", vec![]))
                .collect(),
            vec![],
        );
        let p = plan_of(&file, &ws, &ImportDecisions::default());
        assert_eq!(local_ids(&p), expected, "{here:?} {incoming:?}");
        for s in &p.surfaces {
            assert_eq!(s.id_changed, s.local_id != s.source_id);
        }
        assert!(p.ready);
    }
}

// Conflict B: the same display name is kept, and said.

#[test]
fn a_name_used_here_is_kept_and_pointed_out() {
    let ws = workspace(vec![local("desk", "Mixer"), local("other", "Other")]);
    let p = plan_of(&studio("1.0.0"), &ws, &ImportDecisions::default());
    assert_eq!(p.surfaces[0].name, "Mixer");
    assert_eq!(p.surfaces[0].same_name_as.as_deref(), Some("desk"));
    assert_eq!(p.surfaces[1].same_name_as, None);
    assert!(p.ready);
}

// Conflict C: the same file imported before.

#[test]
fn an_earlier_import_must_be_resolved_and_is_described() {
    let old = studio("1.0.0");
    let ws = workspace(vec![
        imported(&old, "mixer", "mixer", 1),
        imported(&old, "lights", "lights", 1),
        local("desk", "Desk"),
    ]);
    let file = studio("1.1.0");
    let p = plan_of(&file, &ws, &ImportDecisions::default());
    assert_eq!(p.existing.len(), 1);
    let instance = &p.existing[0];
    assert_eq!(
        (
            instance.instance,
            instance.version.as_str(),
            instance.relation
        ),
        (1, "1.0.0", Relation::Newer)
    );
    // The content is the same; only the version changed.
    assert!(instance.identical_content);
    assert_eq!(
        instance
            .surfaces
            .iter()
            .map(|s| (s.local_id.as_str(), s.source_id.as_str(), s.locally_edited))
            .collect::<Vec<_>>(),
        [("mixer", "mixer", false), ("lights", "lights", false)]
    );
    assert_eq!(blockers(&p), [B::ResolutionNeeded]);
    assert_eq!(p.suggested.resolution, Some(Resolution::ImportAsNew));
    // Shown as a new instance until the person decides.
    assert_eq!(p.effects.instance, 2);

    let p = plan_of(&file, &ws, &as_new());
    assert!(p.ready, "{:?}", p.blockers);
    assert_eq!(p.effects.instance, 2);
    assert_eq!(local_ids(&p), ["mixer-2", "lights-2"]);

    // An instance that does not exist is no resolution.
    let p = plan_of(&file, &ws, &replace(7));
    assert_eq!(blockers(&p), [B::ResolutionNeeded]);
}

#[test]
fn the_relation_is_by_semver_precedence_and_identical_content_is_seen() {
    let cases = [
        ("1.0.0", "1.0.0", Relation::Same),
        ("1.0.0", "1.1.0", Relation::Newer),
        ("2.0.0", "1.1.0", Relation::Older),
        ("1.0.0-beta.1", "1.0.0", Relation::Newer),
        ("1.10.0", "1.9.0", Relation::Older),
    ];
    for (here, file, expected) in cases {
        let old = studio(here);
        let ws = workspace(vec![imported(&old, "mixer", "mixer", 1)]);
        let p = plan_of(&studio(file), &ws, &as_new());
        assert_eq!(p.existing[0].relation, expected, "{here} -> {file}");
    }
    // The same content imported again: identical, and still a choice.
    let file = studio("1.0.0");
    let ws = workspace(vec![imported(&file, "mixer", "mixer", 3)]);
    let p = plan_of(&file, &ws, &ImportDecisions::default());
    assert!(p.existing[0].identical_content);
    assert_eq!(p.existing[0].relation, Relation::Same);
    assert_eq!(blockers(&p), [B::ResolutionNeeded]);
    assert_eq!(plan_of(&file, &ws, &as_new()).effects.instance, 4);
    let changed = pack_of(
        manifest("1.0.0"),
        vec![surface("mixer", "Mixer", vec![pad(0, "Mute all", None)])],
        vec![],
    );
    assert!(!plan_of(&changed, &ws, &as_new()).existing[0].identical_content);
    // Another file, or the same id as a Surface file, is not an earlier import.
    let other = pack_of(
        ManifestV1 {
            id: "acme.other".into(),
            ..manifest("1.0.0")
        },
        vec![surface("mixer", "Mixer", vec![])],
        vec![],
    );
    assert!(plan_of(&other, &ws, &ImportDecisions::default())
        .existing
        .is_empty());
}

#[test]
fn replacing_an_import_keeps_ids_adds_what_is_new_and_keeps_what_is_gone() {
    let old = pack_of(
        manifest("1.0.0"),
        vec![
            surface("mixer", "Mixer", vec![pad(0, "Mute", None)]),
            surface("scenes", "Scenes", vec![]),
        ],
        vec![],
    );
    // Instance 1 was imported next to a local "mixer".
    let ws = workspace(vec![
        local("mixer", "My mixer"),
        imported(&old, "mixer", "mixer-2", 1),
        imported(&old, "scenes", "scenes", 1),
    ]);
    let p = plan_of(&studio("1.1.0"), &ws, &replace(1));
    assert!(p.ready, "{:?}", p.blockers);
    assert_eq!(p.effects.instance, 1);
    assert_eq!(
        p.effects.replace_surfaces,
        [ReplacedSurface {
            source_id: "mixer".into(),
            local_id: "mixer-2".into()
        }]
    );
    assert_eq!(
        p.effects
            .add_surfaces
            .iter()
            .map(|s| s.local_id.as_str())
            .collect::<Vec<_>>(),
        ["lights"]
    );
    assert_eq!(
        p.surfaces[0].action,
        SurfaceAction::Replace {
            previous_version: "1.0.0".into(),
            locally_edited: false
        }
    );
    assert!(p.surfaces[0].id_changed);
    // The replaced surface is not "another surface with the same name".
    assert_eq!(p.surfaces[0].same_name_as, None);
    assert_eq!(warnings(&p), [W::KeptFromPreviousVersion]);
    assert!(p.warnings[0].message.contains("Scenes"));
}

#[test]
fn replacing_an_edited_surface_waits_for_a_confirmation() {
    let old = studio("1.0.0");
    let mut edited = imported(&old, "mixer", "mixer", 1);
    edited.pages[0].controls[0].label = "Mute all".into();
    let ws = workspace(vec![edited, imported(&old, "lights", "lights", 1)]);
    let file = studio("1.1.0");

    let p = plan_of(&file, &ws, &ImportDecisions::default());
    assert_eq!(
        p.existing[0]
            .surfaces
            .iter()
            .map(|s| s.locally_edited)
            .collect::<Vec<_>>(),
        [true, false]
    );
    let p = plan_of(&file, &ws, &replace(1));
    assert_eq!(blockers(&p), [B::OverwriteEditsNotConfirmed]);
    assert!(p.blockers[0].message.contains("\"Mixer\""));
    assert_eq!(
        p.surfaces[0].action,
        SurfaceAction::Replace {
            previous_version: "1.0.0".into(),
            locally_edited: true
        }
    );
    let confirmed = ImportDecisions {
        confirm_overwrite_edits: true,
        ..replace(1)
    };
    let p = plan_of(&file, &ws, &confirmed);
    assert!(p.ready, "{:?}", p.blockers);
    // Importing as new overwrites nothing.
    assert!(plan_of(&file, &ws, &as_new()).ready);
}

// Conflict D: context rules.

fn app(name: &str, identities: &[(&str, &str)]) -> AppRefV1 {
    AppRefV1::Application(PortableApplication {
        name: name.into(),
        identities: identities
            .iter()
            .map(|(namespace, id)| PortableIdentity {
                namespace: (*namespace).into(),
                id: (*id).into(),
            })
            .collect(),
    })
}

fn to(application: AppRefV1, surface: &str) -> ContextRuleV1 {
    ContextRuleV1 {
        application,
        surface: surface.into(),
        page: None,
    }
}

fn editor_input() -> InputDef {
    InputDef {
        id: "editor".into(),
        kind: InputType::Application,
        label: "Your editor".into(),
        description: String::new(),
        required: true,
        default: None,
        options: Vec::new(),
        max_length: None,
    }
}

/// Rules for Code, GIMP (also a local rule), Code again, an application
/// named only on macOS, and the editor the person names.
fn with_rules() -> ParsedArtifact {
    pack_of(
        ManifestV1 {
            inputs: vec![editor_input()],
            ..manifest("1.0.0")
        },
        vec![
            surface("mixer", "Mixer", vec![]),
            surface("lights", "Lights", vec![]),
        ],
        vec![
            to(
                app("Code", &[(X11, "code"), (MACOS, "com.microsoft.VSCode")]),
                "mixer",
            ),
            to(app("GIMP", &[(X11, "Gimp")]), "lights"),
            to(app("Code", &[(X11, "code")]), "lights"),
            to(
                app("Final Cut Pro", &[(MACOS, "com.apple.FinalCut")]),
                "mixer",
            ),
            to(
                AppRefV1::Input(InputRef {
                    input: "editor".into(),
                }),
                "lights",
            ),
        ],
    )
}

fn local_rule(id: &str, surface: &str) -> ContextRule {
    ContextRule {
        application: ApplicationIdentity::new(X11, id, id),
        surface_id: surface.into(),
        page_id: None,
    }
}

fn outcomes(p: &ImportPlan) -> Vec<RuleOutcome> {
    p.context_rules.iter().map(|r| r.outcome.clone()).collect()
}

#[test]
fn context_rules_are_added_only_when_selected() {
    let mut ws = workspace(vec![local("desk", "Desk")]);
    ws.context.rules = vec![local_rule("gimp", "desk")];
    let file = with_rules();
    let p = plan_of(&file, &ws, &ImportDecisions::default());
    assert!(outcomes(&p).iter().all(|o| *o == RuleOutcome::NotSelected));
    assert!(p.effects.add_rules.is_empty());
    assert!(p.suggested.context_rules.is_empty());
    // A rule not selected needs no answer.
    assert!(p.ready, "{:?}", p.blockers);
    // Still, what each names here is shown.
    assert_eq!(
        p.context_rules[0]
            .application
            .as_ref()
            .map(|a| a.id.as_str()),
        Some("code")
    );
    assert_eq!(p.context_rules[0].application_name, "Code");
    assert_eq!(p.context_rules[4].application_name, "Your editor");
}

#[test]
fn each_selected_rule_is_added_or_skipped_for_its_reason() {
    let mut ws = workspace(vec![local("mixer", "Mixer here")]);
    ws.context.rules = vec![local_rule("gimp", "mixer")];
    let file = with_rules();
    let all = ImportDecisions {
        context_rules: vec![0, 1, 2, 3, 4],
        ..ImportDecisions::default()
    };
    let p = plan_of(&file, &ws, &all);
    assert_eq!(
        outcomes(&p),
        [
            RuleOutcome::Add { position: 1 },
            RuleOutcome::SkipConflict { local_rule: 0 },
            RuleOutcome::SkipShadowed { by: 0 },
            RuleOutcome::SkipNoIdentity,
            RuleOutcome::NeedsInput {
                input: "editor".into()
            },
        ]
    );
    // The rule follows the surface to its local id.
    assert_eq!(
        p.effects.add_rules,
        [AddedRule {
            index: 0,
            application: ApplicationIdentity::new(X11, "code", "Code"),
            surface_local_id: "mixer-2".into(),
            page_id: None,
        }]
    );
    assert_eq!(p.context_rules[0].surface_local_id, "mixer-2");
    // A selected rule takes the input: it needs an answer.
    assert_eq!(blockers(&p), [B::InputRequired]);
    assert_eq!(p.blockers[0].input.as_deref(), Some("editor"));

    // Answered, the input's rule is added after the others.
    let answered = ImportDecisions {
        inputs: [(
            "editor".to_string(),
            InputValue::Application {
                namespace: X11.into(),
                id: "kate".into(),
                display_name: "Kate".into(),
            },
        )]
        .into(),
        ..all.clone()
    };
    let p = plan_of(&file, &ws, &answered);
    assert!(p.ready, "{:?}", p.blockers);
    assert_eq!(p.context_rules[4].outcome, RuleOutcome::Add { position: 2 });
    assert_eq!(p.context_rules[4].application_name, "Kate");
    assert_eq!(p.effects.add_rules.len(), 2);

    // Without an observer on this system, nothing names an application.
    let blind = Environment {
        namespace: None,
        ..env()
    };
    let p = plan(&file, &ws, &blind, &all);
    assert_eq!(p.context_rules[0].outcome, RuleOutcome::SkipNoIdentity);
    assert_eq!(p.context_rules[0].application, None);
}

#[test]
fn rules_past_the_limit_are_skipped() {
    let mut ws = workspace(vec![local("desk", "Desk")]);
    ws.context.rules = (0..MAX_RULES - 1)
        .map(|i| local_rule(&format!("app{i}"), "desk"))
        .collect();
    let file = pack_of(
        manifest("1.0.0"),
        vec![surface("mixer", "Mixer", vec![])],
        vec![
            to(app("One", &[(X11, "one")]), "mixer"),
            to(app("Two", &[(X11, "two")]), "mixer"),
        ],
    );
    let p = plan_of(
        &file,
        &ws,
        &ImportDecisions {
            context_rules: vec![0, 1],
            ..ImportDecisions::default()
        },
    );
    assert_eq!(
        outcomes(&p),
        [
            RuleOutcome::Add {
                position: MAX_RULES - 1
            },
            RuleOutcome::SkipCapacity
        ]
    );
    // Auto Context's switch and fallback are never part of an import.
    assert!(p.ready);
}

// Conflict E: duplicate ids inside the file refuse it before any plan.

#[test]
fn duplicate_ids_inside_a_file_refuse_it() {
    let refused = |content: ContentV1| {
        let mut m = manifest("1.0.0");
        m.requirements = compute_requirements(&content);
        m.integrity.content = content.digest().unwrap();
        let text = ArtifactV1 {
            kind: ArtifactKind::Pack,
            manifest: m,
            content,
        }
        .to_canonical()
        .unwrap();
        inspect(text.as_bytes(), "x.ldpack").unwrap_err().code
    };
    let pages = |a: &str, b: &str| {
        let mut s = surface("mixer", "Mixer", vec![]);
        s.pages.push(PageV1 {
            id: b.into(),
            name: "Two".into(),
            controls: vec![],
        });
        s.pages[0].id = a.into();
        s
    };
    let mut twin_pads = surface("mixer", "Mixer", vec![pad(0, "A", None), pad(1, "B", None)]);
    twin_pads.pages[0].controls[1].id = "g.0.0".into();
    for content in [
        vec![surface("mixer", "A", vec![]), surface("mixer", "B", vec![])],
        vec![pages("home", "home")],
        vec![twin_pads],
    ] {
        assert_eq!(
            refused(ContentV1 {
                surfaces: content,
                context_rules: vec![],
            }),
            RejectionCode::InvalidContent
        );
    }
}

// The digest.

#[test]
fn the_digest_covers_what_is_written_and_nothing_else() {
    let file = with_rules();
    let ws = workspace(vec![local("desk", "Desk")]);
    let decisions = ImportDecisions {
        context_rules: vec![0],
        ..ImportDecisions::default()
    };
    let first = plan_of(&file, &ws, &decisions);
    let again = plan_of(&file, &ws, &decisions);
    assert_eq!(first.plan_digest, again.plan_digest);
    assert_ne!(first.plan_id, again.plan_id);

    // A device unplugged, another plugin version: the same writes.
    let mut unplugged = env();
    for d in &mut unplugged.devices {
        d.connected = false;
    }
    unplugged.devices.pop();
    assert_eq!(
        plan(&file, &ws, &unplugged, &decisions).plan_digest,
        first.plan_digest
    );

    // A surface that takes an id here, a rule more or less: other writes.
    let crowded = workspace(vec![local("desk", "Desk"), local("mixer", "Mixer")]);
    let moved = plan_of(&file, &crowded, &decisions);
    assert_eq!(local_ids(&moved), ["mixer-2", "lights"]);
    assert_ne!(moved.plan_digest, first.plan_digest);
    assert_ne!(
        plan_of(&file, &ws, &ImportDecisions::default()).plan_digest,
        first.plan_digest
    );
}

// Inputs.

fn input(id: &str, kind: InputType) -> InputDef {
    InputDef {
        id: id.into(),
        kind,
        label: format!("Input {id}"),
        description: String::new(),
        required: true,
        default: None,
        options: if kind == InputType::Enum {
            vec![
                InputOption {
                    value: "fast".into(),
                    label: "Fast".into(),
                },
                InputOption {
                    value: "slow".into(),
                    label: "Slow".into(),
                },
            ]
        } else {
            Vec::new()
        },
        max_length: (kind == InputType::String).then_some(8),
    }
}

fn text(value: &str) -> InputValue {
    InputValue::Text {
        value: value.into(),
    }
}

fn application(namespace: &str, id: &str) -> InputValue {
    InputValue::Application {
        namespace: namespace.into(),
        id: id.into(),
        display_name: "Editor".into(),
    }
}

#[test]
fn each_kind_of_answer_is_checked() {
    use InputType as T;
    let unix = Platform::Linux;
    let cases = [
        (T::Path, text("/Users/me/notes"), true),
        (T::Path, text("notes/today"), false),
        (T::Path, text(""), false),
        (T::Path, text("C:\\Users\\me"), false),
        (T::Path, text("/tmp/a\u{202E}b"), false),
        (T::Url, text("https://example.com/hook"), true),
        (T::Url, text("http://192.168.1.4:8123/api"), true),
        (T::Url, text("ftp://example.com/file"), false),
        (T::Url, text("https://me:secret@example.com"), false),
        (T::Url, text("example.com"), false),
        (T::Enum, text("fast"), true),
        (T::Enum, text("medium"), false),
        (T::String, text(""), true),
        (T::String, text("12345678"), true),
        (T::String, text("123456789"), false),
        (T::String, text("a\u{0}b"), false),
        (T::String, application(X11, "kate"), false),
        (T::Application, application(X11, "kate"), true),
        (T::Application, application(WINDOWS, "kate.exe"), false),
        (T::Application, application(X11, " "), false),
        (T::Application, text("kate"), false),
    ];
    for (kind, value, ok) in cases {
        let issues = check_value(&input("x", kind), &value, Some(X11), unix);
        assert_eq!(issues.is_empty(), ok, "{kind:?} {value:?}: {issues:?}");
    }
    // A path is full by the rules of the system it is imported on.
    let windows = |path: &str| {
        check_value(
            &input("x", T::Path),
            &text(path),
            Some(WINDOWS),
            Platform::Windows,
        )
        .is_empty()
    };
    assert!(windows("C:\\Users\\me") && windows("d:/notes") && windows("\\\\nas\\share"));
    assert!(!windows("/Users/me") && !windows("notes"));
    // Without an observer, no application is an answer.
    assert!(!check_value(
        &input("x", T::Application),
        &application(X11, "kate"),
        None,
        unix
    )
    .is_empty());
}

/// A Notes pad whose folder is asked of the importer, a request whose
/// address is, and a pad that names another surface of the file.
fn with_inputs(folder_required: bool) -> ParsedArtifact {
    let mut folder = input("folder", InputType::Path);
    folder.required = folder_required;
    let mut open = a(
        "navigation",
        "openSurface",
        json!({ "surfaceId": "lights" }),
    );
    open.surface_refs = vec!["surfaceId".into()];
    pack_of(
        ManifestV1 {
            inputs: vec![folder, input("hook", InputType::Url)],
            dependencies: vec![DependencyV1 {
                plugin_id: "acme.notes".into(),
                name: Some("Notes".into()),
                version: "^1.2".into(),
                required: true,
            }],
            ..manifest("1.0.0")
        },
        vec![
            surface(
                "mixer",
                "Mixer",
                vec![
                    pad(
                        0,
                        "Notes",
                        Some(bind(
                            a("acme.notes", "mark", json!({ "folder": "/notes" })),
                            "folder",
                            "folder",
                        )),
                    ),
                    pad(1, "Lights", Some(open)),
                    pad(
                        2,
                        "Ping",
                        Some(bind(
                            a("http", "request", json!({ "url": "https://example.com" })),
                            "url",
                            "hook",
                        )),
                    ),
                ],
            ),
            surface("lights", "Lights", vec![]),
        ],
        vec![],
    )
}

fn answers(pairs: &[(&str, InputValue)]) -> ImportDecisions {
    ImportDecisions {
        inputs: pairs
            .iter()
            .map(|(id, v)| (id.to_string(), v.clone()))
            .collect(),
        runs: Some(Runs::Keep),
        ..ImportDecisions::default()
    }
}

#[test]
fn inputs_need_valid_answers_where_they_are_used() {
    let file = with_inputs(true);
    let ws = workspace(vec![local("lights", "Lights")]);

    let p = plan_of(&file, &ws, &answers(&[]));
    assert_eq!(blockers(&p), [B::InputRequired, B::InputRequired]);
    assert_eq!(p.inputs[0].used_by.len(), 1);
    assert_eq!(p.inputs[0].used_by[0].control_id, "g.0.0");

    let p = plan_of(
        &file,
        &ws,
        &answers(&[
            ("folder", text("notes")),
            ("hook", text("ftp://example.com")),
        ]),
    );
    assert_eq!(blockers(&p), [B::InputInvalid, B::InputInvalid]);
    assert_eq!(p.inputs[0].value, Some(text("notes")));
    assert!(!p.inputs[0].issues.is_empty());
    assert!(p.effects.input_values.is_empty());

    let good = answers(&[
        ("folder", text("/home/me/notes")),
        ("hook", text("https://hooks.example.com/x")),
    ]);
    let p = plan_of(&file, &ws, &good);
    assert!(p.ready, "{:?}", p.blockers);
    assert_eq!(p.effects.input_values, good.inputs);
    // The review shows the answer, not the file's value.
    let request = p
        .review
        .iter()
        .find(|r| r.kind == ReviewKind::SendsRequest)
        .unwrap();
    assert!(
        request.text.contains("hooks.example.com"),
        "{}",
        request.text
    );
}

#[test]
fn an_answer_is_written_into_the_pad_and_a_surface_reference_follows_its_surface() {
    let file = with_inputs(false);
    let defs = &file.manifest.inputs;
    let pads = &file.content.surfaces[0].pages[0].controls;
    let ids: BTreeMap<String, String> = [
        ("mixer".to_string(), "mixer".to_string()),
        ("lights".to_string(), "lights-2".to_string()),
    ]
    .into();
    let values: BTreeMap<String, InputValue> = [("folder".to_string(), text("/srv"))].into();
    let notes = pads[0].press.as_ref().unwrap();
    assert_eq!(
        bound(notes, defs, &values, &ids).args,
        json!({ "folder": "/srv" })
    );
    // Optional and unanswered: the argument is left out.
    assert_eq!(bound(notes, defs, &BTreeMap::new(), &ids).args, json!({}));
    let open = pads[1].press.as_ref().unwrap();
    assert_eq!(
        bound(open, defs, &values, &ids).args,
        json!({ "surfaceId": "lights-2" })
    );
    // A required input not answered yet keeps the file's value.
    let hook = pads[2].press.as_ref().unwrap();
    assert_eq!(
        bound(hook, defs, &values, &ids).args,
        json!({ "url": "https://example.com" })
    );
}

#[test]
fn what_the_installed_provider_says_of_an_answer_belongs_to_the_input() {
    let file = with_inputs(true);
    let ws = workspace(vec![]);
    let host = Host {
        refused_values: vec![(
            "acme.notes.mark".into(),
            "folder".into(),
            json!("/forbidden"),
        )],
        ..Host::default()
    };
    let decisions = answers(&[
        ("folder", text("/forbidden")),
        ("hook", text("https://example.com/x")),
    ]);
    let p = plan(&file, &ws, &with_host(host), &decisions);
    assert_eq!(blockers(&p), [B::InputInvalid]);
    assert_eq!(p.blockers[0].input.as_deref(), Some("folder"));
    // A refused answer is not one that may be written.
    assert_eq!(p.effects.input_values.keys().collect::<Vec<_>>(), ["hook"]);
    assert!(
        p.inputs[0].issues[0].contains("\"folder\" is refused"),
        "{:?}",
        p.inputs[0].issues
    );

    // A request address given as an answer is read as the runtime reads it.
    let decisions = answers(&[
        ("folder", text("/srv")),
        ("hook", text("http://0x7f000001/")),
    ]);
    let p = plan(&file, &ws, &env(), &decisions);
    assert_eq!(blockers(&p), [B::InputInvalid], "{:?}", p.inputs);
    assert_eq!(p.blockers[0].input.as_deref(), Some("hook"));
}

#[test]
fn what_the_installed_provider_says_of_a_pad_or_a_rule_is_a_warning() {
    let mut watch = pad(3, "Watch", None);
    watch.looks.insert("on".into(), watch.visual);
    watch.feedback = vec![
        FeedbackRule::state(StateAddress::new("acme.notes", "unread"), Test::HasValue).show("on"),
        FeedbackRule::state(StateAddress::new("runtime", "surface"), Test::HasValue).show("on"),
    ];
    let file = pack_of(
        ManifestV1 {
            dependencies: vec![DependencyV1 {
                plugin_id: "acme.notes".into(),
                name: None,
                version: "^1".into(),
                required: true,
            }],
            ..manifest("1.0.0")
        },
        vec![surface(
            "mixer",
            "Mixer",
            vec![
                pad(
                    0,
                    "Mark",
                    Some(a("acme.notes", "mark", json!({ "folder": "/x" }))),
                ),
                watch,
            ],
        )],
        vec![],
    );
    let host = Host {
        refused_actions: vec!["acme.notes.mark".into()],
        states: vec![ObservableDefinition::state("runtime", "surface", "Surface", "").text()],
        ..Host::default()
    };
    let p = plan(
        &file,
        &workspace(vec![]),
        &with_host(host),
        &ImportDecisions::default(),
    );
    assert!(p.ready, "{:?}", p.blockers);
    let found: Vec<(PlanWarningCode, Option<&str>)> = p
        .warnings
        .iter()
        .map(|w| (w.code, w.at.as_deref()))
        .collect();
    assert_eq!(
        found,
        [
            (
                W::InvalidForInstalledVersion,
                Some("/content/surfaces/0/pages/0/controls/0/press")
            ),
            (
                W::InvalidForInstalledVersion,
                Some("/content/surfaces/0/pages/0/controls/1/feedback/0")
            ),
        ]
    );
}

// Pads that run things.

fn runner() -> ParsedArtifact {
    let mut agent = pad(
        4,
        "Ask",
        Some(a("agent-control", "newTask", json!({ "task": "Review" }))),
    );
    agent.long_press = Some(a("shell", "execute", json!({ "command": "make" })));
    pack_of(
        manifest("1.0.0"),
        vec![surface(
            "mixer",
            "Mixer",
            vec![
                pad(
                    0,
                    "Build",
                    Some(a("shell", "execute", json!({ "command": "make test" }))),
                ),
                pad(
                    1,
                    "Ping",
                    Some(a(
                        "http",
                        "request",
                        json!({ "url": "https://example.com" }),
                    )),
                ),
                pad(
                    2,
                    "Editor",
                    Some(a(
                        "system",
                        "launchApplication",
                        json!({ "app": "kate", "args": ["--new"] }),
                    )),
                ),
                pad(
                    3,
                    "Browser",
                    Some(a(
                        "system",
                        "launchApplication",
                        json!({ "app": "Firefox" }),
                    )),
                ),
                agent,
            ],
        )],
        vec![],
    )
}

fn at(control: &str, slot: Slot) -> Location {
    Location {
        surface_source_id: "mixer".into(),
        page_id: "home".into(),
        control_id: control.into(),
        slot,
    }
}

#[test]
fn pads_that_run_things_are_reviewed_and_kept_or_removed() {
    let file = runner();
    let ws = workspace(vec![]);
    let p = plan_of(&file, &ws, &ImportDecisions::default());
    let kinds: Vec<(ReviewKind, &str)> = p
        .review
        .iter()
        .map(|r| (r.kind, r.at.control_id.as_str()))
        .collect();
    use ReviewKind as K;
    // A launch with arguments is reviewed; a plain application name is not.
    assert_eq!(
        kinds,
        [
            (K::RunsCommand, "g.0.0"),
            (K::SendsRequest, "g.1.0"),
            (K::LaunchesProgram, "g.2.0"),
            (K::InstructsAgent, "g.4.0"),
            (K::RunsCommand, "g.4.0"),
        ]
    );
    assert_eq!(blockers(&p), [B::RunsDecisionNeeded]);
    assert_eq!(p.suggested.runs, Some(Runs::Remove));

    let keep = ImportDecisions {
        runs: Some(Runs::Keep),
        ..ImportDecisions::default()
    };
    let kept = plan_of(&file, &ws, &keep);
    assert!(kept.ready);
    assert!(kept.effects.removed_slots.is_empty());

    let remove = ImportDecisions {
        runs: Some(Runs::Remove),
        ..ImportDecisions::default()
    };
    let removed = plan_of(&file, &ws, &remove);
    assert!(removed.ready);
    // Never an instruction to an agent.
    assert_eq!(
        removed.effects.removed_slots,
        [
            at("g.0.0", Slot::Press),
            at("g.1.0", Slot::Press),
            at("g.2.0", Slot::Press),
            at("g.4.0", Slot::LongPress),
        ]
    );
    assert_ne!(removed.plan_digest, kept.plan_digest);

    // Nothing that runs things: no decision, nothing suggested.
    let quiet = plan_of(&studio("1.0.0"), &ws, &ImportDecisions::default());
    assert!(quiet.review.is_empty());
    assert_eq!(quiet.suggested.runs, None);
}

// Dependencies.

#[test]
fn missing_disabled_and_incompatible_plugins_never_block() {
    let dep = |id: &str, version: &str, required: bool| DependencyV1 {
        plugin_id: id.into(),
        name: None,
        version: version.into(),
        required,
    };
    let mut lamp = pad(3, "Lamp", None);
    lamp.looks.insert("on".into(), lamp.visual);
    lamp.feedback =
        vec![
            FeedbackRule::state(StateAddress::new("acme.lamp", "power"), Test::HasValue).show("on"),
        ];
    let file = pack_of(
        ManifestV1 {
            dependencies: vec![
                dep("acme.gone", "^2", true),
                dep("acme.lamp", "*", false),
                dep("acme.notes", "^2.0", true),
                dep("acme.spare", "*", false),
                dep("acme.switch", "*", true),
            ],
            ..manifest("1.0.0")
        },
        vec![surface(
            "mixer",
            "Mixer",
            vec![
                pad(0, "Gone", Some(a("acme.gone", "go", json!({})))),
                pad(
                    1,
                    "Notes",
                    Some(a("acme.notes", "mark", json!({ "folder": "/x" }))),
                ),
                pad(2, "Switch", Some(a("acme.switch", "toggle", json!({})))),
                lamp,
            ],
        )],
        vec![],
    );
    let p = plan_of(&file, &workspace(vec![]), &ImportDecisions::default());
    assert!(p.ready, "{:?}", p.blockers);
    let statuses: Vec<(&str, DependencyStatus)> = p
        .dependencies
        .iter()
        .map(|d| (d.plugin_id.as_str(), d.status.clone()))
        .collect();
    assert_eq!(
        statuses,
        [
            ("acme.gone", DependencyStatus::Missing),
            ("acme.lamp", DependencyStatus::Missing),
            (
                "acme.notes",
                DependencyStatus::Incompatible {
                    installed: "1.2.3".into()
                }
            ),
            ("acme.spare", DependencyStatus::Missing),
            (
                "acme.switch",
                DependencyStatus::Disabled {
                    installed: "0.1.0".into()
                }
            ),
        ]
    );
    assert_eq!(p.dependencies[1].used_by[0].label, "Lamp");
    assert!(p.dependencies[3].used_by.is_empty());
    assert_eq!(
        warnings(&p),
        [
            W::MissingDependency,
            W::MissingDependency,
            W::IncompatibleDependency,
            W::UnusedDependency,
            W::MissingDependency,
            W::DisabledDependency,
        ]
    );
    assert!(p.warnings[0]
        .message
        .contains("until you install acme.gone ^2"));
    assert_eq!(p.surfaces[0].missing_providers, ["acme.gone", "acme.lamp"]);

    // A version the requirement accepts is compatible.
    let fits = pack_of(
        ManifestV1 {
            dependencies: vec![dep("acme.notes", "^1.2", true)],
            ..manifest("1.0.0")
        },
        vec![surface(
            "mixer",
            "Mixer",
            vec![pad(
                0,
                "Notes",
                Some(a("acme.notes", "mark", json!({ "folder": "/x" }))),
            )],
        )],
        vec![],
    );
    let p = plan_of(&fits, &workspace(vec![]), &ImportDecisions::default());
    assert_eq!(
        p.dependencies[0].status,
        DependencyStatus::Compatible {
            installed: "1.2.3".into()
        }
    );
    assert_eq!(p.dependencies[0].name, "Provider");
    assert!(p.warnings.is_empty(), "{:?}", p.warnings);
}

// Providers.

#[test]
fn a_provider_this_version_lacks_and_an_outside_source_are_said() {
    let mut listen = pad(1, "CI", None);
    listen.looks.insert("red".into(), listen.visual);
    listen.feedback = vec![
        FeedbackRule::event("ci", "build.failed").show("red"),
        FeedbackRule::event("device", "control.pressed").show("red"),
        FeedbackRule::state(StateAddress::new("weather", "rain"), Test::HasValue).show("red"),
    ];
    let open = a(
        "navigation",
        "openSurface",
        json!({ "surfaceId": "elsewhere" }),
    );
    let file = pack_of(
        manifest("1.0.0"),
        vec![surface(
            "mixer",
            "Mixer",
            vec![
                pad(0, "Beam", Some(a("teleport", "beam", json!({})))),
                listen,
                pad(2, "Away", Some(open)),
            ],
        )],
        vec![],
    );
    let p = plan_of(&file, &workspace(vec![]), &ImportDecisions::default());
    assert!(p.ready);
    let found: Vec<(PlanWarningCode, &str)> = p
        .warnings
        .iter()
        .map(|w| (w.code, w.at.as_deref().unwrap_or_default()))
        .collect();
    assert_eq!(
        found,
        [
            (
                W::UnknownHostProvider,
                "/content/surfaces/0/pages/0/controls/0/press/provider"
            ),
            (
                W::ExternalEventSource,
                "/content/surfaces/0/pages/0/controls/1/feedback/0/when"
            ),
            (
                W::UnknownHostProvider,
                "/content/surfaces/0/pages/0/controls/1/feedback/2/when"
            ),
            (
                W::ExternalSurfaceReference,
                "/content/surfaces/0/pages/0/controls/2/press/args/surfaceId"
            ),
        ]
    );
    assert_eq!(p.surfaces[0].missing_providers, ["teleport", "weather"]);
}

#[test]
fn warnings_are_in_the_order_of_the_file_then_of_their_code() {
    let file = pack_of(
        manifest("1.0.0"),
        (0..12)
            .map(|i| {
                surface(
                    &format!("s{i}"),
                    &format!("S{i}"),
                    vec![pad(0, "Beam", Some(a("teleport", "beam", json!({}))))],
                )
            })
            .collect(),
        vec![],
    );
    let p = plan_of(&file, &workspace(vec![]), &ImportDecisions::default());
    let order: Vec<&str> = p.warnings.iter().filter_map(|w| w.at.as_deref()).collect();
    let mut expected: Vec<String> = (0..12)
        .map(|i| format!("/content/surfaces/{i}/pages/0/controls/0/press/provider"))
        .collect();
    assert_eq!(
        order,
        expected.iter().map(String::as_str).collect::<Vec<_>>()
    );
    expected.sort();
    assert_ne!(
        order,
        expected.iter().map(String::as_str).collect::<Vec<_>>()
    );
}

// Devices.

#[test]
fn each_device_is_told_how_the_file_fits_it() {
    let mut wide = pad(1, "Wide", None);
    wide.span = Some(crate::artifacts::format::Span { w: 3, h: 1 });
    wide.visual.color = Color(Rgb::new(10, 200, 10));
    let file = pack_of(
        manifest("1.0.0"),
        vec![surface("mixer", "Mixer", vec![pad(0, "A", None), wide])],
        vec![],
    );
    let p = plan_of(&file, &workspace(vec![]), &ImportDecisions::default());
    let fits: Vec<(&str, bool, usize, bool)> = p
        .compatibility
        .devices
        .iter()
        .map(|d| (d.model.as_str(), d.fits, d.outside, d.rgb_ok))
        .collect();
    assert_eq!(
        fits,
        [
            ("launchpad-x", true, 0, true),
            ("small-pad", false, 1, false)
        ]
    );
    assert_eq!(p.compatibility.namespace.as_deref(), Some(X11));
    assert!(p.compatibility.requirements.rgb);
    assert!(p.ready);
}

// The size of the workspace.

#[test]
fn a_workspace_holds_at_most_sixty_four_surfaces() {
    let filled = |n: usize| workspace((0..n).map(|i| local(&format!("s{i}"), "S")).collect());
    let file = studio("1.0.0");
    assert!(plan_of(&file, &filled(62), &ImportDecisions::default()).ready);
    let p = plan_of(&file, &filled(63), &ImportDecisions::default());
    assert_eq!(blockers(&p), [B::WorkspaceFull]);
    // Replacing in place adds nothing.
    let mut ws = filled(62);
    ws.surfaces.push(imported(&file, "mixer", "mixer", 1));
    ws.surfaces.push(imported(&file, "lights", "lights", 1));
    assert!(plan_of(&file, &ws, &replace(1)).ready);
    assert_eq!(
        blockers(&plan_of(&file, &ws, &as_new())),
        [B::WorkspaceFull]
    );
}

// Provenance.

#[test]
fn a_surface_digest_changes_with_the_surface_only() {
    let file = studio("1.0.0");
    let s = imported(&file, "mixer", "mixer", 1);
    let digest = surface_digest(&s).unwrap();
    assert!(digest.starts_with("sha256:") && digest.len() == 7 + 64);
    let mut moved = s.clone();
    moved.imported_from = None;
    moved.context = Some(Default::default());
    assert_eq!(surface_digest(&moved).unwrap(), digest);
    let mut edited = s.clone();
    edited.pages[0].controls[0].visual.color = Rgb::new(1, 2, 3);
    assert_ne!(surface_digest(&edited).unwrap(), digest);
    let mut renamed = s;
    renamed.id = "mixer-2".into();
    assert_ne!(surface_digest(&renamed).unwrap(), digest);
}

// Decisions as the import view sends them.

#[test]
fn decisions_are_read_strictly() {
    let read = |v: Value| serde_json::from_value::<ImportDecisions>(v);
    let d = read(json!({
        "resolution": { "kind": "replace", "instance": 2 },
        "runs": "remove",
        "inputs": { "folder": { "kind": "text", "value": "/srv" } },
        "contextRules": [0, 2],
        "confirmOverwriteEdits": true
    }))
    .unwrap();
    assert_eq!(d.resolution, Some(Resolution::Replace { instance: 2 }));
    assert_eq!(d.runs, Some(Runs::Remove));
    assert_eq!(d.context_rules, [0, 2]);
    assert!(d.confirm_overwrite_edits);
    assert_eq!(read(json!({})).unwrap(), ImportDecisions::default());
    assert_eq!(
        read(json!({ "resolution": { "kind": "importAsNew" } }))
            .unwrap()
            .resolution,
        Some(Resolution::ImportAsNew)
    );
    assert!(read(json!({ "apply": true })).is_err());
    assert!(read(json!({ "runs": "maybe" })).is_err());
    assert_eq!(
        serde_json::to_value(ImportDecisions::default()).unwrap(),
        json!({ "inputs": {}, "contextRules": [], "confirmOverwriteEdits": false })
    );
}

// On the real built-in providers.

#[tokio::test]
async fn the_starters_exported_and_planned_back_name_nothing_unknown() {
    use crate::artifacts::{export, ExportMetadata, ExportRequest};
    let ws = default_workspace_for(Platform::Linux);
    let mut env = testkit::environment("0.3.0").await;
    env.namespace = Some(X11.into());
    let request = ExportRequest {
        kind: ArtifactKind::Pack,
        surface_ids: vec!["development".into(), "productivity".into()],
        metadata: ExportMetadata::default(),
        include_context_rules: false,
        inputs: Vec::new(),
        bindings: Vec::new(),
        rule_inputs: Vec::new(),
        extra_identities: Vec::new(),
    };
    let bytes = export(&ws, &env, &request)
        .bytes
        .expect("the starters export");
    let file = inspect(&bytes, "starters.ldpack").unwrap();
    let p = plan(&file, &ws, &env, &ImportDecisions::default());
    assert_eq!(local_ids(&p), ["development-2", "productivity-2"]);
    for code in warnings(&p) {
        assert!(
            ![
                W::UnknownHostProvider,
                W::ExternalEventSource,
                W::InvalidForInstalledVersion
            ]
            .contains(&code),
            "{:?}",
            p.warnings
        );
    }
    assert!(p.surfaces.iter().all(|s| s.missing_providers.is_empty()));
    assert_eq!(p.compatibility.devices.len(), env.devices.len());
    assert!(p.compatibility.devices.iter().all(|d| d.fits));
}
