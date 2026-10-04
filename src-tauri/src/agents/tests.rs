//! Domain tests for Agent Control. They run the real `AgentControl` with a
//! real adapter implementation (`TestAdapter`) and assert on the real events,
//! snapshots and audit trail. Nothing here fakes the code under test.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::Value;
use tokio::sync::broadcast::Receiver;

use super::adapter::*;
use super::model::*;
use super::notify::{Notification, NotificationKind};
use super::service::*;
use super::settings::AgentsStorage;
use super::view::AgentsSnapshot;
use crate::events::{Event, EventBus};

// ---------------------------------------------------------------------------
// Fixtures
// ---------------------------------------------------------------------------

#[derive(Default)]
struct CollectSink {
    notifications: Mutex<Vec<Notification>>,
    last: Mutex<Option<AgentsSnapshot>>,
}

impl AgentsSink for CollectSink {
    fn changed(&self, snapshot: &AgentsSnapshot) {
        *self.last.lock().unwrap() = Some(snapshot.clone());
    }
    fn notify(&self, n: &Notification) {
        self.notifications.lock().unwrap().push(n.clone());
    }
}

/// A real adapter that records commands and can answer them.
struct TestAdapter {
    id: &'static str,
    kind: AdapterKind,
    host: Mutex<Option<AdapterHost>>,
    commands: Mutex<Vec<AdapterCommand>>,
    /// Answer every command immediately with this result.
    auto: Mutex<Option<Result<(), String>>>,
    reject_execute: AtomicBool,
}

impl TestAdapter {
    fn new(id: &'static str, kind: AdapterKind) -> Arc<Self> {
        Arc::new(Self {
            id,
            kind,
            host: Mutex::new(None),
            commands: Mutex::new(Vec::new()),
            auto: Mutex::new(None),
            reject_execute: AtomicBool::new(false),
        })
    }
    fn host(&self) -> AdapterHost {
        self.host.lock().unwrap().clone().expect("attached")
    }
    fn commands(&self) -> Vec<AdapterCommand> {
        self.commands.lock().unwrap().clone()
    }
}

impl AgentAdapter for TestAdapter {
    fn info(&self) -> AdapterInfo {
        AdapterInfo {
            id: self.id.into(),
            name: self.id.into(),
            kind: self.kind,
            description: "test".into(),
            limits: vec![],
        }
    }
    fn attach(&self, host: AdapterHost) {
        *self.host.lock().unwrap() = Some(host);
    }
    fn execute(&self, command: AdapterCommand) -> Result<(), CommandError> {
        if self.reject_execute.load(Ordering::SeqCst) {
            return Err(CommandError::new("adapter refused"));
        }
        let request = command.request_id.clone();
        self.commands.lock().unwrap().push(command);
        let auto = self.auto.lock().unwrap().clone();
        if let Some(result) = auto {
            self.host().command_result(&request, result).unwrap();
        }
        Ok(())
    }
}

struct Fixture {
    ctl: Arc<AgentControl>,
    adapter: Arc<TestAdapter>,
    sink: Arc<CollectSink>,
    rx: Receiver<Event>,
}

fn fixture_with(kind: AdapterKind, timing: Timing, storage: Option<AgentsStorage>) -> Fixture {
    let bus = EventBus::new();
    let rx = bus.subscribe();
    let sink = Arc::new(CollectSink::default());
    let ctl = AgentControl::new(bus, sink.clone(), storage, timing);
    let adapter = TestAdapter::new("test", kind);
    ctl.register_adapter(adapter.clone());
    Fixture {
        ctl,
        adapter,
        sink,
        rx,
    }
}

fn fixture() -> Fixture {
    fixture_with(AdapterKind::Simulated, Timing::default(), None)
}

fn caps(list: &[Capability]) -> Capabilities {
    list.iter().copied().collect()
}

fn full() -> Capabilities {
    caps(&[
        Capability::Spawn,
        Capability::SendInstruction,
        Capability::Pause,
        Capability::Resume,
        Capability::Cancel,
        Capability::Retry,
        Capability::Approve,
        Capability::Reject,
    ])
}

fn spec(id: &str, capabilities: Capabilities) -> AgentSpec {
    AgentSpec {
        id: id.into(),
        name: id.to_uppercase(),
        provider: Some("test".into()),
        harness: Some("harness".into()),
        capabilities,
    }
}

impl Fixture {
    fn register(&self, id: &str, capabilities: Capabilities) {
        self.adapter
            .host()
            .register_agent(spec(id, capabilities))
            .unwrap();
    }
    fn update(&self, agent: &str, session: &str, status: Status) {
        self.adapter
            .host()
            .update_session(agent, SessionUpdate::new(session, status))
            .unwrap();
    }
    fn events(&mut self) -> Vec<Event> {
        let mut v = Vec::new();
        while let Ok(e) = self.rx.try_recv() {
            v.push(e);
        }
        v
    }
    fn kinds(&mut self) -> Vec<String> {
        self.events().into_iter().map(|e| e.kind).collect()
    }
    fn snapshot(&self) -> AgentsSnapshot {
        self.ctl.snapshot()
    }
    fn session(&self, id: &str) -> Session {
        self.ctl.session(id).expect("session exists")
    }
}

fn find<'a>(events: &'a [Event], kind: &str) -> Vec<&'a Event> {
    events.iter().filter(|e| e.kind == kind).collect()
}

fn last_state(events: &[Event], command: &str) -> Option<String> {
    events
        .iter()
        .rev()
        .find(|e| e.kind == "command.state" && e.payload["command"] == command)
        .and_then(|e| e.payload["state"].as_str().map(str::to_string))
}

