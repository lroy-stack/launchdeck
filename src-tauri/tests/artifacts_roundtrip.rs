//! A Pack from export to recovery, on a runtime with a real store (plan
//! 7.10, 7.16, 7.17): a surface whose pad uses the Counter example plugin
//! and whose look follows its level, with a context rule, is exported;
//! the workspace is reset and the plugin removed, as on a computer that
//! never had either; the Pack is imported with its rule opted in, its pads
//! waiting for the plugin and Auto Context left as it was; the plugin is
//! copied back and found by a rescan, and the pad works; after a restart
//! from the same folders everything is still there, and exporting again
//! gives the same bytes.

#[path = "support/artifact_world.rs"]
#[allow(dead_code)]
mod artifact_world;
#[path = "support/artifacts_scenario.rs"]
#[allow(dead_code)]
mod artifacts_scenario;

use std::fs;
use std::time::{Duration, Instant};

use artifact_world::World;
use artifacts_scenario::{counter_pad, counter_request, install_counter};
use launchdeck_lib::artifacts::{
    ApplyOutcome, DependencyStatus, ExportRequest, ImportDecisions, InspectOutcome, PreviewOutcome,
    RuleOutcome,
};
use launchdeck_lib::context::identity::X11;
use launchdeck_lib::context::{ApplicationIdentity, ContextRule, ContextSettings};
use launchdeck_lib::core::model::{Control, ControlPos, Visual};
use launchdeck_lib::devices::Led;
use launchdeck_lib::state::{Scalar, StateAddress};
use launchdeck_lib::surfaces::default_workspace;

const COUNTER: &str = "example.counter";
/// Where the Counter's pad is (`examples/feedback-editor-counter.json`).
const PAD: ControlPos = ControlPos::Grid { col: 3, row: 3 };

/// Generous: CI is slow, and a test that passes quickly pays nothing for it.
const PATIENCE: Duration = Duration::from_secs(20);

