# Agent Control: plan

Status: written before implementation, 2026-10-04. Agent Control is the first flagship vertical on top of the generic LaunchDeck runtime. LaunchDeck remains a general programmable physical-interface platform; Agent Control is a module above it, not a rewrite of it.

Positioning: a physical human control surface for autonomous and semi-autonomous agents. Three capabilities: **observe** (ambient state at a glance), **intervene** (approve, reject, pause, resume, retry, cancel), **orchestrate** (start a task on an agent).

## 1. Existing architecture assessment

What the v0.1 runtime already gives us, and what it lacks for this work.

| Area | State today | Verdict for Agent Control |
| --- | --- | --- |
| Event bus | Tokio broadcast of `Event { source, type, payload, ts }`; the runtime routes every event through bindings and to the UI. | Reuse as is. Agent events are ordinary events with `source: "agent-control"`. |
| Bindings | A control maps `(source, event, when[field = value])` to a named state, including `$payload.field`. | Reuse. Surface projection is expressed purely as bindings. No Launchpad-specific code. |
| Renderer | Resolves the visual for a control state and adapts it to device capabilities (pulse/flash synthesis, luminance, dropped positions). | Reuse. This is what makes the palette "respect device capabilities". |
| Plugins | `Plugin` trait with `invoke` and `publish_state`; plugins never see positions. | Reuse for the *action* surface of Agent Control (`agent-control.approve`, ...). The plugin context needs one addition: publish an arbitrary semantic event. |
| External WebSocket | Localhost only, events only, origin check, size and connection caps, reserved sources. | Extend with a versioned Agent Bridge. Must stay single-directional for generic `event` messages and add an explicit, authenticated-by-registration agent channel. |
| Persistence | Versioned `workspace.json`, atomic write, backup of damaged files. | Reuse the same approach for `agents.json` (settings, access grants) and an append-only `audit.jsonl`. Workspace gets a schema bump (v2) to add the new surface. |
| Surface model | Controls with spans, per-state visuals, press / long press, bindings. | Sufficient. No change. |
| Virtual device | Same trait as hardware; UI drives it. | Reuse. Acceptance demos run on it; physical hardware gets the same frames. |
| UI state | Rust owns runtime state; Zustand holds UI state only. | Agent state is runtime state: it lives in Rust and is mirrored to the UI by events. |

Architecture debt that matters here:

1. **v0.1 "AI Workspace" hardcodes `claude`, `codex`, `chatgpt`** in the surface, in the Demo plugin and in the UI. That violates the provider-neutral rule. The surface is replaced by Agent Control. The Demo plugin stays as a generic example plugin (it has no surface any more).
2. **`idle` is special-cased in the binding router** (state "idle" removes the entry). That is a hidden convention that collides with a normalized `idle` agent status. It is removed: a state is just a name, and a control without a visual for that name shows its base look.
3. **The WebSocket loop is request/reply only.** The bridge needs server-initiated messages (commands back to the agent), so each connection gets an outbound channel.
4. **`PluginContext` can only publish `plugin.state.changed`.** Add a general `publish`.
5. **Sources `runtime` and `device` are reserved; `agent-control` is not.** It must be, otherwise any local client could forge agent events through the generic `event` message.

None of these require redesigning the core.

## 2. Reuse strategy

* New Rust module `src-tauri/src/agents/` above the runtime. It depends on the event bus, the plugin trait, persistence helpers and the UI emitter. The runtime depends on it only to construct it, route `agents-changed` to the UI and call `republish()` after a workspace reset.
* Surfaces never talk to agents directly. `AgentControl` publishes **projection events** (`slot.changed`, `focus.changed`, `command.state`); surface controls bind to them. Pads call `Action::Plugin { plugin: "agent-control", ... }`.
* One state model, three views: the UI, the virtual device and the physical device are all derived from the same `AgentControl` state through the same events, bindings and renderer. There is no second copy.
* Raw MIDI stays in `devices/launchpad_x.rs`. If Agent Control needs a MIDI change, the abstraction is wrong.

## 3. Domain additions (Rust, mirrored with Zod)