fn last_slot(events: &[Event], slot: usize) -> Option<Value> {
    events
        .iter()
        .rev()
        .find(|e| e.kind == "slot.changed" && e.payload["slot"] == slot)
        .map(|e| e.payload.clone())
}

// ---------------------------------------------------------------------------
// Registration and ownership
// ---------------------------------------------------------------------------

#[tokio::test]
async fn registration_creates_an_online_agent_and_announces_it() {
    let mut f = fixture();
    f.register("atlas", full());
    let a = f.ctl.agent("atlas").unwrap();
    assert!(a.online);
    assert_eq!(a.adapter_id, "test");
    assert_eq!(a.provider.as_deref(), Some("test"));
    let kinds = f.kinds();
    assert!(kinds.contains(&"agent.registered".to_string()));
    assert!(kinds.contains(&"agent.online".to_string()));
    // Every event is attributed to the agent-control source.
    f.register("beacon", full());
    assert!(f.events().iter().all(|e| e.source == SOURCE));
}

#[tokio::test]
async fn invalid_registrations_are_rejected() {
    let f = fixture();
    let host = f.adapter.host();
    assert_eq!(
        host.register_agent(spec("bad id!", full())),
        Err(HostError::Invalid("agent id"))
    );
    let mut s = spec("ok", full());
    s.name = "   ".into();
    assert_eq!(
        host.register_agent(s),
        Err(HostError::Invalid("agent name"))
    );
    assert!(f.snapshot().agents.is_empty());
}

#[tokio::test]
async fn an_adapter_cannot_touch_or_take_over_another_adapters_agent() {
    let f = fixture();
    let other = TestAdapter::new("other", AdapterKind::Bridge);
    f.ctl.register_adapter(other.clone());
    f.register("atlas", full());

    let intruder = other.host();
    assert_eq!(
        intruder.register_agent(spec("atlas", full())),
        Err(HostError::IdTaken("atlas".into()))
    );
    assert_eq!(
        intruder.update_session("atlas", SessionUpdate::new("s1", Status::Running)),
        Err(HostError::NotOwner("atlas".into()))
    );
    assert_eq!(
        intruder.set_online("atlas", false),
        Err(HostError::NotOwner("atlas".into()))
    );
    assert!(f.ctl.agent("atlas").unwrap().online);
}

#[tokio::test]
async fn a_session_id_belongs_to_the_agent_that_created_it() {
    let f = fixture();
    f.register("a", full());
    f.register("b", full());
    f.update("a", "shared", Status::Running);
    let err = f
        .adapter
        .host()
        .update_session("b", SessionUpdate::new("shared", Status::Running))
        .unwrap_err();
    assert_eq!(err, HostError::SessionOwnedByOther("shared".into()));
}

#[tokio::test]
async fn bad_session_updates_are_rejected() {
    let f = fixture();
    f.register("a", full());
    let host = f.adapter.host();
    assert!(host
        .update_session("a", SessionUpdate::new("bad session", Status::Running))
        .is_err());
    assert!(host
        .update_session("a", SessionUpdate::new("s", Status::Offline))
        .is_err());
    assert!(host
        .update_session(
            "a",
            SessionUpdate::new("s", Status::Running).progress(f32::NAN)
        )
        .is_err());
    assert!(f.snapshot().sessions.is_empty());
}

// ---------------------------------------------------------------------------
// Sessions: multiple at once, normalized states, events
// ---------------------------------------------------------------------------

#[tokio::test]
async fn several_agents_and_sessions_coexist_in_keyed_collections() {
    let f = fixture();
    f.register("a", full());
    f.register("b", full());
    f.update("a", "a1", Status::Running);
    f.update("a", "a2", Status::ToolUse);
    f.update("b", "b1", Status::Paused);
    let snap = f.snapshot();
    assert_eq!(snap.agents.len(), 2);
    assert_eq!(snap.sessions.len(), 3);
    assert_eq!(snap.summary.active_sessions, 3);
    let a = snap.agents.iter().find(|x| x.agent.id == "a").unwrap();
    assert_eq!(a.session_ids.len(), 2);
    // The agent is represented by its most urgent session.
    assert_eq!(a.status, Status::Running);
    let b = snap.agents.iter().find(|x| x.agent.id == "b").unwrap();
    assert_eq!(b.status, Status::Paused);
    // Each session keeps its own state.
    assert_eq!(f.session("a1").status, Status::Running);
    assert_eq!(f.session("a2").status, Status::ToolUse);
}

#[tokio::test]
async fn session_lifecycle_emits_the_documented_events() {
    let mut f = fixture();
    f.register("a", full());
    f.events();
    f.update("a", "s", Status::Starting);
    f.update("a", "s", Status::Running);
    f.update("a", "s", Status::ToolUse);
    f.update("a", "s", Status::Completed);
    let kinds = f.kinds();
    let position = |k: &str| kinds.iter().position(|x| x == k).unwrap_or(usize::MAX);
    for k in [
        "session.created",
        "session.started",
        "session.starting",
        "session.running",
        "session.tool_use",
        "session.completed",
    ] {
        assert!(kinds.contains(&k.to_string()), "missing {k}: {kinds:?}");
    }
    assert!(position("session.created") < position("session.started"));
    assert!(position("session.running") < position("session.completed"));
    assert_eq!(
        kinds.iter().filter(|k| *k == "session.started").count(),
        1,
        "started is emitted once per run"
    );
}

