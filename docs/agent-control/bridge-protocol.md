# Agent Bridge protocol, version 1

The Agent Bridge lets any local process appear in Agent Control. It is versioned, provider-neutral and local-first. It does not depend on MCP or on any particular agent: if a program can open a WebSocket and send JSON, it can register an agent, report what that agent is doing, ask a person for approval and receive their decision.

It is the wire form of the [Adapter SDK](adapter-sdk.md). Every client message maps to one `AdapterHost` method, and every command LaunchDeck sends back is an `AdapterCommand` rendered as JSON. There is no second model.

A complete client in about 190 lines is in [`examples/generic-agent/`](../../examples/generic-agent/).

## Transport

The bridge shares the existing localhost WebSocket used for generic events.

| | |
| --- | --- |
| URL | `ws://127.0.0.1:47821` by default. The exact address is shown on the Adapters tab. The port is a setting. |
| Binding | `127.0.0.1` only. Connections from any other address are dropped. |
| Browser origins | A handshake that carries an `Origin` header must name `localhost`, `127.0.0.1`, `[::1]` or `tauri.localhost`. Any other site is refused with 403. |
| Framing | One JSON object per WebSocket text frame. Binary frames are ignored. |
| Size | Frames larger than 64 KiB are refused. |
| Connections | At most 16 at a time. |
| Rate | A token bucket per connection: a burst of 60 messages, then 30 per second. Beyond that, `rate_limited`. |

Messages whose `type` starts with `agent.` belong to the bridge. Anything else goes to the original event protocol, which only accepts `event` and `ping` and cannot publish with the reserved sources `runtime`, `device` or `agent-control`. A client cannot use it to forge agent events.

## Conventions

* Field names are camelCase. Identifiers (`agentId`, session and approval ids) are 1 to 64 characters from `A-Za-z0-9_.:-`.
* Every client message may carry an `id` (a number or a string up to 64 characters). It is echoed on the reply so a client can match answers to questions.
* Unknown **fields** are ignored, so the protocol can grow. Unknown **message types, statuses and capabilities** are rejected, never guessed.
* Every message is answered with exactly one reply: a message-specific one, `{"type":"ack"}`, or an `error`.

## Handshake

A connection starts with nothing. It must register an agent before it can report anything, and it can speak only for the agents it registered on that connection.

```json
{ "type": "agent.register", "protocol": 1,
  "agent": { "id": "nightly", "name": "Nightly build", "provider": "acme", "harness": "script",
             "capabilities": ["approve", "reject", "pause", "resume", "cancel"] } }
```

```json
{ "type": "agent.registered", "protocol": 1, "agentId": "nightly",
  "access": "monitor", "capabilities": ["approve", "reject", "pause", "resume", "cancel"] }
```

