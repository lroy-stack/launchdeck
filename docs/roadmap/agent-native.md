# Agent-native LaunchDeck: study and proposed architecture

**Status: study, nothing built.** Written 2026-10-23, against `main` at `0a162cd` (LaunchDeck 0.3.0). It answers one question: what LaunchDeck has to become so that AI agents connect to it in one step, a person orchestrates them from a Launchpad, and the surface adapts to the person's real agents and workflows, while every rule in [Agent Control security](../agent-control/security.md) still holds.

It supersedes the conclusion of [the MCP bridge](mcp-bridge.md) ("should it be built? Not yet"). That page was right for the MCP revision it read; the ecosystem has moved since, and so has the owner's priority: a native MCP server is wanted before 1.0.

Facts come from four research passes run on 2026-10-23 on primary sources (vendor documentation, the MCP specification, source code), with the page and its date. Anything not read in a primary source that day is marked **NOT VERIFIED**. Nothing in this document was run against a real harness, product or device.

## 1. Conclusions first

1. **The cloud agents the owner named exist, and none of them can be approved from outside their own app.** Claude Duet (Anthropic, 2026-10-21), Grok Bot (xAI, 2026-08-11) and OpenAI dots (2026-09-29) are always-on agents with their own cloud computer. All three keep approvals inside their own interface, and none publishes an API for an outside program to read an agent's status or answer its approval. Duet is US-only at launch and dots excludes the EEA, Switzerland and the UK, so neither can be tried from the owner's location today. "Full compatibility" with them is not possible on public interfaces. What is possible is narrower and is described in section 6.
2. **The harnesses that run on the person's machine are where LaunchDeck can be genuinely agent-native today.** Claude Code, Codex, Cursor, Copilot CLI and OpenCode all expose a documented point where an outside program can answer a permission request; Codex and OpenCode also expose real control (start, steer, interrupt). The Agent Client Protocol (ACP) gives one interface to about 45 agents that LaunchDeck could launch itself.
3. **Build one native MCP server inside the runtime, on MCP 2026-07-28 with a fallback to 2025-11-25, reached through stdio.** It lets any MCP-capable assistant report its work, ask the person on the pad, read LaunchDeck's state and *propose* surfaces. It never lets an agent run an action or press a pad.
4. **"Connect in one click" is the harness's own install mechanism, started from LaunchDeck with the exact command shown.** For Claude Code that is a LaunchDeck plugin (MCP server, HTTP hooks, a skill, and later a channel). For other harnesses it is their documented command, deep link or bundle.
5. **"The setup comes with it" is done twice, deterministically and then intelligently.** LaunchDeck reads the documented, declarative parts of a harness's configuration (agent and skill names, commands, MCP servers) only when the person connects that harness, and builds a suggested surface. Then the connected agent can propose a refined surface over MCP, and it goes through the import review LaunchDeck already has for Packs.
6. **The pad should carry the approvals that matter, not all of them.** Anthropic reports that people accept 93% of Claude Code's permission prompts anyway and that sandboxing removed 84% of them. LaunchDeck must never approve by itself, but it should be designed around the escalations a harness's own policy leaves for a person.

## 2. Where LaunchDeck is today

Agent Control has one model (agents, sessions, approvals, commands, eight tracks, a focus), one decision function (`availability`), an audit trail, and three ways in ([domain model](../agent-control/domain-model.md), [security](../agent-control/security.md)):

| Way in | What it carries | Gaps that matter for this study |
| --- | --- | --- |
| Agent Bridge v1, `ws://127.0.0.1:47821` | Any local program registers agents, reports sessions, asks for approvals, receives commands | Not authenticated; control is per agent, not per capability; no sub-sessions, tasks or approval scopes |
| Hook endpoint, `127.0.0.1:47822`, bearer token | Claude Code (verified live on 2026-10-04, v2.1.289) and Codex (written from docs, never run): status and `PermissionRequest` allow or deny | Approve and reject only; no pause, resume, instruct or start |
| Built-in demo | Simulated agents | |

Rules that bind everything below: nothing is discovered or scanned for; registering never grants control; a permission request is never approved automatically; model-written text is untrusted and never logged; nothing arriving from outside can start an action.

Reusable pieces that make this study cheaper than it looks: the Surface and Pack format with a hostile-input-safe review (inspect, plan, apply), the State Hub and feedback rules, the Observable Catalog, `explain_control`, Auto Context, and the device layer for the Launchpad X and Mini MK3 (including scrolling text).

## 3. The landscape, October 2026

### 3.1 MCP itself

