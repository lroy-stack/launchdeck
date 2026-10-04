# Observables

An **observable** is anything a control can react to. There are two kinds, and they stay two kinds all the way through:

| | State | Event |
| --- | --- | --- |
| Says | How something is **now** | Something **happened** |
| Has a value | Yes: one scalar, held until it changes | No: a payload that exists for the moment it is delivered |
| Can be asked at any time | Yes, and it answers `known`, `unknown` or `unavailable` | No, an event nobody heard is gone |
| Drives | A look that lasts as long as the state does (a *state rule*) | A look that lasts for a while (an *event rule*) |
| Example | `agent-control.slot.status {slot: 2}` is `approval_required` | `example.counter.counter.changed` with `level: high` |

A state is never a replayed event, and an event is never stored as a state. A plugin that has both ("the counter changed" and "the counter is high") declares both, with different ids.

The **Observable Catalog** is the one list of all of them, whoever offers them. The hub holds the values; the catalog describes them and says what each reads now. It is assembled in Rust and the interface displays it. The rest of this page describes the catalog. How state and feedback rules behave is in [state-and-feedback.md](state-and-feedback.md), and how a plugin offers states is in [plugin-sdk/states.md](plugin-sdk/states.md).

```text
plugin manifest ─┐
Agent Control ───┼─► Source (who offers what) ─┐
the runtime ─────┘                             ├─► Observable Catalog ─► Tauri ─► interface
                  StateHub (values, lifecycle) ┘                          ▲
                                                 explain_control ─────────┘
```

## Who offers what

| Provider | Id | States | Events |
| --- | --- | --- | --- |
| LaunchDeck itself | `runtime` | `surface.active`, `page.active` (the surface and page on screen) | `action.started`, `action.completed`, `action.failed`, `surface.changed`, `page.changed`, `device.connected`, `device.disconnected` |
| Devices | `device` | none | `control.pressed` |
| Agent Control | `agent-control` | `slot.status {slot}`, `slot.focused {slot}`, `command.state {command}` | `agent.online`, `agent.offline`, `agent.access`, `session.created`, `session.started`, `session.ended`, one `session.<status>` per status, `approval.created`, `approval.resolved`, `command.sent`, `command.completed` |
| Demo | `demo` | `alpha`, `beta`, `gamma` | none |
| Installed plugins | the plugin's id | the `states` of its manifest | the `events` of its manifest |

Only things with a real current value are states. The runtime does not publish "is anything running" or similar placeholders: if a state has no honest source, it is not in the catalog. Agent Control's `slot.changed`, `focus.changed` and `command.state` events are a projection kept for bus and WebSocket subscribers and are not listed as observables: the states above are the way to observe them.

The id of an observable is `<provider>.<local id>`. It is derived, stable and never shown. If two providers would claim the same id, the first keeps it and the other is reported in the catalog's `issues`.

## An entry

```jsonc
{
  "id": "example.channels.status",       // provider.local, stable
  "provider": "example.channels",
  "providerName": "Channels",
  "local": "status",
  "kind": "state",                       // or "event"
  "name": "Channel status",
  "description": "Whether one channel works. There is one instance per channel.",
  "category": "Channels",
  "keywords": ["channel", "online", "health"],
  "params": [{ "name": "channel", "label": "Channel", "type": "string", "maxLength": 32, "required": true }],
  "value": { "type": "enum", "values": [
    { "value": "up",   "label": "Up",   "role": "ok" },
    { "value": "down", "label": "Down", "role": "error" } ] },

  "availability": { "status": "available" },
  "instanceCount": 2,
  "instances": [
    { "args": { "channel": "alpha" }, "status": "known", "value": "up",   "updatedAt": 1791133107853 },
    { "args": { "channel": "beta"  }, "status": "known", "value": "down", "updatedAt": 1791133107854 }
  ]
}
```

