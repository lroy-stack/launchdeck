# States

A plugin can tell LaunchDeck **how something is now**. A pad can then show it for as long as it is true, without the plugin ever knowing there is a pad. This page is for plugin authors. The catalog that lists what you declare, and what the interface shows, is in [observables](../architecture/observables.md).

## Event or state

| | Event | State |
| --- | --- | --- |
| It says | Something happened | How something is now |
| It is | Announced once, then gone | Held until it changes |
| A pad shows it | For a while, then returns to what it was | For as long as it is true |
| Declared in | `events` | `states` |
| Written with | `publish(type, payload)` | `setState(id, value)` |

Ask which one a fact is. "The build finished" is an event. "The build is failing" is a state. "The counter changed" is an event; "the counter is high" is a state. Many plugins have both, and that is correct: declare both, with different ids. LaunchDeck does not turn one into the other: an event is never replayed to find out what a state is, and a state never fires an event.

## Declaring a state

Schema 3 manifests have a `states` list:

```json
{
  "schemaVersion": 3,
  "id": "example.counter",
  "states": [
    {
      "id": "level",
      "name": "Counter level",
      "description": "How full the counter is.",
      "type": "enum",
      "values": [
        { "value": "zero", "label": "Empty", "role": "inactive" },
        { "value": "low",  "label": "Low",   "role": "ok" },
        { "value": "high", "label": "High",  "role": "warning" },
        { "value": "max",  "label": "Full",  "role": "attention" }
      ]
    }
  ]
}
```

