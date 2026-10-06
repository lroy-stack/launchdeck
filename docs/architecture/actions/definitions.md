# Definitions and assignments

## Action definition

What a provider offers. It is computed from the registered providers each time the catalog is requested and is never saved.

```json
{
  "id": "system.openUrl",
  "provider": "system",
  "action": "openUrl",
  "name": "Open URL",
  "description": "Open a link in your default browser.",
  "category": "Apps and links",
  "keywords": ["web", "browser", "link", "website"],
  "params": [
    { "name": "url", "label": "Link", "type": "string", "required": true, "placeholder": "https://example.com" }
  ],
  "availability": { "status": "available" },
  "privileged": false,
  "appearance": { "label": "Open link", "color": "#2f81f7", "icon": "🔗" }
}
```

| Field | Meaning |
| --- | --- |
| `id` | `provider` + `.` + `action`. Stable: it is used for search, drag payloads and event payloads, and never changes when the display name does. |
| `provider`, `action` | The two ids an assignment stores. |
| `name` | What the person reads. Free to change between releases. |
| `description` | One or two sentences. Shown in the library and under the assignment. |
| `category` | A group in the library. A provider chooses its own strings; when an action gives none, the provider's name is used. The library lists whatever categories the catalog contains. |
| `keywords` | Extra search words ("web", "browser"). Up to 12 per action in a plugin manifest. |
| `params` | The settings the action takes. See [parameters](parameters.md). Empty when it takes none. |
| `availability` | `available`, or `unavailable` with a `cause` and a `reason`. See [availability](availability.md). |
| `privileged` | The action runs a command or a network request on the person's behalf. It is always labelled *Privileged* and only a pad the person configured can trigger it. |
| `appearance` | Optional hint for a pad that runs this action: a `label`, a `color` and an `icon` (a glyph). Hints only: they are applied when a person asks, never over what they set. |
| `suggests` | Optional. Ids (`provider.local`) of [observables](../observables.md) a pad for this action usually shows, such as the state the action changes. A hint: nothing is created from it. A plugin lists local ids in its manifest and the catalog makes them full ids. |

### Ids

* The id is derived from two stable ids, never from the display name. Renaming "Approve" to "Accept" changes `name` and nothing else.
* Built-in provider ids are single words: `navigation`, `system`, `shell`, `http`, `agent-control`, `demo`. Installed plugin ids are lowercase and dotted with at least two parts (`acme.notes`), so an installed plugin can never claim a built-in id. The manifest check refuses them.
* An id is unique in the catalog. Providers are ordered built-in first, then modules, then installed plugins, each by name, and **the first to claim an id keeps it**. A later duplicate is left out of the catalog, and a message ("`x.y` was left out: another provider already uses this id") is added to `catalog.issues` and shown at the foot of the library. An assignment never resolves to a shadowed definition.
* A definition with an empty name, a malformed appearance or a bad parameter schema is left out and reported in `catalog.issues` the same way. It is found when the provider registers, not when someone tries to use it.

### Providers

The catalog also lists the providers, so the library can say where an action comes from.

```json
{ "id": "example.hello", "name": "Hello Plugin", "kind": "plugin", "version": "0.1.0",
  "description": "Says hello, ...", "status": "ready" }
```

| `kind` | Shown as | What it is |
| --- | --- | --- |
| `builtin` | *Built-in* | Native code that ships with LaunchDeck: System, Navigation, Shell command, HTTP request, Demo. |
| `module` | The module's own name, for example *Agent Control* | A LaunchDeck module that contributes actions. |
| `plugin` | *Plugin: Hello Plugin* | Installed from a folder and running in the sandbox. |

`status` is `ready`, `disabled` or `error`, with a `reason` for the last two. The reason is the one the Plugins screen shows.

### The catalog

```json
{ "providers": [ ... ], "actions": [ ... ], "issues": [ ... ] }
```

It travels in the snapshot at startup. React asks for it again with `actions_catalog` when the plugin set changes (enable, disable, reload, rescan), when editing starts, and, debounced, when Agent Control state changes, because availability moved with it. Searching never calls Rust.

## Assignment

What a pad's slot does. The only action data that is persisted.

```json
{ "provider": "system", "action": "openUrl", "args": { "url": "https://github.com" } }
```

* `args` is always an object. A missing or `null` value is read as `{}`.
* Unknown fields are refused, so a typo is an error and not a silently ignored setting.
* Defaults are written explicitly when the assignment is saved, so the saved file says what will run.
* A control has two slots, `press` and `longPress`. Either may be empty.