Collections keyed by stable ids; no `currentAgent` singleton.

```
agents:   BTreeMap<AgentId, Agent>
sessions: BTreeMap<SessionId, Session>
approvals live on their session (one pending approval per session) and are kept in the audit trail when resolved
slots:    [Option<Track>; 8]          -- which agent/session each physical slot shows
focus:    Option<slot>                -- a *selection* (UI cursor), not the model
```

* **`Agent`**: `id`, `name`, `provider?`, `harness?`, `adapterId`, `capabilities: set<Capability>`, `access: monitor | control`, `online`, `lastSeen`. Its status is derived from its sessions (priority: approval_required, failed, running, ...), or `idle` / `offline` when it has none.
* **`Session`**: `id`, `agentId`, `task?` (truncated, never persisted), `status`, `startedAt`, `updatedAt`, `pendingApproval?`, `progress?`, `detail?` (current tool), `error?` (concise), `lastCommand?` (outcome for UI feedback).
* **`Status`** (normalized): `idle starting running tool_use waiting approval_required paused completed failed cancelled offline`. Adapters translate; provider strings never reach the UI.
* **`Capability`**: `spawn send_instruction pause resume cancel retry approve reject get_status get_logs get_diff open_session`. Explicit set per agent.
* **`Command`**: `approve reject pause resume cancel retry send_instruction spawn`. `availability(agent, session, command)` is one pure function returning `Available` or `Unavailable(reason)` (reasons: unsupported, monitor only, wrong state, nothing pending, offline). The pads, the UI buttons and the dispatcher all call it.
* **Command lifecycle** shown to the user: unavailable, available, attention (an approval is waiting), pending (sent, no result yet), succeeded, failed.
* **Central visuals** (`agents/visual.rs`): `status_visual(Status) -> Visual` and `command_visual(base, CommandPadState) -> Visual`. The surface builder, the snapshot palette sent to the UI and the tests use these functions only.

Event vocabulary (all on the existing bus, `source: "agent-control"`): `agent.registered`, `agent.online`, `agent.offline`, `session.created`, `session.started`, `session.<status>` for every status, `approval.created`, `approval.resolved`, plus projection events `slot.changed`, `focus.changed`, `command.state`.

## 4. Adapter architecture

```
              AgentControl (registry, sessions, approvals, slots, focus,
                            dispatch, audit, notification policy)
                     ^   events up                 | commands down
                     |   (AdapterHost)             v   (AgentAdapter::execute)
   +-----------------+-----------+-----------+-----------------+
   | DemoAgentAdapter | HookAdapter x2        | BridgeAdapter    |
   | (simulated)      | claude-code, codex    | (generic, WS)    |
   +------------------+-----------------------+------------------+
```

* `AgentAdapter` (trait): `info()`, `attach(AdapterHost)`, `execute(AdapterCommand) -> Result<(), CommandError>`. Adapters never see bus, devices, positions or LEDs; they only get an `AdapterHost` whose methods are `register_agent`, `set_online`, `update_session`, `request_approval`, `withdraw_approval`, `end_session`, `command_result`, `diagnostic`.
* An adapter can only touch agents it registered itself (ownership is enforced by the host).
* One SDK paradigm: the **Rust trait is the model**, and the **Agent Bridge is its wire form**. Anything outside the process implements the bridge protocol (which maps one to one onto `AdapterHost` and `AgentAdapter`). `defineAgentAdapter(...)` in the docs is sugar over the bridge, not a second runtime.
* MCP is an optional transport in front of the bridge, not an abstraction. Documented only.

### Adapters implemented

1. **DemoAgentAdapter** (mandatory). Four simulated agents with scripted sessions covering starting, running, tool use, approval, pause, cancel, failure, retry and success. Deterministic under a paused Tokio clock, so CI tests the real behaviour.
2. **Claude Code adapter** (real). Based on official hooks. Observes sessions and tool use, and approves or rejects permission requests by answering the `PermissionRequest` hook.
3. **Codex adapter** (real, observation plus approvals as far as the docs support). Same hook machinery with a Codex-specific translator.
4. **Generic bridge adapter**: any local harness over WebSocket.

