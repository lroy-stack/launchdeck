# Plugins

A plugin gives LaunchDeck new **actions** and publishes **events** and **state**. It is deliberately blind to hardware.

> A plugin never knows about MIDI, notes, CC numbers, pad positions or the device. It says *what* (an action ran, a state changed). Surfaces decide *where* (which pad) and *how it looks*.

There are two kinds, behind the same `Plugin` trait, registry and Action Engine:

| Kind | Where it runs | Who writes it | Documentation |
| --- | --- | --- | --- |
| **Built in** | Native Rust, inside LaunchDeck | The LaunchDeck team (System, Demo, Agent Control) | This page |
| **Installed** | A WebAssembly sandbox, from a folder on disk | Anyone, in any language that compiles to WebAssembly | [plugin-sdk](plugin-sdk/README.md) |

Installed plugins need no recompiling of LaunchDeck and no Node, Rust or compiler on the machine that runs them. They are discovered, validated, loaded and registered at startup and on **Rescan**, and what they publish travels the same Event Bus and feedback rules as everything else. They can be enabled, disabled, configured and reloaded from the Plugins screen. The rest of this page covers the native contract and the built-in plugins.

## The runtime contract

```rust
pub trait Plugin: Send + Sync {
    fn manifest(&self) -> PluginManifest;
    fn invoke(&self, ctx: &PluginContext, action: &str, args: &Value) -> Result<Value, ActionError>;
}
```

* `PluginContext::publish_state(key, value)` publishes `plugin.state.changed { key, value }` with the plugin id as `source`.
* `PluginContext::spawn_after(delay, f)` schedules follow-up work on the runtime (used by the Demo plugin).
* The context contains no device handle and no shell.

A control calls a plugin with the assignment `{ "provider": "demo", "action": "startTask", "args": {"agent":"alpha"} }` and reacts to its state with a state rule on the `demo` provider (one state per simulated agent: `alpha`, `beta`, `gamma`). Every plugin, built in or installed, is a *provider* in the [Action Library](actions/overview.md): its actions are found, configured and assigned there, so nothing here needs an id typed by hand.

Two methods have defaults, so a plugin only writes them when it needs them:

```rust
fn availability(&self, action: &str) -> Availability   // can it run now? default: yes
fn check(&self, action: &str, args: &Value) -> Result<(), Issue>   // rules a schema cannot say; default: ok
```

The actions a plugin offers are `ActionDef`s in its manifest: id, name, description, category, keywords, parameters and an optional look. See [actions/providers.md](actions/providers.md).

## Built-in plugins

### System (`system`)

| Action | Args | Effect |
| --- | --- | --- |
| `openUrl` | `url` | Opens `http`, `https`, `mailto`, `tel`, `sms`, `facetime` links. Anything else is refused. |
| `launchApplication` | `app`, `args?` (one per line) | Starts an application. |
| `keyboardShortcut` | `keys` (text, `mod+shift+p`) | Sends a key combination. `mod` is Cmd on macOS, Ctrl elsewhere. |
| `volumeUp`, `volumeDown`, `toggleMute`, `playPause` | none | Media keys. |

Permissions declared: `automation`.

Keyboard and media keys use `enigo` behind a `Keyboard` trait, so tests use a recording fake. On macOS the app needs Accessibility permission. On Linux Wayland sessions synthetic input can be blocked. Failures become `action.failed` with a readable message and a red pad.

### Demo (`demo`)

The smallest possible plugin, kept as the reference for plugin authors and for custom pads. It simulates three generic tasks named `alpha`, `beta` and `gamma`, with no network and no processes. It is not Agent Control: the flagship demo is the richer simulation in [agent-control](agent-control/overview.md).

| Action | Args | Effect |
| --- | --- | --- |
| `startTask` | `agent` | `running` |
| `stopTask` | `agent` | `idle` |
| `succeedTask` | `agent` | `success` (stays until the next change) |
| `failTask` | `agent` | `error` (stays until the next change) |
| `requestApproval` | `agent` | `approval-required` |
| `approve` | `agent?` | Moves tasks waiting for approval to `running`, then to `success` 1.5 s later (all waiting tasks if none given) |
| `runScenario` | none | Scripted run of about 12 s across the three tasks: running, waiting, approval needed for `alpha`, then `beta` succeeds and `gamma` fails. The approval step waits until someone calls `approve`; nothing is approved automatically. Restarting or `resetAll` cancels pending steps. |
| `resetAll` | none | Everything idle. Also cancels a running scenario. |

