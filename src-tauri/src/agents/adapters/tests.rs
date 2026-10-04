//! Tests for the hook adapters. Claude Code payloads are real captures
//! (`tests/fixtures/claude-code/`); Codex payloads are built from the shape in
//! OpenAI's hook documentation (read 2026-10-04) and are labelled as such.
//! The adapter runs against the real `AgentControl`; the only thing standing
//! in for the harness is the test itself, posting payloads.

use std::sync::Arc;
use std::time::Duration;

use serde_json::{json, Value};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::broadcast::Receiver;
use tokio::sync::oneshot;

use super::hooks::*;
use super::http::Outcome;
use super::{claude_code, codex, HookManager};
use crate::agents::adapter::*;
use crate::agents::model::*;
use crate::agents::service::{AgentControl, NullSink, Target, Timing};
use crate::agents::settings::HooksSettings;
use crate::events::{Event, EventBus};
use crate::state::StateHub;

const SESSION: &str = "11111111-2222-3333-4444-555555555555";

fn fixture(name: &str) -> Value {
    let text = match name {
        "PermissionRequest" => {
            include_str!("../../../tests/fixtures/claude-code/PermissionRequest.json")
        }
        "PreToolUse" => include_str!("../../../tests/fixtures/claude-code/PreToolUse.json"),
        "PostToolUse" => include_str!("../../../tests/fixtures/claude-code/PostToolUse.json"),
        "Stop" => include_str!("../../../tests/fixtures/claude-code/Stop.json"),
        "SessionEnd" => include_str!("../../../tests/fixtures/claude-code/SessionEnd.json"),
        "UserPromptSubmit" => {
            include_str!("../../../tests/fixtures/claude-code/UserPromptSubmit.json")
        }
        other => panic!("no fixture {other}"),
    };
    serde_json::from_str(text).unwrap()
}

fn with_session(mut v: Value, session: &str) -> Value {
    v["session_id"] = json!(session);
    v
}

// --- translator ------------------------------------------------------------

#[test]
fn real_claude_code_payloads_translate_to_the_expected_events() {
    assert_eq!(
        translate(&fixture("UserPromptSubmit")).unwrap(),
        HookEvent::Prompt {
            session: SESSION.into(),
            prompt: "Create notes.txt containing the word hi".into()
        }
    );
    assert_eq!(
        translate(&fixture("PreToolUse")).unwrap(),
        HookEvent::ToolStarted {
            session: SESSION.into(),
            tool: "Write".into()
        }
    );
    assert_eq!(
        translate(&fixture("PostToolUse")).unwrap(),
        HookEvent::ToolFinished {
            session: SESSION.into()
        }
    );
    assert_eq!(
        translate(&fixture("Stop")).unwrap(),
        HookEvent::TurnFinished {
            session: SESSION.into()
        }
    );
    assert_eq!(
        translate(&fixture("SessionEnd")).unwrap(),
        HookEvent::SessionEnded {
            session: SESSION.into()
        }
    );
    assert_eq!(
        translate(&fixture("PermissionRequest")).unwrap(),
        HookEvent::PermissionRequest {
            session: SESSION.into(),
            tool: "Write".into(),
            message: "Write /home/dev/project/notes.txt".into()
        }
    );
}

#[test]
fn documented_codex_payloads_translate_with_the_same_vocabulary() {
    // Shape from the Codex hooks documentation: common fields plus per-event additions.
    let common = |event: &str| {
        json!({
            "session_id": "codex-session-1", "transcript_path": null, "cwd": "/work/app",
            "hook_event_name": event, "model": "some-model", "permission_mode": "default"
        })
    };
    let mut perm = common("PermissionRequest");
    perm["turn_id"] = json!("t1");
    perm["tool_name"] = json!("shell");
    perm["tool_input"] = json!({"command": ["npm", "test"]});
    assert_eq!(
        translate(&perm).unwrap(),
        HookEvent::PermissionRequest {
            session: "codex-session-1".into(),
            tool: "shell".into(),
            message: "shell: npm test".into()
        }
    );
    assert_eq!(
        translate(&common("Interrupt")).unwrap(),
        HookEvent::Interrupted {
            session: "codex-session-1".into()
        }
    );
    assert_eq!(
        translate(&common("SessionStart")).unwrap(),
        HookEvent::SessionStarted {
            session: "codex-session-1".into(),
            cwd: Some("/work/app".into())
        }
    );
    // Events with no use here are accepted and ignored.
    assert_eq!(
        translate(&common("PreCompact")).unwrap(),
        HookEvent::Ignored
    );
    assert_eq!(
        translate(&common("SubagentStart")).unwrap(),
        HookEvent::Ignored
    );
}

