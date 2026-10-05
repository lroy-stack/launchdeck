# Agent Adapter SDK

An adapter is the only code that knows about a particular agent system. Its job is small: report what an agent is doing in normalized terms, and carry out the commands a person issued. Everything else (colours, pads, MIDI, the event bus, the UI) is not its concern and not visible to it.

## One model, two forms

There is exactly one adapter model. It has two faces that carry the same operations.

| Form | Use it when | How |
| --- | --- | --- |
| **Rust trait** | The adapter runs inside LaunchDeck. | Implement `AgentAdapter` and call `AdapterHost`. |
| **Agent Bridge** | The agent runs in another process, in any language. | Send the matching JSON messages over the localhost WebSocket. See [bridge-protocol.md](bridge-protocol.md). |

The bridge is not a second SDK. The built-in `BridgeAdapter` is an ordinary `AgentAdapter` that forwards each `AdapterHost` call from a socket and each command back to it. A tool that wants to be controlled writes a small client, and a client is nothing more than an out-of-process adapter.

```
adapter  --- AdapterHost ----->  AgentControl      register_agent, set_online, update_session,
                                                   request_approval, withdraw_approval, end_session,
                                                   command_result, diagnostic
adapter  <--- AgentAdapter ----  AgentControl      execute(AdapterCommand)
```

| `AdapterHost` method | Bridge message | Meaning |
| --- | --- | --- |
| `register_agent` | `agent.register` | Announce an agent and its capabilities. |
| `update_session` | `agent.session.update` | Report a session's state. The first report creates it. |
| `request_approval` | `agent.approval.request` | Ask a human to decide. |
| `withdraw_approval` | `agent.approval.withdraw` | The question no longer applies. |
| `end_session` | `agent.session.end` | The session is closed. |
| `command_result` | `agent.command.result` | The outcome of a command. |
| `set_online(false)` | `agent.unregister`, or closing the connection | The agent is gone. |
| `agent_access` | the `access` field of `agent.registered`, then `agent.access` | Whether control is granted. |
| `AgentAdapter::execute` | `agent.command` | A command from the person. |

## The Rust trait

```rust
pub trait AgentAdapter: Send + Sync {
    fn info(&self) -> AdapterInfo;                       // id, name, kind, description, limits
    fn attach(&self, host: AdapterHost);                 // keep the host; register agents when they exist
    fn execute(&self, command: AdapterCommand) -> Result<(), CommandError>;
    fn access_changed(&self, _agent_id: &str, _access: AccessLevel) {}   // optional
}
```

`info().limits` is how an adapter is honest. It is a list of plain sentences about what the integration cannot see or do, shown on the Adapters tab. Write it for the person, not for yourself.

### A complete adapter

This runs: `cargo run --manifest-path src-tauri/Cargo.toml --example adapter_sdk`. It is the file `src-tauri/examples/adapter_sdk.rs`, trimmed.

```rust
struct StopwatchAdapter { host: OnceLock<AdapterHost> }

impl AgentAdapter for StopwatchAdapter {
    fn info(&self) -> AdapterInfo {
        AdapterInfo {
            id: "stopwatch".into(), name: "Stopwatch".into(), kind: AdapterKind::Bridge,
            description: "An example adapter that pretends to be an agent.".into(),
            limits: vec!["Can pause and resume. Cannot cancel, retry or approve.".into()],
        }
    }

    fn attach(&self, host: AdapterHost) {
        host.register_agent(AgentSpec {
            id: "stopwatch".into(), name: "Stopwatch".into(),
            provider: Some("example".into()), harness: Some("in-process".into()),
            // Declare exactly what you can honour. Everything else stays unavailable.
            capabilities: [Capability::Pause, Capability::Resume].into(),
        }).unwrap();
        host.update_session("stopwatch",
            SessionUpdate::new("stopwatch-1", Status::Running).task("Count laps")).unwrap();
        let _ = self.host.set(host);
    }

    fn execute(&self, command: AdapterCommand) -> Result<(), CommandError> {
        let status = match command.command {
            Command::Pause => Status::Paused,
            Command::Resume => Status::Running,
            _ => return Err(CommandError::new("not supported")),
        };
        let host = self.host.get().unwrap().clone();
        tokio::spawn(async move {                       // never block in execute
            let _ = host.update_session("stopwatch", SessionUpdate::new("stopwatch-1", status));
            let _ = host.command_result(&command.request_id, Ok(()));
        });
        Ok(())
    }
}
```

