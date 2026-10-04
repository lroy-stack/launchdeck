//! `DemoAgentAdapter`: four simulated agents that exercise the whole model.
//!
//! No real agent runs here. Each simulated session follows a script on the
//! Tokio clock, reports through the same [`AdapterHost`] a real adapter uses,
//! and obeys the commands the human sends (approve, reject, pause, resume,
//! cancel, retry, instruct, spawn). Because it only uses timers, tests drive
//! the entire workflow deterministically with a paused clock.
//!
//! | agent  | task                      | shows                                              |
//! | ------ | ------------------------- | -------------------------------------------------- |
//! | Atlas  | Refactor settings module  | tool use, **approval**, resume, success; reject path |
//! | Beacon | Summarize the changelog   | plain success, pause / resume / cancel             |
//! | Cinder | Migrate the test fixtures | **failure**, retry, success (no pause: degrades)   |
//! | Delta  | Index the repository      | long run with progress, pause / resume / spawn     |

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender};
use tokio::task::JoinHandle;
use tokio::time::sleep;

use super::adapter::*;
use super::model::*;

pub const ADAPTER_ID: &str = "demo";

type Rx = UnboundedReceiver<(RequestId, Command)>;
type Tx = UnboundedSender<(RequestId, Command)>;

#[derive(Clone)]
enum Step {
    Work {
        status: Status,
        detail: &'static str,
        secs: f32,
    },
    Progress {
        status: Status,
        detail: &'static str,
        secs: f32,
        from: f32,
        to: f32,
    },
    Ask {
        tool: &'static str,
        message: &'static str,
    },
    Fail {
        error: &'static str,
    },
}

use Step::*;

struct DemoAgent {
    id: &'static str,
    name: &'static str,
    task: &'static str,
    capabilities: &'static [Capability],
    /// Delay before the session starts, so that the four agents do not move in lockstep.
    stagger_ms: u64,
    script: fn(attempt: u32) -> Vec<Step>,
}

fn atlas(_: u32) -> Vec<Step> {
    vec![
        Work {
            status: Status::Starting,
            detail: "Preparing",
            secs: 1.0,
        },
        Work {
            status: Status::Running,
            detail: "Reading the code",
            secs: 2.0,
        },
        Work {
            status: Status::ToolUse,
            detail: "Edit",
            secs: 1.5,
        },
        Ask {
            tool: "Bash",
            message: "Apply a schema migration to the local database",
        },
        Work {
            status: Status::Running,
            detail: "Applying the migration",
            secs: 2.0,
        },
        Work {
            status: Status::ToolUse,
            detail: "Test",
            secs: 1.5,
        },
    ]
}

fn beacon(_: u32) -> Vec<Step> {
    vec![
        Work {
            status: Status::Starting,
            detail: "Preparing",
            secs: 0.5,
        },
        Work {
            status: Status::Running,
            detail: "Reading commits",
            secs: 2.5,
        },
        Work {
            status: Status::ToolUse,
            detail: "Read",
            secs: 1.5,
        },
        Work {
            status: Status::Running,
            detail: "Writing the summary",
            secs: 2.0,
        },
    ]
}

fn cinder(attempt: u32) -> Vec<Step> {
    let mut steps = vec![
        Work {
            status: Status::Starting,
            detail: "Preparing",
            secs: 0.8,
        },
        Work {
            status: Status::Running,
            detail: "Converting fixtures",
            secs: 2.0,
        },
    ];
    if attempt <= 1 {
        steps.push(Work {
            status: Status::ToolUse,
            detail: "Test",
            secs: 2.0,
        });
        steps.push(Fail {
            error: "3 of 42 tests failed",
        });
    } else {
        steps.push(Work {
            status: Status::ToolUse,
            detail: "Test",
            secs: 1.5,
        });
    }
    steps
}

