//! Hook-based harness adapters.
//!
//! Some agent harnesses cannot be driven from outside, but they do announce
//! what they are doing through user-configured "hooks": small calls made at
//! lifecycle points (a prompt was submitted, a tool is about to run, a
//! permission is needed, the turn ended). This module turns those calls into
//! the normalized Agent Control model.
//!
//! One translator serves every harness whose hook payloads share this
//! vocabulary; a [`Dialect`] carries the few facts that differ (names, route,
//! honest limits). The translator and the adapter are the only places that
//! know a harness exists. They never see MIDI, pads or colours.
//!
//! What hooks can and cannot do is a property of the harness, not of this
//! code, and it is stated in each dialect's `limits`: hooks observe, and they
//! can answer a permission request. They cannot pause, resume, cancel, retry
//! or instruct a running session, so this adapter declares only `approve` and
//! `reject`, and the rest stay unavailable everywhere.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};
use std::time::Duration;

use serde_json::{json, Value};
use tokio::sync::oneshot;

use super::http::{Outcome, Route};
use crate::agents::adapter::*;
use crate::agents::model::*;

/// The facts that differ between harnesses.
pub struct Dialect {
    pub adapter_id: &'static str,
    pub adapter_name: &'static str,
    pub description: &'static str,
    pub limits: &'static [&'static str],
    pub agent_id: &'static str,
    pub agent_name: &'static str,
    pub provider: &'static str,
    pub harness: &'static str,
    /// Prefix of LaunchDeck session ids derived from harness session ids.
    pub session_prefix: &'static str,
    /// Path the harness posts to.
    pub route: &'static str,
}

/// Hook payload kinds this adapter understands.
#[derive(Debug, Clone, PartialEq)]
pub enum HookEvent {
    SessionStarted {
        session: String,
        cwd: Option<String>,
    },
    Prompt {
        session: String,
        prompt: String,
    },
    ToolStarted {
        session: String,
        tool: String,
    },
    ToolFinished {
        session: String,
    },
    PermissionRequest {
        session: String,
        tool: String,
        message: String,
    },
    TurnFinished {
        session: String,
    },
    TurnFailed {
        session: String,
        error: String,
    },
    Interrupted {
        session: String,
    },
    SessionEnded {
        session: String,
    },
    /// A well-formed event this adapter has no use for (compaction, subagents...).
    Ignored,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum TranslateError {
    #[error("payload is not an object")]
    NotAnObject,
    #[error("missing `{0}`")]
    Missing(&'static str),
}

fn text<'a>(v: &'a Value, key: &str) -> Option<&'a str> {
    v.get(key).and_then(Value::as_str).filter(|s| !s.is_empty())
}

/// One short line describing what a tool wants to do, for the human who must
/// decide. Reads the common input shapes; falls back to the tool name.
pub fn describe_tool(tool: &str, input: &Value) -> String {
    let first_line = |s: &str| s.lines().next().unwrap_or("").trim().to_string();
    let command = match input.get("command") {
        Some(Value::String(s)) => Some(first_line(s)),
        Some(Value::Array(a)) => Some(
            a.iter()
                .filter_map(Value::as_str)
                .collect::<Vec<_>>()
                .join(" "),
        ),
        _ => None,
    };
    let line = if let Some(c) = command.filter(|c| !c.is_empty()) {
        format!("{tool}: {c}")
    } else if let Some(p) = text(input, "file_path").or_else(|| text(input, "path")) {
        format!("{tool} {p}")
    } else if let Some(u) = text(input, "url") {
        format!("{tool} {u}")
    } else if let Some(d) = text(input, "description") {
        format!("{tool}: {d}")
    } else {
        format!("Use {tool}")
    };
    summarize(&line, MAX_MESSAGE_LEN)
}