#[tokio::test]
async fn detail_and_progress_follow_the_status_unless_restated() {
    let f = fixture();
    f.register("a", full());
    let host = f.adapter.host();
    host.update_session(
        "a",
        SessionUpdate::new("s", Status::ToolUse)
            .detail("Bash")
            .progress(0.4)
            .task("Build it"),
    )
    .unwrap();
    assert_eq!(f.session("s").detail.as_deref(), Some("Bash"));
    // Same status, nothing restated: the current step stays.
    host.update_session("a", SessionUpdate::new("s", Status::ToolUse))
        .unwrap();
    assert_eq!(f.session("s").detail.as_deref(), Some("Bash"));
    assert_eq!(f.session("s").progress, Some(0.4));
    // New status: the step ends.
    host.update_session("a", SessionUpdate::new("s", Status::Running))
        .unwrap();
    assert_eq!(f.session("s").detail, None);
    assert_eq!(f.session("s").progress, None);
    assert_eq!(f.session("s").task.as_deref(), Some("Build it"));
}

#[tokio::test]
async fn failure_keeps_a_concise_error_and_clears_it_on_recovery() {
    let f = fixture();
    f.register("a", full());
    let host = f.adapter.host();
    let raw = format!("Tests failed:\n{}", "x".repeat(2000));
    host.update_session("a", SessionUpdate::new("s", Status::Failed).error(&raw))
        .unwrap();
    let e = f.session("s").error.unwrap();
    assert!(e.chars().count() <= MAX_ERROR_LEN);
    assert!(!e.contains('\n'));
    f.update("a", "s", Status::Running);
    assert_eq!(f.session("s").error, None);
}

#[tokio::test]
async fn ending_a_session_removes_it_and_frees_its_slot_for_the_agent() {
    let f = fixture();
    f.register("a", full());
    f.update("a", "s", Status::Running);
    assert!(matches!(f.ctl.slots()[0], Some(Track::Session { .. })));
    f.adapter.host().end_session("s").unwrap();
    assert!(f.ctl.session("s").is_none());
    assert!(matches!(f.ctl.slots()[0], Some(Track::Agent { .. })));
}

// ---------------------------------------------------------------------------
// Slots and focus
// ---------------------------------------------------------------------------

#[tokio::test]
async fn agents_without_sessions_get_a_slot_and_hand_it_to_their_first_session() {
    let f = fixture();
    f.register("a", full());
    f.register("b", full());
    assert_eq!(
        f.ctl.slots()[..2],
        [
            Some(Track::Agent {
                agent_id: "a".into()
            }),
            Some(Track::Agent {
                agent_id: "b".into()
            })
        ]
    );
    f.update("b", "b1", Status::Running);
    // b keeps slot 1; nothing else moves.
    assert_eq!(
        f.ctl.slots()[..2],
        [
            Some(Track::Agent {
                agent_id: "a".into()
            }),
            Some(Track::Session {
                session_id: "b1".into(),
                agent_id: "b".into()
            })
        ]
    );
}

#[tokio::test]
async fn slot_assignment_is_stable_while_other_things_change() {
    let f = fixture();
    f.register("a", full());
    f.register("b", full());
    f.update("a", "a1", Status::Running);
    f.update("b", "b1", Status::Running);
    let before = f.ctl.slots();
    f.update("a", "a1", Status::Completed);
    f.update("b", "b1", Status::Failed);
    f.update("a", "a2", Status::Running);
    let after = f.ctl.slots();
    assert_eq!(before[0], after[0]);
    assert_eq!(before[1], after[1]);
}

#[tokio::test]
async fn a_full_surface_evicts_a_finished_track_but_never_one_that_needs_attention() {
    let f = fixture();
    f.register("a", full());
    // Eight sessions fill every slot: one completed, one approval pending, six running.
    f.update("a", "done", Status::Completed);
    f.update("a", "wait", Status::Running);
    f.adapter
        .host()
        .request_approval(
            "a",
            "wait",
            ApprovalSpec {
                id: None,
                message: "m".into(),
                tool: None,
            },
        )
        .unwrap();
    for i in 0..6 {
        f.update("a", &format!("run{i}"), Status::Running);
    }
    assert!(f.ctl.slots().iter().all(Option::is_some));
    // A ninth session replaces the completed one, not the approval.
    f.update("a", "new", Status::Starting);
    let slots = f.ctl.slots();
    let has = |id: &str| {
        slots
            .iter()
            .any(|t| matches!(t, Some(Track::Session { session_id, .. }) if session_id == id))
    };
    assert!(has("new"));
    assert!(!has("done"), "the completed session gave up its pad");
    assert!(has("wait"), "an approval is never evicted");
    // It is still in the collection and the app.
    assert!(f.ctl.session("done").is_some());
    assert_eq!(
        f.snapshot()
            .sessions
            .iter()
            .find(|s| s.session.id == "done")
            .unwrap()
            .slot,
        None
    );
}

#[tokio::test]
async fn focus_is_a_selection_and_does_not_jump() {
    let f = fixture();
    f.register("a", full());
    f.register("b", full());
    f.update("a", "a1", Status::Running);
    f.update("b", "b1", Status::Running);
    assert_eq!(
        f.ctl.focus(),
        None,
        "nothing is focused until someone chooses"
    );

    f.ctl.focus_slot(0).unwrap();
    assert_eq!(f.ctl.focus(), Some(0));
    // An approval elsewhere does not steal the selection.
    f.adapter
        .host()
        .request_approval(
            "b",
            "b1",
            ApprovalSpec {
                id: None,
                message: "m".into(),
                tool: None,
            },
        )
        .unwrap();
    assert_eq!(f.ctl.focus(), Some(0));
    assert!(
        f.ctl.focus_slot(7).is_err(),
        "empty slots cannot be focused"
    );
    assert!(f.ctl.focus_slot(99).is_err());
}

