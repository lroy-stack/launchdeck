# Security

Agent Control puts a physical button next to decisions that matter: letting an agent run a command, write a file, or go on. The design has one aim: **a person decides, and nothing else ever does.** The rest of this document is how that is kept true, what is recorded, and where the edges are.

## Rules

1. LaunchDeck never accepts arbitrary remote commands from a network interface. Every network input is bound to `127.0.0.1`.
2. A program must explicitly register before it appears. Nothing is discovered or scanned for.
3. Monitoring and control are different permissions. Registering never grants control.
4. A permission request is **never approved automatically.** Timeouts, hang-ups, errors, restarts and revoked control all end in "no decision".
5. Privileged actions keep a human in the loop: approving is a press, by a person, on a pad or in the app.
6. Critical interventions are logged.
7. Prompts and sensitive payloads are not logged.

## Who and what is trusted

| Party | Trust | Why |
| --- | --- | --- |
| The person, in the desktop app or on a pad | Decides everything that matters. | They own the machine and the agents. |
| The desktop UI (webview) | Trusted app code, with a minimal capability set. | It gets core events and the app's own commands; no shell, filesystem or HTTP plugin is exposed to it. The CSP allows only the app itself and Tauri IPC. |
| Adapters inside LaunchDeck | Trusted code, but constrained by the SDK. | They see only `AdapterHost`, only for agents they registered. See [future isolation](#future-permission-isolation-for-adapters). |
| A local process using the Agent Bridge | Untrusted: may describe agents, nothing more. | Not authenticated. It can register read-only agents. It cannot act, press, edit or read others' state. |
| A tool calling the hook endpoint | Authenticated by a token. | Off by default; per-install token; read-only until control is granted. |
| An agent | Untrusted for decisions. | Its reports are shown; its requests wait for a person. |
| A web page | Refused. | The origin checks stop pages on other sites; see the gaps for localhost-served pages. |

## Inputs and what each can do

| Input | Bound to | Accepts | Cannot |
| --- | --- | --- | --- |
| Agent Bridge (`agent.*` on the WebSocket) | `127.0.0.1:47821`, origin-checked, 64 KiB frames, 16 connections, 60-burst / 30 per second | Registering agents; reporting sessions, approvals and command results for **its own** agents | Run an action, press a pad, change a surface, read other agents, or grant itself control |
| Generic events (`event` on the same socket) | the same | State events from named sources | Use the reserved sources `runtime`, `device`, `agent-control`; run anything |
| Hook endpoint (HTTP) | `127.0.0.1:47822`, off by default | `POST` of a harness's hook JSON, with a bearer token | Anything but the routes it was given; browsers; other hosts |
| Tauri IPC | the app's own webview | The `agents_*` commands | Be called from outside the app |

Agent Control adds no command that executes anything. The only things that leave LaunchDeck toward an agent are the commands a person issued, delivered to the adapter that owns that agent, and only if the agent declared the capability and the person granted control.

## Monitor and control

| | `monitor` | `control` |
| --- | --- | --- |
| Appears in the app, on the pads | yes | yes |
| Notifications for it | yes | yes |
| Commands delivered | **no** | yes, for the capabilities it declared |
| Default for built-in demo agents | | yes (they are simulated) |
| Default for everything else | **yes** | |

Control is granted per agent, in the Agents view, with a confirmation step. It is stored by agent id, so a program that reconnects with the same id keeps the person's decision, and a different id does not inherit it. Revoking takes effect at once and the adapter is told (`agent.access` on the bridge). Granting and revoking are recorded in the audit log.

The availability check is one function used by the pads, the app and the dispatcher, so a command refused for monitor-only agents is refused for all three. An adapter cannot widen it, because the service checks before it ever calls the adapter.

## Human approval

When an agent asks for permission, the question is shown on its card, on its pad (white flash) and in a notification. It is answered by pressing APPROVE or REJECT, which issues a command to the adapter, which carries it out and confirms.

What happens when no one answers, or something goes wrong:

| Situation | Outcome |
| --- | --- |
| The person does nothing and the hold time passes (hooks) | Empty reply; the harness shows its own prompt. The session shows "waiting at the harness". |
| The harness hangs up or crashes | The request is dropped; nothing is approved. |
| The agent is read-only when the request arrives | The request is shown but not held; the harness asks its own user at once. |
| Control is revoked while a request is held | The held request is released with no decision. |
| LaunchDeck is not running | The harness's own prompt, as if LaunchDeck were not installed. |
| A bridge client disconnects with an approval pending | The agent goes offline and the approval is withdrawn. |
| An approval command gets no result in 15 seconds | The command fails and the approval stays pending. |
| A newer request arrives for the same session | The older one is superseded, not approved. |

A bridge client decides for itself what a missing answer means. The example client withdraws the request and fails the session. It never proceeds.

## The hook endpoint

Some tools can call a URL or run a command at lifecycle points. That path needs a tiny local HTTP server, which would otherwise be an attractive target for any web page. It is built to refuse them. Checks run in this order:

1. Binds `127.0.0.1` only; connections from other addresses are dropped.
2. The `Host` header must be exactly this loopback address and port, which defeats DNS rebinding.
3. Any request with an `Origin` header is refused. Hooks are not browsers; browsers are.
4. Only `POST`, only to a route that exists.
5. `Authorization: Bearer <token>`, compared in constant time.
6. Bounded headers (16 KiB), body (2 MiB), connections (16) and read time (10 s).
7. Nothing in a request body is logged.

The token is 256 bits from the operating system's random source, created the first time the endpoint is enabled. It is handed to tools through an environment variable (`LAUNCHDECK_HOOK_TOKEN`), so the settings files a person edits never contain it. LaunchDeck keeps it in `agents.json`, which on Unix is created readable by the owner only. The setup panel shows it on request and the **Create a new token** button replaces it, which stops every configured tool until it is updated.

## Audit log

A lightweight record of what happened and who did it, in `audit.jsonl` next to the workspace file. It is append-only, one JSON object per line, rotated at 256 KiB with one previous generation kept (`audit.1.jsonl`), and on Unix created readable by the owner only. It is not a database and needs none.

```json
{"ts":1791073790112,"kind":"approval.approved","actor":"user","agentId":"demo-atlas","sessionId":"demo-atlas-run1","tool":"Bash","detail":null}
```

| Recorded | `kind` |
| --- | --- |
| An agent appeared | `agent.registered` |
| A session began, ended well, failed or was cancelled | `session.started`, `session.completed`, `session.failed`, `session.cancelled` |
| An approval was requested, and how it ended | `approval.requested`, `approval.approved`, `approval.rejected`, `approval.withdrawn`, `approval.superseded` |
| A person intervened | `intervention.pause`, `intervention.resume`, `intervention.cancel`, `intervention.retry`, `intervention.spawn` |
| A command failed or was refused | `command.failed`, `command.refused` |
| Control was granted or revoked | `access.granted`, `access.revoked` |

Each entry has a time, an actor (`user`, `agent` or `system`), and ids. It may carry the **name** of a tool (for example `Bash`) and a short code. It never carries what the agent was asked to do, what it wrote, a command line, a tool's input, a file path or an instruction. Free text sent as an instruction or a task goes to the adapter and nowhere else.

The log is for the person who owns the machine to answer "what did I approve, and when?". The Sessions tab shows the most recent entries.

## What is kept, and what is not

| Data | Where | Kept? |
| --- | --- | --- |
| Task line, current tool name, progress | Memory, shown on cards | Not written to disk. Gone when LaunchDeck quits. |
| Approval message | Memory, shown on cards | Not written to disk, not in the audit log, not in notifications. |
| Prompts, tool inputs, file contents | Never read beyond one display line | Not stored. The Claude Code adapter reads the first line of the prompt for display and does not touch the transcript. |
| Agent and session ids, status changes | Audit log (ids and kinds only) | Yes, owner-only on Unix. |
| Adapter diagnostics | A short list per adapter in memory, plus the application log | Concise summaries. Raw payloads are not logged. |
| Settings, access grants, hook token | `agents.json` | Yes, owner-only on Unix. |

Notifications are limited to four situations (an approval is needed, an agent failed, a long run completed, an adapter disconnected). They name the agent and, for approvals, the tool; a failure repeats the agent's own concise error summary. They never contain prompts, tool inputs or command lines.

Errors shown to people are short sentences. Raw detail goes to the adapter's diagnostics and the log, so a failure is explainable without spilling the content that caused it.

## Known gaps

These are real and deliberate to state.

* **The bridge is not authenticated.** Any process on the machine can connect to `127.0.0.1:47821` and register an agent. The limits above make that low-impact (it appears read-only, can describe only itself, and a person must still grant control), but it can put noise on the pads and the screen. A future option is a bridge token like the hook endpoint's.
* **Pages served from localhost.** The origin check admits `localhost`, `127.0.0.1`, `[::1]` and `tauri.localhost` on any port, so a web page served from a local development server could connect to the bridge. The same limits apply to it.
* **Control is a per-agent switch, not per-capability.** Granting control allows every command the agent declared.
* **Hook adapters run inside the LaunchDeck process.** They are reviewed code, not sandboxed code.
* **No encryption of `agents.json`.** It holds the hook token and access choices. It is protected by file permissions only.
* **The audit log is not tamper-evident.** It is a convenience record, not evidence.
* **Nothing prevents a person from granting control carelessly.** The confirmation step is a prompt, not a policy.

## Future: permission isolation for adapters

Today an in-process adapter is trusted code limited by its API. If third parties ship adapters (for example in a later version of the Pack format, which may carry them), that is not enough. The direction, none of it built:

* **Out of process by default.** Third-party adapters run as separate processes speaking the Agent Bridge, which already has the right shape: they can only describe their own agents. An adapter that needs more says so in its manifest.
* **Declared permissions.** An adapter's manifest lists what it needs: the network hosts it may reach, the folders it may read, whether it may spawn processes, the capabilities it may register. They are shown before install and enforced by a sandbox (WASM or an OS sandbox), the way the plugin design already plans.
* **Per-capability grants.** Control becomes a set: approve and reject without spawn, pause without cancel. The person's grant is stored per agent and capability.
* **A bridge token** with a scope per client, so that a registration can be tied to a program the person recognises.
* **Signed packages and a revocation list**, described in [roadmap/marketplace.md](../roadmap/marketplace.md).
* **Auditable by construction.** Anything an isolated adapter does on the person's behalf is an `AdapterHost` call, which is already the thing the audit log records.

The architecture supports this without redesign because adapters already see nothing but `AdapterHost` and are already owned per agent. What is missing is the enforcement around them, not a different model.
