# Action Library plan

Status: written before implementation, 2026-10-04. Sections 4 to 6 are the contract the code implements; deviations found while building are recorded at the end of each section as "Built as".

The goal is one flow for every capability LaunchDeck has: discover, configure, assign, execute. A person never needs an action id, a plugin id, JSON, MIDI, a Rust type or any WebAssembly knowledge to put "Approve" or "Open GitHub" on a pad.

```
Providers            Unified registry         Action Library        Surface Editor        Action Runtime
(built-in, Agent  -> (Rust, authoritative) -> (React projection, -> (select or drag, -> (existing engine,
 Control, installed   definitions,             search, filter,       generated form,       events, automatic
 plugins)             availability, params     availability)         replace, test)        states, bindings)
```

## 1. Assessment

What exists and what is wrong with it.

* A control holds an `Action`, an enum of eight variants. Three of them (`keyboardShortcut`, `openUrl`, `launchApplication`) are thin sugar over the `system` plugin. Two (`shellCommand`, `httpRequest`) are core actions with their own execution path. Two (`openSurface`, `openPage`) are navigation handled by the runtime. One (`plugin`) is the real extension point: `{plugin, action, args}`.
* The inspector offers the eight variants in a hard-coded `<select>`. To use an installed plugin or an Agent Control command a person must choose the generic "Plugin" variant, pick a plugin id from one list, an action id from another, and type a JSON object for the arguments. Agent Control's fourteen actions are therefore reachable only through that JSON box.
* Installed plugins declare typed arguments (string, number, boolean, json) but the UI ignores them. Built-in plugins declare none, so `newTask` needs a `task` that nothing tells the person about.
* There is no availability concept in the model. Agent Control knows exactly why "Approve" cannot run right now (`agents/model.rs::availability`) and uses it to dim pads, but the editor never sees it. A disabled plugin's action fails at press time.
* There is no search, no categories, no description at the point of choice, and no way to see what a plugin offers without opening its screen.
* Replacing an action silently drops its arguments. Nothing distinguishes the library entry ("Open URL") from the configured instance ("Open https://github.com").

The gap is not in execution. Execution already works and is tested. The gap is in the layer between "what exists" and "what a pad does".

## 2. Existing architecture that stays

The library is a discovery and configuration layer on top of these, not a replacement.

| Piece | Reused as it is |
| --- | --- |
| `Plugin` trait and `PluginRegistry` | The registry of definitions is a view over it. Installed plugins already register here, already can be disabled, and already report errors. |
| `ActionEngine` | Still the only place an action executes. It gains one step in front (prepare) and loses its `match` over variants. |
| `action.started`, `action.completed`, `action.failed` | These are the pending, success and failure of an assigned action. No second feedback system. |
| Runtime automatic states | Unbound controls still get running, success and error from those events. |
| Agent Control `availability()` | The single derivation of "can this command run now". The registry asks it; it does not copy it. |
| Plugin host and manifest | Plugin actions appear in the library from `plugin.json` with no second registration. |
| `validate_control`, `persistence::migrate`, Zod mirror tests | Extended, not replaced. |
| Event bus, bindings, renderer | Untouched. |

## 3. External patterns, and what each one changed

Pages read on 2026-10-04. Only findings that changed a decision are listed.

