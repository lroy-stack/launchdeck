# The surface model and `workspace.json`

How surfaces, pages and controls are modelled and saved. The person's view of the same things, through the editor, is in [surfaces and the editor](../user/surfaces-and-editor.md). The file format for sharing surfaces (`.ldsurface`, `.ldpack`) is a different, versioned format: [artifacts](../artifacts/FORMAT.md).

The types are in `src-tauri/src/core/model.rs`; Rust validates every change (`core/validate.rs`) and the interface parses what it receives with the Zod schemas in `src/lib/schema/`.

## The tree

```
Workspace (schemaVersion, surfaces[], lastActiveSurface, preferences, context)
 └ Surface (id, name, description, icon, category, pages[], importedFrom?)
    └ Page (id, name, controls[])
       └ Control
```

* **Workspace.** `schemaVersion` is 4 today (`SCHEMA_VERSION`; see [versions](versions.md)). `lastActiveSurface` is the surface a person last chose; a switch made by Auto Context does not change it, so the file is not rewritten at every change of application. `preferences` holds `onboardingDone`, the external events port and switch, and `deviceBrightness` (percent for every device that has the feature; absent leaves each device its own level). `context` holds the [Auto Context](context-engine.md) settings: whether it is on, the ordered rules and the fallback; it is absent from a workspace that never used Auto Context, which is then saved exactly as before.
* **Surface.** `category` is one of `ai`, `development`, `productivity`, `smart-home`, `accessibility`, `custom`. `importedFrom` records the Surface file or Pack an imported surface came from (format, artifact id, name and version, which import of it, the surface's id in the file, and two digests that tell whether it changed since); it is local only, never exported, and absent on every surface made here. An older per-surface `context` field (`foregroundApps`, a list of names) is still read so old files load, and nothing uses it: rules in the workspace's `context` are the only thing that switches a surface.

## Control

| Field | Notes |
| --- | --- |
| `id` | Unique on the page. Defaults to the position key (`g.3.4`, `t.0`, `s.7`). |
| `pos` | `{kind:"grid",col,row}` (row 0 is the top), `{kind:"top",index}`, `{kind:"side",index}`. `logo` is output only. |
| `span` | `{w,h}` cells for grid controls. Used by the Accessibility surface for large targets. |
| `label`, `description`, `icon` | Text shown in the editor. |
| `visual` | Resting look: `{color:"#rrggbb", animation:"none"|"pulse"|"flash"}`. |
| `states` | Map of look name to visual: the looks the control's feedback rules can show. |
| `press`, `longPress` | An assignment (below). Long press fires after 500 ms. |
| `feedback` | Ordered rules that decide which look the pad shows: state rules read a provider's value, event rules show a look for a while ([state and feedback](state-and-feedback.md)). |
| `legacyBindings` | Schema 3 bindings that could not be converted, kept as they were. Inert. |

**Validation** (Rust, on every save): label 1 to 40 characters, no overlaps (spans included), positions in range, spans 1x1 to 8x8, and every slot that changed holds a well formed assignment (the provider and action exist, the arguments fit the action's parameters). A slot that did not change is not re-checked, so a surface whose plugin is missing can still be edited and saved.

## Assignments

A slot holds an **assignment**, the only thing saved about an action:

```json
{ "provider": "system", "action": "openUrl", "args": { "url": "https://github.com" } }
```

The built-in providers:

| Provider | Actions | Notes |
| --- | --- | --- |
| `system` | `openUrl`, `launchApplication`, `keyboardShortcut`, `volumeUp`, `volumeDown`, `toggleMute`, `playPause` | `keys` is text such as `mod+shift+p` (`mod` is Cmd on macOS and Ctrl elsewhere). `openUrl` allows http, https, mailto, tel, sms, facetime. `launchApplication` takes one argument per line. |
| `shell` | `execute` | **Privileged.** `command`. |
| `http` | `request` | **Privileged.** `method` (GET, POST, PUT, PATCH, DELETE), `url`, `body`. |
| `navigation` | `openSurface`, `openPage` | Handled by the runtime. `openSurface` takes `surfaceId` and an optional `pageId`. |
| `agent-control`, `demo` | See [plugins](plugins.md) | |
| An installed plugin's id | Whatever it declares | See the [plugin SDK](../plugin-sdk/README.md). |

What an action's parameters are and how arguments are checked: [actions](actions/overview.md), [parameters](actions/parameters.md).

A workspace that names a provider which is not installed still loads. The pad is flagged, the editor shows the saved assignment as missing and keeps its settings, and nothing is deleted. Workspaces written before schema 3 are converted on first load; see [definitions](actions/definitions.md#persistence-and-migration).

## Pads that follow state

The Agent Control surface shows how a surface follows outside state. Its track pads have state rules on the states Agent Control keeps (`agent-control.slot.status`, `slot.focused`, `command.state`), and the colour of each of the eleven normalized statuses is defined once in `agents/visual.rs` ([Agent Control surfaces](../agent-control/surfaces.md#the-mapping-state-to-appearance)). A pad driven by state rules is right whenever it is shown, because it reads the value rather than waiting for an event. Controls *without* feedback rules use the automatic cycle instead: running while the action runs, then success for 2.5 s or error for 4 s, then idle.

## The file

Surfaces are saved in `workspace.json` in the app data folder ([where](../getting-started/install.md#where-launchdeck-keeps-its-data)), written atomically; a file that cannot be read is kept as `workspace.json.bak` and the app starts with the starters and says so. Older schemas are migrated on load (each step keeps a `.v2.bak` / `.v3.bak` copy); a newer schema is refused, never downgraded. Values the State Hub holds are never saved. See [persistence in the overview](overview.md#persistence).

`examples/default-workspace.json` is the authoritative example and a contract fixture (`dump_workspace`). It is written as on Linux on every system; a few starter pads differ by system (which terminal, file manager and screenshot shortcut they name).

A person never edits this file: the editor, the inspector and the import and export screens are the way in. Sharing surfaces is done with [Surface files and Packs](../user/sharing.md).