Its output shows the rules in action:

```
registered   -> running (access: Monitor)
pause before grant -> monitor only (control not granted)
after pause  -> paused
after resume -> running
session events: session.created, session.started, session.running, session.paused, session.running
```

## Rules an adapter lives by

**Report, do not decide.** An adapter says what is true. It never approves, rejects or answers on a person's behalf, and never turns a timeout into consent. If it cannot get an answer, it says nothing and lets the agent fall back to its own prompt.

**Own only what you registered.** `AdapterHost` refuses to touch an agent, session or approval that another adapter created (`NotOwner`, `SessionOwnedByOther`, `IdTaken`). An agent id has one live owner.

**Declare honestly.** Capabilities are a promise. `availability()` already refuses commands outside the set, but a declared capability the adapter cannot really perform produces a pad that lights up and then fails. Declare less, not more.

**Use the whole vocabulary or reject.** Statuses are the eleven normalized ones. Map your tool's states onto them with the alias table or your own mapping, and do not invent new ones. `approval_required` comes only from `request_approval`, and `offline` only from the connection.

**Keep summaries short and diagnostics raw.** `task`, `detail`, `error` and approval `message` are single lines meant for a person (limits in [domain-model.md](domain-model.md#limits)). Put stack traces and raw output in `host.diagnostic(...)`, which goes to the Adapters tab and the log, not onto cards.

**Do not block.** `execute` returns as soon as the command is accepted. Report the real outcome with `command_result`, immediately or later. An accepted command with no result fails after 15 seconds. An approval is resolved only when you confirm the command, never when the button was pressed.

**Respect access.** `host.agent_access(id)` tells you whether the person has granted control. The service refuses commands without it, so use this to avoid promising anything: for example, do not hold a request open for an answer that cannot come.

**Pick a kind that matches the trust.** `AdapterKind::Simulated` is for in-process harmless agents and starts with `control`. `Hooks` and `Bridge` start with `monitor`. A new adapter should almost never be `Simulated`.

**Know nothing else.** No bus, no devices, no positions, no colours. If you find yourself reaching for one, the thing you want belongs to a surface or the renderer.

## Real adapters: what is observable and controllable

Two harnesses have adapters. Both live in `src-tauri/src/agents/adapters/`, the only place a vendor is named, and both share a small local HTTP endpoint (loopback, bearer token, off by default). The facts below were researched on 2026-10-04 from the vendors' public documentation. Claude Code was additionally run live.

### Claude Code: verified

Checked against Claude Code 2.1.289 on 2026-10-04, with the real CLI sending HTTP hooks to a local capture server. The captured payloads are the test fixtures in `src-tauri/tests/fixtures/claude-code/`. Source: Claude Code hooks reference, <https://code.claude.com/docs/en/hooks>.

| Need | How | Result |
| --- | --- | --- |
| Observe sessions and tool use | Hooks `UserPromptSubmit`, `PreToolUse`, `PostToolUse`, `PostToolUseFailure`, `Stop`, `StopFailure`, `SessionEnd`. | Works. A prompt makes the session `running`, a tool call `tool_use` with the tool name, a finished turn `completed`, a turn that ends in an API error `failed`. |
| Approve or reject a permission request | The `PermissionRequest` hook is answered with `allow` or `deny`. | Works. An `allow` ran the tool. A `deny` made Claude report it was denied by a hook, with LaunchDeck's message. |
| Pause, resume, cancel, retry, send an instruction | Not possible through hooks. | Not supported. The agent declares only `approve` and `reject`; the other controls stay unavailable. |
| Logs and diff | A transcript path is provided, but reading it would expose prompts. | Not implemented. |

What to know:

* `SessionStart` did not arrive over HTTP in non-interactive (`-p`) mode, so sessions are created from the first event they send.
* A held request is answered only by a person. If the hold time passes, if Claude Code hangs up, or if control is revoked, the endpoint returns an empty 200, which Claude Code reads as "no decision" and shows its own prompt. Silence is never consent.
* A request that arrives while the agent is read-only is shown in LaunchDeck, but it is not held: Claude Code asks you right away in its own prompt. Grant control before work starts if you want to answer from the pads.
* The task text is the first line of the prompt, kept in memory for display. It is not logged, audited or put in a notification.
* Not verified: whether Claude Code's own terminal dialog appears at the same time as a held hook, in an interactive session.

### Codex: written from documentation, not run

Source: OpenAI's published Codex hook documentation, read on 2026-10-04. **No Codex installation was available, so this adapter has never been run against Codex and the Adapters tab says so.** Treat it as untested.

| Need | How | Status |
| --- | --- | --- |
| Observe sessions and tool use | Command hooks for `SessionStart`, `UserPromptSubmit`, `PreToolUse`, `PostToolUse`, `Stop`, `Interrupt`, `SessionEnd`. | Implemented from the documented fields; unknown fields are ignored. |
| Approve or reject | `PermissionRequest` hook with the same `allow`/`deny` body. | Implemented from the docs. |
| Pause, resume, retry | None in hooks. | Not supported. |
| Real control (start, interrupt, steer, structured approvals) | `codex app-server`, a JSON-RPC protocol over stdio. | **Not implemented.** It would make LaunchDeck own and launch Codex processes, which is a different decision from watching your own sessions. It is the path to `spawn`, `cancel` and `send_instruction`. |

Codex runs hooks as commands, not as HTTP calls, so the setup uses a one-line `curl` relay to the same local endpoint. The relay ends in `|| true`, so a hook that cannot reach LaunchDeck never fails a Codex turn.

### Connecting them

Turn on the **Hook endpoint** on the Adapters tab. It generates a token, shows ready-made snippets, and keeps everything on `127.0.0.1`. The token is passed through an environment variable (`LAUNCHDECK_HOOK_TOKEN`), so the settings files you edit never contain it. Then allow control for the agent on its card. Details and limits of the endpoint are in [security.md](security.md#the-hook-endpoint).

## Sharing adapters (not built)

There is no way to share an adapter as a file today. An adapter is shared as code: compiled into LaunchDeck, or a separate program that speaks the [Agent Bridge](bridge-protocol.md).

LaunchDeck's unit of sharing is the **Pack**, a portable JSON file of surfaces, their pads and feedback, and optional context rules; its format is specified in [PRODUCTIZATION_PLAN.md section 7](../../PRODUCTIZATION_PLAN.md#7-portable-artifacts-plan). Format version 1 carries no agent pieces. Adapters, state mappings (a provider's status words to normalized statuses) and workflow templates (named starting tasks) are candidates for a later Pack format version. Two rules from an earlier draft manifest, which was validated but never loaded, would carry over:

* A state mapping may not target `approval_required` or `offline`: an adapter cannot report those through a status update.
* A workflow template carries text, never a command.

## Testing an adapter

Test the behaviour, not the plumbing: build a real `AgentControl` with a `NullSink` and an `EventBus`, register your adapter, and assert on `ctl.session(...)`, `ctl.snapshot()` and the events on the bus, exactly as `adapter_sdk.rs` and the tests in `src-tauri/src/agents/tests.rs` do. For anything that talks to a real tool, capture real payloads and keep them as fixtures. The Claude Code translator is tested against payloads recorded from a live session, because a summary of what a tool sends is not the same as what it sends.