/// Translate one hook payload. Pure, so it is tested against captured payloads.
pub fn translate(payload: &Value) -> Result<HookEvent, TranslateError> {
    if !payload.is_object() {
        return Err(TranslateError::NotAnObject);
    }
    let name =
        text(payload, "hook_event_name").ok_or(TranslateError::Missing("hook_event_name"))?;
    let session = text(payload, "session_id")
        .ok_or(TranslateError::Missing("session_id"))?
        .to_string();
    let tool = || text(payload, "tool_name").unwrap_or("tool").to_string();
    Ok(match name {
        "SessionStart" => HookEvent::SessionStarted {
            session,
            cwd: text(payload, "cwd").map(str::to_string),
        },
        "UserPromptSubmit" => HookEvent::Prompt {
            session,
            prompt: text(payload, "prompt").unwrap_or("").to_string(),
        },
        "PreToolUse" => HookEvent::ToolStarted {
            session,
            tool: tool(),
        },
        "PostToolUse" | "PostToolUseFailure" => HookEvent::ToolFinished { session },
        "PermissionRequest" => {
            let t = tool();
            let message = describe_tool(&t, payload.get("tool_input").unwrap_or(&Value::Null));
            HookEvent::PermissionRequest {
                session,
                tool: t,
                message,
            }
        }
        "Stop" => HookEvent::TurnFinished { session },
        "StopFailure" => HookEvent::TurnFailed {
            session,
            error: text(payload, "error_message")
                .or_else(|| text(payload, "error_type"))
                .unwrap_or("The turn ended with an error.")
                .to_string(),
        },
        "Interrupt" => HookEvent::Interrupted { session },
        "SessionEnd" => HookEvent::SessionEnded { session },
        _ => HookEvent::Ignored,
    })
}

/// The reply a harness reads as a permission decision.
pub fn decision_body(approve: bool) -> Value {
    let decision = if approve {
        json!({ "behavior": "allow" })
    } else {
        json!({ "behavior": "deny", "message": "Rejected from LaunchDeck." })
    };
    json!({ "hookSpecificOutput": { "hookEventName": "PermissionRequest", "decision": decision } })
}

fn session_id(dialect: &Dialect, raw: &str) -> SessionId {
    let clean: String = raw
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | ':' | '-'))
        .take(MAX_ID_LEN - dialect.session_prefix.len())
        .collect();
    format!("{}{}", dialect.session_prefix, clean)
}

fn project_of(cwd: &str) -> String {
    cwd.trim_end_matches(['/', '\\'])
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or("")
        .to_string()
}

struct Waiter {
    session: SessionId,
    tx: oneshot::Sender<Option<Value>>,
}

#[derive(Default)]
struct Inner {
    enabled: bool,
    hold: Duration,
    /// Status last reported per LaunchDeck session id.
    sessions: HashMap<SessionId, Status>,
    waiters: HashMap<ApprovalId, Waiter>,
    next_approval: u64,
}

pub struct HookAdapter {
    dialect: &'static Dialect,
    host: OnceLock<AdapterHost>,
    inner: Mutex<Inner>,
}

const MAX_TRACKED: usize = 512;

impl HookAdapter {
    pub fn new(dialect: &'static Dialect) -> Arc<Self> {
        Arc::new(Self {
            dialect,
            host: OnceLock::new(),
            inner: Mutex::new(Inner {
                hold: Duration::from_secs(90),
                ..Inner::default()
            }),
        })
    }