#[tokio::test]
async fn with_no_focus_the_first_approval_and_the_first_failure_take_it() {
    let f = fixture();
    f.register("a", full());
    f.register("b", full());
    f.update("a", "a1", Status::Running);
    f.update("b", "b1", Status::Running);
    f.adapter
        .host()
        .request_approval(
            "b",
            "b1",
            ApprovalSpec {
                id: None,
                message: "m".into(),
                tool: None,
            },
        )
        .unwrap();
    assert_eq!(f.ctl.focus(), Some(1));

    f.ctl.clear_focus();
    f.update("a", "a1", Status::Failed);
    assert_eq!(f.ctl.focus(), Some(0));
}

// ---------------------------------------------------------------------------
// Approval lifecycle
// ---------------------------------------------------------------------------

fn ask(f: &Fixture, agent: &str, session: &str, tool: &str) -> String {
    f.adapter
        .host()
        .request_approval(
            agent,
            session,
            ApprovalSpec {
                id: None,
                message: format!("Run {tool}"),
                tool: Some(tool.into()),
            },
        )
        .unwrap()
}

#[tokio::test]
async fn an_approval_request_moves_the_session_and_focuses_it() {
    let mut f = fixture();
    f.register("a", full());
    f.update("a", "s", Status::Running);
    f.events();
    let id = ask(&f, "a", "s", "Bash");
    let s = f.session("s");
    assert_eq!(s.status, Status::ApprovalRequired);
    let approval = s.pending_approval.unwrap();
    assert_eq!(approval.id, id);
    assert_eq!(approval.tool.as_deref(), Some("Bash"));
    assert_eq!(f.ctl.focus(), Some(0));

    let events = f.events();
    let created = find(&events, "approval.created");
    assert_eq!(created.len(), 1);
    assert_eq!(created[0].payload["tool"], "Bash");
    assert!(
        created[0].payload.get("message").is_none(),
        "the request text is for the person deciding, not for the event log"
    );
    assert!(!find(&events, "session.approval_required").is_empty());
    // The projection tells the surface: slot flashes, APPROVE needs attention, REJECT is available.
    assert_eq!(
        last_slot(&events, 0).unwrap()["status"],
        "approval_required"
    );
    assert_eq!(last_state(&events, "approve").as_deref(), Some("attention"));
    assert_eq!(last_state(&events, "reject").as_deref(), Some("available"));
}

#[tokio::test]
async fn approving_resolves_the_approval_and_the_session_resumes() {
    let mut f = fixture();
    f.register("a", full());
    f.update("a", "s", Status::Running);
    let id = ask(&f, "a", "s", "Bash");
    f.events();

    let req = f.ctl.command(Target::Focus, Command::Approve).unwrap();
    // Sent to the adapter with the approval it decides.
    let sent = f.adapter.commands();
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].command, Command::Approve);
    assert_eq!(sent[0].approval_id.as_deref(), Some(id.as_str()));
    assert_eq!(sent[0].session_id.as_deref(), Some("s"));
    // Until the adapter answers, the command is pending and nothing is resolved.
    let events = f.events();
    assert_eq!(last_state(&events, "approve").as_deref(), Some("pending"));
    assert!(f.session("s").pending_approval.is_some());
    assert_eq!(
        f.session("s").last_command.unwrap().state,
        OutcomeState::Pending
    );

    f.adapter.host().command_result(&req, Ok(())).unwrap();
    let events = f.events();
    let resolved = find(&events, "approval.resolved");
    assert_eq!(resolved.len(), 1);
    assert_eq!(resolved[0].payload["resolution"], "approved");
    assert!(f.session("s").pending_approval.is_none());
    assert_eq!(f.session("s").status, Status::Running);
    assert_eq!(last_state(&events, "approve").as_deref(), Some("succeeded"));
    assert_eq!(
        f.session("s").last_command.unwrap().state,
        OutcomeState::Succeeded
    );
    assert_eq!(last_slot(&events, 0).unwrap()["status"], "running");
    assert!(!find(&events, "command.completed").is_empty());
}

#[tokio::test]
async fn rejecting_resolves_the_approval_as_rejected() {
    let mut f = fixture();
    f.register("a", full());
    f.update("a", "s", Status::Running);
    ask(&f, "a", "s", "Write");
    *f.adapter.auto.lock().unwrap() = Some(Ok(()));
    f.events();
    f.ctl.command(Target::Focus, Command::Reject).unwrap();
    let events = f.events();
    assert_eq!(
        find(&events, "approval.resolved")[0].payload["resolution"],
        "rejected"
    );
    assert!(f.session("s").pending_approval.is_none());
}

#[tokio::test]
async fn nothing_is_ever_approved_without_a_command() {
    let f = fixture();
    f.register("a", full());
    f.update("a", "s", Status::Running);
    ask(&f, "a", "s", "Bash");
    f.update("a", "s2", Status::Running);
    // Time, other updates and other sessions do not resolve it.
    assert!(f.session("s").pending_approval.is_some());
    assert!(f.adapter.commands().is_empty());
}

#[tokio::test]
async fn leaving_approval_required_without_a_decision_withdraws_the_approval() {
    let mut f = fixture();
    f.register("a", full());
    f.update("a", "s", Status::Running);
    ask(&f, "a", "s", "Bash");
    f.events();
    // The user answered in the harness's own dialog; the agent carries on.
    f.update("a", "s", Status::Running);
    assert!(f.session("s").pending_approval.is_none());
    let events = f.events();
    assert_eq!(
        find(&events, "approval.resolved")[0].payload["resolution"],
        "withdrawn"
    );
    assert_eq!(
        last_state(&events, "approve").as_deref(),
        Some("unavailable")
    );
}

