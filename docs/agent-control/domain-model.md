# Domain model

The Agent Control model lives in `src-tauri/src/agents/model.rs` and is mirrored for the UI with Zod in `src/lib/agents.ts`. It is deliberately small, and it names no vendor, no harness, no pad and no colour. The test suite enforces the last point: the core, the SDK, the bridge, the settings and the surface may not contain a provider's name. Only `adapters/` may.

## Entities

Everything is held in collections keyed by stable ids. There is no "current agent" singleton anywhere in the model, so any number of agents and sessions can be live at once.

```
agents:    map AgentId   -> Agent
sessions:  map SessionId -> Session        a session belongs to exactly one agent
slots:     8 x Option<Track>               which agent or session each pad shows
focus:     Option<slot index>              the selection the command pads act on
pending:   map RequestId -> command        commands handed to an adapter, awaiting a result
```

### Agent

| Field | Meaning |
| --- | --- |
| `id` | Stable identifier, chosen by the adapter or client. `[A-Za-z0-9_.:-]`, 1 to 64 characters. |
| `name` | Display name, up to 60 characters. |
| `provider`, `harness` | Optional free labels for the person ("who made it", "what runs it"). They are shown, never used for logic. |
| `adapterId` | The adapter that owns the agent. Only that adapter may change it. |
| `capabilities` | The explicit set of things the agent can do (below). |
| `access` | `monitor` or `control` (below). |
| `online`, `registeredAt`, `lastSeen` | Presence. |

An agent's status is not stored. It is derived from its sessions: `offline` if the agent is not online, otherwise the most urgent status among its sessions, or `idle` if it has none.

### Session

| Field | Meaning |
| --- | --- |
| `id`, `agentId` | Identity. A session id belongs to the agent that created it. |
| `status` | One of the eleven normalized statuses. |
| `task` | One line, up to 160 characters, for display. Kept in memory, never written to disk or to the audit log. |
| `startedAt`, `updatedAt` | Milliseconds since the epoch. |
| `pendingApproval` | At most one: `{ id, message, tool?, createdAt }`. |
| `progress`, `detail` | Optional fraction `0..1` and current step or tool name. |
| `error` | Concise failure summary (up to 200 characters). Raw output belongs in the adapter's diagnostics. |
| `lastCommand` | `{ command, state, error?, at }`, so a card can say what just happened. |

### Approval

An approval is a question an agent asks a human: `message` (one concise line, up to 300 characters) and an optional `tool` name. It lives on its session. It ends in one of four ways: approved, rejected, withdrawn by the agent, or replaced by a newer request. **Nothing ever ends it by assuming consent.** A timeout, a disconnect or an error leaves the agent to its own fallback, which for the real adapters is its own prompt.

## Status

Eleven values. Adapters translate whatever their tool calls its states into these, and nothing else ever reaches the UI or a surface.

| Status | Meaning |
| --- | --- |
| `idle` | Online, nothing running. |
| `starting` | A session is being set up. |
| `running` | Working. |
| `tool_use` | Working, currently using a tool. |
| `waiting` | Waiting for input that is not an approval. |
| `approval_required` | Blocked until a human decides. |
| `paused` | Paused. |
| `completed` | Finished successfully. |
| `failed` | Ended with an error. |
| `cancelled` | Stopped before finishing. |
| `offline` | The agent is not connected. |

`approval_required` is never set by a status update. It follows from requesting an approval, so a session cannot claim to need approval without an approval to decide. `offline` is a fact about the connection, not about a session, so a session update cannot say it either.

### Aliases

A small table lets simple clients say what they naturally say. Case, `-` and spaces are interchangeable with `_`. Anything outside the table is rejected with a message listing the valid names. It is never guessed.

| Normalized | Also accepted |
| --- | --- |
| `idle` | `ready` |
| `starting` | `pending`, `queued`, `initializing` |
| `running` | `working`, `thinking`, `in_progress`, `active` |
| `tool_use` | `tooluse`, `tool`, `using_tool` |
| `waiting` | `waiting_for_input`, `input_required`, `blocked` |
| `approval_required` | `awaiting_approval`, `approval`, `needs_approval` |
| `paused` | `suspended` |
| `completed` | `complete`, `done`, `success`, `succeeded`, `finished` |
| `failed` | `failure`, `error`, `errored` |
| `cancelled` | `canceled`, `stopped`, `aborted` |
| `offline` | `disconnected` |

### Urgency

When one pad or card has to stand for several sessions, the most urgent status wins: `approval_required` (100), `failed` (90), `waiting` (80), `running` (70), `tool_use` (65), `starting` (60), `paused` (50), `completed` (40), `cancelled` (30), `idle` (20), `offline` (0). The same order picks the slot to reuse when all eight are taken: only finished, idle or offline tracks are ever replaced, and only by something more urgent.

## Capabilities

Twelve, declared explicitly per agent: `spawn`, `send_instruction`, `pause`, `resume`, `cancel`, `retry`, `approve`, `reject`, `get_status`, `get_logs`, `get_diff`, `open_session`.

An agent only gets what it declares. Everything else degrades the same way everywhere: the pad is dim, the button is absent from the card, and a dispatch attempt is refused with the reason `unsupported`. The last four are monitoring capabilities; the first eight each map to a command.

## Access: monitor versus control

| Level | The agent may | LaunchDeck may |
| --- | --- | --- |
| `monitor` | Report state and ask for approvals. | Show everything. Send nothing. |
| `control` | The same. | Send the commands the agent declared. |

Agents from the built-in demo start as `control`, because they are simulated. **Every other agent starts as `monitor`.** Control is a deliberate, per-agent decision by the person, made in the Agents view, and it is remembered per agent id in `agents.json`. Registering an agent, declaring capabilities and being connected never grant it. Revoking control takes effect immediately and the adapter is told.

