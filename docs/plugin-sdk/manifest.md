# Manifest reference

`plugin.json` sits next to the module, is at most 64 KiB, and is strict: unknown fields are errors, so a typo is reported rather than ignored. It holds no executable content.

```json
{
  "schemaVersion": 2,
  "id": "acme.notes",
  "name": "Notes",
  "version": "1.2.0",
  "description": "Short text shown on the Plugins screen.",
  "author": "Acme",
  "abi": 1,
  "sdk": "0.1.0",
  "main": "notes.wasm",
  "actions": [
    { "id": "notes.add", "label": "Add note", "description": "Adds a note to the list.",
      "category": "Notes", "keywords": ["write", "memo"],
      "appearance": { "label": "Note", "color": "#f2c744", "icon": "📝" },
      "params": [{ "name": "text", "label": "Note", "type": "string", "required": true, "multiline": true }] }
  ],
  "events": [{ "type": "notes.added", "description": "A note was added. Payload: count." }],
  "config": [
    { "key": "limit", "type": "number", "default": 50, "description": "Most notes kept." },
    { "key": "token", "type": "string", "secret": true, "description": "Access token." }
  ],
  "capabilities": []
}
```

## Fields

| Field | Required | Rule |
| --- | --- | --- |
| `schemaVersion` | yes | `1` or `2`. Checked first, so a future shape fails with a clear message. Schema 2 is the current one and adds the library fields below; schema 1 manifests are still read, see [schema 1](#schema-1). |
| `id` | yes | Lowercase dotted, at least two parts, 3 to 64 characters: `[a-z][a-z0-9-]*` per part. Built-in provider ids (`system`, `navigation`, `shell`, `http`, `demo`, `agent-control`) cannot be used because they have no dot, so an installed plugin can never claim, shadow or impersonate one. |
| `name` | yes | 1 to 64 characters, no control characters. |
| `version` | yes | `x.y.z`, numbers only. Your plugin's own version. |
| `description` | no | Up to 280 characters. |
| `author` | yes | 1 to 64 characters. |
| `abi` | yes | Protocol version the module speaks. Must equal the host's, currently `1`. |
| `sdk` | no | SDK version you built with. Informational, checked for compatibility (see [versioning](versioning.md)). |
| `main` | yes | A plain file name ending in `.wasm`: letters, digits, `.`, `-`, `_`. No folders, no `..`, no leading dot, at most 64 characters. |
| `actions` | no | Up to 64. See below. |
| `events` | no | Up to 64. See below. |
| `config` | no | Up to 32 settings. See below. |
| `capabilities` | no | Must be empty in this version. A plugin that lists anything is refused with ``capability `x` is not available in this LaunchDeck``, because this host cannot provide it. The field exists so permissions can be added later without a new manifest shape. |

### Actions

Every action you declare appears in the **Action Library**, with the plugin as its source (*Plugin: Notes*), with no other registration. The person finds it by name, keyword or category, assigns it to a pad and fills in a form drawn from `params`. See [the Action Library](../actions/action-library.md).

| Field | Rule |
| --- | --- |
| `id` | 1 to 64 characters of `[A-Za-z0-9_.:-]`, unique in the plugin. **Stable**: a surface refers to an action as the pair (plugin id, action id), so changing it breaks every pad that uses it. Namespacing it with the plugin's topic (`notes.add`) keeps references readable. |
| `label` | Optional display name, 1 to 64 characters. Free to change. Falls back to the id. |
| `description` | Optional, up to 280 characters. Shown in the library and under the assignment. |
| `category` | Optional, up to 40 characters. The group in the library. Defaults to the plugin's `name`. |
| `keywords` | Optional, up to 12 words of up to 32 characters. Extra search terms. |
| `appearance` | Optional hint for a pad that runs this action: `label` (1 to 40 characters), `color` (`#rrggbb`) and `icon` (a glyph of 1 to 8 characters), each optional. Applied to a new pad, or when the person presses *Use suggested look*. Never applied over what they set. |
| `params` | Up to 16 settings. See below. |

The library id of an action is `<plugin id>.<action id>` (for example `acme.notes.notes.add`). It is derived, so it never changes when a label does. If two providers would claim the same id, the first keeps it and the other is reported in the library's problem list.

#### Params

Each parameter declares one setting. LaunchDeck draws the form and validates it, so your module needs no UI and no checking of its own.

```json
{ "name": "style", "label": "Style", "type": "enum", "default": "short",
  "options": [{ "value": "short", "label": "Short" }, { "value": "long", "label": "Long" }] }
```

| `type` | Value | Extra fields |
| --- | --- | --- |
| `string` | text | `maxLength`, `placeholder`, `multiline`, `monospace` |
| `number` | number | `min`, `max`, `integer`, `placeholder` |
| `boolean` | true or false | none |
| `enum` | one of `options` | `options`: 1 to 64 of `{ value, label }` |
| `reference` | an id of a surface or page | `target` (`surface` or `page`), `scope` |
| `json` | any JSON | for schema 1 compatibility only; prefer another type |

Common fields: `name` (a plain identifier of up to 32 characters, unique in the action), `label` (1 to 40 characters, shown next to the field), `description` (help text, up to 280 characters), `required` (default `false`), `default` and `placeholder`. The full rules and messages are in [parameters](../actions/parameters.md).

Arguments are checked **before** your module runs, against exactly this declaration. Unknown names, wrong types, out-of-range numbers and missing required values fail the call with an invalid-arguments error. Blank text takes its `default`, and defaults are filled in, so the module only ever sees well-typed input. `null` counts as missing.

A manifest whose parameters are malformed (an enum without options, a default that does not fit its type, a `scope` that does not name a surface reference) is refused when it is read, with the action and the parameter in the message.

#### Schema 1

`"schemaVersion": 1` manifests keep working and need no rebuilt module. They declare `args` instead of the fields above:

```json
{ "id": "notes.add", "label": "Add note", "args": [{ "name": "text", "type": "string", "required": true }] }
```

Each `args` entry becomes a parameter whose label is its name, so the action is listed with a plain form. A schema 1 action has no category (it takes the plugin's name), keywords or appearance. Mixing the two shapes is an error either way: `category`, `keywords`, `appearance` and `params` need schema 2, and schema 2 calls `args` `params`. To move a plugin to schema 2, change the number, rename `args` to `params` and add labels; the module does not change.

### Events

`type` follows the same grammar as action ids and must be unique. These prefixes belong to LaunchDeck and are refused: `control.`, `action.`, `surface.`, `page.`, `device.`, `plugin.`, `agent.`, `runtime.`, `system.`. The host stamps the event `source` with your plugin id, so a surface binds to `source: "acme.notes"`, `event: "notes.added"`. Publishing a type you did not declare is rejected.

### Config

Settings the person can change on the Plugins screen. `key` is a plain identifier, `type` is `string`, `number` or `boolean`, `default` must match the type, `description` is up to 280 characters. A setting with `"secret": true` cannot have a default, is never shown and never put in a snapshot or the plugin list, and the host never logs it; the screen only shows whether it is set. Values reach the module in the `init` request. Changing one restarts the plugin so it reads the new value.

## What validation does

Discovery reads each folder, refuses symlinks and anything over the size caps (module 8 MiB), then parses and validates the manifest. A folder that fails is listed under **Installed plugins** with the reason and never runs. A plugin that validates but cannot be loaded (the module is not valid WebAssembly, imports something the host does not provide, lacks an export, answers `init` with a failure, or reports a different `ld_abi` than the manifest) appears with status *error* and the reason. The other plugins are unaffected.