#[tokio::test]
async fn a_newer_request_supersedes_the_older_one() {
    let mut f = fixture();
    f.register("a", full());
    f.update("a", "s", Status::Running);
    let first = ask(&f, "a", "s", "Bash");
    f.events();
    let second = ask(&f, "a", "s", "Write");
    assert_ne!(first, second);
    assert_eq!(f.session("s").pending_approval.unwrap().id, second);
    assert_eq!(
        find(&f.events(), "approval.resolved")[0].payload["resolution"],
        "superseded"
    );
}

#[tokio::test]
async fn the_agent_can_withdraw_only_a_matching_approval() {
    let f = fixture();
    f.register("a", full());
    f.update("a", "s", Status::Running);
    let id = ask(&f, "a", "s", "Bash");
    f.adapter
        .host()
        .withdraw_approval("s", "someone-elses")
        .unwrap();
    assert!(f.session("s").pending_approval.is_some());
    f.adapter.host().withdraw_approval("s", &id).unwrap();
    assert!(f.session("s").pending_approval.is_none());
}

// ---------------------------------------------------------------------------
// Capabilities, access and command handling
// ---------------------------------------------------------------------------

#[tokio::test]
async fn unsupported_commands_are_refused_and_shown_as_unavailable() {
    let f = fixture();
    // Observation plus approvals only, like a hook-based harness.
    f.register("hooked", caps(&[Capability::Approve, Capability::Reject]));
    f.update("hooked", "s", Status::Running);
    let err = f
        .ctl
        .command(Target::Session("s".into()), Command::Pause)
        .unwrap_err();
    assert_eq!(err, CommandRefused::Unavailable(Unavailable::Unsupported));
    assert!(f.adapter.commands().is_empty());

    let view = f
        .snapshot()
        .sessions
        .into_iter()
        .find(|s| s.session.id == "s")
        .unwrap();
    let pause = view
        .commands
        .iter()
        .find(|c| c.kind == CommandKind::Pause)
        .unwrap();
    assert_eq!(pause.state, super::visual::CommandState::Unavailable);
    assert_eq!(pause.reason_code, Some(Unavailable::Unsupported));
    let reject = view
        .commands
        .iter()
        .find(|c| c.kind == CommandKind::Reject)
        .unwrap();
    assert_eq!(reject.reason_code, Some(Unavailable::NothingPending));
}

#[tokio::test]
async fn commands_depend_on_the_session_state() {
    let f = fixture();
    f.register("a", full());
    f.update("a", "s", Status::Running);
    assert_eq!(
        f.ctl
            .command(Target::Session("s".into()), Command::Resume)
            .unwrap_err(),
        CommandRefused::Unavailable(Unavailable::WrongState)
    );
    assert_eq!(
        f.ctl
            .command(Target::Session("s".into()), Command::Retry)
            .unwrap_err(),
        CommandRefused::Unavailable(Unavailable::WrongState)
    );
    assert_eq!(
        f.ctl
            .command(Target::Session("s".into()), Command::Approve)
            .unwrap_err(),
        CommandRefused::Unavailable(Unavailable::NothingPending)
    );
    f.ctl
        .command(Target::Session("s".into()), Command::Pause)
        .unwrap();
    assert_eq!(f.adapter.commands().len(), 1);
}

#[tokio::test]
async fn monitor_only_agents_cannot_be_controlled_until_the_user_grants_access() {
    let dir = tempfile::tempdir().unwrap();
    let mut f = fixture_with(
        AdapterKind::Bridge,
        Timing::default(),
        Some(AgentsStorage::new(dir.path())),
    );
    f.register("ext", full());
    assert_eq!(f.ctl.agent("ext").unwrap().access, AccessLevel::Monitor);
    f.update("ext", "s", Status::Running);
    f.events();

    let err = f
        .ctl
        .command(Target::Session("s".into()), Command::Cancel)
        .unwrap_err();
    assert_eq!(err, CommandRefused::Unavailable(Unavailable::MonitorOnly));
    assert!(
        f.adapter.commands().is_empty(),
        "nothing reached the adapter"
    );
    let audit = f.snapshot().audit;
    assert!(audit.iter().any(|e| e.kind == "command.refused"));

    f.ctl.set_access("ext", AccessLevel::Control).unwrap();
    assert!(f.kinds().contains(&"agent.access".to_string()));
    f.ctl
        .command(Target::Session("s".into()), Command::Cancel)
        .unwrap();
    assert_eq!(f.adapter.commands().len(), 1);

    // The grant survives a restart (same agent id, new service instance).
    let f2 = fixture_with(
        AdapterKind::Bridge,
        Timing::default(),
        Some(AgentsStorage::new(dir.path())),
    );
    f2.register("ext", full());
    assert_eq!(f2.ctl.agent("ext").unwrap().access, AccessLevel::Control);
}

#[tokio::test]
async fn simulated_agents_start_with_control_everything_else_does_not() {
    let sim = fixture_with(AdapterKind::Simulated, Timing::default(), None);
    sim.register("a", full());
    assert_eq!(sim.ctl.agent("a").unwrap().access, AccessLevel::Control);
    for kind in [AdapterKind::Bridge, AdapterKind::Hooks] {
        let f = fixture_with(kind, Timing::default(), None);
        f.register("a", full());
        assert_eq!(f.ctl.agent("a").unwrap().access, AccessLevel::Monitor);
    }
}

#[tokio::test]
async fn a_second_decision_while_one_is_in_flight_is_refused() {
    let f = fixture();
    f.register("a", full());
    f.update("a", "s", Status::Running);
    ask(&f, "a", "s", "Bash");
    f.ctl.command(Target::Focus, Command::Approve).unwrap();
    assert_eq!(
        f.ctl.command(Target::Focus, Command::Approve).unwrap_err(),
        CommandRefused::AlreadyPending
    );
    assert_eq!(
        f.ctl.command(Target::Focus, Command::Reject).unwrap_err(),
        CommandRefused::AlreadyPending,
        "approve and reject exclude each other"
    );
    assert_eq!(f.adapter.commands().len(), 1);
}

