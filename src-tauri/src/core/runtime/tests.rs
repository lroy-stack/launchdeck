use super::*;
use crate::core::model::{Animation, ControlPos, Rgb, Visual};
use crate::plugins::keyboard::{Combo, Keyboard};
use crate::plugins::ActionError;
use crate::state::feedback::FeedbackRule;

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
        }],
        last_active_surface: Some("bound".into()),
        preferences: Preferences::default(),
        context: Default::default(),
    }
}

async fn start(ws: Workspace, store: Option<Store>) -> Arc<Runtime> {
    Runtime::start(RuntimeConfig {
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
    })
    .await
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
    assert!(rt.activate_surface("nope", None).is_err());
    assert!(rt.activate_page("nope").is_err());
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
    assert!(rt.update_control("development", "home", clash).is_err());
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
    assert!(rt.remove_control(&id, &page, "g.3.3").is_err());
    assert!(rt.create_surface("   ").is_err());
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