#[test]
fn stop_failure_carries_a_concise_error() {
    let v = json!({"session_id":"s","hook_event_name":"StopFailure",
                   "error_type":"rate_limit","error_message":"Rate limit reached for this model"});
    assert_eq!(
        translate(&v).unwrap(),
        HookEvent::TurnFailed {
            session: "s".into(),
            error: "Rate limit reached for this model".into()
        }
    );
}

#[test]
fn malformed_payloads_are_errors_not_guesses() {
    assert_eq!(translate(&json!([1, 2])), Err(TranslateError::NotAnObject));
    assert_eq!(
        translate(&json!({"hook_event_name":"Stop"})),
        Err(TranslateError::Missing("session_id"))
    );
    assert_eq!(
        translate(&json!({"session_id":"s"})),
        Err(TranslateError::Missing("hook_event_name"))
    );
}

#[test]
fn tool_descriptions_stay_short_and_single_line() {
    let long = format!("echo start\n{}", "x".repeat(2000));
    let d = describe_tool("Bash", &json!({ "command": long }));
    assert_eq!(d, "Bash: echo start");
    let d = describe_tool("Bash", &json!({ "command": "y".repeat(2000) }));
    assert!(d.chars().count() <= MAX_MESSAGE_LEN);
    assert_eq!(
        describe_tool("WebFetch", &json!({"url":"https://example.com"})),
        "WebFetch https://example.com"
    );
    assert_eq!(describe_tool("Mystery", &json!({})), "Use Mystery");
}

#[test]
fn the_decision_bodies_match_the_documented_shape() {
    assert_eq!(
        decision_body(true),
        json!({"hookSpecificOutput":{"hookEventName":"PermissionRequest","decision":{"behavior":"allow"}}})
    );
    let deny = decision_body(false);
    assert_eq!(deny["hookSpecificOutput"]["decision"]["behavior"], "deny");
    assert!(deny["hookSpecificOutput"]["decision"]["message"].is_string());
}

// --- the adapter against the real service ------------------------------------

struct Rig {
    ctl: Arc<AgentControl>,
    adapter: Arc<HookAdapter>,
    route: Arc<dyn super::http::Route>,
    rx: Receiver<Event>,
}

fn rig_with(dialect: &'static Dialect) -> Rig {
    let bus = EventBus::new();
    let rx = bus.subscribe();
    let ctl = AgentControl::new(
        bus,
        Arc::new(NullSink),
        None,
        Timing::default(),
        StateHub::new(),
    );
    let adapter = HookAdapter::new(dialect);
    ctl.register_adapter(adapter.clone());
    adapter.set_enabled(true, AdapterState::Active, "test", Duration::from_secs(5));
    let route = adapter.route();
    Rig {
        ctl,
        adapter,
        route,
        rx,
    }
}

fn rig() -> Rig {
    rig_with(&claude_code::DIALECT)
}

impl Rig {
    fn post(&self, payload: Value) -> Outcome {
        self.route.handle(payload)
    }

    fn post_reply(&self, payload: Value) {
        match self.post(payload) {
            Outcome::Reply(None) => {}
            Outcome::Reply(Some(v)) => panic!("unexpected reply body {v}"),
            Outcome::Hold { .. } => panic!("unexpected hold"),
        }
    }

    fn sid(&self, raw: &str) -> String {
        format!("{}{raw}", self.adapter.dialect().session_prefix)
    }

    fn status(&self, raw: &str) -> Status {
        self.ctl.session(&self.sid(raw)).expect("session").status
    }

    fn grant(&self) {
        self.ctl
            .set_access(self.adapter.dialect().agent_id, AccessLevel::Control)
            .unwrap();
    }

    fn events(&mut self) -> Vec<Event> {
        let mut v = vec![];
        while let Ok(e) = self.rx.try_recv() {
            v.push(e);
        }
        v
    }
}