## Commands

Eight kinds: `approve`, `reject`, `pause`, `resume`, `cancel`, `retry`, `send_instruction` (with text), `spawn` (with a task and optionally a working directory). Each maps to the capability of the same name.

### One function decides

`availability(agent, session, command)` in `model.rs` is the single decision. The pads, the buttons in the app and the dispatcher all call it, so what you see is what will be accepted. It checks in this order:

| Order | Reason code | Meaning |
| --- | --- | --- |
| 1 | `unsupported` | The agent does not declare the capability. |
| 2 | `offline` | The agent is not connected. |
| 3 | `monitor_only` | Control has not been granted. |
| 4 | `no_session` | Nothing is selected (not needed for `spawn`). |
| 5 | `nothing_pending` | `approve` or `reject` with no approval waiting. |
| 6 | `wrong_state` | Supported, but meaningless now. |

The state rules: `approve` and `reject` need a pending approval; `pause` needs `running` or `tool_use`; `resume` needs `paused`; `cancel` needs any active status; `retry` needs `failed` or `cancelled`; `send_instruction` works while idle, running, using a tool, waiting, paused or completed; `spawn` is agent-level and needs only control.

### Lifecycle

A command pad and a card button are always in one of six states.

| State | Meaning | On a pad |
| --- | --- | --- |
| `unavailable` | Cannot be issued now (any reason above). | Very dim. |
| `available` | Can be issued. | Solid. |
| `attention` | Available, and something is waiting for exactly this decision. | Flashes. |
| `pending` | Sent to the adapter, no result yet. | Yellow pulse. |
| `succeeded` | The adapter confirmed it. | Green for about 2.5 s. |
| `failed` | Refused or errored. | Red flash for about 4 s. |

An accepted command that gets no result within 15 seconds fails with "the adapter did not answer in time". An approval resolves only when the adapter confirms the command, not when the button is pressed, so the approval never clears on an optimistic guess. Free text (instructions and tasks) is delivered to the adapter and never written to the audit log.

## Events

Agent Control publishes on the existing event bus with `source: "agent-control"`. That source is reserved: an external client that tries to publish with it through the generic `event` message is refused, so agent events cannot be forged.

### Semantic events

| Event | Payload (main fields) |
| --- | --- |
| `agent.registered` | `agentId` |
| `agent.online`, `agent.offline` | `agentId` |
| `agent.access` | `agentId`, `access` |
| `session.created` | `agentId`, `sessionId` |
| `session.started` | `agentId`, `sessionId`, `status`, `previous`. Published when a session becomes active for the first time, and again when it starts a new run after finishing (a retry). |
| `session.<status>` | The same, for every status: `session.running`, `session.tool_use`, `session.waiting`, `session.approval_required`, `session.paused`, `session.completed`, `session.failed`, `session.cancelled`, and so on |
| `session.ended` | `agentId`, `sessionId`, `reason` |
| `approval.created` | `approvalId`, `sessionId`, `agentId`, `tool?` |
| `approval.resolved` | `approvalId`, `sessionId`, `agentId`, `resolution` (`approved`, `rejected`, `withdrawn`, `superseded`) |
| `command.sent` | `requestId`, `command`, `agentId`, `sessionId?` |
| `command.completed` | `requestId`, `command`, `agentId`, `sessionId?`, `state`, `error?` |

### Projection events

These describe the surface-facing view, so a surface can bind to them without understanding sessions: `slot.changed` (`slot`, `status`, `agentId`, `sessionId`, `label`), `focus.changed` (`slot`) and `command.state` (`command`, `state`). A projection event is published only when something changed, so a quiet system produces no traffic.

## Slots and focus

The Launchpad has room for eight tracks. A track is a session, or an agent that has none yet. Tracks keep their pad until they disappear, new tracks take a free pad, and when all eight are in use only a finished, idle or offline track is replaced, and only by a more urgent one. Anything without a pad stays fully visible in the app.

`focus` is a selection, a cursor the command pads act on. It is not part of the model of what agents are doing. It never jumps while it points at something real. When nothing is focused, the first approval that arrives is focused automatically so the APPROVE pad is ready.

## Limits

Bounds keep a misbehaving client from filling memory or the screen: 64 agents, 120 sessions, 32 sessions per agent, 2000 characters of command text, 512 of working directory, and 20 diagnostics per adapter. A bridge connection can register at most 8 agents. Finished sessions are evicted oldest first.

## Persistence

| File | Holds | Notes |
| --- | --- | --- |
| `agents.json` | Notification setting, hook endpoint settings and token, per-agent access grants. | Written atomically. Readable by the current user only on Unix, because it holds the hook token. A damaged file is copied to `agents.json.bak` and defaults are used. |
| `audit.jsonl` | The audit trail. | One JSON object per line, rotated at 256 KiB with one generation kept. See [security.md](security.md#audit-log). |

Agents and sessions themselves are not persisted. They are live facts about processes that may no longer exist, so they are rebuilt as adapters register. State survives UI rerenders and surface switching because it lives in the Rust runtime, not in the React tree or on a surface.

## Mirrored in the UI

The UI receives an `AgentsSnapshot` (`agents-changed`) with a monotonically increasing `rev`, and ignores anything older than what it shows. A snapshot carries the agents, sessions, slots, focus, adapters, summary counts, the status palette, recent audit entries and the settings. The command availability in it comes from `availability`, so the app never recomputes a rule. The contract is guarded in both directions: the Zod schemas parse a snapshot captured from the runtime (`examples/agents-snapshot.json`), and a Rust test fails if the runtime starts emitting a field the fixture does not have.