fn delta(_: u32) -> Vec<Step> {
    vec![
        Work {
            status: Status::Starting,
            detail: "Preparing",
            secs: 0.5,
        },
        Progress {
            status: Status::Running,
            detail: "Scanning files",
            secs: 10.0,
            from: 0.0,
            to: 0.6,
        },
        Progress {
            status: Status::ToolUse,
            detail: "Embed",
            secs: 6.0,
            from: 0.6,
            to: 1.0,
        },
    ]
}

fn spawned(_: u32) -> Vec<Step> {
    vec![
        Work {
            status: Status::Starting,
            detail: "Preparing",
            secs: 0.8,
        },
        Work {
            status: Status::Running,
            detail: "Working",
            secs: 2.0,
        },
        Work {
            status: Status::ToolUse,
            detail: "Run",
            secs: 1.5,
        },
        Work {
            status: Status::Running,
            detail: "Wrapping up",
            secs: 1.0,
        },
    ]
}

const ALL_CONTROL: &[Capability] = &[
    Capability::Spawn,
    Capability::SendInstruction,
    Capability::Pause,
    Capability::Resume,
    Capability::Cancel,
    Capability::Retry,
    Capability::Approve,
    Capability::Reject,
    Capability::GetStatus,
];

const AGENTS: &[DemoAgent] = &[
    DemoAgent {
        id: "demo-atlas",
        name: "Atlas",
        task: "Refactor the settings module",
        capabilities: ALL_CONTROL,
        stagger_ms: 0,
        script: atlas,
    },
    DemoAgent {
        id: "demo-beacon",
        name: "Beacon",
        task: "Summarize the changelog",
        capabilities: &[
            Capability::Pause,
            Capability::Resume,
            Capability::Cancel,
            Capability::GetStatus,
        ],
        stagger_ms: 400,
        script: beacon,
    },
    DemoAgent {
        id: "demo-cinder",
        name: "Cinder",
        task: "Migrate the test fixtures",
        capabilities: &[
            Capability::Retry,
            Capability::Cancel,
            Capability::Approve,
            Capability::Reject,
            Capability::GetStatus,
        ],
        stagger_ms: 800,
        script: cinder,
    },
    DemoAgent {
        id: "demo-delta",
        name: "Delta",
        task: "Index the repository",
        capabilities: &[
            Capability::Spawn,
            Capability::Pause,
            Capability::Resume,
            Capability::Cancel,
            Capability::GetStatus,
        ],
        stagger_ms: 1200,
        script: delta,
    },
];

fn agent_def(id: &str) -> Option<&'static DemoAgent> {
    AGENTS.iter().find(|a| a.id == id)
}

// ---------------------------------------------------------------------------
// Adapter
// ---------------------------------------------------------------------------

struct Live {
    tx: Tx,
    task: JoinHandle<()>,
}

struct Inner {
    host: Mutex<Option<AdapterHost>>,
    sessions: Mutex<HashMap<SessionId, Live>>,
    handle: tokio::runtime::Handle,
    pace: f32,
    counter: AtomicU64,
}

pub struct DemoAgentAdapter {
    inner: Arc<Inner>,
}

impl DemoAgentAdapter {
    /// Must be created inside a Tokio runtime. `pace` scales every delay
    /// (1.0 = the documented timings).
    pub fn new(pace: f32) -> Arc<Self> {
        Arc::new(Self {
            inner: Arc::new(Inner {
                host: Mutex::new(None),
                sessions: Mutex::new(HashMap::new()),
                handle: tokio::runtime::Handle::current(),
                pace,
                counter: AtomicU64::new(1),
            }),
        })
    }

    fn host(&self) -> Option<AdapterHost> {
        self.inner
            .host
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone()
    }