type Held = (
    oneshot::Receiver<Option<Value>>,
    Duration,
    Box<dyn FnOnce() + Send>,
);

fn held(o: Outcome) -> Held {
    match o {
        Outcome::Hold {
            rx,
            timeout,
            abandoned,
        } => (rx, timeout, abandoned),
        Outcome::Reply(_) => panic!("expected the request to be held"),
    }
}

#[tokio::test]
async fn the_agent_declares_only_what_hooks_can_really_do() {
    let rig = rig();
    let agent = rig.ctl.agent("claude-code").unwrap();
    assert_eq!(
        agent.capabilities,
        [Capability::Approve, Capability::Reject]
            .into_iter()
            .collect()
    );
    assert_eq!(
        agent.access,
        AccessLevel::Monitor,
        "control is never the default"
    );
    assert_eq!(agent.provider.as_deref(), Some("anthropic"));
    // Pause is refused for the right reason, whatever the access level.
    rig.grant();
    rig.post_reply(with_session(fixture("PreToolUse"), "a"));
    let r = rig
        .ctl
        .command(Target::Session(rig.sid("a")), Command::Pause);
    assert!(r.is_err());
}

#[tokio::test]
async fn a_turn_walks_through_the_normalized_states() {
    let rig = rig();
    rig.post_reply(with_session(fixture("UserPromptSubmit"), "a"));
    assert_eq!(rig.status("a"), Status::Running);
    assert_eq!(
        rig.ctl.session(&rig.sid("a")).unwrap().task.as_deref(),
        Some("Create notes.txt containing the word hi")
    );
    rig.post_reply(with_session(fixture("PreToolUse"), "a"));
    assert_eq!(rig.status("a"), Status::ToolUse);
    assert_eq!(
        rig.ctl.session(&rig.sid("a")).unwrap().detail.as_deref(),
        Some("Write")
    );
    rig.post_reply(with_session(fixture("PostToolUse"), "a"));
    assert_eq!(rig.status("a"), Status::Running);
    rig.post_reply(with_session(fixture("Stop"), "a"));
    assert_eq!(rig.status("a"), Status::Completed);
    // A new prompt in the same session starts a new run.
    rig.post_reply(with_session(fixture("UserPromptSubmit"), "a"));
    assert_eq!(rig.status("a"), Status::Running);
}

#[tokio::test]
async fn several_sessions_are_tracked_side_by_side() {
    let rig = rig();
    rig.post_reply(with_session(fixture("UserPromptSubmit"), "one"));
    rig.post_reply(with_session(fixture("UserPromptSubmit"), "two"));
    rig.post_reply(with_session(fixture("PreToolUse"), "two"));
    rig.post_reply(with_session(fixture("Stop"), "one"));
    assert_eq!(rig.status("one"), Status::Completed);
    assert_eq!(rig.status("two"), Status::ToolUse);
    assert_eq!(rig.ctl.snapshot().sessions.len(), 2);
}

#[tokio::test]
async fn a_failed_turn_is_failed_with_a_concise_summary() {
    let mut rig = rig();
    rig.post_reply(with_session(fixture("UserPromptSubmit"), "a"));
    rig.post_reply(json!({"session_id": rig_raw("a"), "hook_event_name":"StopFailure",
                          "error_type":"overloaded","error_message":"The API is overloaded.\nTry again later."}));
    let s = rig.ctl.session(&rig.sid("a")).unwrap();
    assert_eq!(s.status, Status::Failed);
    assert_eq!(
        s.error.as_deref(),
        Some("The API is overloaded. Try again later.")
    );
    assert!(rig.events().iter().any(|e| e.kind == "session.failed"));
}

fn rig_raw(raw: &str) -> String {
    raw.to_string()
}

#[tokio::test]
async fn without_control_a_permission_request_is_shown_but_not_held() {
    let rig = rig();
    rig.post_reply(with_session(fixture("UserPromptSubmit"), "a"));
    // The harness gets an immediate "no decision" and asks its own user.
    rig.post_reply(with_session(fixture("PermissionRequest"), "a"));
    let s = rig.ctl.session(&rig.sid("a")).unwrap();
    assert_eq!(s.status, Status::ApprovalRequired);
    let approval = s.pending_approval.expect("shown to the human");
    assert_eq!(approval.message, "Write /home/dev/project/notes.txt");
    // It cannot be answered from here, and says why.
    let refused = rig
        .ctl
        .command(Target::Session(rig.sid("a")), Command::Approve);
    assert!(refused.is_err());
    // When the user answers in the harness, work resumes and the gate clears by itself.
    rig.post_reply(with_session(fixture("PostToolUse"), "a"));
    let s = rig.ctl.session(&rig.sid("a")).unwrap();
    assert_eq!(s.status, Status::Running);
    assert!(s.pending_approval.is_none());
}