#[tokio::test]
async fn invalid_command_payloads_are_rejected_before_dispatch() {
    let f = fixture();
    f.register("a", full());
    f.update("a", "s", Status::Running);
    let bad = [
        Command::SendInstruction { text: "  ".into() },
        Command::SendInstruction {
            text: "x".repeat(5000),
        },
        Command::Spawn {
            task: String::new(),
            cwd: None,
        },
        Command::Spawn {
            task: "t".into(),
            cwd: Some("a\nb".into()),
        },
    ];
    for c in bad {
        assert!(matches!(
            f.ctl.command(Target::Session("s".into()), c),
            Err(CommandRefused::Invalid(_))
        ));
    }
    assert!(f.adapter.commands().is_empty());
}

#[tokio::test]
async fn an_adapter_error_fails_the_command_immediately_and_is_audited() {
    let mut f = fixture();
    f.register("a", full());
    f.update("a", "s", Status::Running);
    f.adapter.reject_execute.store(true, Ordering::SeqCst);
    f.events();
    f.ctl
        .command(Target::Session("s".into()), Command::Pause)
        .unwrap();
    let s = f.session("s");
    let outcome = s.last_command.unwrap();
    assert_eq!(outcome.state, OutcomeState::Failed);
    assert_eq!(outcome.error.as_deref(), Some("adapter refused"));
    assert_eq!(
        s.status,
        Status::Running,
        "a failed command changes nothing else"
    );
    assert!(f
        .snapshot()
        .audit
        .iter()
        .any(|e| e.kind == "command.failed"));
    let events = f.events();
    assert_eq!(
        find(&events, "command.completed")[0].payload["state"],
        "failed"
    );
}

#[tokio::test(start_paused = true)]
async fn a_command_the_adapter_never_answers_times_out_as_failed() {
    let timing = Timing {
        command_timeout: Duration::from_secs(5),
        ..Timing::default()
    };
    let f = fixture_with(AdapterKind::Simulated, timing, None);
    f.register("a", full());
    f.update("a", "s", Status::Running);
    f.ctl
        .command(Target::Session("s".into()), Command::Pause)
        .unwrap();
    assert_eq!(
        f.session("s").last_command.unwrap().state,
        OutcomeState::Pending
    );

    tokio::time::sleep(Duration::from_secs(6)).await;
    let outcome = f.session("s").last_command.unwrap();
    assert_eq!(outcome.state, OutcomeState::Failed);
    assert!(outcome.error.unwrap().contains("did not answer"));
}

#[tokio::test(start_paused = true)]
async fn pad_feedback_is_transient_while_the_outcome_stays_on_the_session() {
    let timing = Timing {
        succeeded_feedback: Duration::from_millis(1000),
        ..Timing::default()
    };
    let mut f = fixture_with(AdapterKind::Simulated, timing, None);
    f.register("a", full());
    f.update("a", "s", Status::Running);
    f.ctl.focus_slot(0).unwrap();
    *f.adapter.auto.lock().unwrap() = Some(Ok(()));
    f.events();

    f.ctl.command(Target::Focus, Command::Pause).unwrap();
    assert_eq!(
        last_state(&f.events(), "pause").as_deref(),
        Some("succeeded")
    );
    tokio::time::sleep(Duration::from_millis(1100)).await;
    // The feedback expired; pause is not possible while still "running" in this test, so it
    // returns to a resting state, never stuck on succeeded.
    let events = f.events();
    let state = last_state(&events, "pause").unwrap();
    assert_ne!(state, "succeeded");
    assert_eq!(
        f.session("s").last_command.unwrap().state,
        OutcomeState::Succeeded
    );
}

#[tokio::test]
async fn a_failed_command_is_reported_with_a_concise_error() {
    let mut f = fixture();
    f.register("a", full());
    f.update("a", "s", Status::Running);
    f.ctl.focus_slot(0).unwrap();
    f.events();
    let req = f.ctl.command(Target::Focus, Command::Pause).unwrap();
    f.adapter
        .host()
        .command_result(
            &req,
            Err("harness exited with code 2\nstack trace ...".into()),
        )
        .unwrap();
    let events = f.events();
    assert_eq!(last_state(&events, "pause").as_deref(), Some("failed"));
    let outcome = f.session("s").last_command.unwrap();
    assert!(!outcome.error.unwrap().contains('\n'));
}

#[tokio::test]
async fn results_for_unknown_or_foreign_requests_are_rejected() {
    let f = fixture();
    let other = TestAdapter::new("other", AdapterKind::Bridge);
    f.ctl.register_adapter(other.clone());
    f.register("a", full());
    f.update("a", "s", Status::Running);
    let req = f
        .ctl
        .command(Target::Session("s".into()), Command::Pause)
        .unwrap();
    assert!(matches!(
        other.host().command_result(&req, Ok(())),
        Err(HostError::NotOwner(_))
    ));
    assert!(matches!(
        f.adapter.host().command_result("req-999", Ok(())),
        Err(HostError::UnknownRequest(_))
    ));
}

#[tokio::test]
async fn spawn_targets_the_focused_agent_or_the_first_capable_one() {
    let f = fixture();
    f.register("watcher", caps(&[Capability::Approve]));
    f.register("worker", full());
    let task = || Command::Spawn {
        task: "Run the tests".into(),
        cwd: Some("/work".into()),
    };

    // No focus: the first agent that can spawn.
    f.ctl.command(Target::Focus, task()).unwrap();
    let sent = f.adapter.commands();
    assert_eq!(sent[0].agent_id, "worker");
    assert_eq!(sent[0].session_id, None);

    // Focus on an agent that cannot spawn: unavailable, not silently redirected.
    f.ctl.focus_slot(0).unwrap();
    assert_eq!(
        f.ctl.command(Target::Focus, task()).unwrap_err(),
        CommandRefused::Unavailable(Unavailable::Unsupported)
    );
}

