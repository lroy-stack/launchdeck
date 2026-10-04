# Surfaces

A **surface** is a named layout for a device. It has **pages**, and a page has **controls**.

```
Workspace (schemaVersion, surfaces[], preferences)
 └ Surface (id, name, category, icon, pages[], context hints)
    └ Page (id, name, controls[])
       └ Control
```

## Control

| Field | Notes |
| --- | --- |
| `id` | Unique on the page. Defaults to the position key (`g.3.4`, `t.0`, `s.7`). |
| `pos` | `{kind:"grid",col,row}` (row 0 is the top), `{kind:"top",index}`, `{kind:"side",index}`. `logo` is output only. |
| `span` | `{w,h}` cells for grid controls. Used by the Accessibility surface for large targets. |
| `label`, `description`, `icon` | Text shown in the editor. |
| `visual` | Idle look: `{color:"#rrggbb", animation:"none"|"pulse"|"flash"}`. |
| `states` | Map of state name to visual. The state picks the look. |
| `press`, `longPress` | An [assignment](#actions): `{provider, action, args}`. Long press fires after 500 ms. |
| `bindings` | Event to state rules (see [architecture](architecture.md#bindings)). |

Validation (Rust, on every save): label 1 to 40 characters, no overlaps (spans included), positions in range, spans 1x1 to 8x8, and every slot that changed holds a well formed assignment (the provider and action exist, the arguments fit the action's parameters). A slot that did not change is not re-checked, so a surface whose plugin is missing can still be edited and saved.

## Actions

A slot holds an **assignment**, the only thing saved about an action:

```json
{ "provider": "system", "action": "openUrl", "args": { "url": "https://github.com" } }
```

You rarely write one by hand. In Edit mode the [Action Library](actions/action-library.md) lists everything a pad can do, and the inspector draws the settings. The built-in providers are:

| Provider | Actions | Notes |
| --- | --- | --- |
| `system` | `openUrl`, `launchApplication`, `keyboardShortcut`, `volumeUp`, `volumeDown`, `toggleMute`, `playPause` | `keys` is text such as `mod+shift+p` (`mod` is Cmd on macOS and Ctrl elsewhere). `openUrl` allows http, https, mailto, tel, sms, facetime. `launchApplication` takes one argument per line. |
| `shell` | `execute` | **Privileged.** `command`. |
| `http` | `request` | **Privileged.** `method` (GET, POST, PUT, PATCH, DELETE), `url`, `body`. |
| `navigation` | `openSurface`, `openPage` | Handled by the runtime. `openSurface` takes `surfaceId` and an optional `pageId`. |
| `agent-control`, `demo` | See [plugins](plugins.md) | |
| An installed plugin's id | Whatever it declares | See [plugin-sdk](plugin-sdk/README.md). |

A surface file that names a provider which is not installed still loads. The pad is flagged, the editor shows the saved assignment as missing and keeps its settings, and nothing is deleted. Workspaces written before schema 3 are converted on first load; see [actions/definitions.md](actions/definitions.md#persistence-and-migration).

## Starter surfaces

| Surface | Purpose | Highlights |
| --- | --- | --- |
| **Agent Control** | Watch and steer agents. The flagship surface. | One pad per session coloured by status, large APPROVE and REJECT pads, Pause, Resume, Retry and Stop that light only when they can work, task pads, Run Agent Demo. Provider-neutral. See [agent-control/surfaces.md](agent-control/surfaces.md). |
| **Development** | Build, test, ship. | Terminal, editor, git page. Commands are harmless `echo` placeholders you replace. A "Failing" pad shows the error flash. |
| **Productivity** | Meetings, notes, focus, volume. | Shortcuts and links. |
| **Accessibility** | Fewer, larger, high-contrast targets. | Twelve 2x2 pads in colour-coded groups (navigate, edit, scroll and zoom, volume), Help page. A usability aid, not a medical device. |
| **Smart Home** | Webhook template. | Pads POST to `http://homeassistant.local:8123/api/webhook/launchdeck-<name>` placeholders. They do nothing useful (the request fails and the pad flashes red) until you create matching webhooks or edit the URLs. |

The top row on every surface switches surfaces, so a Launchpad works without the screen.

### Pads that follow events

The Agent Control surface shows how a surface follows outside state. Its track pads are driven by bindings on events with `source: "agent-control"`, and the colour of each of the eleven normalized statuses is defined once in `agents/visual.rs` (see [agent-control/surfaces.md](agent-control/surfaces.md#the-mapping-state-to-appearance)). A pad driven by bindings keeps its state until the next event. Controls *without* bindings (for example a Development build pad) use the automatic cycle instead: running while the action runs, then success for 2.5 s or error for 4 s, then idle.

## Authoring tips

* Keep a surface to one job. Use pages for depth, not for unrelated jobs.
* Use colour as meaning (green good, red bad) and animation for things that need attention.
* For accessibility surfaces prefer spans of 2x2 or larger, high-contrast colours and labels as well as icons.
* To react to an outside tool, add a binding for its `source` and map its event names to state names, then give the control a look for each state. Bindings are written in the surface file; the inspector shows them read-only.
* To react to agents, bind to the `agent-control` events described in [agent-control/surfaces.md](agent-control/surfaces.md#make-your-own) and call the `agent-control` plugin actions on press. Never count or address sessions in a surface; the platform publishes what a pad needs.

## Files

Surfaces live in `workspace.json`. The shape matches `examples/default-workspace.json`, which is the authoritative example. Sharing and packaging of surfaces is described in [roadmap/marketplace.md](roadmap/marketplace.md).
