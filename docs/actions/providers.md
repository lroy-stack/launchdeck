# Providers

A provider is anything that owns actions. Every provider is a `Plugin` registered in the same `PluginRegistry`, and the Action Registry is a view over that registry. There is no second place to register an action, so what the library shows and what can execute cannot drift apart.

| Provider id | Source | Category | Actions |
| --- | --- | --- | --- |
| `navigation` | Built-in | Navigation | Open surface, Open page |
| `system` | Built-in | Apps and links, Keyboard, Media | Open URL, Launch application, Keyboard shortcut, Volume up, Volume down, Toggle mute, Play / pause |
| `shell` | Built-in | Advanced | Run shell command (*privileged*) |
| `http` | Built-in | Advanced | HTTP request (*privileged*) |
| `demo` | Built-in | Demo | Start task, Stop task, Succeed task, Fail task, Request approval, Approve, Run agent demo, Reset |
| `agent-control` | Module | Agent Control | Approve, Reject, Pause, Resume, Cancel, Retry, Continue, New task, Select track, Clear selection, Dismiss track, Run Agent Demo, Reset demo, Open Agents view |
| `acme.notes`, `example.hello`, ... | Installed plugin | The category the manifest gives, or the plugin's name | Whatever its `plugin.json` declares |

The exact catalog of the example setup is captured in `examples/action-catalog.json` (`cargo run --example dump_catalog`). A Rust test fails if it drifts and the Vitest suite parses it with the same Zod schemas the app uses.

## Built-in providers

* **Navigation.** The provider only *declares* its two actions. The runtime performs them, because moving between surfaces needs runtime state; `invoke` on this provider is never how they run. The surface and page are chosen from lists.
* **System.** Native input and launching, behind traits so tests use fakes. Its `check` validates the URL scheme and the key combination, so the form can show the problem next to the field. Details of each action are in [plugins](../plugins.md#system-system).
* **Shell and HTTP.** One action each, marked `privileged`. They are **still not plugin capabilities**:
  * Only a pad a person configured, or the inspector's *Test run*, can cause them to run.
  * No plugin API, WebSocket message or Agent Bridge message invokes them, or any other provider.
  * Installed plugin ids need two dotted parts, so no plugin can register as `shell` or `http`.
  * The library shows a *Privileged* label next to them and the editor shows what will run, verbatim, before saving. They keep the 30 s (shell) and 10 s (HTTP) timeouts and the method allow-list.
* **Demo.** The reference provider for authors, with simulated agents `alpha`, `beta` and `gamma`.

## Agent Control

Agent Control is a module that contributes actions through the same registry. The library has no special case for it.

* Its actions declare their settings like any other (`task`, `cwd`, `text`, `slot`) and its category is *Agent Control*.
* Availability comes from the function the pads already use to dim themselves (`agents/model.rs::availability`, through `focus_availability`). The library asks it; it never copies the rules. So a pad and the library can never disagree about whether *Approve* can run.
* Those commands act on the **focused track**. They are unavailable with the reason from that function: the agent does not support the command, is offline, is monitor-only, there is no track selected, nothing is pending approval, the track is in the wrong status. The reason is a sentence the library shows as it is.
* Unavailable commands are still assignable. A pad for *Approve* has to be configurable while nothing is waiting.

## Installed plugins

A plugin's actions appear in the library automatically, from `plugin.json`. There is no second registration and no code in the host to change.

```json
{ "id": "notes.add", "label": "Add note", "description": "Adds a note to the list.",
  "category": "Notes", "keywords": ["write", "memo"],
  "appearance": { "label": "Note", "color": "#f2c744", "icon": "📝" },
  "params": [ { "name": "text", "label": "Note", "type": "string", "required": true, "multiline": true } ] }
```

* The manifest is the definition source. `label` is the display name, `id` is the stable part, and the library id is `<plugin id>.<action id>`, for example `example.counter.counter.increment`.
* `category` defaults to the plugin's name, so a plugin without one still has a home. A person sees the source as *Plugin: Name*.
* `params` use the schema in [parameters](parameters.md). The host validates and normalizes arguments **before** the module runs, so a module only ever receives well-typed input with defaults applied. This is the same function the editor mirrors.
* Manifest schema 1 is still read: each `args` entry becomes a parameter whose label is its name, and `json` stays `json`. Schema 2 adds `category`, `keywords`, `appearance` and `params`. No `.wasm` module needs rebuilding, because the ABI and the wire protocol did not change. The [manifest reference](../plugin-sdk/manifest.md) has every field and limit.
* If a plugin is disabled or failed to load, its actions stay in the library as unavailable with the plugin's own reason. Assignments to them stay as saved. See [availability](availability.md).
* A plugin cannot pick a built-in id, cannot shadow another provider's action (the first to claim an id keeps it), and cannot invoke another provider.

## Adding a provider

For a built-in provider in Rust:

1. Implement `Plugin`. Return a `PluginManifest` whose `actions` are `ActionDef`s built with `ActionDef::new(id, label, description)` and the builders `.category(..)`, `.keywords(..)`, `.param(..)`, `.look(label, color, icon)` and `.privileged()`.
2. Override `availability(action)` when an action cannot always run, with a `Cause` and a reason in plain words.
3. Override `check(action, args)` for rules a schema cannot express. It receives arguments that already passed the schema.
4. Register it in the `PluginRegistry` where the runtime starts. It shows up in the library with no other change, and the inspector draws its form.
5. Test it: `invoke` with a `PluginContext` on a test event bus, `availability` for each state it distinguishes, and the catalog (`ActionRegistry::catalog`) for the definitions it declares.

For a plugin anyone can install, follow the [plugin SDK](../plugin-sdk/getting-started.md) and write `plugin.json` with schema 2.