| Field | Rule |
| --- | --- |
| `id` | Lowercase letters, digits and `-`, in dotted parts: `level`, `channel.status`. Unique in the plugin. Stable: a pad refers to the state as (plugin id, state id), so changing it breaks the pads that read it. It cannot also be an event type. |
| `name` | 1 to 64 characters. What the person sees. |
| `description` | Up to 280 characters. |
| `category` | Optional, up to 40 characters. The group it is listed under. The plugin's name when absent. |
| `keywords` | Optional, up to 8 words of up to 32 characters. |
| `type` | `enum`, `boolean`, `number` or `string`. |
| `values` | Required for an `enum` (1 to 32 values, no duplicates): `{ "value", "label", "role"? }`. For a `boolean`, optional words for `true` and `false`. Not allowed for `number` and `string`. |
| `params` | Optional. What picks one instance of the state; see [scoped states](#scoped-states). |

The `role` of a value is a hint about what it means: `ok`, `active`, `attention`, `warning`, `error` or `inactive`. It lets the interface suggest a colour; it never forces one, and it is not a colour. The person decides what each value looks like.

### One scalar, no JSON

A state is exactly one scalar: a boolean, a number, a short text (at most 256 bytes) or one value of an enum. There is no state of type `json`, on purpose. A rule compares a state with a value (`is High`, `is at least 3`, `is true`), and a structure cannot be compared that way. If you have a structure, split it into the states a pad could care about (`build.result`, `build.duration`), or send it as the payload of an event, whose fields a rule can test. Numbers are exact up to 2^53.

## Scoped states

When one fact has many instances, declare the state **once** and say what picks the instance. The mechanism is `params`, the same vocabulary as the settings of an action:

```json
{
  "id": "status",
  "name": "Channel status",
  "type": "enum",
  "values": [{ "value": "up", "label": "Up", "role": "ok" }, { "value": "down", "label": "Down", "role": "error" }],
  "params": [{ "name": "channel", "label": "Channel", "type": "string", "required": true, "maxLength": 32 }]
}
```

`example.channels.status {channel: "alpha"}` and `example.channels.status {channel: "beta"}` are two instances of one state. There is no other way to scope a state: not a naming convention, not a list inside a value.

* Arguments are `string`, `number`, `boolean` or `enum` parameters. `reference` and `json` are refused.
* A `string` argument needs a `maxLength` of at most 64, because an address is small. At most 4 arguments.
* A required argument must be given on every write; an optional one takes its `default`. An unknown argument is refused. Two writes that name the same instance in different ways (`2` and `2.0`) are the same instance.
* A plugin holds at most 64 states and 1,024 instances in all. The 1,025th instance is not stored (a warning in the plugin's log), so a plugin that invents a name for every item cannot grow without bound.

## Publishing

With the AssemblyScript SDK:

```ts
import { setState, clearState, Json } from "@launchdeck/plugin-sdk/assembly";

setState("level", "high");
setState("status", "up", Json.object().setString("channel", "alpha"));
clearState("status", Json.object().setString("channel", "alpha"));
```

`setState<T>(id, value, args?)` takes a string, a boolean or a number. It returns `false` when the host refused the write, and the reason is in the plugin's log. `clearState(id, args?)` says an instance has no value now; it reads `unknown` again. For a state without arguments that is rarely what you want: report the new value instead.

In any other language it is the same `publish` import, with a different message (see the [protocol](protocol.md#state-messages)).

What the host checks, every time:

* **The plugin only writes its own states.** The provider is never part of the message: it is the plugin that sent it. A plugin cannot write a state of another plugin or of Agent Control, because it cannot name one.
* The state exists in **this plugin's manifest**, otherwise the write is refused as not allowed.
* The arguments fit the declaration, the value has the declared type, and an enum value is one the state lists; otherwise it is refused as invalid.
* The message is at most 2 KiB and one call writes at most 64 instances. The hub's limits above are applied when the call returns, so a write that exceeds them is dropped with a warning in the log rather than refused to the module.

A refused write never changes anything and never stops your plugin. It is written to the log.

### When it takes effect

States written during a call are delivered when the call returns normally. If the call traps or runs out of fuel, **its states are dropped**, like its events, so a call that failed halfway does not leave a half-true picture. A call that returns `failed` keeps them: report the state *and* fail when that is what happened (the Counter does so at its maximum).

### What to report, and when

Report a state **whenever you learn it**, not only when it changes: writing the value a state already has is not a change and costs almost nothing. An action that fails because "the counter is already at the maximum" is a moment to report `max`.

A write made inside `init` is ignored, with a warning in the log. A plugin has nothing to say before it has done anything, so a state starts `unknown` and becomes `known` when an action first reports it.

## Lifecycle

| Moment | What the state reads |
| --- | --- |
| The plugin is installed or starts | `unknown`: declared, nothing reported |
| It publishes | `known`, with the value |
| It stops, crashes, is disabled or is uninstalled | `unavailable`: everything it said is dropped |
| It returns (enabled again, reloaded, settings changed) | `unknown`, **never the old value** |

LaunchDeck does not save states. A plugin that restarts has lost its own memory, and a value from before the restart would be a guess shown as a fact. So a pad reads `unknown` until the plugin reports, and `unavailable` while it cannot. If a state has to survive a restart, that is the plugin's job (a setting is the only thing a plugin can keep today), and it should report the restored value once it has one.

Whether it is the plugin's status that changed or a call that failed, the lifecycle is applied in one place, so a crash and **Disable** read the same on every pad.

## What the person sees

Nothing about states needs UI from you. Everything you declare appears in LaunchDeck's [Observable Catalog](../architecture/observables.md), listed under your plugin with its name and description, with what it reads now, with the same treatment as the states of LaunchDeck and Agent Control. A rule on a pad can read it (`Counter level is High`), and LaunchDeck can explain it (`Counter Plugin is running but has not reported level yet`).

### Suggesting a state for an action

An action may say which of your states or events a pad for that action usually shows:

```json
{ "id": "counter.increment", "label": "Increment", "suggests": ["level"] }
```

`suggests` lists up to 8 ids from your own `states` and `events`. It is a hint for the interface, never applied by itself and never an error to ignore. A suggestion that names something you do not declare, or names it twice, is a manifest error.

## Events carry more in schema 3

An event may also say how it should be listed and what its payload holds, so a rule can test its fields:

```json
{
  "type": "counter.changed",
  "name": "Counter changed",
  "description": "The value changed.",
  "fields": [
    { "name": "value", "label": "Value", "type": "number" },
    { "name": "level", "label": "Level", "type": "enum",
      "values": [{ "value": "zero", "label": "Empty" }, { "value": "max", "label": "Full" }] }
  ]
}
```

`name` defaults to the type. `fields` are optional, at most 16, each a unique plain name with a type (`enum`, `boolean`, `number`, `string`) and, for an enum, its values. They describe the payload; the host does not enforce them on publish.

## Compatibility

Nothing here changes the module. A plugin built before this release, with a schema 1 or 2 manifest, loads and runs exactly as before; it simply declares no states. Moving to schema 3 means changing `schemaVersion`, adding `states` (and optionally event `name` and `fields`, and `suggests`) and adding the calls to `setState`. The protocol stays at ABI 1, so a rebuilt module is only needed to call `setState`. See [versioning](versioning.md).

## Examples

* `examples/plugins/counter` declares `level`, an enum, and reports it from every action. It also keeps publishing the `counter.changed` event, because "it changed" and "it is high" are different facts.
* `examples/plugins/channels` declares `status` scoped by `channel`: one state, as many instances as you name channels.
