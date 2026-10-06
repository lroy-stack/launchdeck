use std::time::Duration;

use serde_json::json;

use super::editing::CommitError;
use super::input::LONG_PRESS;
use super::*;
use crate::core::model::{
    Animation, Assignment, Control, ControlPos, Page, Preferences, Rgb, Surface, Visual,
};
use crate::devices::virtual_device::VIRTUAL_ID;
use crate::events::types;
use crate::plugins::keyboard::{Combo, Keyboard};
use crate::plugins::ActionError;
use crate::state::feedback::{FeedbackRule, Hold};
use crate::surfaces::default_workspace;

struct NoKeyboard;
impl Keyboard for NoKeyboard {
    fn press(&self, _combo: &Combo) -> Result<(), ActionError> {
        Ok(())
    }
}

/// A generic surface for the runtime tests: one pad that mirrors a demo
/// agent, one that also listens to an external source, and a big approval
/// pad. Nothing here depends on a shipped surface.
fn bound_workspace() -> Workspace {
    use crate::core::model::Span;
    use crate::state::feedback::Test;
    use crate::state::StateAddress;
    let blue = Rgb::new(40, 110, 255);
    let green = Rgb::new(30, 220, 90);
    let red = Rgb::new(255, 40, 50);
    let white = Rgb::new(255, 255, 255);
    // The look named after the agent's state, resting when it is idle.
    let mirror = |agent: &str| {
        FeedbackRule::state(StateAddress::new("demo", agent), Test::HasValue).show_from_value()
    };
    // An external source's events, which the pad keeps showing until the
    // next one (the behaviour schema 3 bindings had).
    let on = |kind: &str, look: &str| {
        FeedbackRule::event("beta", kind)
            .show(look)
            .hold(Hold::Latch)
    };
    let look = |c: &mut Control| {
        c.states.insert("running".into(), Visual::pulse(green));
        c.states
            .insert("waiting".into(), Visual::pulse(Rgb::new(170, 80, 255)));
        c.states
            .insert("approval-required".into(), Visual::pulse(white));
        c.states.insert("success".into(), Visual::solid(green));
        c.states.insert("error".into(), Visual::flash(red));
    };
    let start = |agent: &str| Assignment::new("demo", "startTask", json!({ "agent": agent }));

    let mut alpha = Control::new(ControlPos::grid(0, 0), "Alpha", blue);
    alpha.press = Some(start("alpha"));
    alpha.long_press = Some(Assignment::new(
        "demo",
        "stopTask",
        json!({ "agent": "alpha" }),
    ));
    look(&mut alpha);
    alpha.feedback = vec![mirror("alpha")];

    let mut beta = Control::new(ControlPos::grid(1, 0), "Beta", blue);
    beta.press = Some(start("beta"));
    look(&mut beta);
    beta.feedback = vec![
        mirror("beta"),
        on("task.started", "running"),
        on("task.waiting", "waiting"),
        // The first matching rule wins: the explicit failure goes first.
        FeedbackRule::event("beta", "task.completed")
            .where_field("success", false)
            .show("error")
            .hold(Hold::Latch),
        on("task.completed", "success"),
        on("task.failed", "error"),
    ];

    let mut approve = Control::new(ControlPos::grid(6, 2), "Approve", white.scaled(0.25));
    approve.span = Span { w: 2, h: 2 };
    approve.press = Some(Assignment::new("demo", "approve", json!({})));
    approve
        .states
        .insert("pending".into(), Visual::flash(white));
    approve.feedback = vec![FeedbackRule::state(
        StateAddress::new("demo", "alpha"),
        Test::Equals {
            value: "approval-required".into(),
        },
    )
    .show("pending")];

    Workspace {
        schema_version: crate::core::model::SCHEMA_VERSION,
        surfaces: vec![Surface {
            id: "bound".into(),
            name: "Bound".into(),
            description: String::new(),
            icon: None,
            category: "custom".into(),
            pages: vec![Page {
                id: "home".into(),
                name: "Home".into(),
                controls: vec![alpha, beta, approve],
            }],
            context: None,
            imported_from: None,
        }],
        last_active_surface: Some("bound".into()),
        preferences: Preferences::default(),
        context: Default::default(),
    }
}

fn config(ws: Workspace, store: Option<Store>) -> RuntimeConfig {
    RuntimeConfig {
        workspace: ws,
        store,
        emitter: Arc::new(NullEmitter),
        keyboard: Arc::new(NoKeyboard),
        scan_hardware: false,
        serve_external: false,
        demo_pace: 1.0,
        plugin_dirs: vec![],
        secrets: None,
        observer: None,
    }
}

async fn start(ws: Workspace, store: Option<Store>) -> Arc<Runtime> {
    Runtime::start(config(ws, store)).await
}