## 5. What can really be observed and controlled (researched 2026-10-04)

Sources: Claude Code hooks reference (code.claude.com/docs/en/hooks), Codex hooks and app-server docs (developers.openai.com/codex, served from learn.chatgpt.com), Codex `notify` docs. Claude Code behaviour was additionally **verified against the installed CLI (2.1.289)** by running it with HTTP hooks pointed at a local capture server. Captured payloads are test fixtures in `src-tauri/tests/fixtures/claude-code/`.

### Claude Code (verified)

| Need | Mechanism | Verified |
| --- | --- | --- |
| Session activity | Hooks `UserPromptSubmit`, `PreToolUse`, `PostToolUse`, `Stop`, `StopFailure`, `SessionEnd`, `Notification` (HTTP or command). Common fields `session_id`, `cwd`, `hook_event_name`. | Yes. Order observed: UserPromptSubmit, PreToolUse, PermissionRequest, PostToolUse, Stop, SessionEnd. `SessionStart` did **not** arrive over HTTP in non-interactive mode, so sessions are created lazily from the first event. |
| Approve / reject a permission request | `PermissionRequest` hook answers `{"hookSpecificOutput":{"hookEventName":"PermissionRequest","decision":{"behavior":"allow"|"deny","message":"..."}}}`. A 2xx with an empty body means "no decision" and the normal dialog is used. Hooks may block up to their timeout (default 600 s). | Yes. An `allow` made the tool run, a `deny` made Claude report "denied by a hook" with our message. `permission_suggestions` is an array of objects, not strings. |
| Pause, resume, cancel, retry, send instruction | None through hooks. | n/a. Not supported; the agent declares only `approve`, `reject`. |
| Logs / diff | Transcript path is provided, but reading it would expose prompts. Not implemented. | n/a |

Failure semantic: if LaunchDeck is not running, or the hook times out, Claude Code falls back to its own prompt. **LaunchDeck never answers a permission request by itself.** On expiry or disconnect it returns an empty body.

### Codex (documented, not run here)

| Need | Mechanism | Verified |
| --- | --- | --- |
| Turn completion | `notify = ["program", ...]` receives one JSON argument with `type: "agent-turn-complete"`, `thread-id`, `turn-id`, `cwd`, `last-assistant-message`. That is the only supported notify event. | Docs only. |
| Lifecycle and approvals | Hooks from `hooks.json` or `config.toml`: `SessionStart`, `PreToolUse`, `PermissionRequest`, `PostToolUse`, `UserPromptSubmit`, `Stop`, `Interrupt`, `SessionEnd`. Handler types: `command` and `mcp_tool` **only** (no HTTP). `PermissionRequest` decisions use the same JSON as Claude Code (`allow` / `deny` with `message`); "any deny wins". | Docs only. Codex is not installed in this environment. Fields beyond the common ones (`session_id`, `transcript_path`, `cwd`, `hook_event_name`, `model`, `permission_mode`, `tool_name`, `tool_input`, `tool_use_id`) were not verifiable, so the translator reads only those and ignores the rest. |
| Spawn, steer, interrupt, structured approvals | `codex app-server` (JSON-RPC over stdio): `thread/start`, `turn/start`, `turn/steer`, `turn/interrupt`, server requests `item/commandExecution/requestApproval`, `item/fileChange/requestApproval`. Core methods stable, WebSocket transport experimental. | Docs only. **Not implemented**: it means LaunchDeck would own and spawn Codex processes, which is a different product decision than observing the user's own sessions. Documented as the path to full control (`spawn`, `cancel`, `send_instruction`). |

