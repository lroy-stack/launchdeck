//! Real Auto Context statuses, captured from the real runtime with a scripted
//! observer. Shared by the `dump_context` example (which writes
//! `examples/context-status.json`) and the contract test (which checks that
//! file is still current), so the TypeScript schemas are checked against what
//! Rust emits:
//!
//! * `auto`: an application in front, a rule matched, its surface activated;
//! * `manual`: a surface chosen by hand, LaunchDeck's own window in front;
//! * `paused`: Edit mode, with every kind of problem a rule can have;
//! * `unavailable`: a system where the application in front cannot be seen.

use std::sync::Arc;
use std::time::Duration;

use launchdeck_lib::context::identity::{MACOS, X11};
use launchdeck_lib::context::observer::scripted::ScriptedObserver;
use launchdeck_lib::context::{
    ApplicationIdentity, ContextRule, ContextSettings, Fallback, ObserverStatus, Report, Seen,
};
use launchdeck_lib::core::runtime::{Mode, NullEmitter, Runtime, RuntimeConfig};
use launchdeck_lib::plugins::keyboard::{Combo, Keyboard};
use launchdeck_lib::plugins::ActionError;
use launchdeck_lib::surfaces::default_workspace;
use serde_json::{json, Value};

struct NoKeyboard;
impl Keyboard for NoKeyboard {
    fn press(&self, _combo: &Combo) -> Result<(), ActionError> {
        Ok(())
    }
}

fn app(id: &str, name: &str) -> ApplicationIdentity {
    ApplicationIdentity::new(X11, id, name)
}

fn rule(application: ApplicationIdentity, surface: &str, page: Option<&str>) -> ContextRule {
    ContextRule {
        application,
        surface_id: surface.into(),
        page_id: page.map(str::to_string),
    }
}

async fn runtime(
    status: ObserverStatus,
) -> (
    Arc<Runtime>,
    launchdeck_lib::context::observer::scripted::Script,
) {
    let (observer, script) = ScriptedObserver::new(status);
    let rt = Runtime::start(RuntimeConfig {
        workspace: default_workspace(),
        store: None,
        emitter: Arc::new(NullEmitter),
        keyboard: Arc::new(NoKeyboard),
        scan_hardware: false,
        serve_external: false,
        demo_pace: 0.05,
        plugin_dirs: vec![],
        secrets: None,
        observer: Some(Box::new(observer)),
    })
    .await;
    (rt, script)
}

async fn until(rt: &Arc<Runtime>, mut done: impl FnMut(&Value) -> bool) -> Value {
    for _ in 0..500 {
        let status = serde_json::to_value(rt.context()).unwrap();
        if done(&status) {
            return status;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!(
        "the context never got there: {}",
        serde_json::to_value(rt.context()).unwrap()
    );
}

pub async fn capture() -> Value {
    let code = app("Code", "Visual Studio Code");
    let terminal = app("Gnome-terminal", "Terminal");
    let (rt, script) = runtime(ObserverStatus::available("the X11 window manager")).await;
    rt.update_context(ContextSettings {
        auto: true,
        rules: vec![
            rule(code.clone(), "development", Some("git")),
            rule(terminal.clone(), "productivity", None),
        ],
        fallback: Fallback::Surface {
            surface_id: "agent-control".into(),
            page_id: None,
        },
    })
    .unwrap();
    let front = |identity: &ApplicationIdentity, own: bool| {
        script.report(Report::Seen(Seen::Application {
            identity: identity.clone(),
            own,
        }))
    };

    front(&terminal, false);
    until(&rt, |s| s["decision"]["rule"] == 1).await;
    front(&code, false);
    let auto = until(&rt, |s| {
        s["decision"]["rule"] == 0 && s["decision"]["outcome"] == "activated"
    })
    .await;

    // The person opens LaunchDeck and picks a surface there.
    front(&app("Launchdeck", "LaunchDeck"), true);
    until(&rt, |s| s["foreground"]["kind"] == "launchDeck").await;
    rt.activate_surface("smart-home", None).unwrap();
    let manual = until(&rt, |s| s["mode"] == "manual").await;

    // Editing, with rules that have every kind of problem.
    rt.set_mode(Mode::Edit);
    let mut settings = rt.context().settings;
    settings
        .rules
        .push(rule(code.clone(), "productivity", None));
    settings.rules.push(rule(
        ApplicationIdentity::new(MACOS, "com.apple.Terminal", "Terminal"),
        "development",
        None,
    ));
    rt.update_context(settings).unwrap();
    script.report(Report::Status(ObserverStatus::Degraded {
        mechanism: "the X11 window manager".into(),
        reason: "The connection to the X display was lost.".into(),
    }));
    let paused = until(&rt, |s| {
        s["mode"] == "paused" && s["observer"]["state"] == "degraded"
    })
    .await;

    // A system where nothing can be observed.
    let (blind, _script) = runtime(ObserverStatus::unavailable(
        "This is a Wayland session (GNOME). Wayland does not let an application see which application is in front, and LaunchDeck has no compositor-specific support yet. Auto Context needs an X11 session; switching surfaces by hand works as always.",
    ))
    .await;
    blind
        .update_context(ContextSettings {
            auto: true,
            rules: vec![],
            fallback: Fallback::Keep,
        })
        .unwrap();
    let unavailable = serde_json::to_value(blind.context()).unwrap();

    json!({ "auto": auto, "manual": manual, "paused": paused, "unavailable": unavailable })
}