// ---------------------------------------------------------------------------
// Offline, notifications, audit
// ---------------------------------------------------------------------------

#[tokio::test]
async fn going_offline_fails_pending_commands_and_shows_offline_everywhere() {
    let mut f = fixture();
    f.register("a", full());
    f.update("a", "s", Status::Running);
    f.ctl
        .command(Target::Session("s".into()), Command::Pause)
        .unwrap();
    f.events();

    f.adapter.host().set_online("a", false).unwrap();
    let events = f.events();
    assert!(!find(&events, "agent.offline").is_empty());
    assert_eq!(
        f.session("s").last_command.unwrap().state,
        OutcomeState::Failed
    );
    assert_eq!(last_slot(&events, 0).unwrap()["status"], "offline");
    let view = f
        .snapshot()
        .sessions
        .into_iter()
        .find(|s| s.session.id == "s")
        .unwrap();
    assert_eq!(view.session.status, Status::Offline);
    assert_eq!(
        f.ctl
            .command(Target::Session("s".into()), Command::Pause)
            .unwrap_err(),
        CommandRefused::Unavailable(Unavailable::Offline)
    );
    let kinds: Vec<_> = f
        .sink
        .notifications
        .lock()
        .unwrap()
        .iter()
        .map(|n| n.kind)
        .collect();
    assert!(kinds.contains(&NotificationKind::AdapterDisconnected));

    // Coming back restores the last reported status.
    f.adapter.host().set_online("a", true).unwrap();
    assert_eq!(f.snapshot().sessions[0].session.status, Status::Running);
}

#[tokio::test(start_paused = true)]
async fn only_the_four_documented_situations_raise_notifications() {
    let timing = Timing {
        long_running: Duration::from_secs(120),
        ..Timing::default()
    };
    let f = fixture_with(AdapterKind::Simulated, timing, None);
    f.register("a", full());
    let kinds = |f: &Fixture| -> Vec<NotificationKind> {
        f.sink
            .notifications
            .lock()
            .unwrap()
            .iter()
            .map(|n| n.kind)
            .collect()
    };

    // Ordinary progress and a short completion are ambient only.
    f.update("a", "quick", Status::Starting);
    f.update("a", "quick", Status::Running);
    f.update("a", "quick", Status::ToolUse);
    f.update("a", "quick", Status::Paused);
    f.update("a", "quick", Status::Completed);
    assert!(kinds(&f).is_empty(), "{:?}", kinds(&f));

    // A long run that completes does.
    f.update("a", "long", Status::Running);
    tokio::time::advance(Duration::from_secs(300)).await;
    f.update("a", "long", Status::Completed);
    assert_eq!(kinds(&f), vec![NotificationKind::LongRunningCompleted]);

    // Approval required and failure do.
    f.update("a", "x", Status::Running);
    ask(&f, "a", "x", "Bash");
    f.update("a", "y", Status::Failed);
    assert_eq!(
        kinds(&f),
        vec![
            NotificationKind::LongRunningCompleted,
            NotificationKind::ApprovalRequired,
            NotificationKind::AgentFailed
        ]
    );
    // Their text never carries the request message.
    let n = f.sink.notifications.lock().unwrap();
    assert!(n.iter().all(|n| !n.body.contains("Run Bash")));
}

#[tokio::test]
async fn notifications_can_be_switched_off() {
    let f = fixture();
    f.register("a", full());
    f.ctl.update_settings(|s| s.notifications = false);
    f.update("a", "s", Status::Failed);
    assert!(f.sink.notifications.lock().unwrap().is_empty());
    // The pad and the app still show the failure.
    assert_eq!(f.snapshot().summary.failed_sessions, 1);
}

#[tokio::test]
async fn the_audit_trail_records_decisions_and_never_free_text() {
    let dir = tempfile::tempdir().unwrap();
    let storage = AgentsStorage::new(dir.path());
    let f = fixture_with(
        AdapterKind::Simulated,
        Timing::default(),
        Some(storage.clone()),
    );
    f.register("a", full());
    f.adapter
        .host()
        .update_session(
            "a",
            SessionUpdate::new("s", Status::Running).task("rotate the production keys"),
        )
        .unwrap();
    f.adapter
        .host()
        .request_approval(
            "a",
            "s",
            ApprovalSpec {
                id: None,
                message: "curl https://secret.example/?token=hunter2".into(),
                tool: Some("Bash".into()),
            },
        )
        .unwrap();
    *f.adapter.auto.lock().unwrap() = Some(Ok(()));
    f.ctl.command(Target::Focus, Command::Approve).unwrap();
    f.ctl
        .command(
            Target::Session("s".into()),
            Command::SendInstruction {
                text: "also delete the backups".into(),
            },
        )
        .unwrap();
    f.update("a", "s", Status::Failed);

    let text = std::fs::read_to_string(storage.audit_path()).unwrap();
    for needle in [
        "hunter2",
        "secret.example",
        "rotate the production",
        "delete the backups",
    ] {
        assert!(!text.contains(needle), "audit leaked `{needle}`");
    }
    for kind in [
        "agent.registered",
        "session.started",
        "approval.requested",
        "approval.approved",
        "session.failed",
    ] {
        assert!(text.contains(kind), "audit is missing `{kind}`:\n{text}");
    }
    assert!(text.contains("\"tool\":\"Bash\""));
}