#[tokio::test]
async fn approving_from_launchdeck_releases_the_held_request_with_an_allow() {
    let mut rig = rig();
    rig.grant();
    rig.post_reply(with_session(fixture("UserPromptSubmit"), "a"));
    let (rx, timeout, _abandoned) = held(rig.post(with_session(fixture("PermissionRequest"), "a")));
    assert_eq!(timeout, Duration::from_secs(5));
    assert_eq!(rig.status("a"), Status::ApprovalRequired);

    let request = rig
        .ctl
        .command(Target::Session(rig.sid("a")), Command::Approve)
        .unwrap();
    let body = rx.await.unwrap().expect("a decision");
    assert_eq!(body, decision_body(true));
    let s = rig.ctl.session(&rig.sid("a")).unwrap();
    assert!(s.pending_approval.is_none());
    assert_eq!(s.status, Status::Running);
    assert_eq!(
        s.last_command.unwrap().state,
        OutcomeState::Succeeded,
        "{request}"
    );
    let kinds: Vec<String> = rig.events().into_iter().map(|e| e.kind).collect();
    for k in ["approval.created", "approval.resolved", "command.completed"] {
        assert!(kinds.iter().any(|x| x == k), "missing {k}");
    }
}

#[tokio::test]
async fn rejecting_releases_the_held_request_with_a_deny() {
    let rig = rig();
    rig.grant();
    rig.post_reply(with_session(fixture("UserPromptSubmit"), "a"));
    let (rx, _, _) = held(rig.post(with_session(fixture("PermissionRequest"), "a")));
    rig.ctl
        .command(Target::Session(rig.sid("a")), Command::Reject)
        .unwrap();
    let body = rx.await.unwrap().unwrap();
    assert_eq!(body["hookSpecificOutput"]["decision"]["behavior"], "deny");
}

#[tokio::test]
async fn silence_is_never_consent() {
    let rig = rig();
    rig.grant();
    rig.post_reply(with_session(fixture("UserPromptSubmit"), "a"));
    let (rx, _, abandoned) = held(rig.post(with_session(fixture("PermissionRequest"), "a")));
    // The hold runs out (or the harness hangs up): the server calls `abandoned`
    // and answers with no decision. Nothing was approved.
    abandoned();
    drop(rx);
    let s = rig.ctl.session(&rig.sid("a")).unwrap();
    assert_eq!(s.status, Status::Waiting);
    assert!(
        s.pending_approval.is_none(),
        "an unanswerable approval is withdrawn"
    );
    assert_eq!(s.detail.as_deref(), Some("Waiting at the harness"));
    // A late press finds nothing to approve.
    assert!(rig
        .ctl
        .command(Target::Session(rig.sid("a")), Command::Approve)
        .is_err());
}

#[tokio::test]
async fn withdrawing_control_hands_held_requests_back() {
    let rig = rig();
    rig.grant();
    rig.post_reply(with_session(fixture("UserPromptSubmit"), "a"));
    let (rx, _, _) = held(rig.post(with_session(fixture("PermissionRequest"), "a")));
    rig.ctl
        .set_access("claude-code", AccessLevel::Monitor)
        .unwrap();
    // The held request ends without a decision body.
    assert!(rx.await.is_err());
    assert_eq!(rig.status("a"), Status::Waiting);
}

#[tokio::test]
async fn a_newer_request_for_the_same_session_replaces_the_older() {
    let rig = rig();
    rig.grant();
    rig.post_reply(with_session(fixture("UserPromptSubmit"), "a"));
    let (first, _, _) = held(rig.post(with_session(fixture("PermissionRequest"), "a")));
    let (second, _, _) = held(rig.post(with_session(fixture("PermissionRequest"), "a")));
    assert!(
        first.await.is_err(),
        "the superseded request gets no decision"
    );
    rig.ctl
        .command(Target::Session(rig.sid("a")), Command::Approve)
        .unwrap();
    assert_eq!(second.await.unwrap().unwrap(), decision_body(true));
}

