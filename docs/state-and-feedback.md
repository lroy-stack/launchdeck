# State and feedback

An **event** is something that happened. The bus carries it and forgets it. A **state** is how things are now, and the **state hub** holds exactly that. What a control looks like is a pure function of its rules and the hub, so a pad is right the moment it exists, with no event in flight.

```text
providers ──► StateHub (current values only) ──► feedback rules ──► look ──► renderer ──► devices
events    ──► temporary look (with an expiry) ───────────────────────┘
```

The design and its reasons are in [STATE_FEEDBACK_PLAN.md](../STATE_FEEDBACK_PLAN.md). This page describes what is built. Everything that can be observed, states and events from every provider, is listed in the [Observable Catalog](observables.md); installed plugins declare states in their manifest, see [plugin-sdk/states.md](plugin-sdk/states.md).

## State addresses

A state has a provider, a definition and, when one definition has many instances, arguments.

```jsonc
{ "provider": "agent-control", "id": "slot.status", "args": { "slot": 2 } }
{ "provider": "demo",          "id": "alpha" }
```

* `provider` and `id` are stable lowercase dotted identifiers (segments of letters, digits and `-` joined by dots, such as `agent-control` or `slot.status`, up to 64 characters). Nothing in the hub or the evaluator knows a provider by name.
* `args` are checked against the definition's parameter schema, so an unknown argument is refused, defaults are filled in, and `2` and `2.0` name the same instance. At most four arguments of up to 64 bytes each.
* A provider declares up to 64 definitions and holds up to 1,024 instances.

A definition says what a state is for people: its name, a description, its value type (`enum`, `boolean`, `number`, `string`), the values of an enum with a label and a role (`active`, `ok`, `warning`, `attention`, `error`, `inactive`), and its parameters. It is what the Observable Catalog lists, together with what each state reads now.

## The hub

A value has one of three statuses:

| Status | Meaning |
| --- | --- |
| `known` | The provider reported a value and it is the current one. |
| `unknown` | The state exists and nothing has reported yet. |
| `unavailable` | The provider is stopped, disabled, failed or uninstalled, or does not declare the state. |

The hub keeps the current value, its status and when it was reported. It keeps no history, it is not a bus, and **nothing it holds is ever saved**.

| Moment | Result |
| --- | --- |
| Provider starts | Its states are declared and every value is `unknown`. |
| Provider reports | `known(value)`. Writing the same value again is not a change. |
| Provider stops, fails or is disabled | `unavailable`, and everything it said is dropped. |
| Provider returns | `unknown` until it reports again. An old value is never shown as current. |
| Provider is uninstalled | Declarations and values go with it. |

For an installed plugin the lifecycle is applied where its status changes, so a crash reports the same thing as disabling it. Agent Control, the Demo plugin and the runtime's own states are always running. Plugins write only the states their own manifest declares; the host checks the state, the arguments and the value against the declaration before the hub sees them, and the hub itself is never exposed to a plugin.

Readers never block writers for long: the hub's lock is a leaf (nothing is called while it is held), writes wake a single waiter, and a burst of writes to one state is one entry in the next batch of changes.

## Feedback rules

A control carries one ordered list, `feedback`. There are two kinds of rule.

**A state rule** is continuous: while a state satisfies a test, show a look. The first state rule that is true decides, and if none is, the pad rests.

```json
{ "when": { "state": { "provider": "demo", "id": "alpha" },
            "is": { "op": "equals", "value": "running" } },
  "show": "busy" }
```

Tests: `equals`, `notEquals` (both on `known` values only), `lessThan`, `atMost`, `greaterThan`, `atLeast` (numbers), `hasValue`, `noValue` (the state is `unknown`) and `unavailable`. A state with no value is never "not equal" to anything. Numbers compare as numbers, so `1` equals `1.0`; a text never equals a number.

A rule names its look one of two ways: `"show": "name"`, or `"showFromValue": true` for the look named after the state's value (`running`, `approval_required`, `true`, `3`). The look `idle` means "rest" unless the control has a look called `idle`. A rule that names a look the control does not have is **not an error and not silent**: the pad rests and the explanation says which rule named which missing look.

**An event rule** is momentary: when this event arrives, show a look for a while.

