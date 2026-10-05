# Architecture

**Rust owns the runtime. React only edits, visualises and configures.** If the window is closed, the tray keeps the runtime alive; if the UI crashes, pads still work.

```
React UI (Zustand: UI state only, Zod validates every payload)
   |  Tauri commands (UI to Rust)   |  Tauri events (Rust to UI)
===============================================================================
LaunchDeck Core (Rust)
   Foreground observer --> Context Engine
                                | activate (the same path a person's click takes)
                                v
   Device Manager --- Surface Runtime --- Action Engine --- Plugin Runtime --- Plugin SDK host
        |                  |                   |                 |            (WebAssembly sandbox,
        |             State Engine <------ Event Bus <-----------+             network broker,
        |                  ^                   ^                 |             secret store)
        |              State Hub <-------------+-----------------+
        |                  |                   |
        |              Renderer                |
        v                  |                   |
   SurfaceDevice  <--- SurfaceFrame       External events (WebSocket, 127.0.0.1)
   (VirtualLaunchpadX | LaunchpadX)        Persistence (versioned JSON)
```

Providers (the runtime, Agent Control, installed plugins) write *states* to the State Hub and publish *events* on the Event Bus; the State Engine reads both to decide each control's look.

## Layers

| Layer | Module | Responsibility |
| --- | --- | --- |
| Device Manager | `devices/manager.rs` | Registers devices, connects and disconnects them, fans frames out to every connected device, drops a device that fails to render, rescans hardware every 3 s. |
| Device abstraction | `devices/mod.rs` | `SurfaceDevice` trait, `DeviceCapabilities`, `DeviceInput`, `SurfaceFrame`. |
| Virtual device | `devices/virtual_device.rs` | `VirtualLaunchpadX`, the simulator. It implements the same trait and is the device the UI drives. |
| Launchpad X | `devices/launchpad_x.rs` | The **only** file with MIDI bytes. SysEx encoding, note and CC decoding, port classification, `midir` I/O. |
| Surface Runtime | `core/runtime/` (`navigation.rs`, `input.rs`) | Active surface and page, control lookup (span aware), navigation, Live and Edit modes, long press. |
| State Hub | `state/` | The current value of every state a provider offers (`known`, `unknown`, `unavailable`), addressed by provider, definition and arguments. No history, nothing saved, not a bus. See [state-and-feedback.md](../state-and-feedback.md). |
| State Engine | `core/runtime/feedback.rs`, `core/runtime/actions.rs`, `state/eval.rs`, `state/index.rs` | A control's look, keyed `surface/page/control`: its state rules evaluated from the hub (first true rule wins), under temporary looks (event rules, the `pending` and `failed` looks of the control's own action, the automatic running/success/error of a pad without rules, token-guarded expiry). The runtime keeps the set of actions in flight and publishes it in its state. An index re-evaluates only the controls that read what changed. |
| Event Bus | `events/mod.rs` | Tokio broadcast channel of semantic `Event { source, type, payload, ts }`. |
| Action Registry | `actions/registry.rs`, `actions/params.rs`, `actions/definition.rs` | The Action Library's source of truth. A view over the plugin registry: builds the catalog (definitions, providers, availability), and its `prepare` is the one gate every assignment passes before it is saved or run. See [actions](../actions/overview.md). |
| Action Engine | `actions/mod.rs` | Executes a prepared `Assignment` (`{provider, action, args}`) through the provider that owns it. |
| Plugin Runtime | `plugins/` | `Plugin` trait (with defaulted `availability` and `check`), a shared registry that can change at runtime, built-in System and Demo plugins, the Navigation, Shell and HTTP providers from `actions/builtin.rs`, plus the Agent Control plugin from `agents/plugin.rs`. |
| Plugin SDK host | `plugins/external/` | Installed plugins: manifest validation, discovery, a Wasmtime sandbox with strict limits, `WasmPlugin` (an installed plugin behind the same `Plugin` trait), and `PluginHost` (enabled flags, settings, status, logs). Everything above the trait is unchanged. See [plugin-sdk](../plugin-sdk/README.md). |
| Agent Control | `agents/` | A module on top of the platform: one `AgentControl` state (agents, sessions, approvals, commands, slots, focus), the adapter SDK and adapters, the Agent Bridge, the audit log and the surface projection. Uses the bus, the state hub, feedback rules, plugins, renderer and persistence below. See [agent-control/overview.md](../agent-control/overview.md). |
| Renderer | `renderer/mod.rs` | Resolves each control's visual for its state, then adapts it to the device's capabilities. Pure functions. |
| Context Engine | `context/` | Auto Context: a per-system foreground observer, application identities, ordered rules, and the Auto / Manual Override state machine. It decides; the Surface Runtime's activation does. See below and [context-engine.md](../context-engine.md). |
| Persistence | `persistence/mod.rs` | `workspace.json` in the app data folder with `schemaVersion`, atomic writes, `.bak` copy of a damaged file, `migrate()` upgrades older schemas (schema 2 to 3 turns the old action shapes into assignments, schema 3 to 4 turns `bindings` into `feedback` rules and keeps what it cannot convert in `legacyBindings`; each keeps a `.v2.bak` / `.v3.bak` copy) and refuses newer ones. Values the hub holds are never saved. Agent Control also keeps `agents.json` (settings, control grants, hook token; owner-only on Unix) and an append-only `audit.jsonl`, written the same atomic way. Installed plugins keep their enabled flag and settings in `plugins.json`, the same way, their network grants in `permissions.json` (never in the workspace), and their secrets in the system credential store only (without one, secrets are unavailable; nothing is written to a file). |
| External events | `events/external.rs` | Localhost-only WebSocket. Generic `event` and `ping` messages; `agent.*` messages go to the Agent Bridge on the same socket. |

