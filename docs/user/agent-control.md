# Agent Control: overview

> A physical human control surface for autonomous and semi-autonomous agents.

Agent Control is the first flagship use case built on the LaunchDeck platform. LaunchDeck itself stays a general runtime for programmable physical interfaces: surfaces, plugins, devices and an event bus. Agent Control is a module on top of it. It does not change what LaunchDeck is, and nothing in the platform depends on AI.

It does three things.

| | What it means | Where you see it |
| --- | --- | --- |
| **Observe** | Know what every agent is doing without reading a terminal. | One pad per session, coloured by status, on the Launchpad and in the Agents view. |
| **Intervene** | Step in with one press when an agent needs you. | Approve, Reject, Pause, Resume, Retry and Stop, each lit only when it can work. |
| **Orchestrate** | Start a task on an agent. | Task pads and the **New task** form: which agent, what to do, optionally where. |

## The idea in one picture

```
 Agents and harnesses                 LaunchDeck (Rust runtime)                         Views
 ---------------------   adapters   ---------------------------------   events    --------------------
 Demo agents (built in) ---------->                                   -------->   Virtual Launchpad X
 Hook-capable tools     ---------->    AgentControl                   -------->   Physical Launchpad X
   (HTTP, localhost)                   one state: agents, sessions,   -------->   Agents view (React)
 Any local process      ---------->    approvals, slots, focus,       -------->   Notifications
   (Agent Bridge, WS)   <----------    commands, audit
                         commands
```

Three rules hold the design together.

1. **One state, three views.** `AgentControl` in Rust is the only place agent state lives. The desktop UI, the virtual Launchpad and a physical Launchpad are all derived from it through the same state hub, feedback rules and renderer. They cannot disagree, because there is no second copy to disagree with.
2. **Control sessions and agents, not brands.** The domain model has no vendor in it. Statuses, capabilities and commands are normalized. A tool-specific adapter translates to them and nothing else.
3. **Adapters know nothing about hardware.** An adapter reports what an agent is doing and carries out commands the person issued. It never sees a pad, a colour or a MIDI message. If Agent Control ever needs a MIDI change, the abstraction is wrong.

## What is reused from the platform

Agent Control adds a module and a surface. It does not fork the platform.

| Platform piece | How Agent Control uses it |
| --- | --- |
| Event bus | Every agent event is an ordinary event with `source: "agent-control"`. |
| Feedback rules | Pads change look by reading the states Agent Control keeps in the state hub (`slot.status`, `slot.focused`, `command.state`). There is no agent-specific code in the device layer or the hub. |
| Renderer | Adapts the same visuals to what the device can show (native pulse and flash where a device has them, synthesised animation on the physical Launchpad X, or brightness only). |
| Plugins | Pad presses call `agent-control` plugin actions, like any other plugin. |
| Local WebSocket | The Agent Bridge is a versioned message family on the existing localhost socket. |
| Persistence | Settings and access grants in `agents.json`, an append-only `audit.jsonl`, written the same atomic way as the workspace. |
| Virtual device | The acceptance flows run on it; a physical device receives identical frames. |

## Three ways to connect an agent

| | Demo | Hooks | Agent Bridge |
| --- | --- | --- | --- |
| For | Trying it out, tests, demos | Tools that call a hook URL or command at lifecycle points | Anything else that can open a local WebSocket |
| Starts | Always available | Off until you enable it | On with the local WebSocket |
| Observe | Everything, simulated | Sessions, tool use, permission requests, failures | Whatever the client reports |
| Intervene | Everything, simulated | Approve and reject permission requests | Whatever capabilities the client declares |
| Trust | Control granted | Read-only until you grant control | Read-only until you grant control |

