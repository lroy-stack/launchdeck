# Actions

Everything a pad can do lives in one place, the **Action Library**, and reaches a pad the same way: find it, give it to a pad, fill in its settings, press the pad.

```
Providers              Unified registry         Action Library          Surface editor          Action runtime
built-in, Agent    ->  Rust, authoritative  ->  React, searchable   ->  select or drag,     ->  the existing engine:
Control, installed     definitions,             projection of the       generated form,         events, automatic
plugins                availability, params     catalog                 replace, test           states, feedback
```

A person never needs an action id, a plugin id, JSON, MIDI, a Rust type or any WebAssembly knowledge to put "Approve" or "Open GitHub" on a pad.

## Two things with different lifetimes

| | Belongs to | Says | Stored |
| --- | --- | --- | --- |
| **Action definition** | A provider | What can be done: a name, a description, the settings it takes, whether it can run now. "Open URL" is a definition. | Never. It is computed from what is registered and sent to the UI as the *catalog*. |
| **Assignment** | A pad's *press* or *long press* slot | What this pad does: a provider, an action and the settings. "Open https://github.com" is an assignment. | Yes. It is the only part of an action that is saved. |

```json
{ "provider": "system", "action": "openUrl", "args": { "url": "https://github.com" } }
```

The two ids and the settings are all there is. There is no label, no code and no runtime state in an assignment, so a surface file stays portable: it names what it needs, and whoever opens it either has that provider or sees exactly which one is missing.

## The pieces

| Piece | Where | Job |
| --- | --- | --- |
| Providers | `plugins/` (System, Demo), `actions/builtin.rs` (Navigation, Shell, HTTP), `agents/plugin.rs` (Agent Control), `plugins/external/` (installed plugins) | Declare actions. Each is a `Plugin`; nothing new is registered. |
| Unified registry | `actions/registry.rs` | A view over the plugin registry. Builds the catalog, resolves an assignment, and is the one gate (`prepare`) every assignment passes before it is saved or run. |
| Parameter schema | `actions/params.rs` | The closed set of setting kinds, their validation and their defaults. |
| Action engine | `actions/mod.rs` | Runs a prepared assignment through its provider. The only place an action executes. |
| Runtime | `core/runtime.rs` | Press handling, navigation, `action.*` events, automatic states, feedback rules. Unchanged by the library. |
| Library and editor | `src/features/library`, `src/features/editor` | Search, categories, drag and drop, the generated form, the replace confirmation, Test run. |

Rust owns the catalog and the final validation. React keeps a copy of the catalog so searching never waits for a round trip, and mirrors the parameter rules only to mark a mistake while it is typed. A test runs every action of the default workspace through both implementations.

## One flow

1. **Discover.** The catalog lists every action of every provider, with its source, category, keywords and whether it can run now. Unavailable actions are listed with their reason, never hidden. See [availability](availability.md).
2. **Configure.** An action's settings are declared as a small schema. LaunchDeck draws the form and checks it before saving. See [parameters](parameters.md).
3. **Assign.** Select a pad and press *Assign*, or drag the action onto a pad. Replacing an existing action asks first. See [action library](action-library.md).
4. **Execute.** Pressing the pad calls the registry's `prepare`, then the provider. `action.started`, `action.completed` and `action.failed` report it, and the pad's automatic or bound states follow. Nothing about execution is new.

## Pages

* [Definitions](definitions.md): the shape of a definition and an assignment, ids, persistence and the v2 to v3 migration.
* [Parameters](parameters.md): the settings schema, validation rules and defaults.
* [Providers](providers.md): every provider and action that ships, and how a new one registers.
* [Availability](availability.md): available, unavailable and missing, and what each one does on screen and on a press.
* [Action Library](action-library.md): using the library, search, drag and drop, replacement, keyboard and accessibility.

The research behind the design and the decisions it led to are in [ACTION_LIBRARY_PLAN.md](../../ACTION_LIBRARY_PLAN.md).

## Not built

Marketplace and remote registries, network or filesystem capabilities, plugin-supplied UI, workflow chaining, macros and conditional actions, and recommendations are out of scope. **State-feedback suggestions** (which pad states an action usually drives) are deferred: they only pay off together with an editor for feedback rules, and a field that nothing reads would be dead metadata. A definition has no field for them, so adding them later changes nothing that is stored.
