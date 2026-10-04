//! Auto Context on the real runtime.
//!
//! The observer is scripted (a test says which application is in front); the
//! rest is real: the runtime, the default workspace, the activation path the
//! sidebar and the navigation pads use, persistence, the State Hub, and the
//! virtual Launchpad, whose LEDs are what finally has to change.
//!
//! The eight cases agreed before the engine was built are each a test here,
//! as well as in the engine's own unit tests:
//!
//!   App A → Surface A · App B → Surface B · unknown app → fallback ·
//!   manual switch → override · same app again → no-op · Edit → no auto
//!   activation · leaving Edit → evaluated once · no observer → manual works

use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use launchdeck_lib::context::engine::{Held, Mode as ContextMode, Outcome};
use launchdeck_lib::context::identity::X11;
use launchdeck_lib::context::observer::scripted::{Script, ScriptedObserver};
use launchdeck_lib::context::status::{Foreground, RuleProblem};
use launchdeck_lib::context::{
    ApplicationIdentity, ContextRule, ContextSettings, Fallback, ObserverStatus, Report, Seen,
};
use launchdeck_lib::core::model::ControlPos;
use launchdeck_lib::core::observables::{context_mode, surface_active};
use launchdeck_lib::core::runtime::{ui, Mode, Runtime, RuntimeConfig, UiEmitter};
use launchdeck_lib::devices::SurfaceFrame;
use launchdeck_lib::persistence::Store;
use launchdeck_lib::plugins::keyboard::{Combo, Keyboard};
use launchdeck_lib::plugins::ActionError;
use launchdeck_lib::state::hub::Status;
use launchdeck_lib::surfaces::default_workspace;
use serde_json::Value;

const CODE: &str = "Code";
const TERMINAL: &str = "Gnome-terminal";
const BROWSER: &str = "firefox";

struct NoKeyboard;
impl Keyboard for NoKeyboard {
    fn press(&self, _combo: &Combo) -> Result<(), ActionError> {
        Ok(())
    }
}

#[derive(Default)]
struct Recorder(Mutex<Vec<(String, Value)>>);

impl UiEmitter for Recorder {
    fn emit(&self, event: &str, payload: Value) {
        self.0.lock().unwrap().push((event.to_string(), payload));
    }
}

impl Recorder {
    /// Runtime events of one kind that the interface was sent.
    fn runtime_events(&self, kind: &str) -> Vec<Value> {
        self.0
            .lock()
            .unwrap()
            .iter()
            .filter(|(e, p)| e == ui::EVENT && p["source"] == "runtime" && p["type"] == kind)
            .map(|(_, p)| p["payload"].clone())
            .collect()
    }

    fn count(&self, event: &str) -> usize {
        self.0
            .lock()
            .unwrap()
            .iter()
            .filter(|(e, _)| e == event)
            .count()
    }

    fn last(&self, event: &str) -> Option<Value> {
        self.0
            .lock()
            .unwrap()
            .iter()
            .rev()
            .find(|(e, _)| e == event)
            .map(|(_, p)| p.clone())
    }
}

struct Desk {
    rt: Arc<Runtime>,
    script: Script,
    rec: Arc<Recorder>,
    _root: Option<tempfile::TempDir>,
}

fn app(id: &str) -> ApplicationIdentity {
    ApplicationIdentity::new(X11, id, id)
}

fn rule(id: &str, surface: &str, page: Option<&str>) -> ContextRule {
    ContextRule {
        application: app(id),
        surface_id: surface.into(),
        page_id: page.map(str::to_string),
    }
}