    /// Start the whole scenario: four agents, four sessions, staggered.
    /// Restarts from a clean slate if a previous run is still around.
    pub fn run_demo(&self) -> Result<(), String> {
        let host = self.host().ok_or("the demo adapter is not attached")?;
        self.reset_demo();
        let run = self.inner.counter.fetch_add(1, Ordering::Relaxed);
        for def in AGENTS {
            let session = format!("{}-run{run}", def.id);
            // Visible at once, as a queued session.
            let _ = host.update_session(
                def.id,
                SessionUpdate::new(&session, Status::Idle).task(def.task),
            );
            self.start(
                def,
                session,
                def.task.to_string(),
                def.stagger_ms,
                (def.script)(1),
            );
        }
        Ok(())
    }

    /// Stop every scripted session and remove the demo sessions.
    pub fn reset_demo(&self) {
        let live: Vec<(SessionId, Live)> = self
            .inner
            .sessions
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .drain()
            .collect();
        let host = self.host();
        for (id, l) in live {
            l.task.abort();
            if let Some(h) = &host {
                let _ = h.end_session(&id);
            }
        }
    }

    fn start(
        &self,
        def: &'static DemoAgent,
        session: SessionId,
        task: String,
        delay_ms: u64,
        steps: Vec<Step>,
    ) {
        let Some(host) = self.host() else { return };
        let (tx, rx) = unbounded_channel();
        let ctx = Ctx {
            host,
            agent: def,
            session: session.clone(),
            task,
            pace: self.inner.pace,
        };
        let handle = self
            .inner
            .handle
            .spawn(session_task(ctx, rx, steps, delay_ms));
        self.inner
            .sessions
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .insert(session, Live { tx, task: handle });
    }
}

impl AgentAdapter for DemoAgentAdapter {
    fn info(&self) -> AdapterInfo {
        AdapterInfo {
            id: ADAPTER_ID.into(),
            name: "Demo agents".into(),
            kind: AdapterKind::Simulated,
            description: "Four simulated agents that exercise the full workflow: tool use, \
                          approval, pause and resume, failure and retry, success."
                .into(),
            limits: vec![
                "Simulated. No real agent runs and nothing leaves this computer.".into(),
                "Capabilities differ per agent on purpose, to show graceful degradation.".into(),
            ],
        }
    }

    fn attach(&self, host: AdapterHost) {
        for def in AGENTS {
            let _ = host.register_agent(AgentSpec {
                id: def.id.into(),
                name: def.name.into(),
                provider: Some("demo".into()),
                harness: Some("simulated".into()),
                capabilities: def.capabilities.iter().copied().collect(),
            });
        }
        *self.inner.host.lock().unwrap_or_else(|p| p.into_inner()) = Some(host);
    }

