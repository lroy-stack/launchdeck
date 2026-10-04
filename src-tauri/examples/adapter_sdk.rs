//! The smallest useful in-process adapter, run against the real Agent Control.
//!
//!     cargo run --manifest-path src-tauri/Cargo.toml --example adapter_sdk
//!
//! A "stopwatch" agent registers, reports a session, and obeys pause and
//! resume once the person grants control. The adapter knows nothing about pads,
//! colours or MIDI: it speaks only to `AdapterHost`. Documented in
//! docs/agent-control/adapter-sdk.md.

use std::sync::{Arc, OnceLock};
use std::time::Duration;

use launchdeck_lib::agents::adapter::*;
use launchdeck_lib::agents::model::*;
use launchdeck_lib::agents::service::{AgentControl, NullSink, Target, Timing};
use launchdeck_lib::events::EventBus;

const AGENT: &str = "stopwatch";
const SESSION: &str = "stopwatch-1";

#[derive(Default)]
struct StopwatchAdapter {
    host: OnceLock<AdapterHost>,
}

impl AgentAdapter for StopwatchAdapter {
    fn info(&self) -> AdapterInfo {
        AdapterInfo {
            id: "stopwatch".into(),
            name: "Stopwatch".into(),
            kind: AdapterKind::Bridge,
            description: "An example adapter that pretends to be an agent.".into(),
            // Say what this integration cannot do. The Adapters view shows it.
            limits: vec!["Can pause and resume. Cannot cancel, retry or approve.".into()],
        }
    }

    fn attach(&self, host: AdapterHost) {
        // Register the agent with exactly the capabilities it can honour.
        host.register_agent(AgentSpec {
            id: AGENT.into(),
            name: "Stopwatch".into(),
            provider: Some("example".into()),
            harness: Some("in-process".into()),
            capabilities: [Capability::Pause, Capability::Resume].into(),
        })
        .expect("register");
        host.update_session(
            AGENT,
            SessionUpdate::new(SESSION, Status::Running).task("Count laps"),
        )
        .expect("session");
        let _ = self.host.set(host);
    }

    fn execute(&self, command: AdapterCommand) -> Result<(), CommandError> {
        let status = match command.command {
            Command::Pause => Status::Paused,
            Command::Resume => Status::Running,
            // availability() already refuses what we did not declare; stay honest anyway.
            _ => return Err(CommandError::new("not supported")),
        };
        let host = self.host.get().expect("attached").clone();
        // Never block here. Do the work, report the new state, then the outcome.
        tokio::spawn(async move {
            let _ = host.update_session(AGENT, SessionUpdate::new(SESSION, status));
            let _ = host.command_result(&command.request_id, Ok(()));
        });
        Ok(())
    }
}

#[tokio::main]
async fn main() {
    let bus = EventBus::new();
    let mut events = bus.subscribe();
    let ctl = AgentControl::new(bus, Arc::new(NullSink), None, Timing::default());
    ctl.register_adapter(Arc::new(StopwatchAdapter::default()));

    let status = || {
        ctl.session(SESSION)
            .map(|s| s.status.as_str())
            .unwrap_or("-")
    };
    println!(
        "registered   -> {} (access: {:?})",
        status(),
        ctl.agent(AGENT).unwrap().access
    );

    // Registering never grants control: the command is refused until the person allows it.
    let refused = ctl.command(Target::Session(SESSION.into()), Command::Pause);
    println!("pause before grant -> {}", refused.unwrap_err());

    ctl.set_access(AGENT, AccessLevel::Control).unwrap();
    ctl.command(Target::Session(SESSION.into()), Command::Pause)
        .unwrap();
    tokio::time::sleep(Duration::from_millis(50)).await;
    println!("after pause  -> {}", status());

    ctl.command(Target::Session(SESSION.into()), Command::Resume)
        .unwrap();
    tokio::time::sleep(Duration::from_millis(50)).await;
    println!("after resume -> {}", status());

    // Everything above was published on the event bus for surfaces and the UI.
    let mut kinds = Vec::new();
    while let Ok(e) = events.try_recv() {
        if e.source == "agent-control" && e.kind.starts_with("session.") {
            kinds.push(e.kind);
        }
    }
    println!("session events: {}", kinds.join(", "));
}