## Rules that keep it honest

1. **No MIDI above the device layer.** `ControlPos` (`grid`, `top`, `side`, `logo`) is the only addressing the rest of the system knows. Raw notes and CCs live in `launchpad_x.rs`.
2. **Plugins never see positions.** A plugin publishes an event (`plugin.state.changed {key, value}` for built-ins, a declared event type for installed ones). A control's *feedback rules* decide it cares: an event rule reacts to the event, a state rule to the value a provider reports.
3. **UI state is not runtime state.** Zustand holds the selected pad, the current view and toasts. The workspace, active surface, page, mode and every control state arrive from Rust and are never written locally.
4. **Everything crossing IPC is validated.** Rust validates edits (`core/validate.rs`), TypeScript parses payloads with Zod (`src/lib/schema.ts`).
5. **Agent Control is a module, not a core concept.** The runtime constructs and hosts it at startup, but the model, device, renderer and action layers know nothing about agents, and the agent domain names no vendor. Tool-specific code is confined to `agents/adapters/`. A test fails if the Agent Control surface or an Agent Pack manifest names one.

## One press, end to end

1. A pad is pressed. On hardware, `midir` delivers bytes to `LaunchpadX`, which decodes them to `DeviceInput { device_id, pos, Pressed }`. In the simulator the UI calls `virtual_input`, which injects the same `DeviceInput`.
2. The runtime finds the control at that position (honouring spans) and publishes `control.pressed`.
3. In **Edit** mode it stops there, so the editor can select the control. In **Live** mode it picks the action: press, or long press if the pad is held 500 ms (the tap is deferred until release when a long press exists).
4. The control's assignment is `{provider, action, args}`. Navigation (`navigation.openSurface`, `navigation.openPage`) is performed by the runtime. Everything else goes through the registry's `prepare` and then the Action Engine, with `action.started`, then `action.completed` or `action.failed` on the bus; the payload's `action` is `provider.action`.
5. From `action.started` until the provider answers, the action is **pending**: the runtime lists it in its state, and the control shows its look named `pending` if it has one. Controls without feedback rules get automatic `running`, then `success` (reset after 2.5 s) or `error` (reset after 4 s) as a temporary look. Controls *with* feedback rules are driven by their rules; their own action only adds `pending` while it runs and `failed` (1.5 s) when it fails, and leaves nothing behind when it succeeds.
6. A 25 Hz render tick composes a `SurfaceFrame` for every connected device through the renderer. Only changed LEDs are sent to hardware.

## Capabilities and graceful degradation

```rust
DeviceCapabilities {
    grid: Option<{ width, height }>,
    aux: { top, side, logo },
    rgb, velocity, pressure,
    animations: { pulse, flash },   // native support
}
```

The renderer adapts a frame to the target:

* **Position unsupported** (smaller grid, no side column): the LED is dropped. The control still exists in the model.
* **No native pulse**: pulse is synthesised by scaling brightness between 25% and 100% on a 1.4 s period.
* **No native flash**: flash toggles on and off with a 0.7 s period.
* **No RGB**: colour is reduced to luminance.

The virtual device advertises native pulse and flash and keeps the animation tag, so the UI animates it smoothly with CSS. The Launchpad X advertises *no* native animation (see the device notes), so it receives synthesised frames. Both go through the same renderer.

## Events

All events have the same shape:

```json
{ "source": "demo", "type": "plugin.state.changed", "payload": {"key":"claude","value":"running"}, "ts": 1760000000000 }
```

