//! Flow 5, end to end: an external process registers through the Agent Bridge
//! over a real localhost WebSocket, starts a session, reports state changes,
//! asks for approval and receives a control command back. The assertions read
//! the virtual Launchpad's LEDs and the runtime snapshot, so they check what a
//! person would see on the pads and in the app.

use std::sync::Arc;
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use launchdeck_lib::agents::model::{AccessLevel, Status};
use launchdeck_lib::agents::visual::{self, command_visual, CommandState};
use launchdeck_lib::core::model::{ControlPos, Rgb, Visual};
use launchdeck_lib::core::runtime::{NullEmitter, Runtime, RuntimeConfig};
use launchdeck_lib::devices::Led;
use launchdeck_lib::plugins::keyboard::{Combo, Keyboard};
use launchdeck_lib::plugins::ActionError;
use launchdeck_lib::surfaces::default_workspace;
use serde_json::{json, Value};
use tokio::net::TcpStream;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{connect_async, MaybeTlsStream, WebSocketStream};

type Ws = WebSocketStream<MaybeTlsStream<TcpStream>>;

struct NoKeyboard;
impl Keyboard for NoKeyboard {
    fn press(&self, _combo: &Combo) -> Result<(), ActionError> {
        Ok(())
    }
}

async fn start() -> (Arc<Runtime>, u16) {
    let mut ws = default_workspace();
    ws.preferences.external_events_enabled = true;
    ws.preferences.external_events_port = 0; // any free port
    let rt = Runtime::start(RuntimeConfig {
        workspace: ws,
        store: None,
        emitter: Arc::new(NullEmitter),
        keyboard: Arc::new(NoKeyboard),
        scan_hardware: false,
        serve_external: true,
        demo_pace: 0.05,
        plugin_dirs: vec![],
    })
    .await;
    let st = rt.state();
    assert!(st.external.listening, "{:?}", st.external.error);
    wait_until("first frame", || {
        !rt.virtual_handle().frame().leds.is_empty()
    })
    .await;
    (rt.clone(), st.external.port)
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

async fn connect(port: u16) -> Ws {
    let (ws, _) = connect_async(format!("ws://127.0.0.1:{port}"))
        .await
        .expect("connect");
    ws
}

async fn send(ws: &mut Ws, msg: Value) {
    ws.send(Message::text(msg.to_string())).await.unwrap();
}

/// Next JSON frame, or `None` if nothing arrives within `ms`.
async fn next_frame(ws: &mut Ws, ms: u64) -> Option<Value> {
    let frame = tokio::time::timeout(Duration::from_millis(ms), ws.next())
        .await
        .ok()??
        .ok()?;
    serde_json::from_str(frame.to_text().ok()?).ok()
}

/// Read frames until one has the given `type`.
async fn expect_type(ws: &mut Ws, kind: &str) -> Value {
    for _ in 0..20 {
        match next_frame(ws, 3000).await {
            Some(v) if v["type"] == kind => return v,
            Some(_) => continue,
            None => break,
        }
    }
    panic!("no `{kind}` frame arrived");
}

/// Send a message and return its direct reply.
async fn ask(ws: &mut Ws, msg: Value) -> Value {
    send(ws, msg).await;
    expect_reply(ws).await
}

async fn expect_reply(ws: &mut Ws) -> Value {
    for _ in 0..20 {
        match next_frame(ws, 3000).await {
            Some(v)
                if v["type"] == "ack"
                    || v["type"] == "error"
                    || v["type"] == "agent.registered" =>
            {
                return v
            }
            Some(_) => continue,
            None => break,
        }
    }
    panic!("no reply arrived");
}

fn led(rt: &Runtime, col: u8, row: u8) -> Led {
    rt.virtual_handle()
        .frame()
        .get(ControlPos::grid(col, row))
        .expect("pad exists")
}

fn shows(rt: &Runtime, col: u8, row: u8, v: Visual) -> bool {
    led(rt, col, row)
        == Led {
            color: v.color,
            animation: v.animation,
        }
}

fn tap(rt: &Runtime, col: u8, row: u8) {
    let h = rt.virtual_handle();
    h.press(ControlPos::grid(col, row), 100).unwrap();
    h.release(ControlPos::grid(col, row)).unwrap();
}

fn pad(rt: &Runtime, at: (u8, u8), base: Rgb, state: CommandState) -> bool {
    shows(rt, at.0, at.1, command_visual(base, state))
}

const APPROVE: (u8, u8) = (0, 2);
const PAUSE: (u8, u8) = (4, 2);
const RESUME: (u8, u8) = (5, 2);

fn register(id: &str) -> Value {
    json!({
        "type": "agent.register",
        "protocol": 1,
        "agent": {
            "id": id, "name": "Nightly job", "provider": "example", "harness": "script",
            "capabilities": ["approve", "reject", "pause", "resume", "cancel"]
        }
    })
}

fn session(agent: &str, id: &str, status: &str) -> Value {
    json!({"type": "agent.session.update", "agentId": agent,
           "session": {"id": id, "status": status, "task": "Rotate the logs"}})
}

fn slot_of(rt: &Runtime, agent: &str) -> u8 {
    rt.agents()
        .snapshot()
        .slots
        .iter()
        .position(|s| s.agent_id.as_deref() == Some(agent))
        .expect("agent has a slot") as u8
}

#[tokio::test(flavor = "multi_thread")]
async fn flow_5_external_process_registers_reports_and_receives_commands() {
    let (rt, port) = start().await;
    let mut ws = connect(port).await;

    // 1. Explicit registration. Control is not granted by registering.
    let reg = ask(&mut ws, register("nightly")).await;
    assert_eq!(reg["type"], "agent.registered", "{reg}");
    assert_eq!(reg["access"], "monitor");
    wait_until("agent shown", || rt.agents().agent("nightly").is_some()).await;
    let slot = slot_of(&rt, "nightly");

    // 2. A session and its state changes show on the pad and in the app.
    assert_eq!(
        ask(&mut ws, session("nightly", "run-1", "starting")).await["type"],
        "ack"
    );
    wait_until("starting", || {
        shows(&rt, slot, 0, visual::status_visual(Status::Starting))
    })
    .await;
    ask(&mut ws, session("nightly", "run-1", "running")).await;
    wait_until("running", || {
        shows(&rt, slot, 0, visual::status_visual(Status::Running))
    })
    .await;
    let app = rt.snapshot().agents;
    let s = app
        .sessions
        .iter()
        .find(|s| s.session.id == "run-1")
        .unwrap();
    assert_eq!(s.session.status, Status::Running);
    assert_eq!(s.session.task.as_deref(), Some("Rotate the logs"));

    // 3. Approval request: pad flashes; but a monitored agent takes no commands.
    let ack = ask(
        &mut ws,
        json!({"type": "agent.approval.request", "agentId": "nightly", "sessionId": "run-1",
               "approval": {"id": "ap-1", "message": "Delete logs older than 30 days?", "tool": "fs"}}),
    )
    .await;
    assert_eq!(ack["approvalId"], "ap-1");
    wait_until("slot flashes for approval", || {
        shows(
            &rt,
            slot,
            0,
            visual::status_visual(Status::ApprovalRequired),
        )
    })
    .await;
    wait_until("APPROVE unavailable while monitor-only", || {
        pad(&rt, APPROVE, visual::GREEN, CommandState::Unavailable)
    })
    .await;
    tap(&rt, APPROVE.0, APPROVE.1);
    assert!(
        next_frame(&mut ws, 250).await.is_none(),
        "a monitor-only agent must not receive commands"
    );
    assert!(rt
        .agents()
        .session("run-1")
        .unwrap()
        .pending_approval
        .is_some());

    // 4. The user grants control; the client is told, and APPROVE flashes.
    rt.agents()
        .set_access("nightly", AccessLevel::Control)
        .unwrap();
    let access = expect_type(&mut ws, "agent.access").await;
    assert_eq!(access["access"], "control");
    wait_until("APPROVE needs attention", || {
        pad(&rt, APPROVE, visual::GREEN, CommandState::Attention)
    })
    .await;

    // 5. Pressing APPROVE sends a command back to the process.
    tap(&rt, APPROVE.0, APPROVE.1);
    let cmd = expect_type(&mut ws, "agent.command").await;
    assert_eq!(cmd["command"]["kind"], "approve");
    assert_eq!(cmd["agentId"], "nightly");
    assert_eq!(cmd["sessionId"], "run-1");
    assert_eq!(cmd["approvalId"], "ap-1");
    wait_until("APPROVE pending", || {
        pad(&rt, APPROVE, visual::GREEN, CommandState::Pending)
    })
    .await;
    // Never decided on the agent's behalf: still pending until it confirms.
    assert!(rt
        .agents()
        .session("run-1")
        .unwrap()
        .pending_approval
        .is_some());

    // 6. The process confirms and carries on; everything clears.
    let request_id = cmd["requestId"].as_str().unwrap().to_string();
    let ok = ask(
        &mut ws,
        json!({"type": "agent.command.result", "agentId": "nightly",
               "requestId": request_id, "ok": true}),
    )
    .await;
    assert_eq!(ok["type"], "ack");
    ask(&mut ws, session("nightly", "run-1", "running")).await;
    wait_until("approval cleared and running again", || {
        shows(&rt, slot, 0, visual::status_visual(Status::Running))
            && rt
                .agents()
                .session("run-1")
                .unwrap()
                .pending_approval
                .is_none()
    })
    .await;
    wait_until("APPROVE succeeded feedback", || {
        pad(&rt, APPROVE, visual::GREEN, CommandState::Succeeded)
    })
    .await;

    // 7. Intervention: PAUSE reaches the process, which reports paused.
    wait_until("PAUSE available", || {
        pad(&rt, PAUSE, visual::ORANGE, CommandState::Available)
    })
    .await;
    tap(&rt, PAUSE.0, PAUSE.1);
    let cmd = expect_type(&mut ws, "agent.command").await;
    assert_eq!(cmd["command"]["kind"], "pause");
    let request_id = cmd["requestId"].as_str().unwrap().to_string();
    ask(
        &mut ws,
        json!({"type": "agent.command.result", "agentId": "nightly",
               "requestId": request_id, "ok": true}),
    )
    .await;
    ask(&mut ws, session("nightly", "run-1", "paused")).await;
    wait_until("paused on the pad, RESUME available", || {
        shows(&rt, slot, 0, visual::status_visual(Status::Paused))
            && pad(&rt, RESUME, visual::GREEN, CommandState::Available)
    })
    .await;

    // 8. Finishing turns the pad green.
    ask(&mut ws, session("nightly", "run-1", "completed")).await;
    wait_until("completed", || {
        shows(&rt, slot, 0, visual::status_visual(Status::Completed))
    })
    .await;

    // 9. Disconnecting takes the agent offline on every surface.
    ws.close(None).await.ok();
    drop(ws);
    wait_until("offline", || {
        !rt.agents().agent("nightly").unwrap().online
            && shows(&rt, slot, 0, visual::status_visual(Status::Offline))
    })
    .await;
}

#[tokio::test(flavor = "multi_thread")]
async fn the_bridge_does_not_widen_what_the_socket_accepts() {
    let (_rt, port) = start().await;
    let mut ws = connect(port).await;

    // Plain events still work exactly as before...
    let r = ask(
        &mut ws,
        json!({"type": "event", "source": "ci", "event": "build.done"}),
    )
    .await;
    assert_eq!(r["type"], "ack");
    // ...the reserved source cannot be spoofed to fake agent state...
    let r = ask(
        &mut ws,
        json!({"type": "event", "source": "agent-control", "event": "approval.created",
               "payload": {"sessionId": "x"}}),
    )
    .await;
    assert_eq!(r["type"], "error");
    // ...there is still no way to run anything...
    let r = ask(&mut ws, json!({"type": "exec", "command": "id"})).await;
    assert_eq!(r["type"], "error");
    // ...and an unregistered client cannot report agent state.
    let r = ask(&mut ws, session("ghost", "s", "running")).await;
    assert_eq!(r["type"], "error");
    assert_eq!(r["code"], "not_registered");
}

#[tokio::test(flavor = "multi_thread")]
async fn two_processes_do_not_share_an_agent_id_and_both_are_visible() {
    let (rt, port) = start().await;
    let mut a = connect(port).await;
    let mut b = connect(port).await;
    assert_eq!(
        ask(&mut a, register("alpha-bot")).await["type"],
        "agent.registered"
    );
    let clash = ask(&mut b, register("alpha-bot")).await;
    assert_eq!(clash["code"], "agent_in_use");
    assert_eq!(
        ask(&mut b, register("beta-bot")).await["type"],
        "agent.registered"
    );
    ask(&mut a, session("alpha-bot", "a1", "running")).await;
    ask(&mut b, session("beta-bot", "b1", "failed")).await;
    wait_until("both on pads", || {
        let (sa, sb) = (slot_of(&rt, "alpha-bot"), slot_of(&rt, "beta-bot"));
        shows(&rt, sa, 0, visual::status_visual(Status::Running))
            && shows(&rt, sb, 0, visual::status_visual(Status::Failed))
    })
    .await;
}