Real adapters ship for two tools, and the Adapters tab states exactly what each one can and cannot do. Claude Code was verified against a live installation. Codex was written from its published documentation and has not been run, and says so. Details, with dates and sources, are in [adapter-sdk.md](../agent-control/adapter-sdk.md#real-adapters-what-is-observable-and-controllable).

Neither real adapter can pause, resume, cancel or retry a session, because the hook mechanisms do not allow it. Those controls stay visibly unavailable instead of being faked.

## Try it in two minutes

No hardware and no agent are needed.

1. Start LaunchDeck and open **Agents**, or press **Run the Agent Demo** on Home.
2. Four simulated agents start side by side. The pads in the top row of the Agent Control surface and the cards in the app show the same states.
3. One agent asks for approval. APPROVE flashes. Press it on the virtual Launchpad (or **Approve** in the app) and the agent resumes.

The five acceptance flows, and how to reproduce each, are listed below. Each is covered by an automated test that drives the real runtime and checks the virtual Launchpad's frame.

| Flow | What to do | What you should see |
| --- | --- | --- |
| 1. Observation | Run the demo and watch. | Four agents move through different states at the same time: pads and cards agree. |
| 2. Approval | Wait for the flashing APPROVE pad, then press it. | The agent resumes and the approval clears everywhere. REJECT was available throughout. |
| 3. Intervention | Select a running agent and press Pause, then Resume. | The pad turns orange, then green again. Commands that cannot work stay dim. |
| 4. Failure and retry | Select the agent that fails and press Retry. | Red flash, then a new run starts. |
| 5. Generic integration | Run `node examples/generic-agent/agent.ts`, allow control, approve. | An external process appears, asks, and finishes when you decide. |

## Known limitations

Honest edges of what is built, each also noted where it matters.

* **Hook adapters cannot pause, resume, cancel, retry or instruct.** The harnesses' hook mechanisms only allow observing and answering permission requests. Real control of Codex would need `codex app-server`, which is not implemented.
* **Codex has never been run.** The adapter follows OpenAI's published hook documentation and is marked untested in the app. Claude Code was verified live (version 2.1.289, 2026-10-04).
* **A permission request that arrives while an agent is read-only cannot be answered from LaunchDeck later.** The harness is not held, so it asks you in its own prompt. Grant control before the work starts.
* **The bridge is not authenticated.** Any local process can register an agent, read-only. See [security.md](../agent-control/security.md#known-gaps).
* **Hook sessions started before the hooks were configured** appear at their next event, and `SessionStart` is not delivered over HTTP in non-interactive mode.
* **Eight pads.** More than eight tracks stay fully visible in the app but only eight have a pad.
* **The surface is designed for an 8x8 grid.** Smaller devices drop the pads they do not have.
* **No aggregate "something needs me" light** for custom surfaces yet.
* **Notifications are in-app**, plus a best-effort system notification when the window is in the background. There is no OS-level notification plugin.
* **Agent state is not persisted.** Agents and sessions are rebuilt as adapters reconnect; only settings, access grants and the audit trail are kept.
* **A half-open connection can briefly block re-registration** (`agent_in_use`) until the old socket is noticed as closed.

## Not in scope

Agent Control observes and steers agents that already exist. It is not, and will not become, any of these: cloud agent hosting, model inference, an LLM gateway, a prompt marketplace, workflow graphs, multi-user remote control, accounts, billing, cloud sync, agent memory, a marketplace backend or a scheduler. It never approves anything by itself.

## Where things are

| Path | What it holds |
| --- | --- |
| `src-tauri/src/agents/` | The module: `model`, `visual`, `adapter`, `service`, `bridge`, `adapters/`, `audit`, `settings`, `notify`, `plugin`, `view`, `demo`. |
| `src-tauri/src/surfaces/agent_control.rs` | The Agent Control surface definition. |
| `src/features/agents/` | The Agents section of the desktop UI. |
| `src/lib/agents.ts` | Zod schemas and helpers for what Rust sends. |
| `examples/generic-agent/` | A dependency-free Agent Bridge client. |
| `examples/agents-snapshot.json` | A snapshot captured from the runtime; the contract fixture for the UI. |

## Documents

* [domain-model.md](../agent-control/domain-model.md): entities, statuses, capabilities, commands, events.
* [adapter-sdk.md](../agent-control/adapter-sdk.md): writing an adapter, in Rust or over the bridge, and the real adapters.
* [bridge-protocol.md](../agent-control/bridge-protocol.md): the versioned wire protocol.
* [surfaces.md](../agent-control/surfaces.md): how state becomes light, and how to build your own surface.
* [security.md](../agent-control/security.md): the trust model and what is logged.
* [mcp.md](../roadmap/mcp-bridge.md): the optional MCP bridge (design only).