#[tokio::test]
async fn cancel_is_audited_as_an_intervention() {
    let f = fixture();
    f.register("a", full());
    f.update("a", "s", Status::Running);
    *f.adapter.auto.lock().unwrap() = Some(Ok(()));
    f.ctl
        .command(Target::Session("s".into()), Command::Cancel)
        .unwrap();
    f.update("a", "s", Status::Cancelled);
    let kinds: Vec<String> = f.snapshot().audit.into_iter().map(|e| e.kind).collect();
    assert!(kinds.contains(&"intervention.cancel".to_string()));
    assert!(kinds.contains(&"session.cancelled".to_string()));
}

// ---------------------------------------------------------------------------
// Projection (what surfaces bind to) and housekeeping
// ---------------------------------------------------------------------------

#[tokio::test]
async fn republish_sends_the_whole_projection_again() {
    let mut f = fixture();
    f.register("a", full());
    f.update("a", "s", Status::Running);
    f.events();
    f.ctl.republish();
    let events = f.events();
    assert_eq!(find(&events, "slot.changed").len(), SLOT_COUNT);
    assert_eq!(find(&events, "command.state").len(), CommandKind::ALL.len());
    assert_eq!(find(&events, "focus.changed").len(), 1);
    assert_eq!(last_slot(&events, 0).unwrap()["status"], "running");
    assert_eq!(last_slot(&events, 5).unwrap()["status"], "empty");
}

#[tokio::test]
async fn only_changes_are_projected() {
    let mut f = fixture();
    f.register("a", full());
    f.update("a", "s", Status::Running);
    f.events();
    // A progress report changes nothing a pad can show.
    f.adapter
        .host()
        .update_session("a", SessionUpdate::new("s", Status::Running).progress(0.5))
        .unwrap();
    let events = f.events();
    assert!(find(&events, "slot.changed").is_empty());
    assert!(find(&events, "command.state").is_empty());
}

#[tokio::test]
async fn command_pads_follow_the_focused_track_only() {
    let mut f = fixture();
    f.register("a", full());
    f.register("b", caps(&[Capability::Approve]));
    f.update("a", "a1", Status::Running);
    f.update("b", "b1", Status::Running);
    f.events();

    f.ctl.focus_slot(0).unwrap();
    let events = f.events();
    assert_eq!(last_state(&events, "pause").as_deref(), Some("available"));
    assert_eq!(last_state(&events, "cancel").as_deref(), Some("available"));
    // Resume was unavailable before and still is: unchanged states are not re-sent.
    assert_eq!(last_state(&events, "resume"), None);

    f.ctl.focus_slot(1).unwrap();
    let events = f.events();
    assert_eq!(last_state(&events, "pause").as_deref(), Some("unavailable"));
    assert_eq!(
        last_state(&events, "cancel").as_deref(),
        Some("unavailable")
    );
}

#[tokio::test]
async fn dismissing_finishes_only_finished_work() {
    let f = fixture();
    f.register("a", full());
    f.update("a", "run", Status::Running);
    f.update("a", "ok", Status::Completed);
    f.update("a", "bad", Status::Failed);
    assert!(f.ctl.dismiss_session("run").is_err());
    f.ctl.dismiss_session("ok").unwrap();
    assert!(f.ctl.session("ok").is_none());
    assert_eq!(f.ctl.dismiss_finished(), 1);
    assert!(f.ctl.session("bad").is_none());
    assert!(f.ctl.session("run").is_some());
    assert!(f.ctl.dismiss_session("missing").is_err());
}

#[tokio::test]
async fn the_snapshot_summarises_what_the_overview_needs() {
    let f = fixture();
    f.register("a", full());
    f.register("b", full());
    f.update("a", "r", Status::Running);
    f.update("a", "w", Status::Running);
    ask(&f, "a", "w", "Bash");
    f.update("b", "bad", Status::Failed);
    f.update("b", "ok", Status::Completed);
    f.adapter.host().set_online("b", false).unwrap();
    let s = f.snapshot().summary;
    assert_eq!(s.agents_total, 2);
    assert_eq!(s.agents_online, 1);
    assert_eq!(s.pending_approvals, 1);
    assert_eq!(
        s.failed_sessions, 0,
        "an offline agent's sessions show as offline"
    );
    f.adapter.host().set_online("b", true).unwrap();
    let s = f.snapshot().summary;
    assert_eq!(s.failed_sessions, 1);
    assert_eq!(s.recent_completions, 1);
    assert_eq!(s.active_sessions, 2);
    // The snapshot sent to the UI sink is the same one.
    assert_eq!(f.sink.last.lock().unwrap().as_ref().unwrap().summary, s);
}

#[tokio::test]
async fn snapshot_revisions_only_increase() {
    let f = fixture();
    f.register("a", full());
    let r1 = f.snapshot().rev;
    f.update("a", "s", Status::Running);
    let r2 = f.snapshot().rev;
    f.update("a", "s", Status::Completed);
    assert!(r1 < r2 && r2 < f.snapshot().rev);
}

#[tokio::test]
async fn session_collections_stay_bounded() {
    let f = fixture();
    f.register("a", full());
    for i in 0..100 {
        f.update("a", &format!("s{i}"), Status::Completed);
    }
    let n = f.snapshot().sessions.len();
    assert!(
        n <= 32,
        "per-agent retention of finished sessions is capped, got {n}"
    );
    // Live sessions are never discarded to make room.
    let f = fixture();
    f.register("a", full());
    for i in 0..32 {
        f.update("a", &format!("s{i}"), Status::Running);
    }
    assert!(matches!(
        f.adapter
            .host()
            .update_session("a", SessionUpdate::new("one-more", Status::Running)),
        Err(HostError::LimitReached(_))
    ));
}