async fn wait_until(what: &str, mut cond: impl FnMut() -> bool) {
    for _ in 0..300 {
        if cond() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("timed out waiting for: {what}");
}

fn tap(rt: &Arc<Runtime>, col: u8, row: u8) {
    let h = rt.virtual_handle();
    h.press(ControlPos::grid(col, row), 100).unwrap();
    h.release(ControlPos::grid(col, row)).unwrap();
}

fn state(rt: &Runtime, surface: &str, page: &str, control: &str) -> Option<String> {
    rt.state()
        .control_states
        .get(&control_key(surface, page, control))
        .cloned()
}

fn led(rt: &Runtime, col: u8, row: u8) -> crate::devices::Led {
    rt.virtual_handle()
        .frame()
        .get(ControlPos::grid(col, row))
        .unwrap()
}

#[tokio::test(flavor = "multi_thread")]
async fn starts_on_the_last_active_surface_with_the_virtual_device_connected() {
    let rt = start(default_workspace(), None).await;
    let s = rt.state();
    assert_eq!(s.active_surface_id, "agent-control");
    assert_eq!(s.active_page_id, "home");
    assert_eq!(s.mode, Mode::Live);
    assert!(s
        .devices
        .iter()
        .any(|d| d.identity.id == VIRTUAL_ID && d.connected));
    wait_until("first frame", || {
        !rt.virtual_handle().frame().leds.is_empty()
    })
    .await;
}

#[tokio::test(flavor = "multi_thread")]
async fn surface_and_page_switching_including_navigation_pads() {
    let rt = start(default_workspace(), None).await;
    rt.activate_surface("development", None).unwrap();
    assert_eq!(rt.state().active_surface_id, "development");
    assert_eq!(
        rt.activate_surface("nope", None).unwrap_err(),
        "That surface no longer exists."
    );
    assert_eq!(
        rt.activate_surface("development", Some("nope"))
            .unwrap_err(),
        "That page no longer exists."
    );
    assert_eq!(
        rt.activate_page("nope").unwrap_err(),
        "That page no longer exists."
    );
    // The Git pad is a page navigation action.
    tap(&rt, 0, 2);
    wait_until("git page", || rt.state().active_page_id == "git").await;
    // Back returns home.
    tap(&rt, 0, 7);
    wait_until("home page", || rt.state().active_page_id == "home").await;
    // The grid changes instantly: Terminal (0,0) is lit on dev home.
    assert_ne!(led(&rt, 0, 0).color, Rgb::OFF);
}

#[tokio::test(flavor = "multi_thread")]
async fn top_row_buttons_switch_surfaces() {
    let rt = start(default_workspace(), None).await;
    rt.virtual_handle()
        .press(ControlPos::Top { index: 1 }, 100)
        .unwrap();
    rt.virtual_handle()
        .release(ControlPos::Top { index: 1 })
        .unwrap();
    wait_until("development", || {
        rt.state().active_surface_id == "development"
    })
    .await;
}

#[tokio::test(flavor = "multi_thread")]
async fn pad_press_runs_a_plugin_action_and_the_bound_pad_follows_the_state() {
    let rt = start(bound_workspace(), None).await;
    // Alpha pad: tap starts a simulated task.
    tap(&rt, 0, 0);
    wait_until("running", || {
        state(&rt, "bound", "home", "g.0.0").as_deref() == Some("running")
    })
    .await;
    wait_until("green pulse on the grid", || {
        let l = led(&rt, 0, 0);
        l.animation == Animation::Pulse && l.color.g > l.color.r
    })
    .await;
    // Other agents are untouched.
    assert_eq!(state(&rt, "bound", "home", "g.1.0"), None);
}

#[tokio::test(flavor = "multi_thread")]
async fn external_events_route_to_bound_controls() {
    let rt = start(bound_workspace(), None).await;
    rt.publish_external(Event::new("beta", "task.started", Value::Null));
    wait_until("beta running", || {
        state(&rt, "bound", "home", "g.1.0").as_deref() == Some("running")
    })
    .await;
    rt.publish_external(Event::new(
        "beta",
        "task.completed",
        json!({ "success": false }),
    ));
    wait_until("beta error", || {
        state(&rt, "bound", "home", "g.1.0").as_deref() == Some("error")
    })
    .await;
    wait_until("red flash", || {
        let l = led(&rt, 1, 0);
        l.animation == Animation::Flash && l.color.r > l.color.g
    })
    .await;
    rt.publish_external(Event::new(
        "beta",
        "task.completed",
        json!({ "success": true }),
    ));
    wait_until("beta success", || {
        state(&rt, "bound", "home", "g.1.0").as_deref() == Some("success")
    })
    .await;
    // The documented minimal message has no payload and means success.
    rt.publish_external(Event::new("beta", "task.started", Value::Null));
    wait_until("beta running again", || {
        state(&rt, "bound", "home", "g.1.0").as_deref() == Some("running")
    })
    .await;
    rt.publish_external(Event::new("beta", "task.completed", Value::Null));
    wait_until("bare completion is success", || {
        state(&rt, "bound", "home", "g.1.0").as_deref() == Some("success")
    })
    .await;
    // Events from other sources do not leak into this pad.
    rt.publish_external(Event::new("someone-else", "task.started", Value::Null));
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert_eq!(
        state(&rt, "bound", "home", "g.1.0").as_deref(),
        Some("success")
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn approval_pad_flashes_until_the_user_approves() {
    let rt = start(bound_workspace(), None).await;
    wait_until("first frame", || {
        !rt.virtual_handle().frame().leds.is_empty()
    })
    .await;
    rt.run_action(Assignment::new(
        "demo",
        "requestApproval",
        json!({ "agent": "alpha" }),
    ))
    .await
    .unwrap();
    // Approve is a 2x2 pad at (6,2).
    wait_until("approve pending", || {
        state(&rt, "bound", "home", "g.6.2").as_deref() == Some("pending")
    })
    .await;
    wait_until("white flash on all four cells", || {
        [(6, 2), (7, 2), (6, 3), (7, 3)].iter().all(|&(c, r)| {
            let l = led(&rt, c, r);
            l.animation == Animation::Flash && l.color == Rgb::new(255, 255, 255)
        })
    })
    .await;
    // Pressing any cell of the big pad approves.
    tap(&rt, 7, 3);
    wait_until("alpha running after approval", || {
        state(&rt, "bound", "home", "g.0.0").as_deref() == Some("running")
    })
    .await;
    wait_until("approve pad idle again", || {
        state(&rt, "bound", "home", "g.6.2").is_none()
    })
    .await;
}

fn one_pad_workspace(command: &str) -> Workspace {
    let mut control = Control::new(ControlPos::grid(0, 0), "Task", Rgb::new(0, 0, 255));
    control.press = Some(Assignment::new(
        "shell",
        "execute",
        json!({ "command": command }),
    ));
    control
        .states
        .insert("running".into(), Visual::pulse(Rgb::new(255, 200, 0)));
    control
        .states
        .insert("success".into(), Visual::solid(Rgb::new(0, 255, 0)));
    control
        .states
        .insert("error".into(), Visual::flash(Rgb::new(255, 0, 0)));
    let mut ws = default_workspace();
    ws.surfaces = vec![Surface {
        id: "t".into(),
        name: "T".into(),
        description: String::new(),
        icon: None,
        category: "custom".into(),
        pages: vec![Page {
            id: "home".into(),
            name: "Home".into(),
            controls: vec![control],
        }],
        context: None,
        imported_from: None,
    }];
    ws.last_active_surface = Some("t".into());
    ws
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread")]
async fn unbound_pad_goes_running_then_success_and_emits_action_events() {
    let rt = start(one_pad_workspace("sleep 0.4; echo ok"), None).await;
    let mut rx = rt.bus().subscribe();
    tap(&rt, 0, 0);
    wait_until("running", || {
        state(&rt, "t", "home", "g.0.0").as_deref() == Some("running")
    })
    .await;
    wait_until("yellow pulse", || {
        led(&rt, 0, 0).animation == Animation::Pulse
    })
    .await;
    wait_until("success", || {
        state(&rt, "t", "home", "g.0.0").as_deref() == Some("success")
    })
    .await;
    wait_until("solid green", || {
        led(&rt, 0, 0).color == Rgb::new(0, 255, 0)
    })
    .await;
    let mut kinds = Vec::new();
    while let Ok(e) = rx.try_recv() {
        kinds.push(e.kind);
    }
    let pos = |k: &str| kinds.iter().position(|x| x == k);
    assert!(pos(types::CONTROL_PRESSED).unwrap() < pos(types::ACTION_STARTED).unwrap());
    assert!(pos(types::ACTION_STARTED).unwrap() < pos(types::ACTION_COMPLETED).unwrap());
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread")]
async fn failing_command_ends_in_the_error_state() {
    let rt = start(one_pad_workspace("exit 7"), None).await;
    let mut rx = rt.bus().subscribe();
    tap(&rt, 0, 0);
    wait_until("error", || {
        state(&rt, "t", "home", "g.0.0").as_deref() == Some("error")
    })
    .await;
    let mut failed = false;
    while let Ok(e) = rx.try_recv() {
        if e.kind == types::ACTION_FAILED {
            assert!(e.payload["error"].as_str().unwrap().contains("exit 7"));
            failed = true;
        }
    }
    assert!(failed, "action.failed was published");
}

#[tokio::test(flavor = "multi_thread")]
async fn edit_mode_selects_instead_of_executing() {
    let rt = start(bound_workspace(), None).await;
    let mut rx = rt.bus().subscribe();
    rt.set_mode(Mode::Edit);
    tap(&rt, 0, 0);
    tokio::time::sleep(Duration::from_millis(150)).await;
    assert_eq!(
        state(&rt, "bound", "home", "g.0.0"),
        None,
        "no action in edit mode"
    );
    let mut pressed = false;
    while let Ok(e) = rx.try_recv() {
        pressed |= e.kind == types::CONTROL_PRESSED && e.payload["controlId"] == "g.0.0";
    }
    assert!(
        pressed,
        "control.pressed still tells the editor which control was hit"
    );
    rt.set_mode(Mode::Live);
    tap(&rt, 0, 0);
    wait_until("runs in live mode", || {
        state(&rt, "bound", "home", "g.0.0").as_deref() == Some("running")
    })
    .await;
}

#[tokio::test(flavor = "multi_thread")]
async fn long_press_wins_over_tap() {
    let rt = start(bound_workspace(), None).await;
    // Alpha pad: tap = start, long press = stop.
    tap(&rt, 0, 0);
    wait_until("started by tap", || {
        state(&rt, "bound", "home", "g.0.0").as_deref() == Some("running")
    })
    .await;
    let h = rt.virtual_handle();
    h.press(ControlPos::grid(0, 0), 100).unwrap();
    tokio::time::sleep(LONG_PRESS + Duration::from_millis(250)).await;
    wait_until("stopped by long press", || {
        state(&rt, "bound", "home", "g.0.0").is_none()
    })
    .await;
    h.release(ControlPos::grid(0, 0)).unwrap();
    tokio::time::sleep(Duration::from_millis(150)).await;
    assert_eq!(
        state(&rt, "bound", "home", "g.0.0"),
        None,
        "release after a long press must not also tap"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn edits_and_the_active_surface_persist_across_restarts() {
    let dir = tempfile::tempdir().unwrap();
    let rt = start(default_workspace(), Some(Store::new(dir.path()))).await;
    let mut control = rt
        .snapshot()
        .workspace
        .surface("development")
        .unwrap()
        .page("home")
        .unwrap()
        .control("g.0.0")
        .unwrap()
        .clone();
    control.label = "My Terminal".into();
    control.visual = Visual::solid(Rgb::new(1, 2, 3));
    control.press = Some(Assignment::new(
        "shell",
        "execute",
        json!({ "command": "echo hi" }),
    ));
    rt.update_control("development", "home", control).unwrap();
    rt.activate_surface("development", None).unwrap();

    // "Restart": load from the same directory.
    let loaded = Store::new(dir.path()).load_or_default(|| panic!("workspace file must exist"));
    let c = loaded
        .surface("development")
        .unwrap()
        .page("home")
        .unwrap()
        .control("g.0.0")
        .unwrap();
    assert_eq!(c.label, "My Terminal");
    assert_eq!(c.visual.color, Rgb::new(1, 2, 3));
    assert_eq!(
        c.press,
        Some(Assignment::new(
            "shell",
            "execute",
            json!({ "command": "echo hi" })
        ))
    );
    assert_eq!(loaded.last_active_surface.as_deref(), Some("development"));
    let rt2 = start(loaded, Some(Store::new(dir.path()))).await;
    assert_eq!(rt2.state().active_surface_id, "development");
}

#[tokio::test(flavor = "multi_thread")]
async fn invalid_edits_are_rejected_and_do_not_change_anything() {
    let rt = start(default_workspace(), None).await;
    let before = rt.snapshot().workspace;
    let mut clash = Control::new(ControlPos::grid(1, 0), "Clash", Rgb::new(1, 1, 1));
    clash.id = "clash".into();
    let taken = before.surface("development").unwrap().pages[0]
        .controls
        .iter()
        .find(|c| c.pos == ControlPos::grid(1, 0))
        .map(|c| c.label.clone())
        .unwrap();
    assert_eq!(
        rt.update_control("development", "home", clash.clone())
            .unwrap_err(),
        format!("This pad would overlap “{taken}”.")
    );
    assert_eq!(
        rt.update_control("nope", "home", clash.clone())
            .unwrap_err(),
        "That surface no longer exists."
    );
    assert_eq!(
        rt.update_control("development", "nope", clash).unwrap_err(),
        "That page no longer exists."
    );
    let mut bad = Control::new(ControlPos::grid(7, 7), "Bad", Rgb::new(1, 1, 1));
    bad.press = Some(Assignment::new(
        "system",
        "openUrl",
        json!({ "url": "file:///etc/passwd" }),
    ));
    assert!(rt.update_control("development", "home", bad).is_err());
    assert!(rt
        .update_control(
            "nope",
            "home",
            Control::new(ControlPos::grid(7, 7), "X", Rgb::OFF)
        )
        .is_err());
    assert_eq!(rt.snapshot().workspace, before);
}

#[tokio::test(flavor = "multi_thread")]
async fn new_controls_surfaces_and_pages_can_be_created_and_removed() {
    let rt = start(default_workspace(), None).await;
    let id = rt.create_surface("My Surface").unwrap();
    assert_eq!(id, "my-surface");
    assert_eq!(rt.create_surface("My Surface").unwrap(), "my-surface-2");
    let page = rt.add_page(&id, "Extra").unwrap();
    let mut c = Control::new(ControlPos::grid(3, 3), "New", Rgb::new(9, 9, 9));
    c.press = Some(Assignment::new(
        "system",
        "openUrl",
        json!({ "url": "https://example.com" }),
    ));
    rt.update_control(&id, &page, c).unwrap();
    assert_eq!(
        rt.snapshot()
            .workspace
            .surface(&id)
            .unwrap()
            .page(&page)
            .unwrap()
            .controls
            .len(),
        1
    );
    rt.remove_control(&id, &page, "g.3.3").unwrap();
    assert_eq!(
        rt.remove_control(&id, &page, "g.3.3").unwrap_err(),
        "That pad no longer exists."
    );
    assert_eq!(
        rt.remove_control("nope", &page, "g.3.3").unwrap_err(),
        "That surface no longer exists."
    );
    assert_eq!(
        rt.remove_control(&id, "nope", "g.3.3").unwrap_err(),
        "That page no longer exists."
    );
    assert_eq!(
        rt.create_surface("   ").unwrap_err(),
        "A surface name needs 1 to 40 characters."
    );
    assert_eq!(
        rt.add_page(&id, &"p".repeat(41)).unwrap_err(),
        "A page name needs 1 to 40 characters."
    );
    assert_eq!(
        rt.add_page("nope", "Second").unwrap_err(),
        "That surface no longer exists."
    );
}

/// The names of the UI events a runtime emitted.
#[derive(Default)]
struct Recorder(std::sync::Mutex<Vec<String>>);

impl UiEmitter for Recorder {
    fn emit(&self, event: &str, _payload: Value) {
        self.0.lock().unwrap().push(event.to_string());
    }
}

impl Recorder {
    fn count(&self, event: &str) -> usize {
        self.0
            .lock()
            .unwrap()
            .iter()
            .filter(|e| *e == event)
            .count()
    }

    fn clear(&self) {
        self.0.lock().unwrap().clear();
    }
}

async fn start_recorded(store: &Store) -> (Arc<Runtime>, Arc<Recorder>) {
    let recorder = Arc::new(Recorder::default());
    let rt = Runtime::start(RuntimeConfig {
        workspace: store.load_or_default(default_workspace),
        store: Some(Store::new(store.path().parent().unwrap())),
        emitter: recorder.clone(),
        keyboard: Arc::new(NoKeyboard),
        scan_hardware: false,
        serve_external: false,
        demo_pace: 1.0,
        plugin_dirs: vec![],
        secrets: None,
        observer: None,
    })
    .await;
    (rt, recorder)
}

/// The workspace with one more surface, and a context rule for it (so
/// what Auto Context reports changes too).
fn with_extra_surface(current: &Workspace) -> Workspace {
    use crate::context::identity::ApplicationIdentity;
    use crate::context::rules::ContextRule;
    let mut ws = current.clone();
    ws.surfaces
        .push(crate::surfaces::blank_surface("extra", "Extra"));
    ws.context.rules.push(ContextRule {
        application: ApplicationIdentity::new("x11", "gimp", "GIMP"),
        surface_id: "extra".into(),
        page_id: None,
    });
    ws
}

#[tokio::test(flavor = "multi_thread")]
async fn a_commit_saves_the_new_workspace_then_shows_it_and_says_so_once() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::new(dir.path());
    let (rt, events) = start_recorded(&store).await;
    events.clear();

    rt.commit_workspace(|current| Ok::<_, ()>(with_extra_surface(current)))
        .unwrap();

    let shown = rt.snapshot().workspace;
    assert!(shown.surface("extra").is_some());
    assert_eq!(shown.context.rules.len(), 1);
    let saved = store.load_or_default(|| panic!("the commit saves the workspace"));
    assert_eq!(saved, shown);
    assert_eq!(rt.context().settings.rules.len(), 1);
    assert_eq!(events.count(ui::WORKSPACE), 1);
    assert_eq!(events.count(ui::CONTEXT), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_commit_that_cannot_be_built_or_saved_changes_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::new(dir.path());
    store.save(&default_workspace()).unwrap();
    let file = std::fs::read(store.path()).unwrap();
    let (rt, events) = start_recorded(&store).await;
    events.clear();
    let before = rt.snapshot().workspace;

    let built = rt.commit_workspace(|_| Err::<Workspace, _>("refused"));
    assert!(matches!(built, Err(CommitError::Build("refused"))));

    // A directory where the save writes its temporary file: the save fails.
    std::fs::create_dir_all(dir.path().join("workspace.json.tmp/blocker")).unwrap();
    let saved = rt.commit_workspace(|current| Ok::<_, ()>(with_extra_surface(current)));
    let Err(CommitError::Save(why)) = saved else {
        panic!("the save must fail: {saved:?}");
    };
    assert!(why.contains("io error"), "{why}");

    assert_eq!(rt.snapshot().workspace, before);
    assert!(rt.context().settings.rules.is_empty());
    assert_eq!(std::fs::read(store.path()).unwrap(), file);
    assert_eq!(events.count(ui::WORKSPACE), 0);
    assert_eq!(events.count(ui::CONTEXT), 0);
}

#[tokio::test(flavor = "multi_thread")]
async fn removing_the_surface_on_screen_through_a_commit_shows_the_first_one() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::new(dir.path());
    let (rt, _) = start_recorded(&store).await;
    rt.activate_surface("productivity", None).unwrap();

    rt.commit_workspace(|current| {
        let mut ws = current.clone();
        ws.surfaces.retain(|s| s.id != "productivity");
        Ok::<_, ()>(ws)
    })
    .unwrap();

    let ws = rt.snapshot().workspace;
    let first = &ws.surfaces[0];
    let state = rt.state();
    assert_eq!(state.active_surface_id, first.id);
    assert_eq!(state.active_page_id, first.home_page().unwrap().id);
    // What is saved names a surface that exists.
    let saved = store.load_or_default(|| panic!("the commit saves the workspace"));
    assert_eq!(
        saved.last_active_surface.as_deref(),
        Some(first.id.as_str())
    );
    assert_eq!(saved, ws);
}

/// A commit starts over every control it changes or removes, as an edit
/// does: no temporary look survives on it. Controls it leaves as they were
/// keep theirs.
#[tokio::test(flavor = "multi_thread")]
async fn a_commit_drops_the_temporary_looks_of_controls_it_changes() {
    let rt = start(bound_workspace(), None).await;
    rt.publish_external(Event::new("beta", "task.started", Value::Null));
    wait_until("beta pad running", || {
        state(&rt, "bound", "home", "g.1.0").as_deref() == Some("running")
    })
    .await;
    let relabel = |control: &'static str| {
        move |current: &Workspace| {
            let mut ws = current.clone();
            let page = &mut ws.surface_mut("bound").unwrap().pages[0];
            let c = page.controls.iter_mut().find(|c| c.id == control).unwrap();
            c.label = format!("{} again", c.label);
            Ok::<_, ()>(ws)
        }
    };

    rt.commit_workspace(relabel("g.0.0")).unwrap();
    assert_eq!(
        state(&rt, "bound", "home", "g.1.0").as_deref(),
        Some("running")
    );

    rt.commit_workspace(relabel("g.1.0")).unwrap();
    assert_ne!(
        state(&rt, "bound", "home", "g.1.0").as_deref(),
        Some("running")
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn disconnecting_the_virtual_device_stops_input() {
    let rt = start(default_workspace(), None).await;
    rt.disconnect_device(VIRTUAL_ID).unwrap();
    assert!(rt
        .virtual_handle()
        .press(ControlPos::grid(0, 0), 100)
        .is_err());
    assert!(!rt.state().devices[0].connected);
    rt.connect_device(VIRTUAL_ID).unwrap();
    assert!(rt
        .virtual_handle()
        .press(ControlPos::grid(0, 0), 100)
        .is_ok());
    assert!(rt.connect_device("missing").is_err());
}

#[tokio::test(flavor = "multi_thread")]
async fn external_websocket_events_reach_pads_end_to_end() {
    use futures_util::{SinkExt, StreamExt};
    use tokio_tungstenite::{connect_async, tungstenite::Message};
    let mut ws = bound_workspace();
    ws.preferences.external_events_port = 0; // any free port
    let rt = Runtime::start(RuntimeConfig {
        workspace: ws,
        store: None,
        emitter: Arc::new(NullEmitter),
        keyboard: Arc::new(NoKeyboard),
        scan_hardware: false,
        serve_external: true,
        demo_pace: 1.0,
        plugin_dirs: vec![],
        secrets: None,
        observer: None,
    })
    .await;
    let ext = rt.state().external;
    assert!(ext.listening, "{:?}", ext.error);
    let (mut socket, _) = connect_async(format!("ws://127.0.0.1:{}", ext.port))
        .await
        .unwrap();
    socket
        .send(Message::text(
            r#"{"type":"event","source":"beta","event":"task.started"}"#,
        ))
        .await
        .unwrap();
    assert!(socket
        .next()
        .await
        .unwrap()
        .unwrap()
        .to_text()
        .unwrap()
        .contains("ack"));
    wait_until("beta pad running via websocket", || {
        state(&rt, "bound", "home", "g.1.0").as_deref() == Some("running")
    })
    .await;
    // An attempt to run something is refused and changes nothing.
    socket
        .send(Message::text(
            r#"{"type":"exec","command":"touch /tmp/pwned"}"#,
        ))
        .await
        .unwrap();
    assert!(socket
        .next()
        .await
        .unwrap()
        .unwrap()
        .to_text()
        .unwrap()
        .contains("error"));
}

/// What saving the workspace costs when Auto Context activates a surface, and
/// what the activation costs: a measurement, not a check. It prints the
/// numbers VERIFICATION.md records:
/// `cargo test --lib -- --ignored --nocapture save_on_context_activation`
#[tokio::test(flavor = "multi_thread")]
#[ignore = "a measurement: run it by name with --ignored --nocapture"]
async fn the_save_on_context_activation_is_measured() {
    const ACTIVATIONS: usize = 20;
    let dir = tempfile::tempdir().unwrap();
    let rt = start(default_workspace(), Some(Store::new(dir.path()))).await;
    let mut saves = Vec::new();
    let mut activations = Vec::new();
    let mut serialising = Vec::new();
    for turn in 0..ACTIVATIONS {
        let surface = ["development", "productivity"][turn % 2];
        let began = Instant::now();
        rt.activate(surface, None, By::Context).unwrap();
        activations.push(began.elapsed());
        // The save that activation just made, timed on its own, and the part
        // of it that is only turning the workspace into text.
        let inner = rt.lock();
        let began = Instant::now();
        rt.persist(&inner);
        saves.push(began.elapsed());
        let began = Instant::now();
        serde_json::to_string_pretty(&inner.workspace).unwrap();
        serialising.push(began.elapsed());
    }
    let bytes = std::fs::metadata(dir.path().join("workspace.json"))
        .unwrap()
        .len();
    let report = |what: &str, mut times: Vec<Duration>| {
        times.sort();
        let ms = |d: Duration| d.as_secs_f64() * 1000.0;
        let median = (ms(times[ACTIVATIONS / 2 - 1]) + ms(times[ACTIVATIONS / 2])) / 2.0;
        println!(
            "{what}: min {:.2} ms, median {median:.2} ms, max {:.2} ms ({ACTIVATIONS} runs, {bytes} bytes)",
            ms(times[0]),
            ms(times[ACTIVATIONS - 1]),
        );
    };
    report("Runtime::persist", saves);
    report("of which serialising", serialising);
    report("activate(By::Context)", activations);
}

// ---------------------------------------------------------------------------
// Managing surfaces and pages, and the onboarding flag
// ---------------------------------------------------------------------------

/// What the store holds on disk now, if anything.
fn on_disk(dir: &std::path::Path) -> Option<Workspace> {
    let path = dir.join("workspace.json");
    path.exists()
        .then(|| Store::new(dir).load_or_default(|| panic!("the saved file must load")))
}

/// Every assignment of a workspace that opens `surface`.
fn openers_of(ws: &Workspace, surface: &str) -> Vec<Assignment> {
    ws.surfaces
        .iter()
        .flat_map(|s| &s.pages)
        .flat_map(|p| &p.controls)
        .filter_map(|c| c.press.clone())
        .filter(|a| {
            a.action == crate::actions::builtin::OPEN_SURFACE && a.args["surfaceId"] == surface
        })
        .collect()
}

#[tokio::test(flavor = "multi_thread")]
async fn deleting_the_active_surface_activates_another_and_persists() {
    let dir = tempfile::tempdir().unwrap();
    let rt = start(default_workspace(), Some(Store::new(dir.path()))).await;
    rt.activate_surface("development", None).unwrap();
    let mut others = rt.snapshot().workspace;
    others.surfaces.retain(|s| s.id != "development");
    let openers = openers_of(&others, "development");
    assert!(
        !openers.is_empty(),
        "the starters have pads that open Development"
    );

    rt.remove_surface("development").unwrap();

    let snapshot = rt.snapshot();
    assert!(snapshot.workspace.surface("development").is_none());
    let first = snapshot.workspace.surfaces[0].id.clone();
    assert_eq!(rt.state().active_surface_id, first);
    assert_eq!(rt.state().active_page_id, "home");
    let saved = on_disk(dir.path()).expect("the removal was saved");
    assert!(saved.surface("development").is_none());
    assert_ne!(saved.last_active_surface.as_deref(), Some("development"));
    // Pads elsewhere that opened it keep their arguments, and pressing one
    // says the surface is missing instead of doing something else.
    assert_eq!(openers_of(&snapshot.workspace, "development"), openers);
    assert!(rt.navigate(&openers[0]).is_err());
    assert_eq!(rt.state().active_surface_id, first);
}

#[tokio::test(flavor = "multi_thread")]
async fn the_last_surface_cannot_be_deleted() {
    let rt = start(bound_workspace(), None).await;
    let before = rt.snapshot().workspace;
    let error = rt.remove_surface("bound").unwrap_err();
    assert!(error.contains("only surface"), "{error}");
    assert!(error.ends_with('.'), "a sentence: {error}");
    assert_eq!(rt.snapshot().workspace, before);
    assert!(rt.remove_surface("nope").is_err());
}

#[tokio::test(flavor = "multi_thread")]
async fn rules_targeting_a_deleted_surface_are_removed_and_a_fallback_reset() {
    use crate::context::rules::tests::rule;
    use crate::context::rules::{ContextSettings, Fallback};
    let mut ws = default_workspace();
    ws.context = ContextSettings {
        auto: true,
        rules: vec![
            rule("code", "development", None),
            rule("notes", "productivity", None),
            rule("git", "development", Some("git")),
        ],
        fallback: Fallback::Surface {
            surface_id: "development".into(),
            page_id: None,
        },
    };
    let rt = start(ws, None).await;

    rt.remove_surface("productivity").unwrap();
    let context = rt.snapshot().workspace.context;
    assert_eq!(context.rules.len(), 2);
    assert!(
        matches!(context.fallback, Fallback::Surface { .. }),
        "untouched"
    );

    rt.remove_surface("development").unwrap();
    let context = rt.snapshot().workspace.context;
    assert!(context.rules.is_empty(), "{:?}", context.rules);
    assert_eq!(context.fallback, Fallback::Keep);
    assert!(context.auto, "Auto stays as it was");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_failing_save_leaves_everything_unchanged() {
    let dir = tempfile::tempdir().unwrap();
    let rt = start(default_workspace(), Some(Store::new(dir.path()))).await;
    rt.activate_surface("development", Some("git")).unwrap();
    let saved = on_disk(dir.path());
    let before = rt.snapshot().workspace;
    let state = rt.state();
    // The next save cannot write its temporary file.
    std::fs::create_dir(dir.path().join("workspace.json.tmp")).unwrap();

    for error in [
        rt.remove_surface("development").unwrap_err(),
        rt.update_surface("development", "Code", "", None)
            .unwrap_err(),
        rt.rename_page("development", "git", "Source").unwrap_err(),
        rt.remove_page("development", "git").unwrap_err(),
        rt.set_onboarding_done(true).unwrap_err(),
    ] {
        assert!(error.contains("could not be saved"), "{error}");
    }
    assert_eq!(rt.snapshot().workspace, before);
    assert_eq!(rt.state().active_surface_id, state.active_surface_id);
    assert_eq!(rt.state().active_page_id, state.active_page_id);
    assert_eq!(on_disk(dir.path()), saved);
}

#[tokio::test(flavor = "multi_thread")]
async fn update_surface_and_rename_page_check_lengths_with_sentences() {
    let rt = start(default_workspace(), None).await;
    let before = rt.snapshot().workspace;
    let long = |n: usize| "x".repeat(n);
    let refused = [
        rt.update_surface("development", "  ", "", None),
        rt.update_surface("development", &long(41), "", None),
        rt.update_surface("development", "Code", &long(401), None),
        rt.update_surface("development", "Code", "", Some(long(9))),
        rt.update_surface("nope", "Code", "", None),
        rt.rename_page("development", "git", ""),
        rt.rename_page("development", "git", &long(41)),
        rt.rename_page("development", "nope", "Source"),
        rt.rename_page("nope", "git", "Source"),
    ];
    for result in refused {
        let error = result.unwrap_err();
        assert!(
            error.ends_with('.') && error.chars().next().is_some_and(char::is_uppercase),
            "a sentence: {error}"
        );
    }
    assert_eq!(rt.snapshot().workspace, before);

    rt.update_surface("development", " Code ", &long(400), Some("⌨️".into()))
        .unwrap();
    rt.rename_page("development", "git", &long(40)).unwrap();
    let ws = rt.snapshot().workspace;
    let surface = ws.surface("development").unwrap();
    assert_eq!(surface.name, "Code");
    assert_eq!(surface.description, long(400));
    assert_eq!(surface.icon.as_deref(), Some("⌨️"));
    assert_eq!(surface.page("git").unwrap().name, long(40));
    // An empty icon is no icon.
    rt.update_surface("development", "Code", "", Some(String::new()))
        .unwrap();
    assert_eq!(
        rt.snapshot().workspace.surface("development").unwrap().icon,
        None
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn the_last_page_cannot_be_deleted_and_the_page_on_screen_moves_home() {
    let rt = start(default_workspace(), None).await;
    let error = rt.remove_page("productivity", "home").unwrap_err();
    assert!(error.contains("only page"), "{error}");
    assert!(rt.remove_page("development", "nope").is_err());

    rt.activate_surface("development", Some("git")).unwrap();
    rt.remove_page("development", "git").unwrap();
    let ws = rt.snapshot().workspace;
    assert!(ws.surface("development").unwrap().page("git").is_none());
    assert_eq!(rt.state().active_surface_id, "development");
    assert_eq!(rt.state().active_page_id, "home");
}
/// What a commit puts back on screen is announced as any surface or page
/// shown: surface.changed and page.changed when the surface on screen goes,
/// page.changed alone when only its page goes, nothing when the screen
/// stays as it was.
#[tokio::test(flavor = "multi_thread")]
async fn a_commit_announces_the_surface_and_page_it_puts_on_screen() {
    let rt = start(default_workspace(), None).await;
    rt.activate_surface("development", Some("git")).unwrap();
    let mut rx = rt.bus().subscribe();
    let mut navigation = move || {
        let mut seen = Vec::new();
        while let Ok(e) = rx.try_recv() {
            if e.kind == types::SURFACE_CHANGED || e.kind == types::PAGE_CHANGED {
                seen.push((e.kind, e.payload));
            }
        }
        seen
    };
    let shown = |surface: &str, page: &str| json!({ "surfaceId": surface, "pageId": page });

    rt.rename_page("development", "home", "Start").unwrap();
    assert_eq!(navigation(), []);

    rt.remove_page("development", "git").unwrap();
    assert_eq!(
        navigation(),
        [(
            types::PAGE_CHANGED.to_string(),
            shown("development", "home")
        )]
    );

    rt.remove_surface("development").unwrap();
    let first = rt.snapshot().workspace.surfaces[0].id.clone();
    assert_eq!(
        navigation(),
        [
            (types::SURFACE_CHANGED.to_string(), shown(&first, "home")),
            (types::PAGE_CHANGED.to_string(), shown(&first, "home")),
        ]
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn the_onboarding_flag_changes_alone_and_persists() {
    let dir = tempfile::tempdir().unwrap();
    let rt = start(default_workspace(), Some(Store::new(dir.path()))).await;
    let before = rt.snapshot().workspace;
    assert!(!before.preferences.onboarding_done);

    rt.set_onboarding_done(true).unwrap();

    let mut expected = before.clone();
    expected.preferences.onboarding_done = true;
    assert_eq!(rt.snapshot().workspace, expected);
    assert_eq!(on_disk(dir.path()), Some(expected));
    rt.set_onboarding_done(false).unwrap();
    assert!(!on_disk(dir.path()).unwrap().preferences.onboarding_done);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_reset_keeps_the_onboarding_flag() {
    let rt = start(default_workspace(), None).await;
    rt.set_onboarding_done(true).unwrap();
    rt.create_surface("Mine").unwrap();
    rt.reset_workspace();
    let ws = rt.snapshot().workspace;
    assert!(ws.surface("mine").is_none());
    assert!(ws.preferences.onboarding_done);
}

#[tokio::test(flavor = "multi_thread")]
async fn the_snapshot_carries_the_notices_of_the_start() {
    use crate::persistence::{NoticeKind, StartupNotice};
    let rt = start(default_workspace(), None).await;
    assert!(rt.snapshot().notices.is_empty());

    let notice = StartupNotice {
        kind: NoticeKind::WorkspaceDamaged,
        backup: "/data/workspace.json.bak".into(),
    };
    let rt =
        Runtime::start_with_notices(config(default_workspace(), None), vec![notice.clone()]).await;
    assert_eq!(rt.snapshot().notices, vec![notice]);
    let json = serde_json::to_value(rt.snapshot()).unwrap();
    assert_eq!(json["notices"][0]["kind"], "workspaceDamaged");
}

/// An emitter that is slow at the worst moment: it holds the first `event`
/// it is sent, once armed, until another one has been delivered (or for a
/// second at most), then delivers it.
struct SlowFirst {
    event: &'static str,
    armed: AtomicBool,
    entered: std::sync::Mutex<Option<std::sync::mpsc::Sender<()>>>,
    overtaken: std::sync::Mutex<std::sync::mpsc::Receiver<()>>,
    overtaking: std::sync::Mutex<std::sync::mpsc::Sender<()>>,
    sent: std::sync::Mutex<Vec<Value>>,
}

impl SlowFirst {
    /// A runtime that emits through a slow emitter of `event`, and that
    /// emitter, armed; `entering` says when the first `event` is held.
    async fn runtime(event: &'static str) -> (Arc<Runtime>, Arc<SlowFirst>, Entering) {
        let (entered, entering) = std::sync::mpsc::channel();
        let (overtaking, overtaken) = std::sync::mpsc::channel();
        let emitter = Arc::new(SlowFirst {
            event,
            armed: AtomicBool::new(false),
            entered: std::sync::Mutex::new(Some(entered)),
            overtaken: std::sync::Mutex::new(overtaken),
            overtaking: std::sync::Mutex::new(overtaking),
            sent: std::sync::Mutex::new(Vec::new()),
        });
        let rt = Runtime::start(RuntimeConfig {
            emitter: emitter.clone(),
            ..config(default_workspace(), None)
        })
        .await;
        emitter.armed.store(true, Ordering::SeqCst);
        (rt, emitter, entering)
    }

    fn last(&self, member: &str) -> Value {
        let sent = self.sent.lock().unwrap();
        sent.last().map_or(Value::Null, |p| p[member].clone())
    }
}

type Entering = std::sync::mpsc::Receiver<()>;

impl UiEmitter for SlowFirst {
    fn emit(&self, event: &str, payload: Value) {
        if event != self.event || !self.armed.load(Ordering::SeqCst) {
            return;
        }
        let first = self.entered.lock().unwrap().take();
        match first {
            Some(entered) => {
                entered.send(()).unwrap();
                let overtaken = self.overtaken.lock().unwrap();
                let _ = overtaken.recv_timeout(Duration::from_secs(1));
            }
            None => {
                let _ = self.overtaking.lock().unwrap().send(());
            }
        }
        self.sent.lock().unwrap().push(payload);
    }
}

/// Two emissions at the same time, the first slow to send: what is sent
/// in between does not overtake it.
fn race(rt: &Arc<Runtime>, entering: &Entering, emit: fn(&Runtime), change: impl FnOnce()) {
    let older = {
        let rt = rt.clone();
        std::thread::spawn(move || emit(&rt))
    };
    entering.recv().unwrap();
    change();
    let newer = {
        let rt = rt.clone();
        std::thread::spawn(move || emit(&rt))
    };
    older.join().unwrap();
    newer.join().unwrap();
}

/// Two emissions of the runtime state at the same time deliver the newer
/// state last: reading the state and sending it is one step, never
/// overtaken (the intermittent "told" order of tests/home_assistant.rs).
#[tokio::test(flavor = "multi_thread")]
async fn the_state_sent_last_is_the_newest() {
    let (rt, emitter, entering) = SlowFirst::runtime(ui::STATE).await;
    race(&rt, &entering, Runtime::emit_state, || {
        rt.lock().mode = Mode::Edit;
    });
    assert_eq!(emitter.last("mode"), json!("edit"));
}

/// The same for Auto Context's status, under the same emission lock.
#[tokio::test(flavor = "multi_thread")]
async fn the_context_sent_last_is_the_newest() {
    let (rt, emitter, entering) = SlowFirst::runtime(ui::CONTEXT).await;
    rt.lock().workspace.context.auto = true;
    race(&rt, &entering, Runtime::emit_context, || {
        rt.lock().workspace.context.auto = false;
    });
    assert_eq!(emitter.last("settings")["auto"], json!(false));
}