The top half is the **definition**: it changes when a plugin is installed, replaced or removed. The bottom half is the **live part**: `availability`, `instances` and `instanceCount`. It changes constantly and travels on its own (see [Changes](#changes)).

* **State** entries carry `value` (the type, and for an enum its values with labels and roles) and `params` (what picks one instance). **Event** entries carry `fields`, the payload fields a rule may test, each with a name, label, type and, for an enum, its values. An event has no instances.
* **Availability** says whether the observable can be observed *now*. `{"status": "unavailable", "cause": "provider", "reason": "The plugin is turned off in Plugins."}` is a provider that is disabled or failed, with Rust's own sentence. The `cause` is `provider` or `state` (the provider runs but its state is not reporting).
* **Instances.** A state without arguments has exactly one instance, always: `known`, `unknown` or `unavailable`. A state with arguments lists the instances that hold a value, at most 64; `instanceCount` is always exact. An address that is not listed reads `unknown` while its provider runs, which is also what the hub answers for it. The catalog therefore stays small however many sessions or channels exist.

Statuses mean the same everywhere:

| Status | Meaning |
| --- | --- |
| `known` | The provider reported a value and it is the current one. |
| `unknown` | The provider is running and has not reported this yet. |
| `unavailable` | The provider is stopped, disabled, failed, uninstalled, or does not offer this state. |

## Lifecycle of a plugin's states

| Moment | What every state of the plugin reads |
| --- | --- |
| Plugin installed, or enabled | `unknown`. Its states are declared; nothing has been reported. |
| Plugin publishes | `known(value)` for the instance it named. Writing the same value is not a change. |
| Plugin cleared an instance | `unknown` for that instance. |
| Plugin stops, crashes or is disabled | `unavailable`. Everything it said is dropped. |
| Plugin comes back (enabled, reloaded, restarted) | `unknown`, **never** the old value. |
| Plugin uninstalled | Gone from the catalog. |

Nothing the hub holds is saved to disk. A plugin that restarts has lost its memory with it, so it has to report again; until it does, a pad reads `unknown` and not a stale guess. A plugin cannot report during `init` (those writes are dropped with a warning), so a plugin whose state depends on an action starts `unknown` and becomes `known` the first time an action reports it.

## Changes

The whole catalog is large and its values are volatile, so the interface gets it once and then hears about changes.

* `get_snapshot` carries `observables`: the complete catalog at the moment of the snapshot.
* The `observables-changed` event carries an `ObservablesDelta`:

```jsonc
{
  "catalog": false,                         // true: a definition changed, fetch the catalog again
  "providers": [ /* ProviderInfo of providers whose lifecycle changed */ ],
  "observables": [ { "id": "example.counter.level", "availability": {...}, "instanceCount": 1, "instances": [...] } ]
}
```

| What happened | What the message holds |
| --- | --- |
| A state's value changed | `observables`: that state's live part only |
| A provider started, stopped, failed or was disabled | `providers` with its new info, and `observables` with the live part of **every** observable of the provider, events included (their availability changed) |
| A provider's definitions changed (a plugin installed, replaced or removed) | `catalog: true` and nothing else |

Messages are coalesced for 50 ms, so a burst is one message that holds each observable once, with its latest value. The coalescing is for the interface only: **feedback is evaluated at once**, from the hub, and pads never wait for it. The interface keeps the catalog current by applying the message (`applyDelta` is a pure function), and fetches it again when `catalog` is true, or when a message arrives while a fetch is under way.

## Tauri API

| Command | Returns |
| --- | --- |
| `observables_catalog` | The complete catalog, as above. |
| `observable_state(provider, id, args?)` | What one state reads at one address: `{ status, value, updatedAt }`. The arguments are checked against the definition first, so a wrong or missing argument is an error, not `unknown`. |
| `explain_control(surfaceId, pageId, controlId)` | Why the control looks the way it does. See below. |
| `review_feedback(surfaceId, pageId, control)` | What Rust says about an unsaved draft of a control: what saving would refuse, what it can prove about each rule, the limits, and the draft's own explanation. Read-only. See [feedback-editor.md](feedback-editor.md#what-rust-says-about-a-draft). |
| `get_snapshot` | Now includes `observables`. |

And one event, `observables-changed`, described above. There is nothing to poll.

## explain_control

The answer is built by the same evaluator that decides what the pad shows, from the same hub, so it cannot disagree with the pad.

```jsonc
{
  "surfaceId": "scenario", "pageId": "home", "controlId": "g.0.0",
  "shown": "blink",                       // the look on screen; null is the resting look
  "layer": "overlay",                     // overlay | state | rest
  "overlay": { "look": "blink", "source": "event", "rule": 1, "remainingMs": 29743, "latched": false },
  "derived": "low",                       // what the state rules give, whether or not an overlay covers it
  "winner": 0,                            // the state rule that decided `derived`
  "diagnostics": [ { "kind": "missingLook", "rule": 0, "look": "low" } ],
  "rules": [
    { "index": 0, "kind": "state", "holds": true, "decided": true,
      "observed": { "status": "known", "value": "low", "updatedAt": 1791133107852 },
      "valueLabel": "Low", "look": "low", "lookExists": false,
      "observable": { "id": "example.counter.level", "address": "example.counter.level",
                      "provider": "example.counter", "providerName": "Counter Plugin",
                      "name": "Counter level", "kind": "state" } },
    { "index": 1, "kind": "event", "holds": false, "decided": true, "look": "blink", "lookExists": true,
      "observable": { "id": "example.counter.counter.changed", "kind": "event", ... } }
  ]
}
```

* `layer` is which of the three decides what is on screen: a temporary look (`overlay`) from an event rule, or from the pad's own action running; the state rules (`state`); or nothing (`rest`).
* For every state rule: the observable it reads (with its names, so nothing needs to be looked up), what it reads now, whether its test holds, the value in the words of its definition (`valueLabel`), the look it would show and whether the control has that look.
* `diagnostics` lists a rule that decided on a look the control does not have. The pad shows its resting look, and now the reason is on the record.
* When a state is not `known`, `reason` says why in a sentence: `Channels is running but has not reported `status` for these arguments yet.`, `The plugin is turned off in Plugins.`, `Nothing named `acme.missing` is installed.`, `Hello Plugin does not offer a state called `x`.`

An event rule is listed with the event it waits for. It is `decided` when it put the current overlay there.

## Compatibility

* Manifests of schema 1 and 2 keep loading; they declare no states, and their events appear in the catalog like any other. See [plugin-sdk/manifest.md](plugin-sdk/manifest.md).
* The module protocol is unchanged (ABI 1). Modules built before this milestone run as they are.
* Workspace schema is unchanged (4). The catalog is never saved.
* `actions_catalog` (the Action Library) no longer carries observables: they moved here so there is one authority. Each action definition gains `suggests`, the ids of observables a pad for the action usually shows.

## Cost

A state write is one hash lookup and a comparison; nothing scans the plugin's other states or the controls. A batch of changes builds the delta from the changed keys only, with one pass over a provider's instances per changed state. A scoped state is bounded by the hub (1,024 instances per provider) and the catalog lists at most 64 of them.