    pub fn dialect(&self) -> &'static Dialect {
        self.dialect
    }

    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// The route to hand to the HTTP server.
    pub fn route(self: &Arc<Self>) -> Arc<dyn Route> {
        Arc::new(HookRoute(self.clone()))
    }

    /// Turn the adapter on (the endpoint is listening) or off. While on, the
    /// harness appears as an online agent.
    pub fn set_enabled(&self, enabled: bool, state: AdapterState, detail: &str, hold: Duration) {
        let Some(host) = self.host.get() else { return };
        {
            let mut inner = self.lock();
            inner.enabled = enabled;
            inner.hold = hold;
            if !enabled {
                // Dropping the senders releases every held request with "no decision".
                inner.waiters.clear();
            }
        }
        if enabled {
            let _ = host.register_agent(AgentSpec {
                id: self.dialect.agent_id.into(),
                name: self.dialect.agent_name.into(),
                provider: Some(self.dialect.provider.into()),
                harness: Some(self.dialect.harness.into()),
                capabilities: [Capability::Approve, Capability::Reject]
                    .into_iter()
                    .collect(),
            });
        } else {
            let _ = host.set_online(self.dialect.agent_id, false);
        }
        host.set_state(state, Some(detail));
    }

    fn report(&self, host: &AdapterHost, update: SessionUpdate) {
        let sid = update.session_id.clone();
        let status = update.status;
        match host.update_session(self.dialect.agent_id, update) {
            Ok(()) => {
                let mut inner = self.lock();
                if inner.sessions.len() >= MAX_TRACKED {
                    inner.sessions.retain(|_, s| !s.is_terminal());
                }
                inner.sessions.insert(sid, status);
            }
            Err(e) => host.diagnostic(
                DiagnosticLevel::Warn,
                &format!("could not update a session: {e}"),
            ),
        }
    }

    fn known(&self, sid: &str) -> Option<Status> {
        self.lock().sessions.get(sid).copied()
    }

    fn handle(self: &Arc<Self>, payload: Value) -> Outcome {
        let Some(host) = self.host.get() else {
            return Outcome::Reply(None);
        };
        if !self.lock().enabled {
            return Outcome::Reply(None);
        }
        let event = match translate(&payload) {
            Ok(e) => e,
            Err(e) => {
                host.diagnostic(
                    DiagnosticLevel::Warn,
                    &format!("ignored a malformed hook payload: {e}"),
                );
                return Outcome::Reply(None);
            }
        };
        let d = self.dialect;
        match event {
            HookEvent::Ignored => {}
            HookEvent::SessionStarted { session, cwd } => {
                let sid = session_id(d, &session);
                if self.known(&sid).is_none() {
                    let task = cwd
                        .as_deref()
                        .map(project_of)
                        .filter(|p| !p.is_empty())
                        .map(|p| format!("Session in {p}"));
                    let mut u = SessionUpdate::new(&sid, Status::Idle);
                    u.task = task;
                    self.report(host, u);
                }
            }
            HookEvent::Prompt { session, prompt } => {
                let sid = session_id(d, &session);
                let line = prompt.lines().find(|l| !l.trim().is_empty()).unwrap_or("");
                let mut u = SessionUpdate::new(&sid, Status::Running);
                if !line.is_empty() {
                    u.task = Some(summarize(line, MAX_TASK_LEN));
                }
                self.report(host, u);
            }
            HookEvent::ToolStarted { session, tool } => {
                let sid = session_id(d, &session);
                self.report(
                    host,
                    SessionUpdate::new(&sid, Status::ToolUse).detail(&tool),
                );
            }
            HookEvent::ToolFinished { session } => {
                let sid = session_id(d, &session);
                self.report(host, SessionUpdate::new(&sid, Status::Running));
            }
            HookEvent::TurnFinished { session } => {
                let sid = session_id(d, &session);
                self.report(host, SessionUpdate::new(&sid, Status::Completed));
            }
            HookEvent::TurnFailed { session, error } => {
                let sid = session_id(d, &session);
                self.report(host, SessionUpdate::new(&sid, Status::Failed).error(&error));
            }
            HookEvent::Interrupted { session } => {
                let sid = session_id(d, &session);
                self.report(
                    host,
                    SessionUpdate::new(&sid, Status::Waiting).detail("Interrupted"),
                );
            }
            HookEvent::SessionEnded { session } => {
                let sid = session_id(d, &session);
                // Waiters for this session can no longer be answered.
                self.lock().waiters.retain(|_, w| w.session != sid);
                if self.known(&sid).is_some_and(|s| s.is_active()) {
                    self.report(host, SessionUpdate::new(&sid, Status::Cancelled));
                }
                self.lock().sessions.remove(&sid);
            }
            HookEvent::PermissionRequest {
                session,
                tool,
                message,
            } => return self.permission(host, &session, &tool, message),
        }
        Outcome::Reply(None)
    }

    fn permission(
        self: &Arc<Self>,
        host: &AdapterHost,
        session: &str,
        tool: &str,
        message: String,
    ) -> Outcome {
        let d = self.dialect;
        let sid = session_id(d, session);
        if self.known(&sid).is_none() {
            self.report(host, SessionUpdate::new(&sid, Status::Running).detail(tool));
        }
        let (approval_id, hold) = {
            let mut inner = self.lock();
            inner.next_approval += 1;
            (
                format!("{}ap{}", d.session_prefix, inner.next_approval),
                inner.hold,
            )
        };
        let requested = host.request_approval(
            d.agent_id,
            &sid,
            ApprovalSpec {
                id: Some(approval_id.clone()),
                message,
                tool: Some(tool.to_string()),
            },
        );
        if let Err(e) = requested {
            host.diagnostic(
                DiagnosticLevel::Warn,
                &format!("could not show an approval: {e}"),
            );
            return Outcome::Reply(None);
        }
        self.lock()
            .sessions
            .insert(sid.clone(), Status::ApprovalRequired);

        // Without control, LaunchDeck only watches: the harness asks its own
        // user, right now. The pending approval clears itself when the session
        // moves on.
        let controlled = host.agent_access(d.agent_id).ok() == Some(AccessLevel::Control);
        if !controlled {
            return Outcome::Reply(None);
        }

        let (tx, rx) = oneshot::channel();
        {
            let mut inner = self.lock();
            // A newer request for the same session supersedes the older one.
            inner.waiters.retain(|_, w| w.session != sid);
            inner.waiters.insert(
                approval_id.clone(),
                Waiter {
                    session: sid.clone(),
                    tx,
                },
            );
        }
        let me = self.clone();
        Outcome::Hold {
            rx,
            timeout: hold,
            abandoned: Box::new(move || me.hand_back(&sid, &approval_id)),
        }
    }

    /// The harness will ask its own user: stop showing an approval nobody here
    /// can answer.
    fn hand_back(&self, sid: &str, approval_id: &str) {
        let Some(host) = self.host.get() else { return };
        let was_waiting = self.lock().waiters.remove(approval_id).is_some();
        if was_waiting {
            self.report(
                host,
                SessionUpdate::new(sid, Status::Waiting).detail("Waiting at the harness"),
            );
        }
    }
}