| Fact | Source |
| --- | --- |
| The current revision is **2026-07-28** (released 2026-07-28). The previous one is 2025-11-25. | [versioning](https://modelcontextprotocol.io/specification/versioning), [release post](https://blog.modelcontextprotocol.io/posts/2026-07-28/) |
| The core is stateless: no `initialize` handshake, no `Mcp-Session-Id`. Each request carries `io.modelcontextprotocol/protocolVersion` and `clientCapabilities` in `_meta`; servers MUST answer `server/discover`. | [changelog](https://modelcontextprotocol.io/specification/2026-07-28/changelog) |
| Servers no longer send requests to clients. Elicitation, sampling and roots go through Multi Round-Trip Requests: a `tools/call` returns an `InputRequiredResult`, the client retries with `inputResponses`. `requestState` is attacker-controlled and must be integrity-protected. | [MRTR](https://modelcontextprotocol.io/specification/2026-07-28/basic/patterns/mrtr), [transports](https://modelcontextprotocol.io/specification/2026-07-28/basic/transports) |
| Sampling and roots are deprecated (removable from mid-2027); elicitation is not. | [deprecated features](https://modelcontextprotocol.io/specification/2026-07-28/deprecated) |
| Subscriptions and list changes go through `subscriptions/listen`. Progress flows on the originating request's stream; closing an HTTP response stream cancels the request. | [changelog](https://modelcontextprotocol.io/specification/2026-07-28/changelog), [Streamable HTTP](https://modelcontextprotocol.io/specification/2026-07-28/basic/transports/streamable-http) |
| Tasks (`io.modelcontextprotocol/tasks`), MCP Apps (`io.modelcontextprotocol/ui`, interactive HTML in the client) and Skills over MCP are official extensions, off unless both sides declare them. No client was confirmed to support Tasks. MCP Apps is listed for Claude (web and Desktop), ChatGPT, VS Code Copilot, Cursor, Goose and others. | [extensions](https://modelcontextprotocol.io/docs/extensions/overview), [client matrix](https://modelcontextprotocol.io/extensions/client-matrix) |
| Local servers SHOULD use stdio; an HTTP server MUST validate `Origin`, SHOULD bind 127.0.0.1 and SHOULD authenticate; token passthrough is forbidden; one-click local installs MUST show the exact command and ask. | [security best practices](https://modelcontextprotocol.io/specification/2026-07-28/basic/security_best_practices) |
| The official Rust SDK `rmcp` is at 3.7.0 (2026-10-22), Tier 1, implements 2026-07-28 and stays compatible with 2025-11-25, including MRTR, `subscriptions/listen`, Tasks and HMAC sealing of `requestState`. Its Streamable HTTP server **leaves `Origin` validation off by default** (`allowed_origins` empty); it must be turned on explicitly. | [crates.io](https://crates.io/crates/rmcp), [README](https://github.com/modelcontextprotocol/rust-sdk), `streamable_http_server/tower.rs` on `main` |
| MCP is stewarded by the Agentic AI Foundation under the Linux Foundation (since 2025-12-09). The roadmap (2026-08-22) lists server-initiated events, channels and webhooks as its first priority. | [AAIF post](https://blog.modelcontextprotocol.io/posts/2025-12-09-mcp-joins-agentic-ai-foundation/), [roadmap](https://modelcontextprotocol.io/development/roadmap) |
| `.mcpb` bundles (manifest 0.3, server type `node`, `python` or `binary`) install a local server in Claude Desktop on macOS and Windows with one click. | [mcpb](https://github.com/modelcontextprotocol/mcpb), [Claude docs](https://claude.com/docs/connectors/building/mcpb) |

Consequence for LaunchDeck: an approval held on the pad fits MCP well. The server keeps the `tools/call` open, sends `notifications/progress` while it waits, and returns the person's decision as the result. No elicitation is needed, because the person answers on LaunchDeck, not in the client.

### 3.2 The cloud agents the owner named

| | Claude Duet | Grok Bot | OpenAI dots |
| --- | --- | --- | --- |
| What | A separate app where Claude works as a proactive agent on its own cloud computer, runs on Claude Opus 5.5 | "AI teammates": always-on agents with their own cloud computer; teams of Bots, a chief-of-staff Bot | Always-on agents on GPT-6 Astra with their own cloud computer and browser |
| Status | Research preview, waitlist, **US only**, Pro and Max (announced 2026-10-21) | Beta since 2026-08-11; changelog at v0.75.2 (2026-10-22) | Rolling out since 2026-09-29 to Pro, Business Premium, Enterprise; **not in the EEA, Switzerland or the UK** at launch |
| Approvals | Shows the plan and waits before sending, paying, signing or deleting; in its own app | Review agent allows, blocks or escalates; "Review an action" cards in the app or Slack | Custom rules ("Ask before taking action"…); requests answered in the desktop app's Activity view |
| Outside status or approval API | None found | None found | None found |
| What an outside program can do | Be a remote connector or plugin on the person's Claude account, which Duet inherits (NOT VERIFIED that a custom remote connector reaches Duet) | Be a remote MCP server or plugin (must be publicly reachable); start a routine through its webhook URL and key | Be a plugin (MCP server); trigger a dot through **MCP events** (draft extension, webhook delivery, Standard Webhooks signatures, public HTTPS only) |
| Sources | [article](https://claude.com/resources/articles/claude-duet), [help](https://support.claude.com/en/articles/17631495-get-started-with-claude-duet) | [launch](https://x.ai/news/introducing-grok-bot), [changelog](https://x.ai/changelog/bot), [101](https://x.ai/bot/guides/grok-bot-101) | [launch](https://openai.com/index/introducing-dots/), [controls](https://learn.chatgpt.com/docs/dots/controls), [MCP events](https://developers.openai.com/plugins/build/mcp-events) |

All three reach outside tools only as **remote** servers on the public internet. A server on `127.0.0.1` is invisible to them.

### 3.3 Harnesses on the person's machine

| Harness | Status and events | Answer a permission request | Real control | One-click install | Sources |
| --- | --- | --- | --- | --- | --- |
| **Claude Code** | Hooks incl. `PermissionRequest`, `Notification` (`permission_prompt`, `agent_needs_input`, `agent_completed`…), `SubagentStart/Stop`, `TaskCreated/Completed`, `TeammateIdle`, `Stop`, `SessionEnd`; hook types `command`, `http`, `mcp_tool` | `PermissionRequest` returns `decision.behavior` allow or deny. **Channels** (research preview): an MCP server declaring `claude/channel/permission` receives `permission_request` and answers allow or deny; the first answer (terminal or remote) wins | A channel can push messages into the session (`notifications/claude/channel`); no documented pause or interrupt | `claude plugin install <name>@<marketplace>` with a review pane; plugins bundle MCP servers, hooks, skills; `claude mcp add` | [hooks](https://code.claude.com/docs/en/hooks), [channels](https://code.claude.com/docs/en/channels-reference), [plugins](https://code.claude.com/docs/en/plugins/install), [MCP](https://code.claude.com/docs/en/mcp) |
| **Claude Agent SDK** (personal harnesses) | SDK hooks | `canUseTool` may stay pending indefinitely; MCP tools with `_meta["anthropic/requiresUserInteraction"]` always reach it | Whatever the harness author builds | Library | [permissions](https://code.claude.com/docs/en/agent-sdk/permissions), [user input](https://code.claude.com/docs/en/agent-sdk/user-input) |
| **Codex CLI** | Hooks incl. `PermissionRequest`, `SubagentStart/Stop`, `Stop`, `Interrupt`; must be trusted in `/hooks` before they run | `PermissionRequest` returns allow or deny; sync hooks block, default timeout 600 s | **`codex app-server`** (stdio JSON-RPC): `thread/start`, `turn/start`, `turn/steer`, `turn/interrupt`; typed approval requests with `accept`, `acceptForSession`, `decline`, `cancel`; `thread/status/changed` with `waitingOnApproval` | `mcp_servers.<id>` in `config.toml`; `codex mcp add` NOT VERIFIED | [hooks](https://learn.chatgpt.com/docs/hooks), [config](https://learn.chatgpt.com/docs/config-file/config-reference), [app-server](https://learn.chatgpt.com/docs/app-server), `app-server-protocol` on `main` |
| **OpenCode** | Plugin events incl. `permission.asked`, `session.status`, `session.idle`, `session.error`; server SSE at `GET /event` | `POST /session/:id/permissions/:permissionID` | `POST /session`, `/prompt_async`, `/abort` on `opencode serve` (127.0.0.1:4096, optional basic auth) | Plugin file in `.opencode/plugins/` | [plugins](https://opencode.ai/docs/plugins/), [server](https://opencode.ai/docs/server/) |
| **Cursor** | 21 hook events incl. `beforeShellExecution`, `beforeMCPExecution`, `subagentStart/Stop`, `stop` | allow, deny or ask (how "ask" resolves is not documented) | None documented | `cursor://anysphere.cursor-deeplink/mcp/install?name=…&config=<base64>` with a consent prompt | [hooks](https://cursor.com/docs/agent/hooks), [install links](https://cursor.com/docs/context/mcp/install-links) |
| **GitHub Copilot CLI** | `preToolUse`, `permissionRequest`, `sessionStart`, `agentStop`, `subagentStop`, `notification`; command or HTTP hooks | `preToolUse` allow or deny; default timeout 30 s | Not researched | Hook files in `.github/hooks/` and `~/.copilot/hooks/` | [hooks reference](https://docs.github.com/en/copilot/reference/hooks-configuration) |
| **Gemini CLI** | 11 hook events | Can block a tool (exit 2), cannot approve one | None | `settings.json` at three scopes | [hooks](https://geminicli.com/docs/hooks/) (replaced by Antigravity CLI for unpaid tiers on 2026-06-18) |
| **ACP agents** (Gemini CLI, Copilot, Cursor, Cline, Goose, OpenCode, Claude and Codex through adapters, ~45 in all) | An ACP client launches the agent as a subprocess | Permission requests through the protocol (`session/request_permission` and its options NOT VERIFIED) | Start, prompt, cancel, by construction | The client launches the agent | [agents list](https://agentclientprotocol.com/overview/agents), [introduction](https://agentclientprotocol.com/overview/introduction) |

Two modes appear. **Watch**: the person runs their own sessions and LaunchDeck observes and answers through hooks, channels or a server API. **Run**: LaunchDeck starts the agent itself (ACP, Codex app-server, OpenCode server) and gets full control in exchange for owning the process. Today LaunchDeck only watches.

### 3.4 Prior art

Elgato added an MCP server to Stream Deck 7.4 (2026-05-26): an AI client triggers actions placed on a virtual "MCP Deck". It runs in the opposite direction (the AI drives the deck). Small projects show Claude Code session states on Stream Deck keys through hooks, status only, with no approval. No Launchpad project and no hardware approval surface for agents was found ([press release](https://www.corsair.com/newsroom/press-release/elgato-brings-ai-control-to-stream-deck-launching-with-nvidia-project-g-assist-2), [streamdeck-claude](https://github.com/JulienCr/streamdeck-claude)). The space a physical approval and orchestration surface occupies is open.

### 3.5 What is known about approval UX

Anthropic: approval prompts cause "approval fatigue"; sandboxing removed 84% of prompts internally ([sandboxing](https://www.anthropic.com/engineering/claude-code-sandboxing), 2025-10-20). People accept 93% of prompts; auto mode uses a two-stage classifier, with 0.4% false positives and 17% false negatives on real over-eager actions ([auto mode](https://anthropic.com/engineering/claude-code-auto-mode), 2026-03-25). OpenAI ships the same idea as `approvals_reviewer = "auto_review"` and session-scoped decisions (`acceptForSession`). Grok Bot and dots both have a reviewer agent that escalates to the person. The industry has converged on: policy or a reviewer handles the routine, the person handles the escalation, and "allow for this session" is a first-class answer.

## 4. Proposed architecture

```
 Watch: the person's own sessions        Run: agents LaunchDeck starts       Remote (later, opt-in)
 Claude Code plugin (hooks, MCP,         ACP client (Gemini, Copilot,        Duet, Grok Bot, dots,
 channel), Codex hooks, Cursor,          Cursor, Goose, OpenCode, Claude,     Codex Cloud: LaunchDeck
 Copilot CLI, OpenCode server            Codex), Codex app-server             as a remote connector
            |                                       |                                  |
            v                                       v                                  v
     agents/adapters/*  ------------------>  AgentControl  <-----------------  mcp/ (native MCP server)
     (one per harness, the only               one model, availability,          tools, resources, proposals
      code that names a vendor)               grants, audit                     |
                                                    |                           v
                                                    v                     artifacts review
                                   State Hub, feedback, renderer           (inspect, plan, apply)
                                                    |
                                                    v
                                     Launchpad X / Mini MK3 / virtual
```

### 4.1 The native MCP server (`mcp/`, a new domain)

One server, one module that owns the mapping between MCP and LaunchDeck. It holds no state of its own: every call goes to `AgentControl`, the State Hub or the artifacts service, so validation, ownership, limits and audit stay where they are.

**Transport.** stdio, as the specification recommends for local servers. The LaunchDeck binary gets a `mcp` mode (`launchdeck mcp`), which the harness launches. It connects to the running app over a per-user local socket (a Unix domain socket in the app data folder, mode 0600; a named pipe with a user-only ACL on Windows), never over TCP. If the app is not running, every tool answers "LaunchDeck is not running" and nothing else happens. Streamable HTTP waits for the remote case (section 6), where it needs OAuth 2.1, Protected Resource Metadata and explicit `Origin` enforcement in `rmcp`.

**Protocol.** `rmcp` 3.x, 2026-07-28 first with 2025-11-25 as the fallback, because clients are still migrating. No sampling, no roots (both deprecated). Tool annotations on every tool (`readOnlyHint`, `destructiveHint` false everywhere, since nothing here destroys anything).

**Identity.** As in the earlier design: the agent's id, name and harness come from the configuration the person installed (arguments or environment of the stdio command), never from the model. One install is one agent identity. Registration is `monitor`; control is the person's grant.

**Tools.**

| Tool | What it does | Notes |
| --- | --- | --- |
| `report` | Sets the session's status, task line, progress | Statuses a client may set; model text, untrusted, one line, never logged |
| `ask_person` | Puts a question on the pad and in the app and **holds the call** until a person answers; streams progress while waiting | Returns `approved`, `rejected` or `no_decision` (not granted, timeout, cancelled, app closed). Marked "supplied by the agent" on the pad: cooperation, not enforcement |
| `end_session` | Closes the session | |
| `get_state` | Active surface and page, mode, the agent's own sessions and pending approvals | Never another agent's data, never the foreground application (Auto Context's privacy rule extends to agents) |
| `explain_pad` | Why a pad looks as it does | Reuses `explain_control` |
| `propose_surface` | Takes a Surface file (`.ldsurface` JSON) and runs the existing inspect and plan; returns the review summary and a review id | **Nothing is applied.** The person reviews it in the app, with the privileged actions listed, as for any imported Pack |
| `publish_state` (phase B) | Writes a value to a state the agent declared (for example `tests: passing`), so pads can follow it with ordinary feedback rules | Scoped to the agent's own provider id, like a plugin's states |

**Resources.** `launchdeck://surfaces`, `launchdeck://surfaces/{id}`, `launchdeck://catalog/actions`, `launchdeck://catalog/observables`, `launchdeck://devices`, with `subscriptions/listen` so an assistant designing a surface sees the catalog it may use. Read-only.

**Prompts.** `design-surface`: a prompt that tells the assistant how to describe the person's workflows as a Surface file (what the catalog offers, the 8x8 grid, the rules for task pads), so `propose_surface` gets something reviewable.

**MCP Apps (phase B).** A rendered preview of a proposed surface, and of a pending approval, inside clients that support `io.modelcontextprotocol/ui`. A preview only; the decision stays on the pad or in LaunchDeck.

**Never offered.** Run an action, press a pad, apply an edit without review, read a secret or a plugin's settings, read the foreground application or its history, grant or widen control, read another agent's sessions. These are the rules the platform already has; the MCP server is not an exception to any of them.

### 4.2 Harness adapters, by what each harness really offers

| Harness | Mode | Adapter does | Capabilities it can honestly declare |
| --- | --- | --- | --- |
| Claude Code | Watch | A **LaunchDeck plugin**: the MCP server above, HTTP hooks to the existing endpoint (permission requests, notifications, subagents, tasks, teammates), the `design-surface` skill; the channel with permission relay once it can be loaded without the development flag | approve, reject; `send_instruction` through a channel; sub-sessions from `SubagentStart/Stop` and tasks from `TaskCreated/Completed` |
| Codex | Watch | Hooks (already written, never run) | approve, reject |
| Codex | Run | `codex app-server` over stdio | spawn, send_instruction (`turn/steer`), cancel (`turn/interrupt`), approve, reject, approve for session (`acceptForSession`) |
| OpenCode | Watch | Its server API: SSE events, permission replies, abort, prompt | approve, reject, cancel, send_instruction, spawn |
| Cursor, Copilot CLI | Watch | HTTP or command hooks; Cursor's MCP deep link | approve, reject (Copilot's 30 s timeout is short for a person: the pad must say so) |
| Gemini CLI | Watch | Hooks | observe; block only |
| ACP agents | Run | One ACP client in LaunchDeck | spawn, send_instruction, cancel, approve, reject, as the protocol allows |
| Personal harnesses | Watch | A small LaunchDeck helper for the Agent SDK's `canUseTool` (TypeScript, Python), and Agent Bridge v2 for anything else | what the author declares |

**Run mode is a new decision**, because LaunchDeck would start and own agent processes. It is the only way to give a Launchpad the "start a task from a pad" experience for most harnesses, and ACP makes it one implementation instead of forty. Recommendation: build it, behind an explicit per-harness setting, launching only binaries the person points to, in the working folder the person chose, with the harness's own sandbox and approval policy left on.

### 4.3 Domain model changes

* **Sub-sessions.** A session gains an optional parent and a kind (`main`, `subagent`, `teammate`). Claude Code and Codex both report subagents now; teams are a Claude Code concept. The urgency of a parent includes its children.
* **Tasks.** A session may carry a short list of tasks with a state (from `TaskCreated/Completed`), for display only.
* **Approval category and scopes.** An approval carries the category the harness reports (a command, a file change, a network rule, an MCP tool, other) and the answers the harness offers (once, for this session). LaunchDeck relays what the harness offers; it never invents a scope or a risk score.
* **Per-capability grants.** "Control" becomes a set (approve and reject without spawn, for example). Listed as a known gap today; the MCP server and Run mode make it necessary.
* **Agent Bridge v2.** A per-client token (closing the known gap that any local process can register), the fields above, and v1 kept readable.

## 5. One-click connection and the setup that comes with it

### 5.1 Connecting

A **Connect** button per harness on the Agents view. It shows the exact command or file change, asks, and then uses the harness's own mechanism, never a private file format:

| Harness | What Connect does | Consent inside the harness |
| --- | --- | --- |
| Claude Code | Runs `claude plugin install launchdeck@<marketplace> --scope user` (the LaunchDeck marketplace is a public repository) | Claude Code's own review pane |
| Claude Desktop | Opens the `launchdeck.mcpb` bundle (binary type, macOS and Windows) | Desktop's install dialog |
| Codex | Adds `mcp_servers.launchdeck` and the hooks to `~/.codex/` | Codex asks to trust new hooks in `/hooks` |
| Cursor | Opens the `cursor://…/mcp/install` deep link, writes `hooks.json` | Cursor's install prompt |
| Copilot CLI, Gemini CLI, OpenCode | Writes their documented hook or plugin file | The harness's trust rules where they exist |

The hook token travels in an environment variable, as today, never in a file the person edits. Disconnect removes exactly what Connect added and nothing else.

### 5.2 Reading the person's setup

This needs one rule to change. Today: "nothing is discovered or scanned for". Proposed: **nothing is read until the person connects a harness, and then only the documented, declarative files of that harness, listed on screen before they are read.**

| Harness | Read | Never read |
| --- | --- | --- |
| Claude Code | Names and descriptions (frontmatter) of `~/.claude/agents/*.md` and `.claude/agents/*.md`, `skills/*/SKILL.md`, `commands/*.md`; server names in `.mcp.json`; hook and permission rule summaries in `settings.json` | `~/.claude.json` (account and trust, app-managed), transcripts in `~/.claude/projects/`, `history.jsonl`, the bodies of skills and prompts |
| Codex | `config.toml` server and profile names, `hooks.json` | Transcripts, credentials |
| Others | The equivalent declarative files listed in 3.3 | The same exclusions |

From that LaunchDeck builds a **suggested surface**: one track per agent and subagent, a task pad per command or skill (usable where a mode can start or instruct; dim and explained where it cannot), the approval pads, and the agent's states. It goes through the same review as an imported Pack. Only the surface the person accepts is saved, plus the identifiers its rules need. What was read is not kept.

### 5.3 Refining it with the agent

Once connected, the agent knows its own subagents, skills and the project far better than any file parser. The `design-surface` prompt and `propose_surface` tool let it propose a better surface ("one pad per test suite, the deploy pad next to APPROVE"). The proposal is model-written and therefore untrusted; the artifacts review already treats every import that way, lists privileged actions such as shell commands, and applies nothing without the person. Personalisation then becomes a conversation, at any moment, with the pad as the confirmation.

## 6. The cloud agents: what compatibility can honestly mean

| Wanted | Possible today | How |
| --- | --- | --- |
| See their status on a pad | Only what they choose to report | A LaunchDeck remote connector with `report` and `ask_person`; cooperation, like any model-called tool |
| Approve their actions from the pad | **No** | Their approvals live in their apps; no API exists. Revisit when one is published |
| Start their work from a pad | Yes for Grok Bot and dots | A pad action that calls a Grok Bot routine webhook (URL and key kept as a secret) or delivers a dots MCP event |
| Let them read LaunchDeck or propose a surface | Yes | The same MCP tools, as a remote server |

All of it requires LaunchDeck to be reachable from the internet, which contradicts "bound to 127.0.0.1" and "no cloud". The least-bad shape is an **opt-in remote endpoint through a tunnel the person already trusts** (Claude's MCP tunnels, in research preview, or Tailscale Funnel or Cloudflare Tunnel), Streamable HTTP with OAuth 2.1, explicit `Origin` checks, a separate grant per remote client, and no command or action path beyond what local agents have. Recommendation: do not build it before Watch, Run and the native server are solid and verified; it is a decision for the owner, and the products it targets cannot be used from the EU today.

## 7. The Launchpad experience

**Layouts.** Generated, then editable like any surface:

* *Overview*: the top-level agents as tracks, APPROVE and REJECT large, the side column as the **approval queue** (one pad per pending approval across all agents, oldest at the top, flashing; pressing one focuses it).
* *Agent*: pressing a track for longer opens that agent's page: its subagents and teammates as tracks, its tasks, its command and skill pads.
* The same layouts reduced for the Mini MK3, which shares the grid.

**Gestures.** Press APPROVE: allow once. Hold APPROVE: allow for this session, offered only when the harness offers it, with the scope shown in the app and the hold audited. Double-press REJECT: reject and interrupt, where the harness can interrupt.

**Information without a screen.** When an approval arrives, the Launchpad scrolls the tool name (never its input) across the grid with the text support the device layer already has, then returns to the layout. Categories have their own colour on the APPROVE pad.

**Fewer, better approvals.** LaunchDeck leaves the harness's own policy (auto mode, auto review, sandboxing) in charge of routine requests and is where escalations land. The settings say this plainly; nothing in LaunchDeck approves by itself.

**Personalisation at any moment.** Every generated layout is an ordinary surface: editable in the editor, re-proposable by the agent, shareable as a Pack, and switched by Auto Context when the harness's application comes to the front.

## 8. Security, restated for this study

* A person decides; nothing else does. Timeouts, disconnects, revoked grants and remote failures end in no decision.
* Model text (reports, questions, proposals) is untrusted, length-limited, shown as plain text, never logged, never executed.
* The MCP server runs on stdio and a local socket with user-only permissions; no new TCP port. A future HTTP endpoint enforces `Origin`, OAuth 2.1 and audience-bound tokens, and accepts no passed-through tokens.
* Reading a harness's configuration happens only on Connect, only for that harness, only the files listed; nothing read is stored.
* Agents never run actions, press pads, read secrets, read the foreground or apply edits; proposals go through the artifacts review.
* Run mode launches only what the person pointed to, keeps the harness's own sandbox and policy, and shows on the pad that LaunchDeck owns the process.
* Every grant, connection, held approval and session-scoped approval is in the audit trail, by kind and id, never with content.

## 9. Proposed sequence

The owner wants the native MCP server before 1.0, so agent work comes right after the verification release. It assumes 0.3.1 (Windows green in CI, the desktop suite green, the manual macOS pass) comes first.

| Step | Contents | Can be verified on the owner's Mac |
| --- | --- | --- |
| A, 0.4.0 | The native MCP server (`mcp/`, stdio, `report`, `ask_person`, `end_session`, `get_state`, `explain_pad`, resources); the Claude Code plugin with HTTP hooks for subagents, tasks and notifications; Connect for Claude Code, Claude Desktop and Codex; Agent Bridge v2 token; sub-sessions in the model | Yes: Claude Code is used daily there; Codex can be installed |
| B, 0.5.0 | Run mode with an ACP client and the Codex app-server; OpenCode; per-capability grants; approval categories and session scopes; setup reading and the suggested surface; `propose_surface`, `design-surface`, `publish_state`; the approval queue and gestures on the Launchpad | Yes for the software; the Launchpad part needs a unit |
| C, before 1.0 if the owner decides | The remote endpoint through a tunnel; Grok Bot and dots triggers; MCP Apps previews; the Claude Code channel once it is allowlisted | Partly: Duet and dots cannot be used from the EU today |

## 10. Decisions for the owner

| Decision | Recommendation |
| --- | --- |
| Run mode: may LaunchDeck start and own agent processes? | Yes, opt-in per harness, through ACP and the Codex app-server |
| Change "nothing is discovered or scanned for" to "nothing is read until you connect a harness, then only the listed files" | Yes |
| May agents write states that pads follow? | Yes, scoped to the agent's own declared states |
| May agents run actions or press pads? | No, not in 1.0 |
| A remote endpoint for cloud agents | Not before A and B are verified; then opt-in, through a tunnel the person chooses |
| Name of the LaunchDeck Claude Code marketplace and plugin | A public repository under the owner's account |

## 11. Not verified, and what to check before building on it

* That a custom remote connector on a Claude account reaches Duet.
* Claude Code's channel permission relay with a non-allowlisted plugin (it needs `--dangerously-load-development-channels` and works only interactively today), and the exact `PermissionRequest` input fields in the current version.
* ACP's permission request method and options; `codex mcp add`; Copilot's SDK; Goose's and Cline's hooks.
* Which clients support MCP Tasks, `subscriptions/listen` and elicitation under 2026-07-28 (only Claude Code's list-change support was confirmed).
* Whether a hook may wait for a person as long as the pad needs (Codex documents 600 s by default, Copilot 30 s, Claude Code's limit was not read).
* The status of the official MCP Registry (its README still describes the October 2025 API freeze).

## Sources

Read on 2026-10-23. Pages without a date are marked as such in the research notes, not repeated here.

MCP: [specification 2026-07-28](https://modelcontextprotocol.io/specification/2026-07-28/basic), [changelog](https://modelcontextprotocol.io/specification/2026-07-28/changelog), [MRTR](https://modelcontextprotocol.io/specification/2026-07-28/basic/patterns/mrtr), [deprecated](https://modelcontextprotocol.io/specification/2026-07-28/deprecated), [security best practices](https://modelcontextprotocol.io/specification/2026-07-28/basic/security_best_practices), [extensions](https://modelcontextprotocol.io/docs/extensions/overview), [client matrix](https://modelcontextprotocol.io/extensions/client-matrix), [roadmap](https://modelcontextprotocol.io/development/roadmap) (2026-08-22), [rust-sdk](https://github.com/modelcontextprotocol/rust-sdk), [mcpb](https://github.com/modelcontextprotocol/mcpb), [Bringing MCP 2026-07-28 to Claude](https://claude.com/blog/bringing-mcp-2026-07-28-to-claude) (2026-07-28).

Anthropic: [Claude Duet](https://claude.com/resources/articles/claude-duet) (2026-10-21), [getting started with Duet](https://support.claude.com/en/articles/17631495-get-started-with-claude-duet), [Claude Code hooks](https://code.claude.com/docs/en/hooks), [channels](https://code.claude.com/docs/en/channels-reference), [MCP in Claude Code](https://code.claude.com/docs/en/mcp), [plugins](https://code.claude.com/docs/en/plugins/install), [`.claude` directory](https://code.claude.com/docs/en/claude-directory), [Agent SDK permissions](https://code.claude.com/docs/en/agent-sdk/permissions), [sandboxing](https://www.anthropic.com/engineering/claude-code-sandboxing) (2025-10-20), [auto mode](https://anthropic.com/engineering/claude-code-auto-mode) (2026-03-25).

xAI: [Introducing Grok Bot](https://x.ai/news/introducing-grok-bot) (2026-08-11), [changelog](https://x.ai/changelog/bot) (2026-10-22), [Grok Bot 101](https://x.ai/bot/guides/grok-bot-101), [remote MCP tools](https://docs.x.ai/developers/tools/remote-mcp) (2026-10-10), [connectors](https://docs.x.ai/grok/connectors) (2026-10-10).

OpenAI: [Introducing dots](https://openai.com/index/introducing-dots/) (2026-09-29), [dots controls](https://learn.chatgpt.com/docs/dots/controls), [MCP events](https://developers.openai.com/plugins/build/mcp-events), [custom MCP servers in ChatGPT](https://help.openai.com/en/articles/12584461-connecting-custom-mcp-servers-in-chatgpt), [Codex hooks](https://learn.chatgpt.com/docs/hooks), [Codex config reference](https://learn.chatgpt.com/docs/config-file/config-reference), [Codex app-server](https://learn.chatgpt.com/docs/app-server).

Other harnesses: [OpenCode plugins](https://opencode.ai/docs/plugins/) and [server](https://opencode.ai/docs/server/) (2026-10-20), [Cursor hooks](https://cursor.com/docs/agent/hooks), [Cursor install links](https://cursor.com/docs/context/mcp/install-links), [Copilot hooks](https://docs.github.com/en/copilot/reference/hooks-configuration), [Gemini CLI hooks](https://geminicli.com/docs/hooks/) (2026-04-13), [ACP agents](https://agentclientprotocol.com/overview/agents).

Prior art: [Elgato MCP for Stream Deck](https://www.corsair.com/newsroom/press-release/elgato-brings-ai-control-to-stream-deck-launching-with-nvidia-project-g-assist-2) (2026-05-26), [streamdeck-claude](https://github.com/JulienCr/streamdeck-claude).