State published: `alpha`, `beta`, `gamma`, each one of `idle`, `running`, `waiting`, `approval-required`, `success`, `error`.

Permissions declared: none.

### Agent Control (`agent-control`)

Lets any surface observe, intervene in and orchestrate agents. It acts on the focused track and the service decides whether a command is allowed, so a pad that cannot work is already dim. It never sees MIDI or positions, and it knows no vendor.

| Action | Args | Effect |
| --- | --- | --- |
| `approve`, `reject` | none | Decide the focused session's pending request. |
| `pause`, `resume`, `cancel`, `retry` | none | Act on the focused session, if the agent declared the capability and control is granted. |
| `continue` | `text?` | Send an instruction (default "continue"). |
| `newTask` | `task`, `cwd?` | Start a task on the focused agent. |
| `focusSlot`, `dismissSlot` | `slot` (0-based) | Select a track, or remove a finished one. |
| `clearFocus` | none | Deselect the track. |
| `runDemo`, `resetDemo` | none | Start or clear the four simulated agents. |
| `openApp` | none | Ask the desktop app to show the Agents section. |

State published: the projection events `slot.changed`, `focus.changed` and `command.state` with `source: "agent-control"`. Surfaces bind to those. See [agent-control/surfaces.md](agent-control/surfaces.md).

Permissions declared: none. It starts nothing outside LaunchDeck by itself; commands reach an agent only through the adapter that owns it, only if the person granted control.

### Navigation, Shell and HTTP (`navigation`, `shell`, `http`)

Built-in providers with their own ids, so the library lists them with everything else. Navigation (*Open surface*, *Open page*) is declared by the provider and performed by the runtime. Shell (*Run shell command*) and HTTP (*HTTP request*) are marked **privileged**.

## Why shell and HTTP cannot be reached by plugins

Shell commands and HTTP requests are privileged built-in actions that only a person can attach to a control. They appear in the Action Library like any other action, with a *Privileged* label, but they are not plugin capabilities: if they were, installing a plugin would be a route to running code. Only a pad a person configured, or the inspector's *Test run*, can run them. No plugin API, WebSocket message or Agent Bridge message invokes them or any other provider, installed plugin ids need two dotted parts and so cannot be `shell` or `http`, and no plugin API can execute a string as a command. Tests assert that exactly these two actions are privileged and that the manifest check refuses the built-in ids.

## Permissions and capabilities

**Built-in plugins** declare permissions (`automation`, `network`, `filesystem`, `shell`, `accessibility`, `midi`) for the Plugins screen. They are trusted first-party code, so the declaration is documentation, not a gate.

**Installed plugins** do not get permissions to ask for, because the sandbox has nothing to give: no filesystem, shell, network, MIDI, process, clock or secrets, and no WASI. They can only call the three host functions of the [protocol](plugin-sdk/protocol.md) (log, publish an event, reply). Their manifest has a `capabilities` array that must be empty in this version; a plugin that lists anything is refused, so a host that cannot provide a capability never runs a plugin that needs it. Details: [plugin-sdk/security.md](plugin-sdk/security.md).

How a real capability will be added (not built):

| Capability | Would grant | Example |
| --- | --- | --- |
| `network` | Outbound requests to hosts the manifest names, mediated by the host. | A Home Assistant plugin. |
| `filesystem` | Read or write inside folders the person picks. | A notes plugin. |
| `shell` | Run specific, person-approved commands. Never a general `exec`. | A git plugin. |

The rules for that future: the person grants each capability per install and can revoke it; an update that asks for more is re-approved; every capability call goes through the host so it can be checked and logged (never ambient access); and events a plugin publishes always carry its id as `source`, so it cannot publish as `runtime` or `device`. The last rule is already enforced.

## Writing a built-in plugin

1. Implement `Plugin` in `src-tauri/src/plugins/yours.rs` and return a `PluginManifest` (id, actions, states, permissions).
2. Register it in `PluginRegistry` where the runtime is started.
3. Add tests that call `invoke` with a `PluginContext` built on a test `EventBus` and assert on published events, and that check `availability` and the catalog entries for the actions it declares.
4. Reference its actions from controls and what it publishes from feedback rules. Its actions appear in the Action Library automatically, with the form its parameters describe.

To write a plugin anyone can install instead, follow [plugin-sdk/getting-started.md](plugin-sdk/getting-started.md).