#[tokio::test]
async fn two_sessions_can_wait_for_approval_independently() {
    let rig = rig();
    rig.grant();
    for s in ["a", "b"] {
        rig.post_reply(with_session(fixture("UserPromptSubmit"), s));
    }
    let (rx_a, _, _) = held(rig.post(with_session(fixture("PermissionRequest"), "a")));
    let (rx_b, _, _) = held(rig.post(with_session(fixture("PermissionRequest"), "b")));
    rig.ctl
        .command(Target::Session(rig.sid("b")), Command::Reject)
        .unwrap();
    assert_eq!(
        rx_b.await.unwrap().unwrap()["hookSpecificOutput"]["decision"]["behavior"],
        "deny"
    );
    assert_eq!(
        rig.status("a"),
        Status::ApprovalRequired,
        "a is still waiting"
    );
    rig.ctl
        .command(Target::Session(rig.sid("a")), Command::Approve)
        .unwrap();
    assert_eq!(rx_a.await.unwrap().unwrap(), decision_body(true));
}

#[tokio::test]
async fn ending_a_session_keeps_a_finished_one_and_cancels_an_unfinished_one() {
    let rig = rig();
    rig.post_reply(with_session(fixture("UserPromptSubmit"), "done"));
    rig.post_reply(with_session(fixture("Stop"), "done"));
    rig.post_reply(with_session(fixture("SessionEnd"), "done"));
    assert_eq!(
        rig.status("done"),
        Status::Completed,
        "the green result stays visible"
    );

    rig.post_reply(with_session(fixture("UserPromptSubmit"), "cut"));
    rig.post_reply(with_session(fixture("SessionEnd"), "cut"));
    assert_eq!(rig.status("cut"), Status::Cancelled);
}

#[tokio::test]
async fn garbage_and_unknown_events_change_nothing() {
    let rig = rig();
    rig.post_reply(json!({"nonsense": true}));
    rig.post_reply(json!("string"));
    rig.post_reply(json!({"session_id":"s","hook_event_name":"PreCompact"}));
    rig.post_reply(json!({"session_id":"s","hook_event_name":"SomethingNew"}));
    assert!(rig.ctl.snapshot().sessions.is_empty());
}

#[tokio::test]
async fn a_disabled_adapter_ignores_everything() {
    let rig = rig();
    rig.adapter
        .set_enabled(false, AdapterState::Disabled, "off", Duration::from_secs(5));
    rig.post_reply(with_session(fixture("UserPromptSubmit"), "a"));
    assert!(rig.ctl.snapshot().sessions.is_empty());
    assert!(!rig.ctl.agent("claude-code").unwrap().online);
}

#[tokio::test]
async fn prompts_and_commands_never_reach_events_or_the_audit_trail() {
    let mut rig = rig();
    rig.grant();
    let secret = "SECRET-TOKEN-hunter2";
    let mut prompt = fixture("UserPromptSubmit");
    prompt["prompt"] = json!(format!("deploy with {secret}"));
    rig.post_reply(with_session(prompt, "a"));
    let mut perm = fixture("PermissionRequest");
    perm["tool_name"] = json!("Bash");
    perm["tool_input"] = json!({"command": format!("curl -H 'Authorization: {secret}' https://x")});
    let (rx, _, _) = held(rig.post(with_session(perm, "a")));
    rig.ctl
        .command(Target::Session(rig.sid("a")), Command::Approve)
        .unwrap();
    rx.await.unwrap();

    for e in rig.events() {
        let text = serde_json::to_string(&e.payload).unwrap();
        assert!(!text.contains(secret), "{} leaked into an event", e.kind);
    }
    let snap = rig.ctl.snapshot();
    for entry in &snap.audit {
        let text = serde_json::to_string(entry).unwrap();
        assert!(!text.contains(secret), "audit entry leaked: {text}");
    }
    assert!(
        snap.audit.iter().any(|a| a.kind == "approval.approved"),
        "the intervention itself is logged"
    );
}

