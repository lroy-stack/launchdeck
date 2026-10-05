# Product

## Positioning

LaunchDeck is a **programmable physical interface runtime**. It turns a MIDI grid controller into a surface that controls software, shows live state and adapts to context. The first supported device is the Novation Launchpad X.

LaunchDeck turns MIDI grid controllers into programmable physical interfaces for your computer. Control apps. Automate workflows. Build accessible interfaces. Monitor and orchestrate AI agents.

A Launchpad becomes something like a keyboard for intent: a pad does not mean "note 44", it means "approve this agent run" or "start the build".

## Flagship use case: Agent Control

Agent Control is the first vertical built on the platform: **a physical human control surface for autonomous and semi-autonomous agents.** It observes (which agents are running, which need you), intervenes (approve, reject, pause, resume, retry, stop) and orchestrates (start a task on an agent). It is a module on the platform, not a change to it. The runtime, the device layer and the editor know nothing about AI, and the other surfaces work the same without it. See [agent-control/overview.md](agent-control/overview.md).

## Who it is for

* **People who work with AI agents.** A physical glance tells you which agent is running, which is waiting for you and which failed, and a pad approves, rejects or stops it. This is the flagship use case.
* **Developers.** Build, test, git and deploy shortcuts with feedback on the pad itself.
* **People who want fewer, bigger, easier targets.** The Accessibility surface uses 2x2 high-contrast pads. It is a usability aid, not a medical device.
* **Tinkerers.** Smart-home webhooks, plugins that talk to a service such as Home Assistant, and anything reachable from a URL or a local command.

## Principles

1. **Physical first.** The surface is the product. The desktop app is where you design it and where you see it when the hardware is not there.
2. **Semantic, not MIDI.** Everything above the device adapter talks in controls, actions, states and events.
3. **Graceful degradation.** Devices declare capabilities. Missing capabilities degrade (a device without RGB gets luminance, without pulse gets a software pulse, with a smaller grid loses controls outside it) instead of failing.
4. **The simulator is a first-class device.** The Virtual Launchpad X implements the same interface, so everything works, and is tested, without hardware.
5. **Safe by construction.** Privileged actions are explicit, local, visible. Remote input can only change state.
6. **Local and boring.** JSON files on disk. No account, no cloud, no telemetry.

## Core concepts

| Concept | Meaning |
| --- | --- |
| Device | Something with controls and LEDs, described by capabilities. |
| Surface | A named layout for a device, made of pages. |
| Page | One screenful of controls inside a surface. |
| Control | A position on the surface with a look per state and actions. |
| Action | A semantic thing to do: keyboard shortcut, open URL, shell command, plugin action... |
| Look | A named appearance of a control (idle, running, success...). Saved in the control's `states` map. |
| State | How something is now, reported by a provider and held by the state hub (`known`, `unknown` or `unavailable`). A rule reads it. |
| Event | Something that happened: a pad press, an action result, a plugin state change, an external message. |
| Feedback rule | A rule on a control that picks its look: a state rule reads a provider's value, an event rule shows a look for a while. First true state rule wins. See [state-and-feedback](state-and-feedback.md). |
| Plugin | Provides actions, publishes events and reports states. Never sees MIDI. Built in (native) or installed (a WebAssembly module in a sandbox). |
| Capability | What a device can do (grid size, RGB, native animation), or what an installed plugin is allowed to do beyond its sandbox. The only plugin capability is network access to addresses the person granted. See [plugins/permissions.md](plugins/permissions.md). |
| Secret | A value such as an API token that LaunchDeck keeps in the system credential store and adds to a plugin's request itself. The plugin never sees it. See [security/secrets.md](security/secrets.md). |
| Context | Which application is in front. Auto Context switches the surface by ordered rules (application to surface and page) and never fights a surface picked by hand. See [context-engine.md](context-engine.md). |

## What the prototype demonstrates

* The full loop on a virtual device: press, action, state, render.
* Agent Control: several agents and sessions at once on one surface, an approval that flashes until a person decides it, pause, resume and retry that light up only when they can work, driven by simulated agents, by Claude Code hooks (verified against a live install), by Codex hooks (written from its documentation, not run), or by any program over the local Agent Bridge.
* One state behind three views: the desktop app, the virtual Launchpad and a physical Launchpad are all derived from the same Rust state.
* A usable editor: Live and Edit modes, an Action Library that lists everything a pad can do, an inspector that draws each action's settings, pages, spans, per-state looks, and a feedback editor that makes a pad follow a state or react to an event without writing JSON.
* A plugin platform: someone outside the project ships a folder with a manifest and a WebAssembly module, and it is discovered, sandboxed, listed in the library and shown on pads, with no recompiling. A plugin can reach an outside service only through LaunchDeck, only at addresses the person allowed, with tokens it never sees (the Home Assistant example).
* A surface that follows the application in front, with rules the person wrote.
* The same runtime driving a real Launchpad X when one is plugged in.

All of it was built and tested on Linux in earlier milestones. On macOS it is built, and the Rust (715 passed, 0 failed, 2 ignored) and TypeScript (309) tests pass on macOS 15.4.1, Apple M2, Rust 1.97.0; the Auto Context observer, Keychain, CoreMIDI and `enigo` compile there but have not been seen running, and the app window has not been checked by hand. Windows has never been built, and nothing has been run with a physical Launchpad. What has and has not been verified, per system, is in the [README](../README.md#verified-where).

## Out of scope

Accounts, cloud sync, a marketplace backend, plugin signing, telemetry, a full MCP server, other Launchpad models and other controllers, mobile, and a database. The roadmap documents describe how marketplace packages and an MCP adapter would fit.

Agent Control adds its own boundaries: it does not host agents, run models, proxy LLM calls, schedule work, keep agent memory or build workflow graphs, and it never approves anything by itself. See [agent-control/overview.md](agent-control/overview.md#not-in-scope).