An assignment stores no label. A pad's label, icon, colour, state looks and feedback rules are properties of the pad, and assigning or replacing an action does not change them.

### The gate

`ActionRegistry::prepare` is the only way an assignment becomes something that can be saved or run. In order:

1. The provider exists.
2. The provider has that action, and the action is not shadowed by an earlier provider.
3. The arguments are normalized against the action's schema: undeclared names are errors, blanks take their default, required values are checked, types and ranges are checked.
4. The provider's own `check` accepts the normalized arguments. This is for rules a type schema cannot express: the URL scheme of *Open URL*, the key names of *Keyboard shortcut*.

The result is the normalized assignment. `ActionEngine::execute` calls it before `invoke`, so nothing reaches a provider unvalidated. `update_control` calls it for every slot that **changed**, so nothing malformed is saved.

A slot that did *not* change is never validated on save. A pad whose plugin was uninstalled can still have its label edited and be saved. See [availability](availability.md#missing).

### Where validation shows up

| Where | What it does |
| --- | --- |
| `update_control` | Rejects a changed slot that fails `prepare`, with every message. |
| `actions_validate` | Returns the issues and the normalized assignment without saving or running, so the editor can show a provider's own check (for example a URL scheme) next to the field. |
| `run_action` | The inspector's Test run. Takes an assignment, runs it through the same engine and returns the outcome. |
| A press | Runs `prepare` again against the live registry. The catalog is never trusted for execution. |

## The plugin trait

A provider is a `Plugin`. Two defaulted methods make it a provider with availability and semantic checks, and every existing implementation keeps compiling and behaving the same:

```rust
pub trait Plugin: Send + Sync {
    fn manifest(&self) -> PluginManifest;
    fn invoke(&self, ctx: &PluginContext, action: &str, args: &Value) -> Result<Value, ActionError>;
    fn availability(&self, action: &str) -> Availability { Availability::Available }
    fn check(&self, action: &str, args: &Value) -> Result<(), Issue> { Ok(()) }
}
```

`manifest().actions` is a list of `ActionDef`: id, label, description, category, keywords, params, privileged and appearance. The registry turns each one into an `ActionDefinition`, adding the provider's identity and the live availability. An installed plugin's `ActionDef`s are built from its `plugin.json`; see [providers](providers.md#installed-plugins).

## Persistence and migration

The workspace schema is **3**. The old `Action` enum, with its eight shapes, is gone from Rust and TypeScript; there is one stored shape.

A workspace written by an earlier version is converted on the JSON, before typed parsing, for every control's `press` and `longPress`:

| Schema 2 action | Schema 3 assignment |
| --- | --- |
| `keyboardShortcut { keys }` | `system.keyboardShortcut { keys: "mod+shift+p" }` (a list becomes one text joined by `+`) |
| `openUrl { url }` | `system.openUrl { url }` |
| `launchApplication { app, args }` | `system.launchApplication { app, args: "one per line" }` |
| `shellCommand { command }` | `shell.execute { command }` |
| `httpRequest { method, url, body }` | `http.request { method, url, body }` (method upper-cased) |
| `openSurface { surfaceId, pageId }` | `navigation.openSurface { surfaceId, pageId }` |
| `openPage { pageId }` | `navigation.openPage { pageId }` |
| `plugin { plugin, action, args }` | `{ provider: plugin, action, args }`, with the same text conversion for `system` |
| anything else | `legacy.<type>` with every other field kept as `args` |

* The conversion is structural and lossless. Nothing is dropped: an action it does not recognise is kept under the `legacy` provider, where the editor shows it as missing and keeps its settings readable.
* It is idempotent: an action that already is an assignment is left alone.
* Schema 1 files run the same step before the existing "add the Agent Control surface" step. A file from a **newer** build is still refused rather than downgraded.
* The first time an older file is migrated, the original is copied to `workspace.json.v2.bak` (or `.v1.bak`, named after the version found) and an existing backup is never overwritten.
* Events change with it: the `action` field in the payload of `action.started`, `action.completed` and `action.failed` is now `provider.action`. Navigation used to report itself as `surface.open` and `page.open`; it now reports `navigation.openSurface` and `navigation.openPage`. Navigation is performed by the runtime and publishes `action.completed` or `action.failed`, without a `started`, because it finishes at once.