Types in use: `control.pressed`, `action.started`, `action.completed`, `action.failed`, `surface.changed`, `page.changed`, `plugin.state.changed`, `device.connected`, `device.disconnected`, the semantic agent events published by Agent Control (`agent.registered`, `session.running`, `approval.created` and the rest, with `source: "agent-control"`; see [agent-control/domain-model.md](../agent-control/domain-model.md#events)), the events an installed plugin declares in its manifest (with the plugin id as `source`), and anything an external tool sends. The sources `runtime`, `device` and `agent-control`, and the id of every installed plugin, are reserved: the WebSocket refuses them from clients.

### Feedback rules

A control carries one ordered list of rules that decide its look. A **state rule** reads the value a provider reported; an **event rule** reacts to an event for a while:

```json
{ "when": { "state": { "provider": "demo", "id": "alpha" }, "is": { "op": "equals", "value": "running" } },
  "show": "busy" }

{ "when": { "event": { "provider": "codex", "id": "task.completed" }, "where": [{ "field": "success", "equals": false }] },
  "show": "error", "hold": { "ms": 4000 } }
```

The first state rule that is true decides; an event rule's look sits over it until its hold ends. `show` names one of the control's `states`, or `showFromValue` / `showFromField` takes the name from the value or a payload field. The full model (addresses, the hub and its lifecycle, tests, holds, the gate, migration from `bindings`) is in [state-and-feedback.md](../state-and-feedback.md).

## UI to runtime contract

Commands (UI to Rust), all registered in `src-tauri/src/lib.rs`:

| Area | Commands |
| --- | --- |
| Runtime and editing | `get_snapshot`, `virtual_frame`, `set_mode`, `activate_surface`, `activate_page`, `virtual_input`, `update_control`, `remove_control`, `create_surface`, `add_page`, `reset_workspace`, `update_preferences` |
| Actions | `run_action` (a test run that returns the outcome), `actions_catalog` (the Action Library's catalog), `actions_validate` (check an assignment without saving or running it) |
| State and feedback | `observables_catalog` (every state and event a control can react to), `observable_state` (what one state reads now), `explain_control` (why a pad looks as it does), `review_feedback` (check a draft list of rules without saving it) |
| Auto Context | `context_status`, `context_update` (save Auto on or off, the rules and the fallback), `context_resume` (end a manual override) |
| Devices and events | `connect_device`, `disconnect_device`, `rescan_devices`, `send_event` |
| Agent Control | `agents_command`, `agents_focus`, `agents_dismiss`, `agents_set_access`, `agents_demo`, `agents_update_settings`, `agents_hook_setup`, `agents_rotate_hook_token` |
| Installed plugins | `plugins_list`, `plugins_set_enabled`, `plugins_set_config`, `plugins_reload`, `plugins_rescan`, `plugins_logs`, `plugins_set_permission` (allow, deny or revoke one address), `plugins_set_secret`, `plugins_clear_secret`, `plugins_migrate_legacy_secrets`, `plugins_discard_legacy_secrets`, `plugins_migrate_plugin_legacy_secrets` and `plugins_remove_plugin_legacy_secrets` (one plugin's plain-text secrets), `plugins_examples` and `plugins_install_example` (the example plugins the app ships: Hello, Counter, Home Assistant), `plugins_reveal_folder` (show the plugins folder) |

No command returns a secret's value, and none reports which application was in front in the past.

Events (Rust to UI): `runtime-state`, `workspace-changed`, `runtime-event`, `virtual-frame`, `observables-changed` (the catalog or a value in it changed), `context-changed` (the Auto Context status), `agents-changed` (a full snapshot with a monotonic `rev`, so an older one is ignored) and `agent-notification`.

Six files under `examples/` are contract fixtures, each written by an example program in `src-tauri/examples/` from the real runtime:

| Fixture | Written by | Holds |
| --- | --- | --- |
| `default-workspace.json` | `dump_workspace` | The starter workspace |
| `agents-snapshot.json` | `dump_agents` | A running Agent Control |
| `plugins-overview.json` | `dump_plugins` | The Plugins screen payload, from real example plugins |
| `action-catalog.json` | `dump_catalog` | The Action Library catalog of the same setup |
| `observables.json` | `dump_observables` | The Observable Catalog, the messages the interface hears and three explanations of a control |
| `context-status.json` | `dump_context` | The Auto Context status in each mode |

A Rust test fails if any of them drifts, and the Vitest suite parses all of them with the Zod schemas, so the two type systems cannot silently disagree. Two of them (`default-workspace.json`, `context-status.json`) hold values that depend on the system they were recorded on, which is Linux; see [NEXT_PHASE.md](../../NEXT_PHASE.md#3-tests-expected-to-fail-on-macos).

## Security architecture

| Concern | Decision |
| --- | --- |
| Shell commands | Privileged built-in provider (`shell`), not a plugin capability. It is listed in the Action Library with a *Privileged* label but runs only for a control the user configured (or the inspector's *Test run*). Shown verbatim. 30 s timeout, output truncated. |
| HTTP requests | Same rules (`http` provider). Method allowlist, 10 s timeout. |
| Remote input | WebSocket accepts `event`, `ping` and the `agent.*` bridge messages. No message executes anything: bridge clients can only describe agents they registered, and control over them is a separate per-agent grant by the person. See [agent-control/security.md](../agent-control/security.md). |
| Network exposure | Binds `127.0.0.1` only. Rejects non-local `Origin` headers (protects against a web page connecting to the socket). Max 64 KB per message, max 16 connections. |
| Spoofing | Sources `runtime`, `device`, `agent-control` and the id of every installed plugin are rejected from clients. Identifiers must match `[A-Za-z0-9_.:-]{1,64}`. |
| URLs | `system.openUrl` allows `http`, `https`, `mailto`, `tel`, `sms`, `facetime` only. |
| Action Library | The catalog is information; execution never trusts it. Every press and every save runs `prepare` on the live registry. Parameters are a closed schema drawn by LaunchDeck, so a provider ships no UI code. Provider ids are reserved: installed plugins need a dotted id and cannot shadow a built-in action. |
| Plugins | Built-in plugins are first-party native code. Installed plugins are third-party WebAssembly in a sandbox: no WASI, three host functions (five with the network capability), fuel, wall-clock, memory and size limits, declared events only, source stamped by the host, settings kept out of workspaces. `capabilities` fail closed: the only one is `network`, where the host makes each request after checking the exact origin, the person's grant, where the name resolves and every redirect, and applies secrets the plugin never sees. No shell API, no raw MIDI, no coordinates for either kind. See [plugin-sdk/security.md](../plugin-sdk/security.md). |
| Hook endpoint | Optional, off by default, `127.0.0.1` only, bearer token, exact `Host`, any `Origin` refused, bounded size and time. Used by the Claude Code and Codex adapters. |
| Installs | Nothing is downloaded or installed at runtime. A plugin is a folder the person puts in the plugins folder; nothing in it runs until the host has validated and sandboxed it. |

## Context Engine

`context/` makes the surface follow the application in front. A native observer per system (X11 events on Linux; `NSWorkspace` notifications on macOS and a WinEvent hook on Windows, both written and not yet built here) reports an `ApplicationIdentity { namespace, id, displayName }`. Ordered rules in the workspace's `context` (first match wins, then an explicit fallback) say which surface it calls for, and a pure state machine decides *when* that may be applied: a different application accepted after 150 ms in front, Resume Auto, Auto switched on, Edit left for Live, or the rules saved. A surface chosen by hand is kept until one of those. The engine's only output is `Runtime::activate`, the path the sidebar and navigation pads use, so events, the hub, the renderer and the devices follow unchanged. Nothing about the foreground is saved, not even the surface Auto showed last: only a person's activation saves the workspace. Where no observer exists (Wayland, X11 without an EWMH window manager) the engine says why and LaunchDeck works by hand as before. See [context-engine.md](../context-engine.md).

## Concurrency

Tokio multi-thread runtime started inside Tauri's async runtime. Device input arrives on an unbounded channel from MIDI callbacks. The runtime holds one mutex around workspace and state, never across `.await`, and emits UI events after releasing it. Action execution is spawned per press; blocking work (HTTP, plugin calls) uses `spawn_blocking`. Each installed plugin has its own sandbox instance behind a mutex, so calls to one plugin are serialized and different plugins run in parallel; a small background ticker (started with the first plugin) enforces the wall-clock limit. The foreground observer has one thread of its own (`foreground-observer`), which waits for the system to report a change of application and hands it to the runtime; the Context Engine itself runs under the runtime's mutex and is given the time, so it has no clock and no thread.

## Persistence

Everything is in the OS app data directory:

| System | Folder |
| --- | --- |
| Linux | `~/.local/share/dev.launchdeck.app/` |
| macOS | `~/Library/Application Support/dev.launchdeck.app/` |
| Windows | `%APPDATA%\dev.launchdeck.app\` |

| File | Holds |
| --- | --- |
| `workspace.json` | Surfaces, pages, controls, feedback rules, preferences, and the Auto Context settings (`context`). Schema 4. |
| `agents.json` | Agent Control settings, control grants, the hook token. Owner-only on Unix. |
| `plugins.json` | Each installed plugin's enabled flag and settings. |
| `permissions.json` | Network grants per plugin. Never in the workspace, so a shared surface carries none. |
| `audit.jsonl` | Append-only trail of decisions (approvals, grants, secret moves). No prompts, no secret values. |
| `plugins/` | Installed plugins, one folder each. |

Secrets are in none of these: they are in the system credential store only ([security/secrets.md](../security/secrets.md)). Values the State Hub holds and anything about which application is in front are never written.

Writes go to a temp file and rename. If `workspace.json` cannot be parsed it is copied to `workspace.json.bak` and the starter surfaces are used. A file written by a newer LaunchDeck is refused rather than downgraded.