#[tokio::test]
async fn codex_uses_the_same_adapter_with_its_own_identity() {
    let rig = rig_with(&codex::DIALECT);
    let agent = rig.ctl.agent("codex").unwrap();
    assert_eq!(agent.provider.as_deref(), Some("openai"));
    rig.post_reply(
        json!({"session_id":"x1","hook_event_name":"UserPromptSubmit","prompt":"fix the build"}),
    );
    assert_eq!(rig.ctl.session("cx-x1").unwrap().status, Status::Running);
    let info = rig.adapter.info();
    assert!(
        info.limits
            .iter()
            .any(|l| l.contains("not yet run against")),
        "the untested status must be stated in the product"
    );
}

#[test]
fn only_the_adapter_modules_name_a_vendor() {
    // The domain boundary: the core, the surface and the SDK stay provider-neutral.
    let core = [
        ("model.rs", include_str!("../model.rs")),
        ("service.rs", include_str!("../service.rs")),
        ("visual.rs", include_str!("../visual.rs")),
        ("view.rs", include_str!("../view.rs")),
        ("adapter.rs", include_str!("../adapter.rs")),
        ("bridge.rs", include_str!("../bridge.rs")),
        ("audit.rs", include_str!("../audit.rs")),
        ("notify.rs", include_str!("../notify.rs")),
        ("plugin.rs", include_str!("../plugin.rs")),
        ("demo.rs", include_str!("../demo.rs")),
        ("settings.rs", include_str!("../settings.rs")),
        ("surface", include_str!("../../surfaces/agent_control.rs")),
    ];
    for (name, source) in core {
        // Tests may mention vendors; production code may not.
        let production = source.split("#[cfg(test)]").next().unwrap().to_lowercase();
        for vendor in ["claude", "codex", "openai", "anthropic", "gpt"] {
            assert!(!production.contains(vendor), "{name} mentions `{vendor}`");
        }
    }
}

// --- the endpoint, end to end ------------------------------------------------

async fn http_post(addr: std::net::SocketAddr, path: &str, token: &str, body: &Value) -> String {
    let body = body.to_string();
    let request = format!(
        "POST {path} HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nAuthorization: Bearer {token}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
        addr.port(),
        body.len()
    );
    let mut s = TcpStream::connect(addr).await.unwrap();
    s.write_all(request.as_bytes()).await.unwrap();
    let mut out = Vec::new();
    let _ = tokio::time::timeout(Duration::from_secs(5), s.read_to_end(&mut out)).await;
    String::from_utf8_lossy(&out).to_string()
}

fn body_of(response: &str) -> &str {
    response.split("\r\n\r\n").nth(1).unwrap_or("")
}

fn manager() -> (Arc<AgentControl>, Arc<HookManager>) {
    let bus = EventBus::new();
    let ctl = AgentControl::new(
        bus,
        Arc::new(NullSink),
        None,
        Timing::default(),
        StateHub::new(),
    );
    let manager = HookManager::new(ctl.clone());
    (ctl, manager)
}

/// Persist hook settings, then bring the endpoint in line with them.
async fn enable(
    ctl: &Arc<AgentControl>,
    manager: &Arc<HookManager>,
    edit: impl FnOnce(&mut HooksSettings),
) -> std::net::SocketAddr {
    ctl.update_settings(|s| {
        s.hooks.enabled = true;
        s.hooks.port = 0;
        edit(&mut s.hooks);
    });
    manager.apply().await.unwrap().expect("listening")
}

#[tokio::test]
async fn the_endpoint_is_off_until_enabled_and_comes_up_with_a_generated_token() {
    let (ctl, manager) = manager();
    assert_eq!(manager.apply().await.unwrap(), None);
    assert!(!ctl.agent("claude-code").map(|a| a.online).unwrap_or(false));

    let addr = enable(&ctl, &manager, |_| {}).await;
    assert!(addr.ip().is_loopback());
    let token = ctl.settings().hooks.token;
    assert!(token.len() >= 32, "a token was generated");
    assert!(ctl.agent("claude-code").unwrap().online);
    assert!(ctl.agent("codex").unwrap().online);
    // Same token on the next apply: harness configurations stay valid.
    manager.apply().await.unwrap();
    assert_eq!(ctl.settings().hooks.token, token);
}

