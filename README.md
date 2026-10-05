# LaunchDeck

**LaunchDeck turns MIDI grid controllers into programmable physical interfaces for your computer. Control apps. Automate workflows. Build accessible interfaces. Monitor and orchestrate AI agents.**

It is a runtime, not a music toy: pads that open apps, run actions, switch contexts and show live state, including the state of things outside your computer through plugins you grant network access to, starting with the Novation Launchpad X. A virtual Launchpad is built in, so everything works without hardware.

> Status: **v0.2 prototype.** The runtime, editor, virtual device, Launchpad X adapter, Agent Control, the plugin platform and Auto Context work and are tested, **on Linux**. Nothing has been built or run on macOS or Windows yet, and the Launchpad X path has **not** been exercised on real hardware (see [verified where](#verified-where) and [known limitations](#known-limitations)). What comes next, and how to pick it up on a Mac, is in [NEXT_PHASE.md](NEXT_PHASE.md).

## Flagship: Agent Control

**A physical human control surface for autonomous and semi-autonomous agents.**

Agents run for minutes or hours and stop to ask permission at moments you do not control. Agent Control puts that on a pad you can see across the room and press without finding a window.

| | |
| --- | --- |
| **Observe** | One pad per session, coloured and animated by status. Four agents at once, at a glance, on the Launchpad and in the app. |
| **Intervene** | A flashing APPROVE pad when an agent needs you, with REJECT next to it. Pause, Resume, Retry and Stop light up only when they can work. |
| **Orchestrate** | Start a task on an agent: which agent, what to do, optionally where. |

It is provider-neutral. The core knows about agents, sessions, approvals and commands, not about brands. Tool-specific code lives in adapters, and adapters never touch MIDI, pads or colours. One state in the Rust runtime feeds the desktop app, the virtual Launchpad and a physical Launchpad, so they cannot disagree. A permission request is never approved automatically: a person presses a pad or a button.

Try it in two minutes, no hardware and no agent needed: start the app, open **Agents** and press **Run Agent Demo**. The five demo flows (observe, approve, intervene, fail and retry, connect your own program) are in [docs/agent-control/overview.md](docs/agent-control/overview.md#try-it-in-two-minutes).

| Way to connect an agent | For | Controls |
| --- | --- | --- |
| Built-in demo agents | Trying it out | Everything, simulated |
| Hooks (Claude Code verified, Codex written from its docs and not run) | Tools that call a hook URL at lifecycle points | Observe, approve and reject. Nothing else is possible through hooks. |
| Agent Bridge (`ws://127.0.0.1:47821`) | Any local program | Whatever the program declares. [Example client](examples/generic-agent/) |

Agents you connect start read-only. You grant control per agent, in the app. See [what is built and what is not](docs/agent-control/overview.md#known-limitations).

## What works today

| Area | State |
| --- | --- |
| Virtual Launchpad X (simulator, no hardware needed) | Working. Same interface as real hardware. |
| Launchpad X over USB (discovery, Programmer mode, press/release, RGB LEDs, clear) | Implemented and byte-tested. Untested on a physical unit. |
| Surfaces, pages, controls, spans, per-state looks | Working, persisted as versioned JSON. |
| Live / Edit modes, inspector | Working. |
| Feedback editor: make a pad follow a state or react to an event from the inspector, with one searchable picker for every provider, numbered rules you can reorder, looks you can create in place, and a live explanation of why the pad looks as it does | Working. Tested against the real Counter plugin and in the real desktop app. [Guide](docs/feedback-editor.md) |
| Action Library: every built-in, Agent Control and installed-plugin action in one searchable panel, assigned by selecting or dragging, configured in a form generated from a declared schema, with availability, replacement warnings and test runs | Working. Tested end to end with a real compiled plugin. [Guide](docs/actions/overview.md) |
| Agent Control: agents, sessions, approvals, commands, surface, Agents section in the app | Working. Demo agents, Agent Bridge, hook adapters. |
| Starter surfaces: Agent Control, Development, Productivity, Accessibility, Smart Home | Included. |
| System plugin, Demo plugin, Agent Control plugin | Working. |
| Plugin SDK: external plugins in a WebAssembly sandbox, discovered from a folder, with typed actions, events on the existing bus, states, settings, logs, enable and disable | Working. AssemblyScript SDK and examples (Hello, Counter, Channels, a C one) in this repository. [Guide](docs/plugin-sdk/README.md) |
| Plugin capabilities: a plugin asks for network access to exact addresses, you allow or deny each one on the Plugins screen, and LaunchDeck sends the requests itself. Secrets are kept in the system credential store, never in a file and never handed to the plugin. Actions show as pending while a request is out | Working. Tested with a real local server, and in the real desktop app. [Permissions](docs/plugins/permissions.md), [capabilities](docs/plugin-sdk/capabilities.md) |
| Home Assistant example plugin: toggle a light from a pad and see on the pad whether it is on | Working against a stand-in server that implements the documented REST endpoints. **Not run against a real Home Assistant.** [Guide](examples/plugins/home-assistant/README.md) |
| Local WebSocket for external events and the Agent Bridge (`127.0.0.1` only) | Working. |
| Hook endpoint for Claude Code and Codex (`127.0.0.1` only, token, off by default) | Working for Claude Code (verified live). Codex untested. |
| System tray | Working where the OS supports it. |
| Auto Context: the surface follows the application in front, by ordered rules (application to surface and page), with an explicit fallback, a manual override that is never fought, and a panel that says which rule matched and why | Working on Linux/X11, tested in the real desktop app with real windows. macOS and Windows observers are written and **have not been compiled or run**. Not available on Wayland. [Guide](docs/context-engine.md) |
| Marketplace, MCP, Agent Pack loading | Documented as roadmap or manifest only. |

## Verified where

Everything in the table above was built and tested in one Linux environment. This is what that does and does not cover:

| | Linux | macOS | Windows |
| --- | --- | --- | --- |
| Builds, unit and integration tests (Rust and TypeScript) | Yes | **Never built** | **Never built** |
| The real desktop app, driven through its window ([tools/desktop-e2e](tools/desktop-e2e/README.md)) | Yes, 218 checks | No (the harness is Linux only) | No |
| Secrets in the system credential store | Yes, a real Secret Service keyring | Keychain: not run | Credential Manager: not run |
| Auto Context observer | Yes, X11 with a window manager. Wayland: not available | Written, **not compiled** | Written, **not compiled** |
| Keyboard shortcuts and media keys (`enigo`) | Behind a fake in tests; not run against a real desktop | Not run | Not run |
| Launchpad X over USB | Byte-tested, **no physical unit** | Same | Same |
| Home Assistant example | Against a stand-in server, **not a real installation** | Same | Same |

There is no continuous integration yet. Four Rust tests compare against fixtures recorded on Linux and are expected to fail elsewhere; they are listed, with the fix, in [NEXT_PHASE.md](NEXT_PHASE.md#3-tests-expected-to-fail-on-macos).

## Run it

Requirements: Node 20+, Rust stable, and the [Tauri 2 prerequisites](https://v2.tauri.app/start/prerequisites/) for your OS. On Linux this means WebKitGTK 4.1, GTK 3, `libayatana-appindicator3` and ALSA headers (`libasound2-dev`).

```bash
npm install
npm run tauri dev        # desktop app with the full Rust runtime
```

Other useful commands:

```bash
npm run dev              # UI only in a browser, with a simulated runtime (for UI work)
npm run typecheck
npm run lint
npm test                 # vitest: schemas, contract with Rust, actions, store selectors
npm run build            # type check + production frontend build
npm run rust:fmt         # cargo fmt --check
npm run rust:check       # cargo check
npm run rust:test        # cargo test
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
npm run tauri build      # production bundle
cargo run --manifest-path src-tauri/Cargo.toml --example foreground   # print which application is in front, as Auto Context sees it
npm run plugins:build    # rebuild the example plugin modules (Hello, Counter, Channels, Home Assistant, C Ping)
python3 tools/desktop-e2e/run.py   # drive the real built app (Linux, slow; see tools/desktop-e2e/README.md)
sh scripts/test-secret-service.sh  # the secret store against a real, private Secret Service keyring (Linux)
LAUNCHDECK_PLUGINS="$PWD/examples/plugins" npm run tauri dev   # run with the example plugins
```

The files under `examples/*.json` are contract fixtures: Rust generates them, a Rust test fails when one is stale, and the TypeScript tests parse them. After changing what Rust sends to the interface, regenerate the one concerned from `src-tauri/` with `cargo run --example dump_workspace` (or `dump_agents`, `dump_plugins`, `dump_catalog`, `dump_observables`, `dump_context`) redirected to the matching file; each test's failure message names the command.

### Simulator mode (no hardware)

The app always starts with a **Virtual Launchpad X** connected. Open **Agent Control** (or any other surface), stay in **Live** mode and click or hold the pads. They go through exactly the same input path as a physical device: virtual pad press, `DeviceInput`, surface runtime, action engine, state engine, renderer, frame. Press **Run Agent Demo** to start four simulated agents and watch their pads move through running, tool use, waiting for approval, paused, failed and completed. When APPROVE flashes, press it.

`npm run dev` (browser only) uses a small in-memory stand-in for the runtime so the interface can be developed without Tauri. It shows a banner and does not run real actions.

### Connect a Launchpad X

1. Plug the Launchpad X in over USB. No driver is needed on macOS, Windows 10+ or Linux (class compliant).
2. Open **Settings** and click **Rescan**. A `Launchpad X` entry appears.
3. Click **Connect**. LaunchDeck switches the device to Programmer mode and mirrors the active page onto the pads. The sidebar shows **Launchpad X / Connected**.
4. Close other software that holds the Launchpad's MIDI ports (a DAW, Novation Components).

Disconnecting, or unplugging, returns the app to the simulator and the UI keeps working. Details and the exact messages sent are in [docs/devices/launchpad-x.md](docs/devices/launchpad-x.md).

## Connect your own agent

Any program that can open a WebSocket can appear in Agent Control. The example client registers, creates a session, reports progress, asks for approval, receives your decision and finishes:

```bash
node examples/generic-agent/agent.ts      # Node 22.18+, nothing to install
```

It appears read-only. Open **Agents**, allow control for it, and press APPROVE when it asks. The protocol is versioned and documented in [docs/agent-control/bridge-protocol.md](docs/agent-control/bridge-protocol.md), and the same model is available to Rust adapters through the [Adapter SDK](docs/agent-control/adapter-sdk.md).

## Send plain events from other tools

The same socket also takes generic events that change how pads look:

```json
{"type":"event","source":"build","event":"run.started"}
{"type":"event","source":"build","event":"run.failed","payload":{"code":2}}
```

Make a control react to an event with an event rule: **Feedback, Add feedback, An event** in the inspector (see [docs/feedback-editor.md](docs/feedback-editor.md)), or by writing it in the `feedback` list of the surface file, as described in [docs/surfaces.md](docs/surfaces.md) and [docs/state-and-feedback.md](docs/state-and-feedback.md). Try it with **Settings, External events, Send test event**, or with any WebSocket client:

```bash
npx wscat -c ws://127.0.0.1:47821 -x '{"type":"event","source":"build","event":"run.started"}'
```

Events can change how pads look. They cannot execute anything. The sources `runtime`, `device` and `agent-control`, and the id of every installed plugin, are reserved and cannot be used by clients.

## Security model in short

* **Shell commands and HTTP requests are privileged.** They are always shown verbatim in the inspector, carry a *Privileged* badge, and only run from a control the user configured in this app.
* **Nothing arriving over the network can start an action.** The WebSocket accepts `event`, `ping` and the `agent.*` bridge messages only, binds to `127.0.0.1`, checks the `Origin` header, bounds message size, rate and connection count, and does not let clients impersonate the runtime, devices or Agent Control.
* **Agents are read-only until you say otherwise.** Registering never grants control, and control is granted per agent.
* **A permission request is never approved automatically.** Timeouts, hang-ups and errors all end with no decision. Only a person presses APPROVE.
* **Prompts and sensitive payloads are not logged.** The audit log records what happened and who decided, not what the agent was asked to do.
* **No silent installs, no telemetry, no cloud.** Nothing is downloaded. A plugin is a folder you put in the plugins folder.
* **Installed plugins are sandboxed.** They run as WebAssembly with no filesystem, shell, sockets, MIDI or WASI, a handful of host functions only, and limits on CPU time, memory and message size. They can publish only the events their manifest declares, never as someone else, and a crash stops only that plugin. They never see MIDI notes or pad positions. See [docs/plugin-sdk/security.md](docs/plugin-sdk/security.md), including what this does not protect against.
* **A plugin reaches the network only through LaunchDeck, and only where you allowed.** It asks for exact addresses (scheme, host, port); nothing is granted by installing or updating; a redirect or a DNS answer cannot lead somewhere you did not approve; and local addresses are shown as local. Tokens are kept in the system credential store and nowhere else (without one, secrets are unavailable rather than written to a file), sent to one address only, and never given to the plugin. Decisions are in a file outside the workspace, so a shared surface carries none. See [docs/plugins/permissions.md](docs/plugins/permissions.md).
* Built-in plugins declare permissions (`network`, `filesystem`, `shell`, `automation`, `accessibility`, `midi`) that the Plugins screen displays; they are trusted first-party code, not gated. See [docs/plugins.md](docs/plugins.md) and [docs/agent-control/security.md](docs/agent-control/security.md).

## Project map

```
src/                  React + TypeScript editor (Zustand for UI state only, Zod at the IPC boundary)
  features/           one folder per part of the interface: surfaces, editor (inspector), library (Action Library),
                      feedback (feedback editor), plugins, agents, context (Auto Context), devices
  lib/                schema.ts (Zod, every payload), ipc.ts (commands and events), preview.ts (the browser-only stand-in)
src-tauri/src/
  core/               model, runtime (surface runtime, state engine, event routing), validation
  state/              State Hub: the current value of every state, the rule evaluator and its index
  devices/            SurfaceDevice trait, capabilities, virtual device, Launchpad X (the only MIDI code)
  renderer/           semantic visual state to capability-aware frames
  actions/            Action Library: unified registry, definitions, parameter schema, action engine, Navigation, Shell and HTTP providers
  plugins/            plugin runtime, System plugin, Demo plugin
  plugins/external/   Plugin SDK host: manifest, discovery, WebAssembly sandbox, lifecycle,
                      capabilities (requests and grants), network broker, secret stores
  agents/             Agent Control: model, service, adapters, bridge, audit, pack manifest
  context/            Auto Context: foreground observers (one per system), application identity, ordered rules, the Auto / Manual Override engine
  events/             event bus, localhost WebSocket
  surfaces/           starter surfaces, including Agent Control
  persistence/        versioned JSON store (workspace schema 4), migrations from schema 2 and 3
src-tauri/tests/      integration tests: real plugins, the network broker, the secret store, Agent Control, Auto Context, contracts
src-tauri/examples/   dump_* (regenerate the contract fixtures), foreground (which application is in front), headless, adapter_sdk
sdk/assemblyscript/   the plugin SDK for AssemblyScript
docs/                 product, architecture, surfaces, state and feedback, context engine, actions, plugins, plugin-sdk,
                      security, device notes, roadmap, agent-control
scripts/              build-example-plugins.sh, test-secret-service.sh
tools/desktop-e2e/    checks that drive the real desktop app through WebDriver (Linux, run on request)
examples/             default-workspace.json, agents-snapshot.json, plugins-overview.json, action-catalog.json, observables.json
                      and context-status.json (generated by Rust and parsed by the TypeScript tests), feedback-editor-*.json
                      (controls exactly as the feedback editor saves them), generic-agent/ (bridge client),
                      agent-pack/ (manifest example), plugins/ (Hello, Counter, Channels, Home Assistant and a C plugin,
                      built outside the core)
NEXT_PHASE.md         what to do next and how to continue on a Mac
```

## Documentation

* [Next phase](NEXT_PHASE.md): what is unverified, what to do first on macOS, and the milestone after that
* [Product](docs/product.md)
* [Architecture](docs/architecture.md)
* [Surfaces](docs/surfaces.md)
* State and feedback: [the model](docs/state-and-feedback.md), [observables and the catalog](docs/observables.md), [the feedback editor](docs/feedback-editor.md)
* [Context Engine](docs/context-engine.md): the surface follows the application in front
* [Plugins](docs/plugins.md) and [plugin permissions](docs/plugins/permissions.md)
* Security: [where secrets are kept](docs/security/secrets.md)
* Actions: [overview](docs/actions/overview.md), [definitions](docs/actions/definitions.md), [parameters](docs/actions/parameters.md), [providers](docs/actions/providers.md), [availability](docs/actions/availability.md), [Action Library](docs/actions/action-library.md)
* Plugin SDK: [overview](docs/plugin-sdk/README.md), [getting started](docs/plugin-sdk/getting-started.md), [manifest](docs/plugin-sdk/manifest.md), [capabilities](docs/plugin-sdk/capabilities.md), [network](docs/plugin-sdk/network.md), [secrets](docs/plugin-sdk/secrets.md), [AssemblyScript SDK](docs/plugin-sdk/assemblyscript.md), [protocol](docs/plugin-sdk/protocol.md), [security](docs/plugin-sdk/security.md), [packaging](docs/plugin-sdk/packaging.md), [testing](docs/plugin-sdk/testing.md), [versioning](docs/plugin-sdk/versioning.md)
* [Launchpad X notes](docs/devices/launchpad-x.md)
* Agent Control: [overview](docs/agent-control/overview.md), [domain model](docs/agent-control/domain-model.md), [adapter SDK](docs/agent-control/adapter-sdk.md), [bridge protocol](docs/agent-control/bridge-protocol.md), [surfaces](docs/agent-control/surfaces.md), [security](docs/agent-control/security.md), [MCP (optional, design only)](docs/agent-control/mcp.md)
* [Marketplace (roadmap)](docs/roadmap/marketplace.md)
* [MCP at platform level (roadmap)](docs/roadmap/mcp.md)
* [Desktop end-to-end checks](tools/desktop-e2e/README.md)
* Plans, kept as the record of the research and decisions behind each milestone, in the order they were built: [implementation](IMPLEMENTATION_PLAN.md), [Agent Control](AGENT_CONTROL_PLAN.md), [Plugin SDK](PLUGIN_SDK_PLAN.md), [Action Library](ACTION_LIBRARY_PLAN.md), [state and feedback](STATE_FEEDBACK_PLAN.md), [feedback editor](FEEDBACK_EDITOR_PLAN.md), [plugin capabilities](PLUGIN_CAPABILITIES_PLAN.md) and [Context Engine](CONTEXT_ENGINE_PLAN.md). The `docs/` pages describe what is built; where a plan and a page disagree, the page is right.

## Known limitations

* **Built and tested on Linux only, and there is no CI.** macOS and Windows have never compiled this code. Two files exist only for those systems and have never been through a compiler (`src-tauri/src/context/observer/macos.rs` and `windows.rs`), and four Rust tests are tied to fixtures recorded on Linux. See [verified where](#verified-where) and [NEXT_PHASE.md](NEXT_PHASE.md).
* **Not tested on a physical Launchpad X.** The environment this prototype was built in had no MIDI hardware. SysEx and note mappings follow Novation's Programmer's Reference Manual and are covered by byte-level tests, but real-world behaviour (port names on your OS, timing) is unconfirmed. Agent Control sends the same frames to the virtual and the physical device and is tested on the virtual one.
* **Auxiliary buttons are experimental.** The manual shows the top row, side column and logo in diagrams only. The controller numbers used (top CC 91 to 98, side CC 89 down to 19, logo 99) follow the layout commonly documented for the Launchpad MK3 family and are unverified.
* **Pulse and flash are software animations on the Launchpad X.** Colour palette indices are not invented, so static colours use RGB and animations are rendered by LaunchDeck at 25 Hz. Hardware pulse/flash encoders exist and are tested but unused. Urgent agent states rely on this animation, so it is the first thing to check on a real unit.
* **Hook adapters can only approve and reject.** Claude Code and Codex hooks cannot pause, resume, cancel, retry or instruct a session, so those controls stay unavailable for them. Codex has not been run. More in [docs/agent-control/overview.md](docs/agent-control/overview.md#known-limitations).
* **The Agent Bridge is not authenticated.** Any local process can register an agent, read-only. Control still needs your explicit grant.
* **Agent state is not persisted.** Agents and sessions are rebuilt as adapters reconnect. Settings, control grants and the audit trail are kept.
* Keyboard shortcuts and volume keys use OS automation (`enigo`). On macOS this needs Accessibility permission, and on Linux Wayland sessions may block synthetic input.
* **Auto Context is verified on Linux/X11 only.** The macOS and Windows observers were written without being able to build them here; on Wayland it is not available at all (no standard way to learn the focused application), and LaunchDeck says so and works by hand. Rules match the application only: no window titles, projects or conditions. See [docs/context-engine.md](docs/context-engine.md#platform-support).
* **The only plugin capability is HTTP.** A plugin can make HTTP and HTTPS requests to addresses you granted, with text bodies, one at a time. There are no files, no shell, no WebSocket or other long-lived connection, no MQTT, no OAuth flow and no proxy support. State from an outside system is therefore **polled** (the Home Assistant example every 5 seconds), not pushed.
* **The Home Assistant example was checked against a stand-in, not a real installation**, and switches lights and switches on and off only.
* **Secrets need a system credential store.** Keychain on macOS, Credential Manager on Windows, a Secret Service keyring on Linux. Without one, a plugin's secrets are unavailable and the app says why; there is no file fallback. Only the Linux path has been run here. A secret is not removed when its plugin's folder is deleted. See [docs/security/secrets.md](docs/security/secrets.md).
* **Plugins are not signed.** There is no signature, checksum check or revocation, and no marketplace. Install only plugins you trust; the sandbox protects LaunchDeck from the plugin, not you from a misleading one.
* **Action settings use a closed set of field kinds.** Text, number, on/off, choice, and a surface or page. There is no list, colour or file picker kind, and a plugin cannot ship its own form. Anything else is a text field until the schema grows.
* **A feedback rule is one comparison on one state, or one event.** There is no AND / OR, no timer that marks a state stale, no scripting and no aggregate state ("anything needs me"). The editor reports a rule that can never be reached only when it can prove it. See [docs/feedback-editor.md](docs/feedback-editor.md#not-built).
* **Library search is word-and-typo matching.** It covers names, keywords, categories, sources and descriptions. It does not understand meaning ("make it louder" will not find Volume up unless a keyword says so).
* **Only AssemblyScript has an SDK.** Any language that emits a core WebAssembly module with the documented exports can write a plugin (a C one is included), but a Rust SDK is not built, and Rust-built plugins have not been tested here.
* Changing the WebSocket port or turning it off takes effect on the next launch.
* The app is unsigned. First-run OS warnings are expected.

## License

No license has been chosen yet.
