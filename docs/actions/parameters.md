# Parameters

The settings of an action are declared as a small, closed schema. A provider describes them; LaunchDeck draws the form, checks it before saving and fills in defaults. No provider ships form code, so a plugin cannot put its own UI in the editor.

```json
{ "name": "method", "label": "Method", "type": "enum", "required": false, "default": "GET",
  "options": [ { "value": "GET", "label": "GET" }, { "value": "POST", "label": "POST" } ] }
```

## Kinds

| `type` | Value | Form control | Extra fields |
| --- | --- | --- | --- |
| `string` | text | One-line input, or a text area when `multiline` | `maxLength`, `placeholder`, `multiline`, `monospace` |
| `number` | number | Number input | `min`, `max`, `integer` |
| `boolean` | `true` or `false` | Checkbox | none |
| `enum` | one of the option values | Select | `options`: a list of `{ value, label }` |
| `reference` | the id of a surface or a page | Select of surfaces or pages | `target`: `surface` or `page`; `scope` |
| `json` | any JSON value | Text area that must parse | none. Only for manifest schema 1 plugins that declared `json` arguments. New actions should not need it. |

A field that does not apply to the kind is an error when the provider registers: `maxLength` on a number, `options` on a string and so on.

## Common fields

| Field | Rule |
| --- | --- |
| `name` | The key in `args`. A plain identifier: letters, digits and `_`, not starting with a digit, up to 32 characters. Unique in the action. |
| `label` | What the person reads next to the field. 1 to 40 characters. |
| `description` | Help text under the field. Up to 280 characters. |
| `required` | Defaults to `false`. |
| `default` | Used when the person leaves the setting empty. It must itself pass the kind's rules. |
| `placeholder` | An example shown in an empty field. Up to 120 characters. When there is none and the setting has a text default, the field shows `Default: <value>`. |

An action takes at most 16 settings, an enum at most 64 options.

## Rules

Rust applies them in `normalize`; React mirrors them for instant feedback, with the same messages.

* `args` must be an object. `null` counts as no arguments.
* A name that is not declared is an error: ``\`x\` is not a setting of this action``.
* **Blank** means missing, `null`, or, for `string` and `reference`, text that is only whitespace. A blank takes its `default`. A blank required setting without a default is an error: `<label> is required`. A blank optional setting without a default is left out of the stored arguments.
* `string`: text, at most `maxLength` characters (and never more than 16 384).
* `number`: finite; a whole number when `integer`; within `min` and `max` (`<label> must be between 0 and 7`, `at least`, `at most`).
* `boolean`: `true` or `false`. Anything else is `<label> must be on or off`.
* `enum`: one of the option values.
* `reference`: a non-empty id up to 128 characters. Whether the surface or page still exists is a runtime fact, not a save-time one, because a surface can be deleted after the pad was configured. The editor lists an id that no longer exists as `(not found)` so it is visible, and a press fails with a clear message.
* Every problem is reported, each naming its setting, so a form marks all the wrong fields at once.

The result of `normalize` is the arguments to store or run: declared settings only, defaults filled in, blanks removed.

## References to surfaces and pages

A `reference` to a `surface` is a select of the workspace's surfaces. A reference to a `page` is a select of pages. Its `scope` names a sibling setting that holds a surface id, and the page list then follows that surface:

```json
[ { "name": "surfaceId", "label": "Surface", "type": "reference", "target": "surface", "required": true },
  { "name": "pageId",    "label": "Page",    "type": "reference", "target": "page", "scope": "surfaceId" } ]
```

Without `scope`, the page belongs to the surface being edited. Changing the surface clears every page that is scoped to it, so a page from another surface is never kept by accident. `scope` must name a surface reference of the same action.

## How the form behaves

* A setting that is empty and required is marked *(needed)* but is not called an error until the person has touched the field. Typing a mistake shows the message under the field straight away.
* Saving a slot is blocked while any of its settings has an issue. The rules above answer instantly as a person types. A quarter of a second after the last change, and only when those rules pass, the editor asks Rust (`actions_validate`) and adds the provider's own check (an unsupported URL scheme, an unknown key name), shown under the setting it is about. A slot nobody changed is not checked, exactly as saving does not check it.
* Help text and error messages are attached to the field (`aria-describedby`), and an invalid field is marked `aria-invalid`.
* A value that no longer fits (an enum choice a plugin removed) is shown as `<value> (not a choice any more)` instead of being replaced silently.
* Defaults are written to the saved assignment, so it says what will run.

## The built-in actions as examples

The System actions show every day-to-day case:

| Action | Settings |
| --- | --- |
| Open URL | `url`: string, required. The provider's check allows `http`, `https`, `mailto`, `tel`, `sms` and `facetime`. |
| Launch application | `app`: string, required. `args`: multi-line string, one argument per line. |
| Keyboard shortcut | `keys`: string, required, for example `mod+shift+p`. `mod` is Cmd on macOS and Ctrl elsewhere. |
| HTTP request | `method`: enum, default `GET`. `url`: string, required. `body`: multi-line string. |
| Open surface | `surfaceId`: surface reference, required. `pageId`: page reference scoped to `surfaceId`, optional (the first page). |

No kind for lists exists on purpose: a shortcut keeps its text form and launch arguments are one per line, so the schema stays small.