```json
{ "when": { "event": { "provider": "ci", "id": "build.finished" },
            "where": [ { "field": "result", "equals": "failed" } ] },
  "show": "failed", "hold": { "ms": 4000 } }
```

`where` compares payload fields as values, `showFromField` names the look after a payload field, and `hold` is `{"ms": 200 to 60000}` (default 3 s) or `"latch"`. A temporary look sits **over** the look the state rules give. When it ends the pad returns to what the state rules say now. Events are never written to the hub or to the file. The newest event replaces an older temporary look of the same control, and an older timer cannot take a newer look away. `latch` keeps the look until another event replaces it, `idle` clears it or the control is edited; it exists so that migrated bindings behave as they did.

What decides a pad's look, top to bottom: a temporary look (an event rule, or the automatic `running` / `success` / `error` of a pad **without** feedback rules), then the state rules, then the resting look.

### The gate

`update_control` checks every rule that is new or changed: it must name a state something offers, its arguments must fit, a comparison must make sense for the value's type (a number test on text is refused, an enum value must be one of the listed ones), exactly one way of choosing the look, a hold in range, at most 32 rules and 8 payload conditions. A rule that is already saved is not re-checked, so a pad whose plugin was uninstalled can still be renamed and recoloured.

### Cost

An index maps each state, provider and event to the controls that read it, and is rebuilt when the workspace changes. A change evaluates exactly the controls that read it; an event nobody listens to is one hash lookup. `Runtime::feedback_stats()` counts evaluations, routed events and events the bus dropped, and `Runtime::explain_control()` (the Tauri command `explain_control`) reports for one pad which layer shows, and for every rule the observable it reads, what it reads now, whether it is true, whether its look exists and, for a state that is not `known`, why. See [observables.md](observables.md#explain_control).

## Who provides states today

| Provider | States |
| --- | --- |
| `agent-control` | `slot.status {slot}`, `slot.focused {slot}`, `command.state {command}`. The agent domain stays authoritative; its projection is written to the hub on every change, and the track, marker and command pads read it. Nothing is replayed to repaint a surface. |
| `runtime` (LaunchDeck) | `surface.active`, `page.active`: the surface and page on screen. |
| `demo` | One state per simulated agent (`alpha`, `beta`, `gamma`): `idle`, `running`, `waiting`, `approval-required`, `success`, `error`. |
| Installed plugins | The `states` of their manifest (schema 3), written with `setState`. The Counter reports `level`; Channels reports `status {channel}`. A plugin that declares none keeps working: its events work as event rules. |

The event projection of Agent Control (`slot.changed`, `focus.changed`, `command.state`) is still published, for subscribers of the bus and the WebSocket.

## Workspace schema 4

`bindings` became `feedback`. On the first start with a schema 3 file the app keeps a copy next to it (`workspace.json.v3.bak`) and converts in memory; the new file is written the first time something is saved.

| Schema 3 binding | Schema 4 |
| --- | --- |
| `agent-control` `slot.changed`, `slot` = n, `$payload.status` | State rule on `slot.status {slot: n}`, `hasValue`, look from the value |
| `agent-control` `focus.changed`, `slot` = n, `focused` | State rule on `slot.focused {slot: n}` equals `true`, show `focused`. The catch-all `idle` after it is dropped: no rule true is already rest |
| `agent-control` `command.state`, `command` = c, `$payload.state` | State rule on `command.state {command: c}`, `hasValue`, look from the value |
| `demo` `plugin.state.changed`, `key` = a, `$payload.value` | State rule on `demo.a`, `hasValue`, look from the value |
| Anything else | Event rule on the same source and event, same payload tests, same look, `hold: latch` |
| What cannot be expressed | Kept word for word in `legacyBindings`; inert; the inspector says so |

Only the first four were states in disguise, so only they became state rules; everything else keeps the behaviour it had. The conversion is tested against the schema 3 workspace the last release shipped: it migrates to exactly the workspace the code builds today, control by control, and the Agent Control surface draws the same LEDs.

A schema 4 file holds rules only: never a value, a frame or a device.

## Not built yet

A rule editor and a rule explanation drawn in the inspector (the inspector lists the rules read-only; the explanation is available through `explain_control`), staleness timers, `and` / `or`, scripting and aggregate states ("anything needs me").