#[tokio::test]
async fn a_real_http_permission_round_trip_waits_for_the_human() {
    let (ctl, manager) = manager();
    let addr = enable(&ctl, &manager, |_| {}).await;
    let token = ctl.settings().hooks.token;
    ctl.set_access("claude-code", AccessLevel::Control).unwrap();

    let prompt = with_session(fixture("UserPromptSubmit"), "e2e");
    let r = http_post(addr, "/hooks/claude-code", &token, &prompt).await;
    assert!(r.starts_with("HTTP/1.1 200"), "{r}");
    assert_eq!(body_of(&r), "");

    let perm = with_session(fixture("PermissionRequest"), "e2e");
    let pending =
        tokio::spawn(async move { http_post(addr, "/hooks/claude-code", &token, &perm).await });
    // The request stays open while the approval is visible to the human.
    for _ in 0..100 {
        if ctl
            .session("cc-e2e")
            .is_some_and(|s| s.pending_approval.is_some())
        {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert!(ctl.session("cc-e2e").unwrap().pending_approval.is_some());
    assert!(
        !pending.is_finished(),
        "nothing is decided without the human"
    );

    ctl.command(Target::Session("cc-e2e".into()), Command::Approve)
        .unwrap();
    let response = pending.await.unwrap();
    let body: Value = serde_json::from_str(body_of(&response)).unwrap();
    assert_eq!(body, decision_body(true));
}

#[tokio::test]
async fn the_endpoint_rejects_a_wrong_token_and_routes_only_enabled_harnesses() {
    let (ctl, manager) = manager();
    let addr = enable(&ctl, &manager, |h| {
        h.harnesses.insert("codex".into(), false);
    })
    .await;
    let token = ctl.settings().hooks.token;
    let p = with_session(fixture("UserPromptSubmit"), "x");

    let r = http_post(addr, "/hooks/claude-code", "wrong-token-wrong-token", &p).await;
    assert!(r.starts_with("HTTP/1.1 401"), "{r}");
    assert!(ctl.session("cc-x").is_none());
    let r = http_post(addr, "/hooks/codex", &token, &p).await;
    assert!(r.starts_with("HTTP/1.1 404"), "{r}");
    assert!(
        !ctl.agent("codex").map(|a| a.online).unwrap_or(false),
        "a switched-off harness is not online"
    );
}

#[tokio::test]
async fn rotating_the_token_invalidates_the_old_one() {
    let (ctl, manager) = manager();
    let _first = enable(&ctl, &manager, |_| {}).await;
    let old = ctl.settings().hooks.token;
    let addr = manager.rotate_token().await.unwrap().unwrap();
    let new = ctl.settings().hooks.token;
    assert_ne!(old, new);
    let p = with_session(fixture("UserPromptSubmit"), "r");
    assert!(http_post(addr, "/hooks/claude-code", &old, &p)
        .await
        .starts_with("HTTP/1.1 401"));
    assert!(http_post(addr, "/hooks/claude-code", &new, &p)
        .await
        .starts_with("HTTP/1.1 200"));
}

#[tokio::test]
async fn setup_snippets_use_the_environment_not_the_secret() {
    let (ctl, manager) = manager();
    enable(&ctl, &manager, |_| {}).await;
    let setup = manager.setup();
    assert!(setup.shell_export.contains(&setup.token));
    // The snippets for the harness config files never embed the token.
    assert!(!setup.claude_code_settings.contains(&setup.token));
    assert!(!setup.codex_config.contains(&setup.token));
    let claude: Value = serde_json::from_str(&setup.claude_code_settings).unwrap();
    let hook = &claude["hooks"]["PermissionRequest"][0]["hooks"][0];
    assert_eq!(hook["type"], "http");
    assert_eq!(
        hook["headers"]["Authorization"],
        "Bearer $LAUNCHDECK_HOOK_TOKEN"
    );
    assert_eq!(hook["allowedEnvVars"][0], "LAUNCHDECK_HOOK_TOKEN");
    assert!(hook["url"]
        .as_str()
        .unwrap()
        .starts_with("http://127.0.0.1:"));
    assert!(setup
        .codex_config
        .contains("[[hooks.PermissionRequest.hooks]]"));
    assert!(setup.codex_config.contains("|| true"));
    // The Codex snippet is valid TOML.
    assert!(setup
        .codex_config
        .lines()
        .any(|l| l.starts_with("command = '")));
}