async fn wait_until(what: &str, mut done: impl FnMut() -> bool) {
    let until = Instant::now() + PATIENCE;
    while Instant::now() < until {
        if done() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    panic!("timed out waiting for: {what}");
}

/// Look in the plugins folder again, as the Plugins screen's Rescan does.
async fn rescan(world: &World) {
    let host = world.rt.plugin_host().clone();
    tokio::task::spawn_blocking(move || host.scan())
        .await
        .unwrap();
}

fn installed(world: &World) -> bool {
    world.rt.catalog().providers.iter().any(|p| p.id == COUNTER)
}

/// The Pack: the Counter surface and its rule.
fn request() -> ExportRequest {
    ExportRequest {
        include_context_rules: true,
        ..counter_request()
    }
}

fn code() -> ApplicationIdentity {
    ApplicationIdentity::new(X11, "Code", "Visual Studio Code")
}

/// The look the pad's rules give for the Counter's level, as the pad shows it.
fn look_for(pad: &Control, level: &str) -> Led {
    let name = match level {
        "zero" => None,
        "low" => Some("active"),
        "high" => Some("warning"),
        _ => Some("error"),
    };
    let v: Visual = name.map_or(pad.visual, |n| pad.states[n]);
    Led {
        color: v.color,
        animation: v.animation,
    }
}

fn level(world: &World) -> Option<String> {
    let observed = world
        .rt
        .state_hub()
        .read_address(&StateAddress::new(COUNTER, "level"));
    ["zero", "low", "high", "max"]
        .into_iter()
        .find(|l| observed.value == Some(Scalar::text(*l)))
        .map(str::to_string)
}

#[tokio::test(flavor = "multi_thread")]
async fn export_reset_import_install_recover() {
    let world = World::start(default_workspace()).await;

    // A surface with a Counter pad and a rule for it, made here.
    install_counter(&world.plugins);
    rescan(&world).await;
    assert!(installed(&world));
    let id = world.rt.create_surface("Counter").unwrap();
    assert_eq!(id, "counter");
    world.rt.update_control(&id, "home", counter_pad()).unwrap();
    world
        .rt
        .update_context(ContextSettings {
            rules: vec![ContextRule {
                application: code(),
                surface_id: id.clone(),
                page_id: None,
            }],
            ..ContextSettings::default()
        })
        .unwrap();

    // Exported.
    let saved = world.service.export_save(&request()).unwrap();
    let pack = fs::read(world.exports.join(&saved.file_name)).unwrap();
    let preview = world.service.export_preview(&request());
    assert_eq!(preview.dependencies.len(), 1);
    assert_eq!(
        (
            &preview.dependencies[0].plugin_id[..],
            preview.dependencies[0].required
        ),
        (COUNTER, true)
    );

    // A computer that has neither: the workspace reset, Auto on, the
    // plugin gone.
    world.rt.reset_workspace();
    world
        .rt
        .update_context(ContextSettings {
            auto: true,
            ..ContextSettings::default()
        })
        .unwrap();
    fs::remove_dir_all(world.plugins.join("counter")).unwrap();
    rescan(&world).await;
    assert!(!installed(&world));
    let context_before = world.rt.context().settings;

    // Imported, with the rule opted in.
    let InspectOutcome::Plan { plan } = world.service.inspect(&pack, &saved.file_name) else {
        panic!("the exported Pack is refused");
    };
    assert_eq!(plan.dependencies.len(), 1);
    assert_eq!(plan.dependencies[0].status, DependencyStatus::Missing);
    assert_eq!(plan.surfaces[0].missing_providers, [COUNTER]);
    assert_eq!(plan.context_rules[0].outcome, RuleOutcome::NotSelected);
    assert!(
        plan.ready,
        "a missing plugin never blocks: {:?}",
        plan.blockers
    );
    let decisions = ImportDecisions {
        context_rules: vec![0],
        ..plan.decisions.clone()
    };
    let PreviewOutcome::Plan { plan } = world.service.preview(&plan.plan_id, &decisions) else {
        panic!("still pending");
    };
    let outcome = world
        .service
        .apply(&plan.plan_id, &decisions, &plan.plan_digest);
    let ApplyOutcome::Applied { report } = outcome else {
        panic!("{outcome:?}");
    };
    assert_eq!(report.surfaces_added, ["counter"]);
    assert_eq!(report.rules_added, 1);
    assert_eq!(report.missing_dependencies[0].plugin_id, COUNTER);
    assert!(report.auto_context);
    world.assert_nothing_ran();

    // The pad waits for its plugin; the rule is there; Auto as it was.
    let ws = world.rt.snapshot().workspace;
    let pad = ws.surface("counter").unwrap().pages[0]
        .control_at(PAD)
        .unwrap()
        .clone();
    let issues = world
        .rt
        .validate_assignment(pad.press.as_ref().unwrap())
        .unwrap_err();
    assert!(
        issues[0].message.contains("not installed"),
        "{}",
        issues[0].message
    );
    let context = world.rt.context().settings;
    assert_eq!(context.auto, context_before.auto);
    assert_eq!(context.fallback, context_before.fallback);
    assert_eq!(
        context.rules,
        [ContextRule {
            application: code(),
            surface_id: "counter".into(),
            page_id: None,
        }]
    );

    // The plugin is installed again: the pad works, and its look follows.
    install_counter(&world.plugins);
    rescan(&world).await;
    assert!(installed(&world));
    world.rt.activate_surface("counter", None).unwrap();
    wait_until("the pad on the device", || {
        world.rt.virtual_handle().frame().get(PAD).is_some()
    })
    .await;
    let handle = world.rt.virtual_handle();
    handle.press(PAD, 100).unwrap();
    handle.release(PAD).unwrap();
    wait_until("the Counter to count", || {
        level(&world).is_some_and(|l| l != "zero")
    })
    .await;
    let now = level(&world).unwrap();
    wait_until("the pad to show the Counter's level", || {
        world.rt.virtual_handle().frame().get(PAD) == Some(look_for(&pad, &now))
    })
    .await;

    // Restarted from the same folders: the setup is intact.
    let before = world.rt.snapshot().workspace;
    let first_file = saved.file_name.clone();
    let world = world.restart().await;
    assert_eq!(world.rt.snapshot().workspace, before);
    assert_eq!(world.rt.context().settings, context);
    assert!(installed(&world));

    // Exported again with the same request: the same bytes.
    let again = world.service.export_save(&request()).unwrap();
    assert_ne!(again.file_name, first_file, "the first file is kept");
    let bytes = fs::read(world.exports.join(&again.file_name)).unwrap();
    assert!(bytes == pack, "the export after the round trip differs");
    assert_eq!(fs::read(world.exports.join(&first_file)).unwrap(), pack);
}