| Source | Finding | Decision |
| --- | --- | --- |
| VS Code, [contribution points](https://code.visualstudio.com/api/references/contribution-points) | A command is declared with a stable `command` id, a `title`, an optional `category`, an optional `icon` and an optional `enablement`. `enablement` applies to the command everywhere, while a menu `when` clause only hides one menu entry. | Definitions have a stable id separate from the display name. Availability is a first-class value on the definition, like `enablement`, and is not a visibility filter: an unavailable action stays in the library with its reason. |
| VS Code, same page | Declaring a command in the manifest without a handler produces a command that appears and does nothing. | A definition exists only because a registered provider owns it. Provider removal removes its definitions; assignments that point at them become "missing", never silently run. |
| Stream Deck, [manifest reference](https://docs.elgato.com/streamdeck/sdk/references/manifest/) | Actions have a reverse-DNS UUID, a name, an icon, a tooltip. Per-button configuration is separate "settings", not part of the manifest. A plugin may ship custom HTML for its settings (`PropertyInspectorPath`). | Definition versus assignment is a hard split: the definition is static metadata, the assignment is `{provider, action, args}` on a control. The custom HTML property inspector is deliberately not adopted; LaunchDeck renders every form itself from a declarative schema, so a plugin cannot ship UI code. |
| Home Assistant, [service actions](https://developers.home-assistant.io/docs/dev_101_services) | Each action describes its fields with `required`, `default`, `example` and a `selector` that decides which UI control renders. | `ParamDef` carries `required`, `default`, `placeholder` (example) and a closed `kind` that decides the control. Defaults are applied by the registry, not by each form. |
| Raycast, [manifest](https://developers.raycast.com/information/manifest) | Commands have `title`, `subtitle` (the owning service), `description` and `keywords`. Arguments are `text`, `password` or `dropdown` with `{title, value}` pairs, `required` defaulting to false. | Search runs over name, provider, description, keywords and category. Enum options are `{value, label}`. `required` defaults to false. |

Answers to the ten questions the brief asks.

1. **Smallest `ActionDefinition`.** Stable id, name, description, provider, category, keywords, params, availability, plus optional appearance hint and a privileged flag. Nothing else. The id and the provider are enough to find the owner; everything the person sees comes from the rest.
2. **Built-in and plugin registration.** Both are `Plugin` implementations in the same registry. A built-in returns its definitions from `manifest()`; an installed plugin's definitions are derived from `plugin.json`. There is no separate registration call.
3. **Agent Control without special UI.** `AgentControlPlugin` already is a `Plugin`. It declares params (`task`, `cwd`, `text`, `slot`) and answers `availability(action)` from the same function the pads use. The library renders it like any other provider.
4. **Parameter declaration.** A list of `ParamDef` on each definition: `string`, `number`, `boolean`, `enum`, `reference` (a surface or a page) and, only to keep manifest v1 plugins working, `json`.
5. **Availability.** `Available` or `Unavailable { cause, reason }`. The cause is `provider` (disabled, failed to load) or `state` (nothing is pending approval). Both stay visible and both remain assignable.
6. **Search and categories.** The Rust catalog is sent once to React, which filters it locally with a token match over name, provider, description, keywords and category. Categories are strings a provider chooses; the library lists whatever the catalog contains.
7. **Configuration persistence.** `{provider, action, args}` only. No label, no executable code, no runtime state. Defaults are written explicitly on save so the saved file says what will run.
8. **Missing or disabled providers.** The assignment is kept as written. The inspector says "Provider missing" or "Plugin disabled" and offers the existing controls. Pressing the pad fails loudly with an `action.failed` event and the usual error state.
9. **Execution feedback.** Pending is `action.started`, success is `action.completed`, failure is `action.failed`. The inspector's Test button now awaits the same outcome and shows it inline.
10. **Marketplace compatibility.** A package's plugin manifest already is the definition source. The assignment format names a provider id and an action id, which a future install flow can check against. Nothing marketplace-specific is built.

## 4. Domain model

Two things with different lifetimes.

**ActionDefinition** belongs to a provider and describes what can be done. It is computed, never persisted, and sent to the UI as the catalog.

```json
{
  "id": "system.openUrl",
  "provider": "system",
  "action": "openUrl",
  "name": "Open URL",
  "description": "Open a link in your default browser.",
  "category": "Apps and links",
  "keywords": ["web", "browser", "link", "website"],
  "params": [ { "name": "url", "label": "Link", "type": "string", "required": true, "placeholder": "https://example.com" } ],
  "availability": { "status": "available" },
  "privileged": false,
  "appearance": { "label": "Open link", "color": "#2f81f7", "icon": "🔗" }
}
```

**Assignment** belongs to a control slot (`press` or `longPress`) and describes what this pad does. It is the only thing persisted.

```json
{ "provider": "system", "action": "openUrl", "args": { "url": "https://github.com" } }
```

* The definition id is `provider + "." + action`. It is derived for display, search, drag payloads and event payloads; the assignment stores the two parts, so there is nothing to split.
* Ids never come from display names. Renaming "Approve" to "Accept" changes `name` only. Uniqueness of the joined id is enforced by the registry: the first provider to claim an id keeps it, a later duplicate is dropped and reported as a catalog issue.
* Built-in provider ids are single words (`navigation`, `system`, `shell`, `http`, `agent-control`, `demo`). Installed plugin ids are lowercase dotted with at least two segments, so they cannot collide with a built-in. Installed plugins cannot register in a reserved provider.
* Source information survives: `ProviderInfo.kind` is `builtin`, `agentControl` or `plugin`, and the library shows "Built-in", "Agent Control" or "Plugin: Counter" next to the human name.
* `appearance` holds hints only: a label, a colour and a glyph. They are applied explicitly (new control from a drop, or the "Use suggested look" button) and never overwrite what a person set.
* State-feedback suggestions (which control states an action usually drives) are deferred. They only make sense together with the State Binding Editor, which is out of scope, and shipping a field nothing reads would be dead metadata. Section 11 records this.

Built as: `ProviderKind` serialises as `builtin`, `module` or `plugin` (Agent Control is a `module`, not a special case in the type). `appearance.color` is the same `#rrggbb` text controls use.

## 5. Registry

`ActionRegistry` is a view over `PluginRegistry`. It owns no table of its own, so it cannot drift from what is registered.

```rust
registry.catalog()                    // providers + definitions + issues, availability evaluated now
registry.definition(provider, action) // one definition or why there is none
registry.prepare(&assignment)         // resolve, normalize args, run the provider check; the only gate to execution
```

* **Providers.**
  * `navigation`: Open surface, Open page. Execution is handled by the runtime because it needs runtime state; the provider only declares them.
  * `system`: Open URL, Launch application, Keyboard shortcut, Volume up, Volume down, Toggle mute, Play / pause.
  * `shell` and `http`: one action each, marked `privileged`. They stay exactly as constrained as today (only a person configuring a pad, or the inspector's Test button, can trigger them; there is no path from a plugin, the WebSocket or the Agent Bridge to them).
  * `agent-control`: the fourteen commands, with availability.
  * `demo`: the reference plugin.
  * Installed plugins: whatever `plugin.json` declares.
* **Plugin trait.** Two defaulted methods are added, so every existing implementation keeps compiling and behaving the same: `availability(&self, action) -> Availability` and `check(&self, action, &args) -> Result<(), Issue>`. `check` carries semantic validation that a type schema cannot express (a URL scheme, a key combination).
* **`prepare`.** Order: provider exists, action exists, arguments normalized against the schema (unknown argument is an error, defaults filled, types and ranges checked), provider `check`. The result is a normalized assignment. `ActionEngine::execute` calls it, so nothing reaches `invoke` unvalidated, and `update_control` calls it, so nothing malformed is saved.
* **Unchanged assignments are never rejected on save.** `update_control` validates a slot only when it differs from what is stored. Editing a pad's label while its plugin is uninstalled must work.
* **Sync with React.** The catalog is part of the snapshot and is refetched with `actions_catalog` when the plugin set changes (enable, disable, rescan, reload) and, debounced, when Agent Control state changes, because availability moved. Search never leaves React.

## 6. Parameter schema

Small, closed and declarative. LaunchDeck draws the form.

```json
{ "name": "method", "label": "Method", "type": "enum", "required": false, "default": "GET",
  "options": [ { "value": "GET", "label": "GET" }, { "value": "POST", "label": "POST" } ] }
```

| Kind | Value | Form control | Extra fields |
| --- | --- | --- | --- |
| `string` | string | text input, or text area when `multiline` | `maxLength`, `placeholder`, `monospace` |
| `number` | number | number input | `min`, `max`, `integer` |
| `boolean` | boolean | checkbox | none |
| `enum` | string | select | `options` as `{value, label}` |
| `reference` | string (an id) | select of surfaces or pages | `target` is `surface` or `page`; `scope` names a sibling parameter holding a surface id |
| `json` | any | text area that must parse | only so manifest v1 plugins with `json` arguments keep working; new plugins should not use it |

Common fields: `name` (identifier), `label`, `description` (help text), `required` (default false), `default`.

Rules, enforced by Rust and mirrored in React only for instant feedback:

* Arguments must be an object. A name that is not declared is an error.
* A missing optional argument takes its default; a missing required one is an error; `null` counts as missing.
* Wrong types, out-of-range numbers, values outside an enum, text over `maxLength` are errors, and each error names its parameter so the form can mark the field.
* Reference values are checked for shape (non-empty id). Whether the target still exists is a runtime fact, shown as a warning in the editor and a failure at press time, because a surface can be deleted after the pad was configured.

Mapping of the old actions: the keyboard shortcut keeps its text form (`mod+shift+p`), the launch arguments become one argument per line in a multi-line text, so neither needs a list kind.

Manifest: `plugin.json` schema 1 is still read. Schema 2 adds, per action, `category`, `keywords`, `appearance` and richer `params` (label, description, default, options and the other fields above). Schema 1 `args` are converted on load (`label` is the name, `json` stays `json`). The host accepts 1 and 2. `abi` and the wire protocol do not change, so no existing `.wasm` needs a rebuild.

Built as: the field is named `type` in JSON (`kind` in Rust), as the manifest already named argument types. Slot numbers of Agent Control stay 0-based, as before, with a range check in the schema. The editor shows a provider's own check (for example the URL scheme) 250 ms after the last change, by asking Rust (`actions_validate`) only once the local rules pass. Unknown legacy actions are kept under the provider id `legacy` instead of being dropped. The Library mirrors the parameter rules in TypeScript (same messages) and a test runs every default-workspace action through both. State-feedback suggestions were not built (section 11). The details are in `docs/actions/`.

## 7. Availability model

* `Available`, or `Unavailable { cause, reason }` with a short human sentence.
* `cause: provider` when the owning plugin is disabled or failed to load, taken from the plugin host status. `cause: state` when the action exists and works but not now, taken from Agent Control's `availability()` for the focused track, so the library and the dimmed pads can never disagree.
* The library never hides an unavailable action. It shows a text badge ("Unavailable") and the reason, so it is not colour only.
* Assigning an unavailable action is allowed. A pad for "Approve" must be configurable while nothing is waiting. The inspector shows the reason next to the assignment.
* An assignment whose provider is gone is "Missing", not "Unavailable": the library has nothing to show for it, so the inspector says which provider and action it expected and keeps the arguments readable.

## 8. UX flow

The physical Surface stays in the centre. The library is a collapsible panel on the left of the editor, in the same visual language as the inspector.

1. **Find.** The search field takes focus with `/` or when the panel opens. Typing filters instantly. A category row narrows by one category. Results are grouped by category, each row shows name, source badge, one line of description and an "Unavailable" badge when it applies. `Up` and `Down` move through the list, `Enter` assigns, `Escape` clears the search.
2. **Assign without dragging.** Select a pad, select an action, press **Assign to "Pad name"**. A segmented choice picks Press or Long press. Double click or `Enter` on a row does the same.
3. **Assign by dragging.** Drag a row onto any pad. Dropping on an empty cell creates a control there with the action's suggested look; dropping on an existing control assigns to its Press slot.
4. **Replace on purpose.** If the slot already holds an action, nothing changes until a confirmation names the current action and its settings and says they will be lost. Look and bindings are never touched by an assignment.
5. **Configure.** The inspector shows the assignment with a form generated from the schema: labels, help, required marks, dropdowns for enums and for surfaces and pages. Errors appear under the field and block saving that slot.
6. **Test.** The Test button runs the assignment through the real engine and shows Running, Done or the error inline.
7. **Suggested look.** When the action has an appearance hint that differs from the pad, a "Use suggested look" button applies label, colour and glyph on request.

Accessibility: everything above works with the keyboard alone; focus rings stay visible; status is always a word as well as a colour; interactive targets are at least 32 px; the list has `listbox` semantics and the panel is a labelled region.

## 9. Migration strategy

* Workspace `schemaVersion` 2 becomes 3. The `Action` enum is removed, not kept next to the new type.
* The migration works on the JSON, before typed parsing, for every control's `press` and `longPress`:

| Old | New |
| --- | --- |
| `keyboardShortcut {keys}` | `system.keyboardShortcut {keys: "a+b"}` |
| `openUrl {url}` | `system.openUrl {url}` |
| `launchApplication {app, args}` | `system.launchApplication {app, args: "one per line"}` |
| `shellCommand {command}` | `shell.execute {command}` |
| `httpRequest {method, url, body}` | `http.request {method, url, body}` |
| `openSurface {surfaceId, pageId}` | `navigation.openSurface {surfaceId, pageId}` |
| `openPage {pageId}` | `navigation.openPage {pageId}` |
| `plugin {plugin, action, args}` | `{provider: plugin, action, args}` (a `system` plugin call gets the same argument conversion as above) |

* Nothing is dropped. A malformed legacy action is kept as a best-effort assignment and then fails visibly at validation or at press time, rather than being deleted.
* Version 1 files run the same JSON migration before the existing "add Agent Control surface" step. A file from a newer build is still refused.
* Event payloads: `action` in `action.started`, `action.completed` and `action.failed` becomes `provider.action`. `surface.open` and `page.open` become `navigation.openSurface` and `navigation.openPage`. `shell.execute`, `http.request` and the plugin ids are unchanged.
* Tauri: `run_action` takes an assignment and now returns the outcome. New commands: `actions_catalog`, `actions_validate`.
* The TypeScript `Action` union, `ActionEditor` and the plugin picker are deleted, not kept as a second path.

## 10. Implementation phases

1. **Rust model.** `Assignment`, `ParamDef`, `ActionDefinition`, `Availability`, `Issue`. Parameter normalization with tests.
2. **Providers.** Trait extension; `navigation`, `shell`, `http` providers; params, categories, keywords and hints on `system`, `demo` and `agent-control`; Agent Control focus availability; installed plugins from manifest schema 2 and schema 1.
3. **Registry and engine.** `ActionRegistry`, `prepare`, `ActionEngine::execute(&Assignment)`, runtime navigation through the `navigation` provider, `update_control` validation, `run_action` returning the outcome, new Tauri commands.
4. **Persistence.** Schema 3, JSON migration, starter surfaces rewritten, fixtures regenerated.
5. **React.** Zod schemas, catalog store, search and category logic, `ActionLibrary`, generated assignment form, replace confirmation, missing and unavailable states, drag and drop, preview transport.
6. **Tests.** The list in section 12, including the end-to-end test with a real compiled plugin.
7. **Docs.** `docs/actions/` (six pages), updates to the plugin SDK, architecture, plugins and README pages.
8. **Verification.** Rust format, clippy and tests, TypeScript typecheck, lint and tests, production build, and the desktop UI exercised for real.

## 11. Risks

| Risk | Handling |
| --- | --- |
| The enum removal touches the runtime, validation, starter surfaces and the editor at once. | One persisted shape; the compiler finds every use; the migration has its own tests and a fixture of a real v2 workspace. |
| A user's saved workspace changes on first launch. | The migration is structural and lossless; the old file is not overwritten until the next normal save, and the pre-migration file is copied to `workspace.json.v2.bak` the first time a v2 file is migrated. |
| Shell and HTTP become registry entries and might look like plugin capabilities. | They are built-in providers with reserved ids; installed plugins cannot register in them and have no API that invokes another provider. A test asserts both. |
| Availability goes stale in the UI. | The catalog is refetched on plugin changes and, debounced, on Agent Control changes. Execution never trusts the catalog: `prepare` evaluates the live registry. |
| Manifest growth makes the format hard to keep stable. | Schema 2 only adds optional fields; schema 1 keeps working; every new field has a documented default. |
| Duplicate ids across providers. | First registration wins, the duplicate is dropped and listed as a catalog issue shown in the library footer. |
| State suggestions deferred. | Documented as not built; the definition has no field for them, so there is nothing to migrate when the State Binding Editor arrives. |

## 12. Acceptance criteria

* Built-in, Agent Control and installed-plugin actions all appear in one library, with their source shown.
* A pad can be given any of them without typing an id, JSON or a plugin name, by selecting and by dragging.
* Arguments are edited in a generated form, validated before saving, and defaults are applied.
* Search finds actions by name, provider, description, keyword and category; the category filter narrows the list.
* Unavailable actions stay visible with a reason; assignments to missing or disabled providers load, display and stay editable, and nothing is deleted.
* Replacing an assigned action asks first and never touches look or bindings.
* A version 2 workspace opens as version 3 with the same behaviour.
* A real compiled plugin's action, discovered from its manifest, can be assigned to the virtual pad, pressed, executed in the sandbox, and its declared event reaches the binding path.
* Tests cover: definition validation, duplicate ids, registry composition, built-in, plugin and Agent Control registration, search, category filter, parameter validation, defaults, assignment persistence and reload, missing provider, disabled plugin, availability, replacement, and invocation from an assigned control.
* `cargo fmt --check`, `cargo clippy -D warnings`, `cargo test`, `npm run typecheck`, `npm run lint`, `npm test`, `npm run build` and `tauri build` pass, and the real desktop UI was used to find, assign, configure and run an action.

## Deliberately out of scope

Marketplace, network and filesystem capabilities, plugin signing, payments, the State Binding Editor, workflow chaining, macros, conditional workflows, AI recommendations, semantic search, plugin-supplied React or HTML, a remote action registry and cloud profiles.