async fn wait_until(what: &str, mut cond: impl FnMut() -> bool) {
    for _ in 0..600 {
        if cond() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("timed out waiting for: {what}");
}

async fn start_in(
    dir: Option<&Path>,
    status: ObserverStatus,
) -> (Arc<Runtime>, Script, Arc<Recorder>) {
    let store = dir.map(Store::new);
    let workspace = match &store {
        Some(store) => store.load_or_default(default_workspace),
        None => default_workspace(),
    };
    let (observer, script) = ScriptedObserver::new(status);
    let rec = Arc::new(Recorder::default());
    let rt = Runtime::start(RuntimeConfig {
        workspace,
        store,
        emitter: rec.clone(),
        keyboard: Arc::new(NoKeyboard),
        scan_hardware: false,
        serve_external: false,
        demo_pace: 0.05,
        plugin_dirs: vec![],
        secrets: None,
        observer: Some(Box::new(observer)),
    })
    .await;
    wait_until("first frame", || {
        !rt.virtual_handle().frame().leds.is_empty()
    })
    .await;
    (rt, script, rec)
}

/// A runtime with an observer that works, Auto on, and the usual two rules:
/// the editor opens Development, the terminal opens Productivity.
async fn desk() -> Desk {
    let (rt, script, rec) = start_in(None, ObserverStatus::available("test")).await;
    rt.update_context(ContextSettings {
        auto: true,
        rules: vec![
            rule(CODE, "development", None),
            rule(TERMINAL, "productivity", None),
        ],
        fallback: Fallback::Keep,
    })
    .unwrap();
    Desk {
        rt,
        script,
        rec,
        _root: None,
    }
}

impl Desk {
    fn shown(&self) -> String {
        self.rt.state().active_surface_id
    }

    fn page(&self) -> String {
        self.rt.state().active_page_id
    }

    fn mode(&self) -> ContextMode {
        self.rt.context().mode
    }

    fn frame(&self) -> SurfaceFrame {
        self.rt.virtual_handle().frame()
    }

    fn see(&self, seen: Seen) {
        self.script.report(Report::Seen(seen));
    }

    /// An application comes to the front and stays long enough to count.
    async fn front(&self, id: &str) {
        self.see(Seen::Application {
            identity: app(id),
            own: false,
        });
        let rt = self.rt.clone();
        let want = app(id);
        wait_until(&format!("{id} to be accepted"), || {
            rt.context().application.as_ref() == Some(&want)
        })
        .await;
    }

    async fn launchdeck_in_front(&self) {
        self.see(Seen::Application {
            identity: ApplicationIdentity::new(X11, "Launchdeck", "LaunchDeck"),
            own: true,
        });
        let rt = self.rt.clone();
        wait_until("LaunchDeck to be seen in front", || {
            rt.context().foreground == Foreground::LaunchDeck
        })
        .await;
    }

    /// Long enough for anything that was going to happen to have happened.
    async fn quiet(&self) {
        tokio::time::sleep(Duration::from_millis(400)).await;
    }

    async fn until_shown(&self, surface: &str) {
        let rt = self.rt.clone();
        wait_until(&format!("{surface} to be shown"), || {
            rt.state().active_surface_id == surface
        })
        .await;
    }
}

/// What the virtual Launchpad shows for a surface, taken by opening it by hand.
async fn frame_of(d: &Desk, surface: &str) -> SurfaceFrame {
    let before = d.frame();
    let was = d.shown();
    d.rt.activate_surface(surface, None).unwrap();
    let rt = d.rt.clone();
    if was != surface {
        wait_until("the frame to change", || {
            rt.virtual_handle().frame() != before
        })
        .await;
    }
    tokio::time::sleep(Duration::from_millis(120)).await;
    d.frame()
}

// ---------------------------------------------------------------------------
// The application in front decides
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread")]
async fn app_a_shows_surface_a_app_b_shows_surface_b_on_the_launchpad() {
    let d = desk().await;
    // What each surface looks like on the device, for comparison.
    d.rt.update_context(ContextSettings {
        auto: false,
        ..d.rt.context().settings
    })
    .unwrap();
    let development = frame_of(&d, "development").await;
    let productivity = frame_of(&d, "productivity").await;
    assert_ne!(development, productivity);
    d.rt.activate_surface("agent-control", None).unwrap();
    d.rt.update_context(ContextSettings {
        auto: true,
        ..d.rt.context().settings
    })
    .unwrap();
    assert_eq!(d.mode(), ContextMode::Auto);
    let changes_before = d.rec.runtime_events("surface.changed").len();

    d.front(CODE).await;
    d.until_shown("development").await;
    let rt = d.rt.clone();
    wait_until("the Launchpad to show Development", || {
        rt.virtual_handle().frame() == development
    })
    .await;

    d.front(TERMINAL).await;
    d.until_shown("productivity").await;
    wait_until("the Launchpad to show Productivity", || {
        rt.virtual_handle().frame() == productivity
    })
    .await;

    // It went through the ordinary activation: the same events, the hub, the saved choice.
    let changes = d.rec.runtime_events("surface.changed");
    assert_eq!(changes.len() - changes_before, 2);
    assert_eq!(changes.last().unwrap()["surfaceId"], "productivity");
    let on_screen = d.rt.state_hub().read_address(&surface_active());
    assert_eq!(
        on_screen.value.map(|v| v.look_name()),
        Some("productivity".into())
    );
    assert_eq!(
        d.rt.snapshot().workspace.last_active_surface.as_deref(),
        Some("productivity")
    );

    // And says why.
    let status = d.rt.context();
    let decision = status.decision.unwrap();
    assert_eq!(
        (decision.rule, decision.outcome, decision.held),
        (Some(1), Outcome::Activated, None)
    );
    assert_eq!(decision.application, app(TERMINAL));
    assert_eq!(
        status.foreground,
        Foreground::Application {
            application: app(TERMINAL)
        }
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn an_unknown_application_gets_the_fallback() {
    let d = desk().await;
    d.front(CODE).await;
    d.until_shown("development").await;

    // Keep: nothing changes.
    d.front(BROWSER).await;
    d.quiet().await;
    assert_eq!(d.shown(), "development");
    let decision = d.rt.context().decision.unwrap();
    assert_eq!((decision.rule, decision.outcome), (None, Outcome::Kept));

    // A surface: it is shown for every application without a rule.
    let mut settings = d.rt.context().settings;
    settings.fallback = Fallback::Surface {
        surface_id: "smart-home".into(),
        page_id: None,
    };
    d.rt.update_context(settings).unwrap();
    d.until_shown("smart-home").await;
    d.front(CODE).await;
    d.until_shown("development").await;
    d.front(BROWSER).await;
    d.until_shown("smart-home").await;
}

#[tokio::test(flavor = "multi_thread")]
async fn the_same_application_again_is_a_no_op() {
    let d = desk().await;
    d.front(CODE).await;
    d.until_shown("development").await;
    let (events, states, contexts) = (
        d.rec.runtime_events("surface.changed").len() + d.rec.runtime_events("page.changed").len(),
        d.rec.count(ui::STATE),
        d.rec.count(ui::CONTEXT),
    );
    for _ in 0..5 {
        d.see(Seen::Application {
            identity: app(CODE),
            own: false,
        });
    }
    // Under another display name, still the same application.
    d.see(Seen::Application {
        identity: ApplicationIdentity::new(X11, "code", "Visual Studio Code"),
        own: false,
    });
    d.quiet().await;
    assert_eq!(d.shown(), "development");
    assert_eq!(
        d.rec.runtime_events("surface.changed").len() + d.rec.runtime_events("page.changed").len(),
        events,
        "nothing was activated"
    );
    assert_eq!(
        d.rec.count(ui::STATE),
        states,
        "the runtime state was not sent again"
    );
    // The display name changed once, so the interface heard about that and nothing more.
    assert!(d.rec.count(ui::CONTEXT) <= contexts + 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn an_application_that_flashes_past_does_not_move_the_surface() {
    let d = desk().await;
    d.front(CODE).await;
    d.until_shown("development").await;
    // The terminal takes the front for a moment while switching back.
    d.see(Seen::Application {
        identity: app(TERMINAL),
        own: false,
    });
    tokio::time::sleep(Duration::from_millis(40)).await;
    d.see(Seen::Application {
        identity: app(CODE),
        own: false,
    });
    d.quiet().await;
    assert_eq!(d.shown(), "development");
    assert_eq!(d.rt.context().application, Some(app(CODE)));
    assert_eq!(d.rt.context().recent, vec![app(CODE)]);
}

// ---------------------------------------------------------------------------
// A choice made by hand
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread")]
async fn a_manual_switch_is_not_reverted_until_another_application_or_resume() {
    let d = desk().await;
    d.front(CODE).await;
    d.until_shown("development").await;

    // By hand, from the LaunchDeck window.
    d.launchdeck_in_front().await;
    assert_eq!(
        d.shown(),
        "development",
        "LaunchDeck in front changes nothing"
    );
    d.rt.activate_surface("smart-home", None).unwrap();
    assert_eq!(d.mode(), ContextMode::Manual);
    assert_eq!(d.rt.context().decision.unwrap().held, Some(Held::Manual));
    let mode = d.rt.state_hub().read_address(&context_mode());
    assert_eq!(
        (mode.status, mode.value.map(|v| v.look_name())),
        (Status::Known, Some("manual".into()))
    );

    // Back to the same application: not undone, however long it stays.
    d.see(Seen::Application {
        identity: app(CODE),
        own: false,
    });
    d.quiet().await;
    assert_eq!(
        (d.shown(), d.mode()),
        ("smart-home".to_string(), ContextMode::Manual)
    );

    // Resume Auto: the application's rule is applied at once.
    let status = d.rt.resume_context();
    assert_eq!(status.mode, ContextMode::Auto);
    d.until_shown("development").await;

    // Override again, and this time another application comes to the front.
    d.rt.activate_surface("smart-home", None).unwrap();
    assert_eq!(d.mode(), ContextMode::Manual);
    d.front(TERMINAL).await;
    d.until_shown("productivity").await;
    assert_eq!(d.mode(), ContextMode::Auto);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_navigation_pad_and_a_page_change_are_manual_choices_too() {
    let d = desk().await;
    d.front(CODE).await;
    d.until_shown("development").await;

    // A page chosen by hand on the surface the rule opened.
    d.rt.activate_page("git").unwrap();
    assert_eq!(d.mode(), ContextMode::Manual);
    d.see(Seen::Application {
        identity: app(CODE),
        own: false,
    });
    d.quiet().await;
    assert_eq!(d.page(), "git");
    d.rt.resume_context();
    assert_eq!(
        (d.shown(), d.page()),
        ("development".to_string(), "git".to_string()),
        "the rule names no page, so the page stays"
    );

    // A pad of the top row switches surface: the same as the sidebar.
    let before = d.shown();
    let h = d.rt.virtual_handle();
    for col in 0..8u8 {
        h.press(ControlPos::Top { index: col }, 100).ok();
        h.release(ControlPos::Top { index: col }).ok();
        tokio::time::sleep(Duration::from_millis(60)).await;
        if d.shown() != before {
            break;
        }
    }
    assert_ne!(d.shown(), before, "a top-row button opened another surface");
    assert_eq!(d.mode(), ContextMode::Manual);

    // Opening what is already shown is not an override.
    d.rt.resume_context();
    d.until_shown("development").await;
    d.rt.activate_surface("development", Some(&d.page()))
        .unwrap();
    assert_eq!(d.mode(), ContextMode::Auto);
}

// ---------------------------------------------------------------------------
// Edit mode
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread")]
async fn edit_mode_suspends_activation_and_leaving_it_evaluates_once() {
    let d = desk().await;
    d.front(CODE).await;
    d.until_shown("development").await;

    d.rt.set_mode(Mode::Edit);
    assert_eq!(d.mode(), ContextMode::Paused);
    // Choosing what to edit is not an override.
    d.rt.activate_surface("accessibility", None).unwrap();
    d.quiet().await;
    let activations = d.rec.runtime_events("surface.changed").len();

    // Applications come and go while editing: nothing is activated...
    d.front(TERMINAL).await;
    d.front(BROWSER).await;
    d.front(TERMINAL).await;
    d.quiet().await;
    assert_eq!(d.shown(), "accessibility");
    assert_eq!(d.rec.runtime_events("surface.changed").len(), activations);
    // ...and the context kept up, and says what it would do.
    let status = d.rt.context();
    assert_eq!(status.application, Some(app(TERMINAL)));
    let decision = status.decision.unwrap();
    assert_eq!(
        (decision.rule, decision.outcome, decision.held),
        (Some(1), Outcome::Activated, Some(Held::Paused))
    );

    // Back to Live: applied once.
    d.rt.set_mode(Mode::Live);
    d.until_shown("productivity").await;
    assert_eq!(d.mode(), ContextMode::Auto);
    d.quiet().await;
    assert_eq!(
        d.rec.runtime_events("surface.changed").len(),
        activations + 1
    );
}

// ---------------------------------------------------------------------------
// No observer
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread")]
async fn without_an_observer_launchdeck_works_and_says_why_auto_does_not() {
    let reason = "This is a Wayland session (GNOME). Wayland does not let an application see which application is in front.";
    let (rt, _script, rec) = start_in(None, ObserverStatus::unavailable(reason)).await;
    let status = rt.context();
    assert_eq!(
        status.observer,
        ObserverStatus::Unavailable {
            reason: reason.into()
        }
    );
    assert_eq!(status.mode, ContextMode::Off);
    assert_eq!(status.foreground, Foreground::Unknown);

    // Auto can be switched on. It says it cannot work here, and does nothing.
    let status = rt
        .update_context(ContextSettings {
            auto: true,
            rules: vec![rule(CODE, "development", None)],
            fallback: Fallback::Keep,
        })
        .unwrap();
    assert_eq!(status.mode, ContextMode::Unavailable);
    let mode = rt.state_hub().read_address(&context_mode());
    assert_eq!(
        mode.value.map(|v| v.look_name()),
        Some("unavailable".into())
    );

    // Everything a person does by hand works exactly as before.
    for surface in ["development", "productivity", "agent-control"] {
        rt.activate_surface(surface, None).unwrap();
        assert_eq!(rt.state().active_surface_id, surface);
    }
    rt.activate_surface("development", Some("git")).unwrap();
    rt.activate_page("home").unwrap();
    rt.set_mode(Mode::Edit);
    rt.set_mode(Mode::Live);
    rt.resume_context();
    assert_eq!(rt.state().active_surface_id, "development");
    assert_eq!(
        rt.context().mode,
        ContextMode::Unavailable,
        "never Manual: there is nothing to override"
    );
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(
        rec.runtime_events("surface.changed").len(),
        4,
        "the four surfaces opened by hand, and nothing else"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_runtime_given_no_observer_at_all_starts_and_runs() {
    let rec = Arc::new(Recorder::default());
    let rt = Runtime::start(RuntimeConfig {
        workspace: default_workspace(),
        store: None,
        emitter: rec,
        keyboard: Arc::new(NoKeyboard),
        scan_hardware: false,
        serve_external: false,
        demo_pace: 0.05,
        plugin_dirs: vec![],
        secrets: None,
        observer: None,
    })
    .await;
    assert!(matches!(
        rt.context().observer,
        ObserverStatus::Unavailable { .. }
    ));
    rt.activate_surface("productivity", None).unwrap();
    assert_eq!(rt.state().active_surface_id, "productivity");
}

#[tokio::test(flavor = "multi_thread")]
async fn an_observer_that_fails_later_is_reported_and_nothing_resets() {
    let d = desk().await;
    d.front(CODE).await;
    d.until_shown("development").await;
    d.script.report(Report::Status(ObserverStatus::Degraded {
        mechanism: "test".into(),
        reason: "The connection was lost.".into(),
    }));
    let rt = d.rt.clone();
    wait_until("the status to say so", || {
        matches!(rt.context().observer, ObserverStatus::Degraded { .. })
    })
    .await;
    assert_eq!(d.shown(), "development");
    assert_eq!(d.rt.context().application, Some(app(CODE)));
    d.rt.activate_surface("productivity", None).unwrap();
    assert_eq!(d.shown(), "productivity");
}

// ---------------------------------------------------------------------------
// Rules, as the interface changes them
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread")]
async fn rules_are_checked_reordered_and_flagged() {
    let d = desk().await;
    // A rule for a surface that does not exist is refused, and nothing changes.
    let mut bad = d.rt.context().settings;
    bad.rules.push(rule(BROWSER, "nope", None));
    let e = d.rt.update_context(bad).unwrap_err();
    assert_eq!(e, "rule 3: it names a surface that does not exist (`nope`)");
    assert_eq!(d.rt.context().settings.rules.len(), 2);
    // So is one without an application.
    let mut bad = d.rt.context().settings;
    bad.rules[0].application.id = String::new();
    assert!(d
        .rt
        .update_context(bad)
        .unwrap_err()
        .contains("an application needs an identifier"));

    // Two rules for one application: the first wins, the second is flagged.
    let mut two = d.rt.context().settings;
    two.rules.push(rule(CODE, "smart-home", None));
    two.rules.push(rule(BROWSER, "development", Some("git")));
    let status = d.rt.update_context(two).unwrap();
    assert_eq!(
        status.problems,
        vec![None, None, Some(RuleProblem::Shadowed { by: 0 }), None]
    );
    d.front(CODE).await;
    d.until_shown("development").await;

    // Reordered: now the other one is first, and it takes effect as it is saved.
    let mut swapped = d.rt.context().settings;
    swapped.rules.swap(0, 2);
    let status = d.rt.update_context(swapped).unwrap();
    assert_eq!(status.problems[2], Some(RuleProblem::Shadowed { by: 0 }));
    d.until_shown("smart-home").await;
    assert_eq!(d.rt.context().decision.unwrap().rule, Some(0));

    // A rule with a page opens that page.
    d.front(BROWSER).await;
    d.until_shown("development").await;
    assert_eq!(d.page(), "git");
}

// ---------------------------------------------------------------------------
// What is kept, and what is not
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread")]
async fn rules_and_the_auto_choice_persist_and_nothing_about_the_foreground_does() {
    let root = tempfile::tempdir().unwrap();
    let dir = root.path().join("data");
    {
        let (rt, script, _rec) = start_in(Some(&dir), ObserverStatus::available("test")).await;
        rt.update_context(ContextSettings {
            auto: true,
            rules: vec![rule(CODE, "development", Some("git"))],
            fallback: Fallback::Surface {
                surface_id: "productivity".into(),
                page_id: None,
            },
        })
        .unwrap();
        // A morning's worth of switching between applications.
        for id in [
            CODE,
            TERMINAL,
            BROWSER,
            "Slack",
            "keepassxc",
            CODE,
            TERMINAL,
        ] {
            script.report(Report::Seen(Seen::Application {
                identity: app(id),
                own: false,
            }));
            let want = app(id);
            let rt = rt.clone();
            wait_until("accepted", || {
                rt.context().application.as_ref() == Some(&want)
            })
            .await;
        }
        assert_eq!(rt.context().recent.len(), 5);
    }

    // On disk: the rules, with the one application they name. No other
    // application, no order of use, no times.
    let mut written = String::new();
    for entry in std::fs::read_dir(&dir).unwrap().flatten() {
        if entry.path().is_file() {
            written.push_str(&std::fs::read_to_string(entry.path()).unwrap_or_default());
        }
    }
    let saved: Value =
        serde_json::from_str(&std::fs::read_to_string(dir.join("workspace.json")).unwrap())
            .unwrap();
    assert_eq!(
        saved["context"],
        serde_json::json!({
            "auto": true,
            "rules": [{ "application": { "namespace": "x11", "id": "Code", "displayName": "Code" }, "surfaceId": "development", "pageId": "git" }],
            "fallback": { "kind": "surface", "surfaceId": "productivity" }
        })
    );
    for seen in [TERMINAL, BROWSER, "Slack", "keepassxc"] {
        assert!(
            !written.contains(seen),
            "{seen} was in front and is in a file"
        );
    }
    assert_eq!(saved["schemaVersion"], 4, "no new workspace schema");

    // After a restart: the same rules and the same choice, and the context is
    // observed afresh. Nothing is remembered of what was in front.
    let (rt, script, _rec) = start_in(Some(&dir), ObserverStatus::available("test")).await;
    let status = rt.context();
    assert!(status.settings.auto);
    assert_eq!(
        status.settings.rules,
        vec![rule(CODE, "development", Some("git"))]
    );
    assert_eq!(status.mode, ContextMode::Auto);
    assert_eq!(
        (status.application, status.decision, status.recent.len()),
        (None, None, 0)
    );
    assert_eq!(status.foreground, Foreground::Unknown);
    assert_eq!(
        rt.state().active_surface_id,
        "productivity",
        "the surface it closed on, not a guess about the application"
    );
    script.report(Report::Seen(Seen::Application {
        identity: app(CODE),
        own: false,
    }));
    let rt2 = rt.clone();
    wait_until("the rule to apply after the restart", || {
        rt2.state().active_page_id == "git"
    })
    .await;
    assert_eq!(rt.state().active_surface_id, "development");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_workspace_that_never_used_auto_context_is_saved_as_before() {
    let root = tempfile::tempdir().unwrap();
    let dir = root.path().join("data");
    let (rt, _script, _rec) = start_in(Some(&dir), ObserverStatus::available("test")).await;
    rt.activate_surface("development", None).unwrap();
    let saved: Value =
        serde_json::from_str(&std::fs::read_to_string(dir.join("workspace.json")).unwrap())
            .unwrap();
    assert!(
        saved.get("context").is_none(),
        "nothing is written until something is set"
    );
    assert_eq!(rt.context().mode, ContextMode::Off);

    // Resetting the workspace takes the rules with the surfaces they named.
    rt.update_context(ContextSettings {
        auto: true,
        rules: vec![rule(CODE, "development", None)],
        fallback: Fallback::Keep,
    })
    .unwrap();
    rt.reset_workspace();
    let status = rt.context();
    assert!(!status.settings.auto && status.settings.rules.is_empty());
    assert_eq!(status.mode, ContextMode::Off);
}

#[tokio::test(flavor = "multi_thread")]
async fn the_interface_is_told_when_the_context_changes_and_only_then() {
    let d = desk().await;
    let before = d.rec.count(ui::CONTEXT);
    d.front(CODE).await;
    d.until_shown("development").await;
    assert!(d.rec.count(ui::CONTEXT) > before);
    let last = d.rec.last(ui::CONTEXT).unwrap();
    assert_eq!(last["mode"], "auto");
    assert_eq!(last["application"]["id"], "Code");
    assert_eq!(last["decision"]["outcome"], "activated");
    assert_eq!(last["decision"]["rule"], 0);
    assert_eq!(last["observer"]["state"], "available");
    // The snapshot the interface starts from holds the same thing.
    let snapshot = serde_json::to_value(d.rt.snapshot()).unwrap();
    assert_eq!(snapshot["context"]["mode"], "auto");
    assert_eq!(snapshot["context"]["recent"][0]["id"], "Code");
}