* `protocol` is required and must be `1`. A different value is refused with `unsupported_protocol`, so a client and a build that disagree fail loudly instead of drifting.
* `capabilities` is the explicit set of things this agent can really do. See [domain-model.md](domain-model.md#capabilities). An empty list is valid: a pure observer.
* `provider` and `harness` are display labels.
* `access` in the reply is `monitor` for a new agent. **Registering never grants control.** The person grants it in the app, per agent, and it is remembered by agent id.
* An agent id has one live owner. Registering an id that is connected from another process is refused with `agent_in_use`. Re-registering on the same connection updates the declaration.
* A connection may register at most 8 agents.

## Client to LaunchDeck

### `agent.session.update`

Report the state of a session. The first report for an unknown session id creates it.

```json
{ "type": "agent.session.update", "agentId": "nightly",
  "session": { "id": "nightly-42", "status": "running", "task": "Build and test",
               "progress": 0.4, "detail": "Compiling" } }
```

| Field | Notes |
| --- | --- |
| `status` | A normalized status or an accepted alias ([domain-model.md](domain-model.md#status)). `approval_required` is refused: use `agent.approval.request`. `offline` is refused: close the connection or unregister. |
| `task` | One line for display, up to 160 characters. Not stored on disk. |
| `progress` | Optional number from 0 to 1. |
| `detail` | Optional current step or tool, up to 120 characters. |
| `error` | For `failed`: a concise summary, up to 200 characters. Put raw output elsewhere. |

A session id belongs to the agent that first used it. Reply: `ack`.

### `agent.approval.request`

Ask a human to decide. The session moves to `approval_required`, the APPROVE pad starts flashing (once control is granted), and a notification is raised.

```json
{ "type": "agent.approval.request", "agentId": "nightly", "sessionId": "nightly-42",
  "approval": { "id": "ap-1", "message": "Publish the release to the registry", "tool": "publish" } }
```

`approval.id` is optional (one is generated). `message` is one line a person can decide on, up to 300 characters. A session has at most one pending approval; a newer request replaces the older one. Reply: `{"type":"ack","approvalId":"ap-1"}`.

The client then **waits**. It never assumes an answer.

### `agent.approval.withdraw`

The question no longer applies (answered elsewhere, timed out, the work changed). `{ "type":"agent.approval.withdraw", "agentId", "sessionId", "approvalId" }`. Reply: `ack`.

### `agent.session.end`

The session is closed and leaves the live set. `{ "type":"agent.session.end", "agentId", "sessionId" }`. Reply: `ack`.

### `agent.command.result`

The outcome of an `agent.command`. Always answer, even to refuse.

```json
{ "type": "agent.command.result", "agentId": "nightly", "requestId": "r-7", "ok": true }
{ "type": "agent.command.result", "agentId": "nightly", "requestId": "r-8", "ok": false, "error": "nothing is paused" }
```

A command with no result after 15 seconds is marked failed. An approval or rejection takes effect only when the result is `ok: true`. A `requestId` that is not waiting for this agent is refused with `unknown_request`. Reply: `ack`.

### `agent.unregister`

`{ "type":"agent.unregister", "agentId" }`. The agent goes offline and releases its id. Closing the connection does the same for every agent on it.

## LaunchDeck to client

### `agent.command`

A command the person issued, after LaunchDeck has checked that the agent declared the capability, is online, has been granted control and is in a state where the command makes sense.

```json
{ "type": "agent.command", "requestId": "r-7", "agentId": "nightly",
  "sessionId": "nightly-42", "approvalId": "ap-1",
  "command": { "kind": "approve" } }
```

| `command.kind` | Extra fields | Declared by capability |
| --- | --- | --- |
| `approve`, `reject` | `approvalId` on the envelope names the request being decided | `approve`, `reject` |
| `pause`, `resume`, `cancel`, `retry` | none | the same name |
| `send_instruction` | `text` | `send_instruction` |
| `spawn` | `task`, optional `cwd`; `sessionId` is null | `spawn` |

The client does the thing, reports its new state with `agent.session.update`, and confirms with `agent.command.result` carrying the same `requestId`. LaunchDeck delivers commands; whether the agent honours them is up to the client, which is why capabilities must be honest.

### `agent.access`

`{ "type":"agent.access", "agentId", "access":"control" | "monitor" }`. Sent when the person grants or revokes control. Informational: a client that is not reading it simply misses it, and commands stop being delivered the moment control is revoked.

## Errors

```json
{ "type": "error", "code": "not_registered", "message": "agent `x` is not registered on this connection; send agent.register first" }
```

`message` is a short sentence for a developer. Raw payloads are never echoed back.

| Code | Meaning |
| --- | --- |
| `invalid_message` | The JSON did not match the message's shape. |
| `unsupported_type` | Unknown `agent.*` message type. |
| `unsupported_protocol` | `protocol` is missing or not `1`. |
| `invalid_capability` | A capability name is not one of the twelve. |
| `invalid_status` | Unknown status, or a status that must not be set this way. The message lists the valid names. |
| `invalid_field` | An id, name or text field is empty, too long or has forbidden characters. |
| `not_registered` | The agent is not registered on this connection. |
| `not_owner` | The session or approval belongs to a different agent. |
| `agent_in_use` | The agent id is connected from another process. |
| `unknown_session`, `unknown_request` | Nothing with that id. |
| `limit_reached` | Too many agents, sessions or pending items. |
| `rate_limited` | Too many messages. Slow down. |
| `too_large` | The frame exceeded 64 KiB. |

## A session, end to end

```
client                                              LaunchDeck
  | agent.register {protocol:1, agent:{...}}  ----->  |
  | <-----  agent.registered {access:"monitor"}       |   agent appears, read-only
  | agent.session.update {status:"starting"}  ----->  |
  | agent.session.update {status:"running"}   ----->  |   pad turns green and pulses
  | agent.approval.request {message:"..."}    ----->  |   pad flashes white (APPROVE flashes once control is granted)
  |                          (person allows control, presses APPROVE)
  | <-----  agent.command {kind:"approve", requestId} |
  | agent.command.result {ok:true}            ----->  |   approval clears
  | agent.session.update {status:"running"}   ----->  |
  | agent.session.update {status:"completed"} ----->  |   pad turns green; a long run raises a notification
  | agent.unregister                          ----->  |
```

## Versioning

* The only version a build speaks is in `protocol`. A breaking change gets a new number and clients say which one they speak. A build refuses a client it does not understand rather than guessing.
* Additive changes (new optional fields, new status aliases, new notification kinds) do not bump the version, because unknown fields are ignored. New message types, statuses and capabilities need a new version, because they are rejected today.
* Clients should ignore fields and message types they do not know.

## Writing a client

1. Open the socket and send `agent.register` with only the capabilities you can honour.
2. Treat `access` as a fact about the world, not a request: do not retry until you get `control`.
3. Report state changes as they happen. LaunchDeck keeps the latest; it does not poll.
4. When you need a human, send `agent.approval.request` and wait. Decide in advance what a missing answer means for your agent. It should never mean "proceed".
5. Answer every `agent.command` with `agent.command.result`.
6. On reconnect, register again. If you get `agent_in_use`, the old connection has not been noticed as closed yet; wait a moment and retry.

## Security notes

The bridge is local-first, not authenticated. It is protected by being bound to loopback, by the origin check, by size, connection and rate limits, and above all by what it can do: **a bridge client can only describe the state of agents it registered.** It cannot run an action, press a pad, change a surface or read any other agent's state. Control is a separate decision the person makes per agent. See [security.md](security.md) for the trust model and the known gaps (any local process can register an agent, and a local web page served from a localhost origin can reach the socket).