    fn execute(&self, command: AdapterCommand) -> Result<(), CommandError> {
        let host = self
            .host()
            .ok_or_else(|| CommandError::new("the demo adapter is not attached"))?;
        match command.command {
            Command::Spawn { task, .. } => {
                let def = agent_def(&command.agent_id)
                    .ok_or_else(|| CommandError::new("unknown demo agent"))?;
                let n = self.inner.counter.fetch_add(1, Ordering::Relaxed);
                let session = format!("{}-t{n}", def.id);
                let task = summarize(&task, MAX_TASK_LEN);
                let _ = host.update_session(
                    def.id,
                    SessionUpdate::new(&session, Status::Idle).task(&task),
                );
                self.start(def, session, task, 0, spawned(1));
                let _ = host.command_result(&command.request_id, Ok(()));
                Ok(())
            }
            other => {
                let sid = command
                    .session_id
                    .ok_or_else(|| CommandError::new("no session"))?;
                let sessions = self
                    .inner
                    .sessions
                    .lock()
                    .unwrap_or_else(|p| p.into_inner());
                let live = sessions
                    .get(&sid)
                    .ok_or_else(|| CommandError::new("that demo session is not running"))?;
                live.tx
                    .send((command.request_id, other))
                    .map_err(|_| CommandError::new("that demo session has ended"))
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Scripted session
// ---------------------------------------------------------------------------

#[derive(Clone)]
struct Ctx {
    host: AdapterHost,
    agent: &'static DemoAgent,
    session: SessionId,
    task: String,
    pace: f32,
}

impl Ctx {
    fn report(&self, update: SessionUpdate) {
        let _ = self.host.update_session(self.agent.id, update);
    }

    fn status(&self, status: Status, detail: Option<&str>, progress: Option<f32>) {
        let mut u = SessionUpdate::new(&self.session, status).task(&self.task);
        if let Some(d) = detail {
            u = u.detail(d);
        }
        if let Some(p) = progress {
            u = u.progress(p);
        }
        self.report(u);
    }

    fn ok(&self, request: &str) {
        let _ = self.host.command_result(request, Ok(()));
    }

    fn fail(&self, request: &str, why: &str) {
        let _ = self.host.command_result(request, Err(why.to_string()));
    }

    fn supports(&self, c: Capability) -> bool {
        self.agent.capabilities.contains(&c)
    }

    fn dur(&self, secs: f32) -> Duration {
        Duration::from_secs_f32((secs * self.pace).max(0.0))
    }
}

#[derive(Debug, PartialEq, Eq)]
enum End {
    Completed,
    Failed,
    Cancelled,
    /// Rejected approval: waiting for new instructions.
    Waiting,
    /// The command channel closed (reset).
    Gone,
}

async fn session_task(ctx: Ctx, mut rx: Rx, first: Vec<Step>, delay_ms: u64) {
    if delay_ms > 0 {
        sleep(ctx.dur(delay_ms as f32 / 1000.0)).await;
    }
    let mut attempt = 1;
    let mut steps = first;
    loop {
        match play(&ctx, &mut rx, &steps).await {
            End::Gone => return,
            End::Completed => {
                // Idle until someone asks for a follow-up.
                if !wait_for(&ctx, &mut rx, |c| {
                    matches!(c, Command::SendInstruction { .. })
                })
                .await
                {
                    return;
                }
                steps = follow_up();
            }
            End::Failed | End::Cancelled => {
                if !wait_for(&ctx, &mut rx, |c| matches!(c, Command::Retry)).await {
                    return;
                }
                attempt += 1;
                steps = (ctx.agent.script)(attempt);
            }
            End::Waiting => {
                if !wait_for(&ctx, &mut rx, |c| {
                    matches!(c, Command::SendInstruction { .. })
                })
                .await
                {
                    return;
                }
                steps = follow_up();
            }
        }
    }
}

fn follow_up() -> Vec<Step> {
    vec![
        Work {
            status: Status::Running,
            detail: "Following the new instruction",
            secs: 2.0,
        },
        Work {
            status: Status::ToolUse,
            detail: "Test",
            secs: 1.0,
        },
    ]
}

/// Wait for a command of the expected kind, refusing everything else.
/// Returns `false` when the channel closed.
async fn wait_for(ctx: &Ctx, rx: &mut Rx, expected: impl Fn(&Command) -> bool) -> bool {
    while let Some((request, command)) = rx.recv().await {
        if expected(&command) {
            ctx.ok(&request);
            return true;
        }
        ctx.fail(&request, "not possible right now");
    }
    false
}

async fn play(ctx: &Ctx, rx: &mut Rx, steps: &[Step]) -> End {
    for step in steps {
        match step {
            Work {
                status,
                detail,
                secs,
            } => {
                ctx.status(*status, Some(detail), None);
                let restore = || ctx.status(*status, Some(detail), None);
                if dwell(ctx, rx, *secs, &restore).await.is_err() {
                    ctx.status(Status::Cancelled, None, None);
                    return End::Cancelled;
                }
            }
            Progress {
                status,
                detail,
                secs,
                from,
                to,
            } => {
                const TICKS: u32 = 10;
                for i in 0..=TICKS {
                    let p = from + (to - from) * (i as f32 / TICKS as f32);
                    ctx.status(*status, Some(detail), Some(p));
                    if i == TICKS {
                        break;
                    }
                    let restore = || ctx.status(*status, Some(detail), Some(p));
                    if dwell(ctx, rx, secs / TICKS as f32, &restore).await.is_err() {
                        ctx.status(Status::Cancelled, None, None);
                        return End::Cancelled;
                    }
                }
            }
            Ask { tool, message } => match approval(ctx, rx, tool, message).await {
                Decision::Approved => {}
                Decision::Rejected => {
                    ctx.status(
                        Status::Waiting,
                        Some("Skipped; waiting for new instructions"),
                        None,
                    );
                    return End::Waiting;
                }
                Decision::Cancelled => {
                    ctx.status(Status::Cancelled, None, None);
                    return End::Cancelled;
                }
                Decision::Gone => return End::Gone,
            },
            Fail { error } => {
                ctx.report(
                    SessionUpdate::new(&ctx.session, Status::Failed)
                        .task(&ctx.task)
                        .error(error),
                );
                return End::Failed;
            }
        }
    }
    ctx.status(Status::Completed, None, Some(1.0));
    End::Completed
}

struct Cancelled;

/// Let `secs` pass while obeying pause, cancel and instructions.
async fn dwell(
    ctx: &Ctx,
    rx: &mut Rx,
    secs: f32,
    restore: &(dyn Fn() + Sync),
) -> Result<(), Cancelled> {
    let mut remaining = ctx.dur(secs);
    loop {
        let started = tokio::time::Instant::now();
        tokio::select! {
            _ = sleep(remaining) => return Ok(()),
            msg = rx.recv() => {
                let Some((request, command)) = msg else { return Err(Cancelled) };
                match command {
                    Command::Pause if ctx.supports(Capability::Pause) => {
                        remaining = remaining.saturating_sub(started.elapsed());
                        ctx.status(Status::Paused, Some("Paused by you"), None);
                        ctx.ok(&request);
                        // Frozen until resumed or cancelled.
                        loop {
                            let Some((r, c)) = rx.recv().await else { return Err(Cancelled) };
                            match c {
                                Command::Resume if ctx.supports(Capability::Resume) => {
                                    ctx.ok(&r);
                                    restore();
                                    break;
                                }
                                Command::Cancel if ctx.supports(Capability::Cancel) => {
                                    ctx.ok(&r);
                                    return Err(Cancelled);
                                }
                                _ => ctx.fail(&r, "the session is paused"),
                            }
                        }
                    }
                    Command::Cancel if ctx.supports(Capability::Cancel) => {
                        ctx.ok(&request);
                        return Err(Cancelled);
                    }
                    Command::SendInstruction { .. } => {
                        remaining = remaining.saturating_sub(started.elapsed());
                        ctx.ok(&request);
                    }
                    _ => {
                        remaining = remaining.saturating_sub(started.elapsed());
                        ctx.fail(&request, "not possible right now");
                    }
                }
            }
        }
    }
}

enum Decision {
    Approved,
    Rejected,
    Cancelled,
    Gone,
}

async fn approval(ctx: &Ctx, rx: &mut Rx, tool: &str, message: &str) -> Decision {
    let approval_id = format!("{}-approval", ctx.session);
    let spec = ApprovalSpec {
        id: Some(approval_id.clone()),
        message: message.to_string(),
        tool: Some(tool.to_string()),
    };
    if ctx
        .host
        .request_approval(ctx.agent.id, &ctx.session, spec)
        .is_err()
    {
        return Decision::Gone;
    }
    while let Some((request, command)) = rx.recv().await {
        match command {
            Command::Approve => {
                ctx.ok(&request);
                return Decision::Approved;
            }
            Command::Reject => {
                ctx.ok(&request);
                return Decision::Rejected;
            }
            Command::Cancel if ctx.supports(Capability::Cancel) => {
                ctx.ok(&request);
                let _ = ctx.host.withdraw_approval(&ctx.session, &approval_id);
                return Decision::Cancelled;
            }
            _ => ctx.fail(&request, "waiting for your decision"),
        }
    }
    Decision::Gone
}