struct HookRoute(Arc<HookAdapter>);

impl Route for HookRoute {
    fn handle(&self, payload: Value) -> Outcome {
        self.0.handle(payload)
    }
}

impl AgentAdapter for HookAdapter {
    fn info(&self) -> AdapterInfo {
        AdapterInfo {
            id: self.dialect.adapter_id.into(),
            name: self.dialect.adapter_name.into(),
            kind: AdapterKind::Hooks,
            description: self.dialect.description.into(),
            limits: self.dialect.limits.iter().map(|s| s.to_string()).collect(),
        }
    }

    fn attach(&self, host: AdapterHost) {
        host.set_state(
            AdapterState::Disabled,
            Some("Turn on the local hook endpoint in Agents > Adapters."),
        );
        let _ = self.host.set(host);
    }

    fn execute(&self, c: AdapterCommand) -> Result<(), CommandError> {
        let approve = match c.command {
            Command::Approve => true,
            Command::Reject => false,
            _ => {
                return Err(CommandError::new(
                    "Hooks can only answer permission requests.",
                ))
            }
        };
        let host = self
            .host
            .get()
            .ok_or_else(|| CommandError::new("The adapter is not attached."))?
            .clone();
        let approval_id = c
            .approval_id
            .ok_or_else(|| CommandError::new("There is no request to answer."))?;
        let waiter = self.lock().waiters.remove(&approval_id);
        let Some(waiter) = waiter else {
            return Err(CommandError::new(
                "That request was already handed back; answer it in the harness.",
            ));
        };
        if waiter.tx.send(Some(decision_body(approve))).is_err() {
            return Err(CommandError::new(
                "The harness stopped waiting; answer it there.",
            ));
        }
        let _ = host.command_result(&c.request_id, Ok(()));
        // The harness continues as soon as it reads the decision.
        self.report(&host, SessionUpdate::new(&waiter.session, Status::Running));
        Ok(())
    }

    fn access_changed(&self, _agent_id: &str, access: AccessLevel) {
        if access == AccessLevel::Control {
            return;
        }
        // Control was withdrawn: give every held request back to the harness.
        let handed: Vec<Waiter> = self.lock().waiters.drain().map(|(_, w)| w).collect();
        if let Some(host) = self.host.get() {
            for w in handed {
                self.report(
                    host,
                    SessionUpdate::new(&w.session, Status::Waiting)
                        .detail("Waiting at the harness"),
                );
                drop(w.tx);
            }
        }
    }
}