Because Codex hooks are `command` handlers, the integration is a one-line `curl` relay to the same local HTTP endpoint Claude Code uses (curl prints the response body, which is exactly the hook's stdout). No extra binary.

Honest limits: no pause or resume for either harness; no retry; no cancel. Those capabilities are therefore not declared and the UI and pads show them as unavailable ("not supported by this agent").

## 6. Agent Bridge protocol (v1, generic)

Local WebSocket on the existing `127.0.0.1` server. JSON text frames. Everything versioned with `protocol: 1`.

Client to LaunchDeck: `agent.register`, `agent.session.update`, `agent.approval.request`, `agent.approval.withdraw`, `agent.session.end`, `agent.command.result`, `agent.unregister`. LaunchDeck to client: `agent.registered`, `agent.command`, `agent.access`, `error`.

Rules: strict validation (identifier charset, lengths, known statuses and capabilities); the client may only touch agents it registered on this connection; a session id belongs to the agent that created it; registration of an id that is connected elsewhere is refused; closing the connection marks the agent offline. **Registered agents start as `monitor`.** Commands are sent only after the user grants `control` in the app (persisted per agent id). Full spec in `docs/agent-control/bridge-protocol.md`.

## 7. Surface design

Principles: glanceability, low cognitive load, fast intervention, clear state. 8x8 grid plus the usual top row for surface switching.

```
row 0  [slot 1][slot 2][slot 3][slot 4][slot 5][slot 6][slot 7][slot 8]   one track per pad (session, or an agent without sessions)
row 1  [ focus marker under the selected slot ............................]
row 2  [ APPROVE  2x2 ][ REJECT  2x2 ][Pause][Resume][Retry][Stop]
row 3  [          ...               ][New Task][Continue][Logs][  ]
row 4  [Run tests][Review diff][Summarize]  task templates for the focused agent
row 5-6  (free)
row 7  [Run Agent Demo 2x1][Reset demo][Agents (opens the app)]
```

* Slot pads show the normalized status colour. A slot with an approval pending flashes white. Press a slot to focus its track.
* Command pads act on the **focused** track and display one of six states (unavailable dim, available solid, attention flashing white, pending pulse, succeeded green, failed red flash). Focus never jumps while a track already has focus (a late approval for another agent will not hijack the APPROVE pad); with no focus, the first approval is focused automatically.
* Default palette: idle blue, starting yellow, running green pulse, tool_use cyan pulse, waiting purple, approval_required white flash, paused orange, completed green, failed red flash, cancelled dim red, offline off.

## 8. Security implications

* Control requires explicit user action: bridge registration is monitor-only until a grant; hook adapters are off until enabled and use a per-install bearer token.
* Remote input never executes anything: `event` messages still only change state; `agent.*` messages can only report state or request approval, never run actions. The sources `runtime`, `device` and `agent-control` are reserved.
* HTTP hook endpoint: `127.0.0.1` only, `Authorization: Bearer <token>` (constant-time compare), `Host` must be loopback, requests with an `Origin` header are refused, body size capped, connection count capped. Without that, a web page could POST to localhost.
* **No automatic approval**, ever: timeouts, disconnects and errors return "no decision".
* Privacy: prompts and tool inputs are held in memory for display only (truncated); the audit log stores kinds, ids and tool *names*, never prompt text or command lines.
* Audit log of interventions and lifecycle events in `audit.jsonl` (capped and rotated).
* Future: per-adapter permission isolation (documented in `docs/agent-control/security.md`).

## 9. Testing strategy

Domain logic first, no mocks of our own code.

* Status normalization and the alias table.
* Availability function (capability x state x access matrix).
* Registry: registration, ownership, session updates, multiple simultaneous sessions, slot assignment and eviction, focus rules.
* Approval lifecycle: created, resolved by approve / reject / withdraw / expiry; audit entries.
* Command dispatch: pending then succeeded/failed, timeouts, monitor-only refusal.
* Projection events and **surface bindings**: run the real runtime with the real Agent Control surface and assert the **virtual device frame** (colour and animation) after agent events.
* Demo adapter under a paused Tokio clock (all five acceptance flows).
* Bridge protocol: parse and validate every message type, ownership, size and charset limits, and a full WebSocket round trip including a command sent back.
* Hook translators against the **real captured Claude Code payloads** and an end-to-end HTTP test (hold, decide, release; deny; timeout returns empty).
* Frontend: Zod contract against a Rust-generated agents fixture, store selectors, command availability rendering.
* Example client executed against a running runtime.

## 10. Implementation phases

1. Plan and research (this file).
2. Domain model, visuals, availability, audit, settings, `AgentControl` core, plugin. Runtime wiring, binding fix, reserved source, workspace v2 migration. Unit tests.
3. Agent Control surface; projection; virtual-device tests.
4. `DemoAgentAdapter` and acceptance-flow tests.
5. Bridge protocol on the WebSocket; example client in `examples/generic-agent`.
6. Hook server and Claude Code / Codex adapters; real-payload tests; live check against Claude Code.
7. Tauri commands, Zod schemas, Agents UI (overview, sessions, adapters, surface), notifications.
8. Docs, README, final validation (TypeScript, lint, Rust fmt/clippy/tests, production build, run the app and the example client).

## 11. Out of scope

Cloud agent hosting, model inference, an LLM gateway, prompt marketplace, workflow DAGs, remote multi-user control, accounts, billing, sync, agent memory, a marketplace backend, complex scheduling. LaunchDeck consumes external agent systems; it does not become one.

## 12. Where the implementation differs from this plan

Written after implementation. The plan above is kept as it was written, so the decisions and the reasons stay readable. These are the places where building it changed the answer.

* **One hook manager, not two adapters.** `adapters/hooks.rs` has a `HookManager` that owns the single HTTP endpoint and the two translators (`claude_code.rs`, `codex.rs`). The endpoint, token, hold time and per-harness switches are settings, applied through a `SettingsPatch` so the UI can send partial updates. The per-harness map is replaced as a whole, and a missing key means enabled.
* **The vocabulary is bigger than the implementation.** `get_logs`, `get_diff` and `open_session` exist as capabilities, but no adapter implements them, so no surface offers a Logs pad (the layout in section 7 listed one). The surface in `docs/agent-control/surfaces.md` is the authoritative layout: tracks in row 0, selection markers in row 1, APPROVE and REJECT as 2x2 pads, Pause, Resume, Retry, Stop and Continue, task pads, and Run Agent Demo, Reset and Open Agents on the bottom row.
* **No `defineAgentAdapter` sugar.** Section 4 promised a helper over the bridge. It was not written. The example client in `examples/generic-agent/` is the reference for the wire form, and `src-tauri/examples/adapter_sdk.rs` for the Rust trait.
* **`PluginContext::publish` is used by one action only** (`openApp`, which asks the desktop app to show the Agents section). Everything else a surface needs comes from projection events published by `AgentControl`.
* **The Demo plugin stayed** as a generic example (`alpha`, `beta`, `gamma`) and no longer carries any vendor name or any automatic approval.
* **Additions that were not planned.**
  * `agents/pack.rs`: the Agent Pack manifest type, validated and tested, with `examples/agent-pack/pack.json`. Nothing loads it.
  * `src-tauri/examples/headless.rs`: a windowless runner, so the example client can be exercised without the desktop app. `dump_agents.rs` captures `examples/agents-snapshot.json`, the contract fixture the UI tests parse.
  * `persistence::write_private`: owner-only (0600) files on Unix for `agents.json`, because it holds the hook token.
  * Bridge `ack` replies, an optional echoed `id`, and per-connection rate limiting (burst 60, then 30 per second).
  * Audit kinds beyond the plan: `approval.withdrawn`, `approval.superseded`, `command.failed`, `command.refused`, `access.granted`, `access.revoked`.
  * A six-state command lifecycle that includes `attention` (available, and a decision is waiting for exactly this command).
* **The physical Launchpad X synthesises pulse and flash.** The plan's "native" wording for the renderer applied to devices that report native animation, which today is only the virtual device. Section 7's palette is unchanged, and the first thing to check on real hardware is how the synthesised animation looks.
* **MCP** is documented as a stdio translator in front of the Agent Bridge, with three tools, rather than as a transport inside the runtime. See `docs/agent-control/mcp.md`. It is not built.
* **Open from the plan, still open:** Codex has never been run against a real installation, `codex app-server` is not implemented, and nothing has run on a physical Launchpad X.
