# State and feedback plan

Status: research and plan, written before any implementation, 2026-10-04. No product code was changed. The only code written for this phase was a throwaway integration test (nine probes against the real runtime and the real compiled Counter plugin); its results are in section 1 and it was removed afterwards. Section 18 turns each probe into a permanent test.

The Action Library answers "what should this control do?". This milestone answers "what external state should this control represent?", so that a pad can both trigger something and keep showing what is true, and so that a person can set that up without JSON, payload paths, source ids or event-bus knowledge.

```
Providers                  State hub                Feedback rules            Look                   Renderer
(Agent Control, installed  latest value + status    per control, ordered,     one look name per      existing: look ->
 plugins, built-ins)    ->  per state, lossless  -> first match wins       ->  control, derived  ->   Visual -> frame ->
                            (Rust, leaf lock)       (pure function)           from the hub            device
Events (bus) ----------------------------------------> event rules: a momentary overlay on top of the derived look
```

## Decisions at a glance

| Brief | Decision |
| --- | --- |
| Event and State are different things | Yes, formally. Events are things that happened and are never stored. States are named current values with a status and are stored. Both are listed in one authoring catalog (section 7). |
| A. Do we need a State Registry? | **Hybrid.** The bus stays the transport for events. A small materialized layer, the state hub, keeps the latest value of each declared state. Proven necessary by probes P1, P2 and P7 and by the lifecycle trace; the bus and today's bindings cannot provide it (sections 1 to 4, 6). |
| B. State Definition | The smallest model in which every field has a stated requirement (section 7). Current value, status and timestamp are not part of a definition; they live in the hub, the same split as `ActionDefinition` and `Assignment`. |
| C. Event and State relationship | Three producer styles, all supported: derived from a domain runtime (Agent Control), projected from declared events (installed plugins), event only (`workflow.completed`). |
| D. Rehydration | Rendering never waits for an event. A control's look is a pure function of its rules and the hub; it is recomputed when a value, a rule, a provider status or the workspace changes (section 8). |
| E. Precedence | Ordered list, **first match wins**, one look per control. Event rules are a momentary overlay above the state rules. Nothing depends on arrival order (section 9). |
| F. Conditions | `is`, `is not`, `is below`, `is at most`, `is above`, `is at least`, `has a value`, `has no value yet`, `is not available`. No AND/OR, no expressions (section 10). |
| G. Unknown, stale, offline | Three statuses: `known`, `unknown`, `unavailable`. No time-based `stale` in v1, with the reason and the trigger for adding it (section 11). |
| Observable | One catalog type, `ObservableDefinition`, with `kind: state \| event`. It is justified by the authoring flow, not by elegance (section 7). |
| Plugin SDK | Manifest schema 3, additive: `states`, richer `events`, `suggests` on actions. ABI stays 1, SDK stays 0.1.0, no `.wasm` rebuild (section 12). |
| Persistence | Workspace schema 4. `bindings` becomes `feedback`. Only rules and looks are saved. Values are never saved. Automatic migration with backup (section 15). |

## 1. Current architecture assessment

LaunchDeck already has most of the machinery: a single bus, a semantic look per control (`Control.states`, a map of name to `Visual`), a renderer that turns a look into a device frame, and a runtime that remembers one look name per control. The problem is not missing parts. It is that **the runtime remembers the result of evaluating an event, never the thing the event was about.**

```
event ──► binding_state(event) ──► Inner.states[control] = "failed" ──► renderer
          (evaluated once, at arrival)      (a cache of a result; the input is gone)
```

Everything that goes wrong follows from that. Once the event is gone, the result cannot be recomputed: not when the rules change, not after a restart, not when a provider disappears.

Three different mechanisms hold "current state" today, each correct for its own case:

| Where | What it holds | Survives |
| --- | --- | --- |
| `Inner.states` in `core/runtime.rs` | One look name per control (`surface/page/control`), written by bindings and by the automatic running/success/error cycle | Surface and page switches, device reconnects. Not restarts, not new or edited pads |
| Agent Control `State` and `Projection` in `agents/service.rs` | The authoritative sessions, slots, focus and command states, diffed into `slot.changed`, `focus.changed`, `command.state` events; `republish()` replays the whole projection | Everything, but only for Agent Control, and the replay is a private workaround |
| Plugin memory (for example the Counter's `count`) | The plugin's own value | Nothing the host can see. The host only learns it when the plugin publishes during an action call |

There is also a half-built concept that points the right way: `PluginManifest.states: Vec<StateDef { key, description, values }>` exists, the Plugins screen displays it, and Agent Control and the demo plugin fill it. But Agent Control puts event names in it (`slot.changed`), installed plugins always leave it empty, and nothing in the editor uses it. This plan replaces it with a real model (section 7).

### Verified, not assumed

Nine probes were run against the real runtime (`Runtime::start`, the virtual Launchpad X, the real compiled `counter.wasm`). Each asserts what the code does today, so a pass means the reading of the code is right.

| Probe | Setup | Observed | Meaning |
| --- | --- | --- | --- |
| P1 restart | A pad bound to `ci/build.finished` shows red. The workspace is saved and a new runtime starts from the file. | The pad shows its resting blue. | States do not survive a restart, and nothing can rebuild them. |
| P2 late binding | The event fires first. Then a pad with a binding to it is created. | Resting blue, until the next event. | A new or edited pad never learns what is already true. |
| P3a inactive surface | A pad on a surface that was never active; the event fires; the surface is opened. | Red immediately. | Today's global routing already makes surface and page switches correct. This is kept. |
| P3b device reconnect | The device is unplugged, the state changes, the device is plugged in. | The new state is drawn. | The render loop reads state every tick, so reconnects are already correct. This is kept. |
| P4 precedence | Two rules of one binding match; two bindings of one source match; two sources fire in turn. | First rule wins inside a binding, **last binding wins** across bindings, **last event to arrive wins** across sources. | The result depends on arrival order. It cannot be reconstructed after a restart. |
| P5 equality | `json!(1)` against `1.0`. | Not equal. | A condition written as `1` misses a plugin that sends `1.0`. |
| P6 unknown look | A rule sets a look name the control does not define. | The state is recorded; the LED shows the resting look. | A typo is silent and looks like "nothing happened". |
| P7 plugin restart | Counter at `high`; disable the plugin; enable it; its count is now 0. | The pad still shows `high`, before and after. | A pad can show a value the provider no longer has, forever. |
| P8 bus lag | 2,000 events published before a reader runs. | The reader gets `Lagged(976)`, then the oldest survivor is event 976. | The bus drops events for a slow reader. The runtime loop ignores it (`Lagged(_) => continue`), so a state carried only by events can be lost for good. |
| P9 cost | 5,000 unrelated events, starter workspace (109 controls) and a synthetic one (2,669 controls). | 4 to 6 µs per event, and 25 to 29 µs per event (debug build, two runs). | Routing scans every control of every surface for every event. Fine today, linear in the size of the workspace. |

Conclusion for decision A: events plus today's bindings are sufficient for P3 only. They cannot provide correct current-state projection for P1, P2, P7 or P8. The hybrid is not a preference; it is the smallest thing that fixes the failures that were reproduced.

### What stays as it is

| Piece | Reused as it is |
| --- | --- |
| `Control.states` (looks), `Visual`, `Animation` | The only appearance model. No second visual-state system. |
| `renderer::compose`, `resolve_visual`, `adapt` | Unchanged. See section 4 (renderer). |
| `Runtime::render_tick`, device manager, devices | Unchanged. They already repaint from state every 40 ms and a reconnect starts from a cleared cache. |
| Event bus and WebSocket | Unchanged transport for events. |
| `ActionRegistry`, `ParamDef`, generated forms, `Availability` | Reused for state addresses, validation messages and provider status. |
| Plugin host, sandbox, ABI 1 | Unchanged. |
| Workspace persistence, atomic writes, versioned migration with backup | Extended with one more step. |

## 2. Current event bus behavior

All of this was read in `events/mod.rs`, `events/external.rs` and `core/runtime.rs`, and the lag and ordering rows were run (P8, P4).

| Property | Today |
| --- | --- |
| Structure | `Event { source, type, payload: Value, ts }`. `source` and `type` are free text; clients are limited to `[A-Za-z0-9_.:-]{1,64}`. |
| Routing | One `tokio::sync::broadcast` channel. Every subscriber sees every event. The runtime's loop is the only consumer that matters. |
| Retention | None on the bus. The runtime keeps a ring of the last 100 events (`recent`) for the UI log only; evaluation never reads it. Projection events from Agent Control are kept out of the ring. |
| Ordering | `send` order, preserved per receiver. Producers are plugin host threads, WebSocket tasks, Agent Control (under its lock) and the runtime, so cross-producer order is whoever called `send` first. |
| Replay | None. A subscriber sees only events published after it subscribed. |
| Lag | Capacity 1024. A slow reader loses the oldest events. The runtime loop discards the notice without a log line or a resync (P8). |
| Persistence | None. |
| Ownership | `Runtime.bus`, cloned into Agent Control, the action engine, the plugin context and the WebSocket server. |

Direct answers to the brief:

* **Are events ephemeral?** Yes.
* **Is any latest-value state retained?** Yes, in three unconnected places (table in section 1), none of them in the bus.
* **Can a new Surface reconstruct current state from existing runtime data?** Only if the pad existed when the event arrived (P3a). Not for a new or edited pad, a new surface, or after a restart (P1, P2).
* **Can a device reconnect and immediately render correctly?** Yes (P3b), because the render loop recomposes from `Inner.states` every tick and the device cache is cleared on connect. That part of the design is sound and stays.

## 3. Current binding behavior

Read in `core/model.rs` (`Binding`, `BindingRule`, `Condition`) and `core/runtime.rs` (`binding_state`, `route_bindings`).

| Question | Today |
| --- | --- |
| How is source and event selected? | `binding.source == event.source`, then `rule.event == event.type`, both exact string equality. |
| How are conditions evaluated? | `rule.when` is a list; every entry must satisfy `event.payload[field] == equals` on a **top-level** field, with JSON structural equality (P5). |
| How is a semantic state assigned? | `setState` is a literal, or `$payload.<field>` to take a string field of the payload. A missing or non-string field skips the rule. |
| Order dependence? | Yes, three kinds: rule order inside a binding (first match wins), binding order inside a control (last match wins, because results are applied in sequence), and event arrival order across sources (last wins). P4. |
| Several bindings on one control? | Allowed on purpose ("a control can listen to several sources"). |
| Several rules match? | See above. |
| No rule matches? | Nothing happens. The previous look **stays**. A binding never says "now it is idle" unless a rule for that exact event does. |
| Provider disappears? | Nothing. The binding keeps its source string and the pad keeps its last look (P7). No flag, no message. |
| Persistence? | `bindings` array inside each control in `workspace.json` (schema 3). Only the Agent Control surface and hand-edited files use it; the inspector shows bindings read-only. |
| Depends on event timing? | Yes (P2, P4). |

### Hidden conventions and special cases

1. `setState` starting with `$payload.` is a mini-language for "take this field". Easy to mistype, silently ignored when wrong.
2. The look name `idle` is reserved: it removes the control's state unless the control defines an `idle` look of its own.
3. Controls with at least one binding are excluded from the automatic running/success/error cycle that unbound controls get.
4. A look name the control does not define draws the resting look without a warning (P6).
5. Routing walks **every control of every surface and page** for every event. This is why inactive surfaces are correct (P3a) and also why cost is linear in the workspace (P9).
6. Agent Control's `slot.changed`, `focus.changed` and `command.state` are hidden from the event log by source and type (`is_projection`). They are state in disguise: they only exist so that bindings can follow Agent Control.
7. `Lagged` is ignored (P8).
8. The sources `runtime`, `device`, `agent-control` and every installed plugin id are reserved against spoofing by WebSocket clients. Unchanged by this plan.

## 4. Surface, device and renderer lifecycle

### Lifecycle trace (mandatory)

"Recompute" means: is the look of a pad derived again from something that is true now.

| Moment | What happens today (function) | Recompute? | Gap | Plan |
| --- | --- | --- | --- | --- |
| LaunchDeck starts | `Store::load_or_default` migrates the file; `Runtime::start` builds the bus, Agent Control, the registry, scans plugins (`spawn_blocking`), connects the virtual device, picks `last_active_surface`, starts the event, render and rescan loops, then `agents.republish()`. `Inner.states` starts empty. | No. Only Agent Control replays itself. | Plugin and WebSocket states are simply absent (P1). | Providers report into the hub; `evaluate_all` runs before the first render tick (section 8). |
| Workspace loads | `persistence::migrate` v1/v2 to v3. Nothing about states. | No. | Rules cannot be checked against values that do not exist yet. | v3 to v4 migration; indexes built after load. |
| A Surface becomes active | `activate_surface` sets ids, clears held keys, saves `last_active_surface`, publishes `surface.changed` and `page.changed`, calls `agents.republish()`, emits state. | No. The look already sits in `Inner.states` for every control. | `republish` exists because a surface "may have missed projection events", i.e. P2. | Not needed once rules are evaluated from the hub on every change. Removed from navigation. |
| A Page changes | `activate_page` sets the id, publishes `page.changed`, emits state. | No. | None for existing pads. | Unchanged. |
| A plugin connects | `PluginHost::scan`, `reload`, `set_enabled` change the registry. Nothing is published on the bus. | No. | Pads keep showing the old value (P7). | Host reports provider up: its states become `unknown`, dependent pads are re-evaluated. |
| A plugin disconnects | Same entry points. | No. | Same. | Host reports provider down: its states become `unavailable`, dependent pads are re-evaluated. |
| A device connects or reconnects | `connect_device` or `rescan`; the Launchpad clears its `last` cache and enters programmer mode; `announce` publishes `device.connected`. The next tick composes a full frame. | Yes, trivially: the frame is drawn from current looks (P3b). | None. | Unchanged. |
| Agent Control changes state | `mutate` then `flush` publishes events and `publish_projection` (diffed); the runtime loop routes them to bindings; `emit_state`. | Yes, per event. | Depends on bus delivery (P8). | `publish_projection` also writes the hub directly. Same diff, lossless path. |
| Bus lags | `Lagged` is dropped. | No. | Silent loss (P8). | Logged once with a count; Agent Control is asked to republish; event rules are the only thing that can be lost, and that is documented. |

### Renderer (decision: unchanged)

`compose(page, state_of, caps, t)` already is the right abstraction:

```
look name ──► resolve_visual(control, name) ──► Visual { color, animation } ──► adapt(caps, t) ──► Led ──► device
```

| Need | Already handled by |
| --- | --- |
| Static look | `Animation::None`. |
| Pulse, flash | Native on devices that report `animations.pulse/flash`; otherwise synthesised from timed frames (`pulse_factor`, `flash_on`). Covered by renderer tests. |
| Dimmed, offline | An ordinary look with a darker colour. Agent Control does exactly this (`Offline` is 30% grey, `Unavailable` command is 10% of the base colour). No dim modifier is needed. |
| Unsupported capabilities | Positions the device lacks are dropped; non-RGB devices receive luminance only. |
| Unknown look name | Falls back to the resting look. Kept, but no longer silent: the editor creates looks as it creates rules and the explain panel reports a missing one (section 14). |

The desktop pad grid draws the Rust-composed `virtual-frame`, so the UI is not a second implementation. Nothing in this plan touches `renderer/`.

## 5. External research findings

Read on 2026-10-04. For each pattern: the problem it solves, whether LaunchDeck has that problem, and the decision. Dates are the ones the pages show; where a page shows none, it says so.

### 5.1 Home Assistant: state object, `unknown` and `unavailable`

Sources: [State and state object](https://www.home-assistant.io/docs/configuration/state_object/) (no date on the page) and [Mark entity unavailable if appropriate](https://developers.home-assistant.io/docs/core/integration-quality-scale/rules/entity-unavailable/) (2 Aug 2026).

A state object carries `state`, `attributes`, `last_changed`, `last_updated` and `last_reported`. Two values mean "no value": `unavailable` ("The entity cannot provide its state right now") and `unknown` ("The entity has no value for its state"). The quality rule says an entity whose data cannot be fetched must be unavailable, and one that is only missing a few pieces must be unknown, so that consumers see "a better state, than just showing the last known state".

* **Problem solved.** Automations and dashboards must not act on a value that is no longer true.
* **Does LaunchDeck have it?** Yes, exactly: P7, a pad lying after a plugin restart.
* **Decision.** Adopt the two non-value statuses with Home Assistant's distinction, plus one timestamp (`updatedAt`). Reject the `attributes` dictionary (a state here is one scalar; anything else is another state), the entity registry and restored-state persistence (the brief forbids persisting values), and three separate timestamps (nobody needs `last_changed` apart from `last_updated` yet).

### 5.2 Bitfocus Companion: feedbacks

Sources: [Feedbacks, user guide v5.0](https://companion.free/user-guide/v5.0/config/buttons/creating/feedbacks/) and [Module Feedback Definitions](https://companion.free/for-developers/module-development/connection-basics/feedbacks/) (neither page shows a date).

Boolean feedbacks: "when their condition is true, your configured overrides are applied". They are applied top down and "a feedback lower in the list wins", which reverses Companion 4.x, where the higher one won. Feedbacks override properties of button elements. When a feedback is added "the callback will be called", so it is evaluated at once. Modules re-evaluate by feedback type (`checkFeedbacks('id')`) instead of everything.

* **Problem solved.** Showing live device state on a button. This is the closest product to the one being designed here.
* **Does LaunchDeck have it?** All four sub-problems: condition to look, several matches, re-evaluation scope, first evaluation (P2).
* **Decision.** Adopt: a condition that selects a look, a visible order, evaluation the moment a rule is added or edited, and re-evaluation scoped to what a rule depends on (the dependency index of section 16). Reject: merging per-property overrides in order, because a LaunchDeck look is one atomic `{color, animation}` and there is nothing to merge; and "lowest wins", because rules here do not stack, so "first match wins" reads like a switch statement. That Companion reversed its own order between 4.x and 5.0 is evidence that the order must be visible to the person, which is why section 14 has an explain panel.

### 5.3 Elgato Stream Deck SDK: key states

Source: [Keys guide](https://docs.elgato.com/streamdeck/sdk/guides/keys/) (no date on the page).

States are declared in the manifest ("up to two states"). A plugin calls `setState`, `setImage` and `setTitle` on an action instance. A user-chosen image or title takes priority over what the plugin sets, which takes priority over the manifest default.

* **Problem solved.** Showing on/off on a key from plugin logic.
* **Does LaunchDeck have it?** It solved it the other way on purpose: plugins never see pads.
* **Decision.** Adopt the precedence principle: the person's look beats the provider's suggestion. Reject the push model per instance (the plugin must know instances, which breaks "one plugin works on every controller"), the two-state cap and plugin-set images. A plugin publishes a value; the surface decides how it looks; a plugin may attach only a `role` (section 7).

### 5.4 VS Code: when clauses and context keys

Source: [when clause contexts](https://code.visualstudio.com/api/references/when-clause-contexts) (updated 30 Sep 2026).

Extensions set named context keys with `setContext`. Conditions combine them with `!`, `&&`, `||`, `==`, `!=`, `>`, `>=`, `<`, `<=`, `=~`, `in`, `not in` and parentheses.

* **Problem solved.** Enable UI from current context without the UI knowing the sources.
* **Does LaunchDeck have it?** Partly: `Availability` already covers enabling an action. Feedback needs conditions on values.
* **Decision.** Adopt "a context key is the latest value under a stable name" (that is the hub) and the six comparison operators. Reject `&&`, `||`, `!`, parentheses, regular expressions and `in`: an expression string is a language to learn and to parse, the brief forbids scripting, first-match order already gives OR, and provider-derived states give AND (Agent Control's `command.state` is computed by the provider from several facts).

### 5.5 MQTT: retained messages and Last Will

Source: [MQTT Retained Messages Explained: Last Known Value](https://scadaprotocols.com/mqtt-retained-messages/) (28 May 2026).

The broker stores the last retained message of a topic and hands it to a late subscriber on subscribe, before anything new. Its hazard: "A device that crashed three hours ago with a retained Online status still has that status on the broker until something updates it". The standard answer is a retained `Online` on connect plus a Last Will that publishes a retained `Offline` on ungraceful disconnect.

* **Problem solved.** A late consumer needs the current value without waiting, and a retained value alone lies when its producer dies.
* **Does LaunchDeck have it?** Both halves: P2 (late consumer) and P7 (producer restart).
* **Decision.** Adopt: the last value is retained for late consumers (the hub); producer lifecycle drives availability (the Last Will analogue is the plugin host marking a provider down); producers report their state when they connect. Reject the broker, QoS, topic wildcards, and persisting retained values to disk, because a persisted retained value is the stale-value hazard itself.

### 5.6 Event sourcing and materialized views

Source: [Event Sourcing pattern, Microsoft Learn](https://learn.microsoft.com/en-us/azure/architecture/patterns/event-sourcing) (updated 28 Mar 2026).

Event sourcing "might not be suitable" for "straightforward CRUD operations that don't require auditability, replay, or historical reconstruction". Replaying large streams is costly and is mitigated by snapshots; materialized views are "read-only projections of the event store"; consistency between store and projections is only eventual.

* **Problem solved.** Audit, history and rebuilding state from what happened.
* **Does LaunchDeck have it?** No. There is no history requirement (historical analytics is out of scope) and only the current value matters.
* **Decision.** Adopt only the idea of a materialized latest-value projection. Reject the event store, replay and snapshots: events are not durable here, replaying them would resurrect momentary ones (a `deployment.failed` flash returning after a restart), and eventual consistency over a lossy bus is the P8 hazard.

### 5.7 Derived state (Redux selectors)

Source: [Deriving Data with Selectors](https://redux.js.org/usage/deriving-data-selectors) (updated 24 Sep 2026).

"Keep the Redux state minimal, and derive additional values from that state whenever possible", because stored derivations need logic to keep them in sync. Memoized selectors recompute only when their inputs change.

* **Problem solved.** Caches of derived data drift from their source.
* **Does LaunchDeck have it?** Yes: `Inner.states` is a stored derivation whose inputs are gone (section 1).
* **Decision.** The look of a control is a pure function of its rules and the hub. The cached derived map is rebuilt from that function and is never authoritative. Recompute only when inputs change is the dependency index. Reject a general reactive or selector framework: one function and one index are enough.

### 5.8 Latest-value channels (`tokio::sync::watch`)

Source: [tokio::sync::watch](https://docs.rs/tokio/latest/tokio/sync/watch/index.html) (tokio 1.53.2): "a multi-producer, multi-consumer channel that only retains the last sent value"; a receiver can miss intermediate values.

* **Problem solved.** Coalescing when only the latest value matters.
* **Does LaunchDeck have it?** The semantics, yes: a pad needs the latest value, never every value.
* **Decision.** Adopt the semantics (a dirty set coalesces bursts per key). Reject the primitive: one channel per state would mean thousands of channels; a map, a dirty set and one `Notify` do the same job with less machinery.

## 6. Event versus State: the decision

### 6.1 The distinction

| | Event | State |
| --- | --- | --- |
| Meaning | Something happened | Something currently has a value |
| Example | `workflow.completed`, `notification.received`, `deployment.failed` | `example.counter.level = high`, `agent-control.slot.status(slot 0) = waiting` |
| Lifetime | An instant. Not stored. | Until it changes or its provider goes away. Stored in memory. |
| Late consumer | Missed, correctly. | Gets the current value at once. |
| After a restart | Gone, correctly. | `unknown` until the provider reports. Never restored from disk. |
| What a pad does with it | Shows a look for a while (a momentary overlay). | Shows a look for as long as the value holds. |
| Can be unavailable? | No. | Yes: `known`, `unknown`, `unavailable`. |

The two are never merged. One fact can arrive as both: the Counter publishes the event `counter.changed` and its `level` is also a state. The plugin says so in its manifest (section 12); LaunchDeck does not guess.

### 6.2 Decision A: a hybrid, and the alternatives that were rejected

| Alternative | What it would fix | Why not |
| --- | --- | --- |
| Keep bindings only ("no") | Nothing new | Fails P1, P2, P7 and P8. |
| A generic cache of the last event per `(source, type)`, re-evaluated on demand | P1 and P2 for every source with no SDK change | It conflates the two kinds: it would replay `deployment.failed` as if it were current. It has no status, so P7 stays. It cannot express Agent Control's derived values. |
| Persist `Inner.states` to disk | P1 | Writes a derivation whose inputs are gone, and shows it after restart as truth. That is the stale-value hazard (5.5) and the brief forbids it. |
| Ask each provider for its current state when a surface opens (pull) | P1, P2 | Agent Control can answer. A WebAssembly plugin cannot: ABI 1 has no query export and adding one is a protocol change. A WebSocket client cannot. The hub is the push-side cache of the same information. A pull export remains a future option (section 12). |
| **State hub, written by providers, read by a pure evaluator (chosen)** | P1 (honestly: `unknown`), P2, P7, P8 | Adds one map with a leaf lock and one task. |

The hub is not a registry of definitions. Definitions are static metadata in a catalog (section 7), the same way actions are. The hub holds only the volatile part.

### 6.3 Decision C: three producer styles, all supported

| Style | Example | Who writes the hub | What the bus carries |
| --- | --- | --- | --- |
| Derived from a domain runtime | Agent Control | The module, from the same `Projection` diff it already computes in `publish_projection` | The existing projection events, unchanged and now optional |
| Projected from declared events | A WebAssembly plugin such as the Counter | The plugin host, from a projection the manifest declares (`counter.changed.level` is the state `level`) | The plugin's own event, as today |
| Event only | `workflow.completed` | Nobody | The event, matched by an event rule |

A WebSocket client can be a producer too; that needs one new message and is deliberately its own optional phase (section 19, phase 5).

### 6.4 The brief's examples against what can feed them today

| Example | Can it be fed in this milestone? |
| --- | --- |
| Agent session: running, waiting, approval required, failed | Yes. Agent Control, no new provider code beyond writing the hub. |
| Counter level equals high gives warning | Yes. The Counter plugin, with a manifest change only. It is the real-plugin test of this milestone. |
| Spotify: playing, paused, error | The model supports it. Nothing in the sandbox can observe Spotify (no network, no process capability; both are out of scope). It would be fed by a local WebSocket publisher (phase 5) or by a future host-mediated capability. |
| Home Assistant light: on, off | Same as Spotify: needs an integration that does not exist. The parameterised state (`entity` as an address) is already in the model so it is a manifest change later. |

## 7. State, Event and Observable model

### 7.1 Words

* **Observable.** Anything a pad can react to. Two kinds: `state` and `event`.
* **State.** A named current value with a status.
* **Event.** Something that happened.
* **Look.** A named appearance of a control. Today this is an entry of `Control.states`. The saved field keeps its name to avoid churn; the interface says "look", because "state" now also means an observed value. This naming risk is in section 20.
* **Rule.** One line of a control's feedback list: a condition and the look to show.
* **Role.** A closed set of intents (`ok`, `active`, `attention`, `warning`, `error`, `inactive`) a provider may attach to a value as a suggestion. LaunchDeck owns the default appearance of each role.

### 7.2 One catalog type, not two

Question from the brief: is a single Observable cleaner than separate event and state definitions? **Yes, for the catalog and the reference, no for the semantics.** One list is justified by the authoring flow: the editor offers one searchable source picker, grouped by provider, exactly like the Action Library, and a saved rule needs one reference shape `{provider, id, args}` whatever the kind. The overlap in fields is large. The semantics stay separate: only states are stored, only states have a status, only events carry a payload. The word "Observable" appears in code and docs; the interface says "state" and "event".

### 7.3 `ObservableDefinition`

```jsonc
{
  "id": "example.counter.level",     // provider id + "." + local id. Stable, never shown
  "provider": "example.counter",
  "local": "level",                  // id inside the provider ("slot.status" for Agent Control)
  "kind": "state",                   // "state" | "event"
  "name": "Counter level",
  "description": "How full the counter is.",
  "category": "Counter",
  "keywords": ["fill", "amount"],

  // state only. The address of one instance, for providers with many (first party in v1)
  "params": [],                      // ParamDef[], the same schema and form as action parameters

  // state only
  "value": {
    "type": "enum",                  // enum | boolean | number | string
    "values": [                      // enum and boolean only
      { "value": "zero", "label": "Empty",  "role": "inactive" },
      { "value": "low",  "label": "Low",    "role": "ok" },
      { "value": "high", "label": "High",   "role": "warning" },
      { "value": "max",  "label": "Full",   "role": "error" }
    ]
  },

  // event only: the payload fields a rule may test
  "fields": []                       // [{ "name", "label", "type", "values"? }]
}
```

| Field | Why it exists |
| --- | --- |
| `id`, `provider`, `local` | A rule must name its source stably (`provider` + `id` + `args`), and the catalog needs a unique key. Same convention as actions: first claim wins, duplicates are reported in `Catalog.issues`. |
| `kind` | Separates states from events inside one list (7.2). |
| `name`, `description` | The only text a person sees. |
| `category`, `keywords` | Grouping and search with the Action Library's own scoring. Optional for plugins. |
| `params` | Agent Control needs it today (8 slots, 8 commands); a Home Assistant light would need it. Reuses `ParamDef`, validation and the generated form. **Plugin manifests do not get `params` in schema 3** (section 12); the model allows it later without breaking anything. |
| `value.type` | Decides which operators and which input the editor offers. |
| `value.values[].value`, `.label` | Possible values and the words for them. Booleans use the same list for "On"/"Off" labels. |
| `value.values[].role` | The "feedback suggestion" the brief asks for, as an intent, never a colour (7.5). |
| `fields` | Lets the editor offer a payload field from a list instead of asking a person to type a path. |

Deliberately **not** in a definition: `current value`, `status` and `updatedAt` (they are observations, held in the hub and returned by the explain call, in the same way an `Assignment` is separate from an `ActionDefinition`); `availability` (it is the provider's status, already in `Catalog.providers`); `min`, `max`, `unit` (no use case needs them); `attributes`, `tags`, `history`.

`ObservableRef` is the reference saved in rules: `{ "provider": "agent-control", "id": "slot.status", "args": { "slot": 2 } }`. The hub key is the canonical form: `agent-control.slot.status?slot=2` (arguments sorted by name).

The catalog is extended, not duplicated: `Catalog { providers, actions, observables, issues }`. It already ships in the snapshot and is refreshed on plugin changes, on entering Edit and, debounced, on Agent Control changes. Values are **not** part of it, because they change far more often than the catalog.

### 7.4 What the hub holds

```
Observed { value: Option<string | number | boolean>, status: known | unknown | unavailable, updatedAt: ms }
```

Values are scalars. A projection that finds an object, an array or the wrong type leaves the state as it was and writes a line to the plugin's own log (section 12).

### 7.5 Roles: suggestions without owning appearance

A plugin may say `"role": "error"` for a value. It cannot say a colour. LaunchDeck owns one small table, used only to pre-fill a look when a person asks for it, and the person's look always wins (5.3):

| Role | Default look | Rationale |
| --- | --- | --- |
| `ok` | solid green | calm, done |
| `active` | pulsing green | working, alive (same family as `running` in `agents/visual.rs`) |
| `attention` | flashing white | needs a person now (same as `approval_required`) |
| `warning` | solid amber | worth noticing |
| `error` | flashing red | failed (same as `failed`) |
| `inactive` | dim blue-grey | off or idle |

Urgent roles move, calm roles do not, so colour is never the only signal (the existing accessibility rule). A value with no role gets a neutral look the person is expected to change. Agent Control's own surface keeps its exact looks from `agents/visual.rs`; the role table is for pads people build themselves.

## 8. Rehydration model

### 8.1 The rule

```
provider value ──► hub ──► evaluate(rules, hub) ──► derived look ──► renderer ──► device
                              ▲
        also triggered by: rule edited, provider up/down, workspace loaded
```

A control's look is the result of a pure function of its rules and the hub. It is never "the last thing an event said". So the question "what should this pad show right now" has an answer at every moment, including the moment a surface opens, with no event in flight.

Two layers, fixed order, because event rules are momentary and state rules are continuous:

```
effective look = overlay (event rule, while its hold lasts, or the automatic running/success/error)
              else derived (first matching state rule)
              else none (the resting look)
```

`Inner.states` becomes the overlay. The derived layer is new. The renderer asks for one name per control, exactly as today.

### 8.2 When each case is computed

| Case | Source of truth | What recomputes | Result |
| --- | --- | --- | --- |
| Opening a Surface | The hub and the derived layer, which already cover every control of every surface | Nothing; the next frame (at most 40 ms) draws the stored look | Correct immediately, no event needed (the mandatory test of section 18) |
| Switching Page | Same | Nothing | Same |
| Device connects or reconnects | Same | Nothing; the device cache is cleared on connect, so a full frame goes out | Same (P3b, kept) |
| A new or edited pad | The hub | That control only, at the moment of `update_control` | Correct immediately (fixes P2) |
| Plugin reloads, is enabled or disabled | Plugin host status | Its states become `unknown` (up) or `unavailable` (down); controls that depend on them | Honest, not stale (fixes P7) |
| A provider reconnects (Agent Control adapter, later a WebSocket publisher) | The provider reporting its state again | The states it reports | Correct as soon as it reports |
| Application restarts | Providers reporting at startup | `evaluate_all` once, before the first render tick | Agent Control correct from the first frame. Plugin states `unknown` until the plugin reports. Never restored from disk |
| Bus lag | Hub (lossless path) | Nothing for states. Logs once; asks Agent Control to republish | Only event rules can lose an occurrence, and the docs say so |

### 8.3 Startup order (fixed)

1. Load and migrate the workspace; build the rule indexes.
2. Register providers; Agent Control writes its full projection into the hub; the plugin host scans, and each loaded plugin's states are declared `unknown`, each failed or disabled plugin's `unavailable`.
3. `evaluate_all()`.
4. Start the event loop, the hub loop and the render loop.

Step 3 before step 4 is why the first frame is already right.

### 8.4 Concurrency contract

The hub has its own mutex and is a **leaf lock**: nothing is called while it is held. Runtime takes the hub lock after its own, never the reverse. This matters because `Runtime::snapshot` already calls into Agent Control while holding the runtime lock (through `availability`), so an Agent Control that wrote into the runtime would deadlock. Writers (Agent Control, the plugin host) only touch the hub; the hub marks keys dirty and notifies; one runtime task drains the dirty set and evaluates. Writes never go through the broadcast bus, which is what makes them lossless (P8).

## 9. Binding precedence

### 9.1 The rule

A control's `feedback` is one **ordered list** of rules. Evaluation is:

1. **State rules** are tested top to bottom. The first rule whose condition is true decides the derived look. If none is true, there is no derived look and the pad shows its resting look. (Today the previous look would stay; that is exactly the staleness this plan removes.)
2. **Event rules** act when their event arrives. The first event rule, top to bottom, that matches starts an overlay for its `hold`. A later matching event replaces the overlay.
3. **Layering.** An active overlay wins over the derived look. When the overlay ends, the derived look is already there.
4. A control with at least one rule is excluded from the automatic running/success/error cycle, as a control with bindings is today.

One look per control is shown at any moment. There are no priority numbers and no specificity ranking. The order the person sees is the order that is saved.

### 9.2 Why this one

| Model | Verdict |
| --- | --- |
| Explicit priority numbers | Order-independent, but a number is something to maintain and explain. The list order already is a priority. |
| Last match wins | Right for Companion, whose feedbacks stack as property overrides (5.2). Here rules do not stack, so "last wins" reads backwards. |
| Specificity ranking | Needs a definition of "more specific" across `is`, ranges and `has a value`. Unpredictable to explain. |
| **First match wins, visible order** | Reads like a switch statement. Easy to say, easy to see, easy to debug, and the editor can show which rule won. |

On "stable under reordering": the brief asks for a result that is deterministic and does not shift unexpectedly. What this plan removes is every dependence the current model has that the person cannot see: the order events arrived in (P4), the order bindings sit in the file, whether the pad existed when the event fired (P2), and whether the process restarted. The result is a function of the current values and the visible list. Reordering the list changes the outcome only when two rules can be true at once, which is the point of reordering, and the editor marks that case (9.3).

### 9.3 Making the winner visible

* The explain panel (section 14) names the winning rule and marks every rule that is true now.
* A **shadowed rule** warning is shown when a rule can never win: the same source with the same test above it, or `has a value` for the same source above it. This is a simple structural check, not a theorem prover; it only warns.
* A look-from-value rule (`showFromValue`, 10.3) that names a look the control does not define is reported by explain as "no look named ‘x’", where today it is silent (P6).

### 9.4 Event holds

An event rule has a `hold`: `{ "ms": 3000 }` by default (200 to 60,000), or `"latch"`, which stays until another event rule on the same control replaces it, the control is edited, or the app restarts. `latch` exists so migrated bindings keep behaving exactly as they did. It is offered in the interface as "Stay until another rule changes it" and is the one place a stale look can still be chosen on purpose.

The look name `idle` keeps its meaning for an event rule: it clears the overlay, which reveals the derived look, or the resting look.

## 10. Condition language

### 10.1 State conditions

One condition per state rule, on one state.

| Interface text | Saved as | Applies to | True when |
| --- | --- | --- | --- |
| is | `equals` | enum, boolean, number, string | status is `known` and the value equals. Numbers compare numerically, so `1` equals `1.0` (fixes P5). A string never equals a number |
| is not | `notEquals` | same | `known` and different. **Not** true for `unknown` or `unavailable` |
| is below | `lessThan` | number | `known` and less than |
| is at most | `atMost` | number | `known` and at most |
| is above | `greaterThan` | number | `known` and greater than |
| is at least | `atLeast` | number | `known` and at least |
| has a value | `hasValue` | any | `known` |
| has no value yet | `noValue` | any | `unknown` |
| is not available | `unavailable` | any | `unavailable` |

Four number comparisons rather than two so nobody writes "below 21" to mean "at most 20".

### 10.2 Event conditions

The event itself, plus optional `where` tests: a list of `{ field, equals }`, all of which must hold, on a **top-level** payload field chosen from the event's declared `fields`. The same typed equality as above. An undeclared event (a WebSocket source) accepts a field name typed by the person.

### 10.3 Showing a look

A rule has exactly one of three ways to name its look: `show`, a look name; `showFromValue`, on a state rule, which shows the look named after the state's value; or `showFromField`, on an event rule, which shows the look named after a payload field. The second and third replace the `$payload.<field>` convention as typed fields instead of a magic prefix. Agent Control's pads use `showFromValue`: one rule per pad instead of twelve. `showFromField` exists for migration fidelity: it is the one old convention with real users (the demo plugin and hand-written files).

### 10.4 What is left out, and how each need is still met

| Left out | Why | How the need is met |
| --- | --- | --- |
| `truthy` | `is` on a boolean says it | Boolean states list their labelled values |
| `contains` | No example in the brief or in any provider needs it | Strings are compared exactly; an enum lists its values |
| AND of several states | Every example in the brief is one state. AND is the first step toward an expression tree | The provider derives it: Agent Control computes `command.state` from several facts; a plugin can declare a derived value |
| OR | Same | Two rules with the same look; first match wins |
| Nested payload paths, regular expressions, arithmetic, scripts | Out of scope by the brief | Not offered |

Revisit AND only if a real use case appears that a provider-derived state cannot serve. That is a decision for after the first working version, not a v1 feature.

### 10.5 Value typing

States are scalars. A rule's value is checked against the declared type when the state is declared (an enum value must be one of the listed ones; a number must be a finite number). A value that a provider sends and the definition did not list is stored anyway, so providers can add values without breaking old hosts; it simply matches no `is` rule, and explain says "value ‘buffering’ is not one of the declared values".

## 11. Unknown, stale and offline behavior

### 11.1 Three statuses

| Status | Means | Home Assistant word |
| --- | --- | --- |
| `known` | The provider reported a value and it is still valid | a normal state |
| `unknown` | The state exists but has no value: never reported since the provider (re)started, or since the app restarted | `unknown` |
| `unavailable` | The provider cannot answer: disabled, failed, uninstalled | `unavailable` |

### 11.2 The cases in the brief

| Case | Status | What the person can do | With no rule for it |
| --- | --- | --- | --- |
| Provider missing (uninstalled, not loaded) | `unavailable` | Rule "is not available" | Resting look |
| Provider offline (plugin disabled or failed) | `unavailable` | Same | Resting look |
| State unknown | `unknown` | Rule "has no value yet" | Resting look |
| Value never received | `unknown` | Same | Resting look |
| State stale | Not modelled in v1 | n/a | n/a |

Two things must not be confused. Agent Control's `offline` is a **value** of `slot.status` (an agent that is not connected), and it has its own look. `unavailable` is the **status** of a state whose provider is gone. Agent Control is part of the app, so its states are never `unavailable`.

### 11.3 No silent forever

Because a value stays only while its provider is up, an old value cannot outlive its provider (fixes P7). The rule "nothing matches, so the pad rests" replaces "nothing matches, so the last look stays". The interface makes the safe choice the default: when the first rule for a state is added, **Also show a look when this is not available** is checked and appends a final dim rule. A dim pad is a visible "I don't know".

### 11.4 Why there is no `stale`

`stale` means "a live provider has stopped updating". No provider in this milestone can do that: Agent Control is the app, and an installed plugin only publishes inside an action call, so it cannot go quiet while being up. The first provider that can (a polling integration, or a WebSocket publisher) brings the need. The hook is small and known: a state may declare a `ttl`; an entry older than its `ttl` is read as `unavailable`; a deadline queue re-evaluates dependents at expiry. It is not built, because it makes evaluation depend on the clock and nothing can use it yet. Until then `updatedAt` is shown in explain ("changed 4 s ago"), so a person can see an old value with their own eyes.

## 12. Plugin SDK impact

### 12.1 What a plugin can do today

Manifest schema 2 declares `actions` (id, label, description, category, keywords, appearance, params), `events` (`{ type, description }`), `config` and `capabilities`. A module publishes events with `launchdeck.publish` **only while an action call is running**; events published during `init` are dropped, and a call that fails publishes nothing. So a plugin cannot push state on its own, and the host knows its value only when an action ran. Nothing in the manifest says what an event's payload means or which of its fields is a state.

### 12.2 The minimum extension: manifest schema 3, additive

```jsonc
{
  "schemaVersion": 3,
  "id": "example.counter",
  // ...everything schema 2 has, unchanged...
  "actions": [
    { "id": "counter.increment", "label": "Increment", /* ... */ "suggests": ["level"] }
  ],
  "events": [
    {
      "type": "counter.changed",
      "label": "Counter changed",
      "description": "The value changed.",
      "fields": [
        { "name": "value", "label": "Value", "type": "number" },
        { "name": "level", "label": "Level", "type": "enum", "values": [
            { "value": "zero", "label": "Empty" }, { "value": "low", "label": "Low" },
            { "value": "high", "label": "High" }, { "value": "max",  "label": "Full" } ] }
      ]
    }
  ],
  "states": [
    {
      "id": "level",
      "label": "Counter level",
      "description": "How full the counter is.",
      "type": "enum",
      "values": [
        { "value": "zero", "label": "Empty", "role": "inactive" },
        { "value": "low",  "label": "Low",   "role": "ok" },
        { "value": "high", "label": "High",  "role": "warning" },
        { "value": "max",  "label": "Full",  "role": "error" }
      ],
      "from": { "event": "counter.changed", "field": "level" }
    },
    { "id": "value", "label": "Counter value", "type": "number",
      "from": { "event": "counter.changed", "field": "value" } }
  ]
}
```

| Addition | Covers the brief's list |
| --- | --- |
| `states[]` with `id`, `label`, `description`, `category`, `keywords`, `type`, `values`, `from` | observable state, possible values, human-readable labels |
| `values[].role` | feedback suggestions, as intents, never colours |
| `events[].label` and `events[].fields[]` | event schemas |
| `actions[].suggests[]` (ids of the plugin's own states) | the optional Action to Feedback hint |

`from` is the whole mechanism: "this state is the field `level` of the event `counter.changed`". The plugin keeps doing what it does today (publishing an event during an action call); the host does the rest.

### 12.3 Validation (host, at load)

| Rule | Limit |
| --- | --- |
| States per plugin | at most 16; ids unique, `[a-z][a-z0-9.-]{0,31}` |
| `type` | `enum`, `boolean`, `number`, `string` |
| `values` | required for `enum` (1 to 32), optional labels for `boolean`, forbidden for `number` and `string`; each value 1 to 64 characters and unique; each label at most 40 |
| `role` | one of the six; anything else is an error (a new role would come with a new schema number) |
| `from.event` | must be declared in `events`; if that event lists `fields`, `from.field` must be one of them and its type must match |
| `events[].fields` | at most 16 per event; names `[A-Za-z_][A-Za-z0-9_]{0,31}` |
| `suggests` | each entry must name a state of the same plugin |
| Mixing | `states`, `fields`, `suggests` and `role` need `"schemaVersion": 3`, with the same error style the host uses for schema 2 features today |

### 12.4 Host behaviour

* After a call that ended normally, for each published event in order, the host applies the matching projection (writes the hub) and then publishes the event. A failed call publishes nothing and writes nothing, as today. The state is always written before its event is routed, so an event rule and a state rule on the same control never disagree about order.
* A projection that finds the field missing, an object or an array, or the wrong type leaves the state as it was and writes a warning to the plugin's own log, which the Plugins screen already shows: ``state `level` not updated: `counter.changed` has no string field `level` ``.
* A value outside the declared list is stored and logged once per distinct value.
* Lifecycle: loaded without error, every state of the plugin is `unknown`; disabled or failed, `unavailable`; reloaded (including by a settings change, which reloads), `unknown` again because the module's memory is gone. This is what turns P7 from a lie into an honest "no value yet".
* Declared event types must still be unique and may not collide with built-in types; this is unchanged.

### 12.5 Compatibility

| Manifest | Current LaunchDeck (reads schemas 1 and 2) | LaunchDeck with this plan |
| --- | --- | --- |
| Schema 1 or 2 | Loads | Loads. No states. Its events are listed and usable by event rules, without field lists (the person types a field name) |
| Schema 3 | Refused with the existing message, "manifest schema version 3 is not supported (this LaunchDeck reads versions 1 to 2)" | Loads |

* **ABI** stays `1`: no new export and no new import. **SDK** stays `0.1.0`: the AssemblyScript library is not involved. **No `.wasm` file is rebuilt**; only `plugin.json` changes in the examples. The C Ping example stays on schema 1 on purpose, as the living compatibility test.
* Plugin authors move to schema 3 only when they want states. Nothing breaks if they do not.

### 12.6 Deliberately not added

| Idea | Why not now | Trigger to reconsider |
| --- | --- | --- |
| A host function to publish a state directly | `from` already covers it with no protocol change | A plugin whose state is not a field of any event it publishes |
| An optional `state` export the host can call on load to read current values (pull) | It would make a reloaded plugin `known` instead of `unknown`. Additive and optional, so no ABI bump, but still new protocol surface | A real plugin whose important state survives a reload and cannot republish |
| Timer-driven publishing | A plugin has no entry point other than an action call. A timer is a separate capability discussion | Network capability work |
| Parameterised states in manifests (`params`) | No plugin needs an address yet. The model supports it without breaking schema 3 | A plugin that exposes many instances |
| Anything about colour or position | Appearance belongs to the surface | Never |

## 13. Agent Control integration

### 13.1 How its state is derived today

```
State (agents, sessions, slots, focus, flashes)
   └ reconcile_slots ─► project(st) ─► Projection { slots, focus, commands }
        ├ effective_status: an agent that is not online makes its sessions `offline`
        ├ command_state(): Unavailable | Available | Attention | Pending | Succeeded | Failed
        └ publish_projection: diff against the last published, emit slot.changed / focus.changed / command.state
```

`model::availability(agent, session, kind)` is the single derivation of "can this command run now". The Action Library reaches it through `AgentControlPlugin::availability` and `focus_availability`; the pads reach it through `command_state()`, which calls it and layers pending, succeeded and failed on top. The status to appearance mapping lives only in `agents/visual.rs`. Both are good and are kept: the new `command.state` observable is the existing `command_state()`, not a second derivation.

### 13.2 Decision: expose it through the same catalog and hub; no Agent-only editor

Agent Control becomes a producer of the first style in 6.3 and declares three states. They are parameterised, which is the case that justifies `params` in the model.

| State id | Address | Values |
| --- | --- | --- |
| `agent-control.slot.status` ("Agent track status") | `slot` 0 to 7 ("Track") | The 11 statuses plus `empty`. Roles: idle, cancelled, offline, empty are `inactive`; starting, running, tool_use are `active`; waiting, paused are `warning`; approval_required is `attention`; completed is `ok`; failed is `error` |
| `agent-control.slot.focused` ("Agent track selected") | `slot` | Boolean: Selected, Not selected |
| `agent-control.command.state` ("Agent command state") | `command`: one of the 8 `CommandKind`s | The six command states: unavailable (`inactive`), available (`ok`), attention (`attention`), pending (`active`), succeeded (`ok`), failed (`error`) |

Its existing domain events (documented in `docs/agent-control/domain-model.md`) are listed as event observables with labels; no new event is invented. Aggregates such as "how many approvals are pending" are a plausible follow-up and are not needed to meet the acceptance tests.

### 13.3 How it writes

`publish_projection` receives the hub handle at construction, the way it receives the bus, and writes the keys that differ in the diff it already computes. The three projection events keep being published and stay hidden from the event log; they are now optional and documented as deprecated for one release. The hub is a leaf lock, so writing under Agent Control's lock cannot deadlock (8.4).

### 13.4 The shipped surface

`surfaces/agent_control.rs` builds feedback rules instead of bindings: a mirror rule on `slot.status(slot = i)` for each track pad, `slot.focused(slot = i) is Selected` for each marker, a mirror rule on `command.state(command = c)` for each command pad. The looks (`Control.states`) are unchanged, so nothing about the surface changes visually. A golden test proves it (section 18): for every status and every command state the composed frame is identical before and after.

### 13.5 What is removed and what is not done

* `agents.republish()` is removed from `activate_surface` and `reset_workspace` because rules are evaluated from the hub whenever anything changes. It stays for a bus-lag resync.
* No Agent-only feedback screen. A person who wants a pad to show agent 3's status uses the same Add feedback flow as for the Counter.
* No change to Agent Control security. Feedback never executes anything; shell and HTTP remain unreachable from plugins, the WebSocket and the bridge.

## 14. UI and UX proposal

### 14.1 Where

The inspector gets a **Feedback** section between the press sections and the looks. The existing "Look in each state" editor is renamed **Looks** and stays, because it is where the colours live. The read-only "State bindings" block is deleted. The Action Library is untouched.

### 14.2 The flow

```
Feedback                                                         [ Add feedback ]

 1  When  [ Counter level ▾ ]  [ is ▾ ]  [ High ▾ ]      show  [ warning ▾ ]   ↑  ↓  ✕
        amber, steady
 2  When  [ Counter level ▾ ]  [ is ▾ ]  [ Full ▾ ]      show  [ danger  ▾ ]   ↑  ↓  ✕
        red, flashing
 3  When  [ Counter level ▾ ]  [ is not available ▾ ]    show  [ offline ▾ ]   ↑  ↓  ✕
        dim grey, steady

 The first rule that is true decides. If none is true, the pad rests.

 Now: Counter level is High, changed 4 seconds ago.  Rule 1 is true. Showing “warning”.
```

1. **Add feedback** opens a source picker built like the Action Library: search box, list grouped by provider, the words *State* or *Event* on each row, unavailable providers listed with their reason. If the pad's action suggests a state, it is the first row ("Suggested for Increment").
2. A state with parameters (an Agent Control track) shows the generated form from its `params`.
3. The rule is a sentence: **When** source, operator, value. The operators and the value input follow the state's type (10.1): a select of labelled values for an enum; the labels "On"/"Off" for a boolean; six comparisons and a number box for a number. An event reads **When** *Event name* **happens**, with optional **and** *field* **is** *value* rows, and **stay for** 3 seconds or **stay until another rule changes it**.
4. **Show** picks a look from the control's own looks, or **New look…**, which opens the existing colour and animation fields inline. The preview is a swatch **and a text** ("amber, steady"), so it never depends on colour alone. New looks are pre-filled from the value's role (7.5). The rule and the look are created together, so a rule never points at a look that does not exist.
5. **Fill from values** (enum and boolean states) creates one rule per declared value, in the provider's order, each with a look named after the value and pre-filled from its role, plus the "Also show a look when this is not available" rule. The result is an ordinary list the person edits.
6. **Move up**, **Move down** and **Remove** are real buttons with names ("Move rule 2 up"). Dragging is an extra, never the only way.
7. A shadowed rule shows "Rule 3 never applies: rule 1 is always true when it is."
8. A source that is missing (plugin removed) shows "Counter level (plugin not installed)", keeps its saved settings, behaves as not available, and can still be edited or removed. Saving works because only changed rules are re-validated, as with actions.

Everything above is plain words. The person never sees a provider id, a state id, a payload path, JSON, `$payload` or the word "bus". A tooltip carries the id for support.

### 14.3 Action and feedback cooperate without merging

An `ActionDefinition` may carry `suggests: [{ observable, args?, argsFrom? }]`, filled from the manifest's `suggests` (plugins) or by Agent Control (`approve` suggests `command.state` with `command = approve`; `focusSlot` suggests `slot.status` and `slot.focused` with `slot` copied from the action's own `slot`). `argsFrom` is the only mapping field and exists because of that last case.

When a person assigns an action to a pad that has no feedback, one dismissible line appears under the action: "Add feedback for Counter level? [Add] [No thanks]". **Add** opens the flow above with that source selected. Nothing is created without the click and the action is never changed. An action never writes a rule and a rule never names an action. The pad owns the final look. Dismissal is interface state for the session, not saved.

### 14.4 Diagnostics: the smallest version that helps

One IPC call, `feedback_explain(surface, page, control)`, answers the three questions of the brief for the selected pad, and nothing more:

| Question | Shown |
| --- | --- |
| Current observable value | Per source: label, value, status, "changed 4 seconds ago" |
| Matched rule | Every rule marked "true now" or "not now" in words, with the reason when not: "value is Low", "no value yet", "not available". The winner is named |
| Resulting look | The look name, its appearance in words, and whether it comes from an event overlay ("for 2 more seconds" or "until replaced") or from the rules |

It also reports problems: a missing look, a value outside the declared list, a missing provider. The Plugins screen shows the same value, status and age for each declared state of a plugin. It is pulled when a pad is selected and refreshed on the existing `runtime-state` event for that pad only; there is no event console and no recording of history.

### 14.5 Accessibility

The sentence builder is one labelled group per rule, readable as a sentence. Every select, number box and button has a name. Operators and values are real `select` elements. Targets are at least 32 px, as in the library. State is always a word ("true now", "not available", "stays for 3 s"). Focus moves to the new rule when one is added and to the neighbour when one is removed.

## 15. Persistence and migration

### 15.1 What is saved

Only semantic configuration: each control's `feedback` rules and its looks (`states`, unchanged). **Never** saved: observed values, statuses, timestamps, the derived and overlay look maps, hub contents, provider handles, MIDI values, renderer frames, explain data. Nothing new is written next to `workspace.json`.

### 15.2 Schema 4

`SCHEMA_VERSION` becomes `4`. `bindings` is replaced by `feedback` (the old name is read only by the migration).

```jsonc
"feedback": [
  // a state rule: continuous
  { "when": { "state": { "provider": "example.counter", "id": "level" },
              "is": { "op": "equals", "value": "high" } },
    "show": "warning" },

  // a state rule that names its look after the value, with an address
  { "when": { "state": { "provider": "agent-control", "id": "slot.status", "args": { "slot": 2 } },
              "is": { "op": "hasValue" } },
    "showFromValue": true },

  // an event rule: momentary, with a hold
  { "when": { "event": { "provider": "ci", "id": "build.finished" },
              "where": [ { "field": "result", "equals": "failed" } ] },
    "show": "failed", "hold": { "ms": 4000 } }
]
```

Limits (validated in Rust, mirrored in TypeScript with the same messages, as for action parameters): at most 32 rules per control; look names 1 to 40 characters; `hold.ms` 200 to 60,000 or `"latch"`; exactly one of `show`, `showFromValue`, `showFromField`; operators allowed for the state's declared type; arguments normalised by the state's `params`. As with assignments, only a rule that **changed** is re-validated, so a pad whose plugin is gone can still be renamed, recoloured and saved.

### 15.3 Migration v3 to v4

JSON to JSON, like `legacy::upgrade_actions`, before the final parse. Composition: v1 and v2 files go through the existing steps and then this one, so every old file opens.

| v3 binding | v4 rule |
| --- | --- |
| `agent-control`, `slot.changed`, `when slot == i`, `setState: $payload.status` | state `agent-control.slot.status(slot = i)`, `hasValue`, `showFromValue` |
| `agent-control`, `focus.changed` with `slot == i` then `focused`, followed by a catch-all `idle` rule | state `agent-control.slot.focused(slot = i)` `equals true`, `show: focused`. The catch-all disappears: "nothing true, so rest" replaces it |
| `agent-control`, `command.state`, `when command == c`, `setState: $payload.state` | state `agent-control.command.state(command = c)`, `hasValue`, `showFromValue` |
| `demo`, `plugin.state.changed`, `when key == a`, `setState: $payload.value` | state `demo.<a>`, `hasValue`, `showFromValue` |
| Any other source: a rule with a literal `setState` | event rule on `{ provider: source, id: event }`, `where` from `when`, `show: literal`, **`hold: latch`** so behaviour is unchanged |
| Any other source: `setState: $payload.field` | event rule with `showFromField: field`, `hold: latch` |
| Anything that does not fit (a non-scalar `equals`, an empty rule list, a malformed entry) | Kept verbatim in `legacyBindings` on the control. It is inert, shown in the inspector as "Old settings that could not be converted" with **Delete**, and never lost |

The rest of the control is untouched: positions, spans, labels, icons, looks, assignments.

### 15.4 Backup, downgrade, order

* `Store::load_or_default` already copies the file to `workspace.json.v{N}.bak` the first time an older file is upgraded and never overwrites that copy (tested for v2). For a v3 file it creates `workspace.json.v3.bak`. The file is rewritten as v4 on the first save.
* A v4 file opened by an older LaunchDeck is refused with the existing "newer than this build supports" error; nothing is damaged.
* `add_agent_control` (the v1 path) is called after parsing, so it adds a surface already in v4 form.

### 15.5 Compatibility tests

A frozen copy of the current `examples/default-workspace.json` is committed as a v3 fixture before the schema changes. Tests: every v1, v2 and v3 fixture migrates and parses; control counts, positions and assignments are identical before and after; the migrated Agent Control surface and the surface the builder now emits compose **identical frames** for every status and every command state; a hand-written event binding keeps latch behaviour; an unconvertible binding round-trips through `legacyBindings`; the `.bak` exists and is byte-identical after a save; a v4 file round-trips; the regenerated `examples/default-workspace.json`, `examples/action-catalog.json` (now with `observables`) and `examples/plugins-overview.json` are guarded by the existing drift tests on the Rust side and parsed by Zod on the TypeScript side.

## 16. Performance considerations

### 16.1 What was measured, and by whom things are produced

P9: 5,000 unrelated events took 4 to 6 µs each on the 109-control starter workspace and 25 to 29 µs each on a synthetic 2,669-control workspace (debug build; it includes logging and serialising for the UI emitter). Linear in the workspace, not a problem today, and easy to avoid.

| Producer | Frequency | Bound |
| --- | --- | --- |
| Agent Control | Per mutation, only changed slots, focus or commands | Human and agent speed |
| Installed plugin | Only inside an action call | A person pressing pads; at most `MAX_EVENTS_PER_CALL` events per call |
| WebSocket client | Unbounded in principle | 64 KB per message, 16 connections. The hub coalesces per key, so a flood on one state costs one evaluation per drain |
| Render loop | 25 Hz, per connected device | The active page only (at most 128 controls); devices diff themselves |
| UI emission | Once per effective change | A map of non-resting looks |

### 16.2 Design

* A **reverse index** from state key to the controls that reference it, and from `(provider, event)` to the event rules that reference it. Built at load and rebuilt when the workspace changes (cost is the number of rules).
* An unrelated event does one hash lookup. A state change evaluates only the controls in its entry, each at most 32 rules.
* **Coalescing.** The hub keeps a dirty set and one `Notify`. A burst of writes to one key is one evaluation per drain.
* `evaluate_all` runs only at startup, load, migration and reset. Editing a pad evaluates that pad.
* Not optimised on purpose: the 25 Hz recomposition (it scales with the active page, not the workspace) and the size of the UI state message.

### 16.3 Gates

The index is tested by counting evaluations, not by timing: an unrelated event causes none; a state change causes exactly the number of dependents; a burst of N writes to one key causes one evaluation per dependent. P9 is kept as a benchmark: with the index, the 2,669-control case must be within twice the 109-control case.

## 17. Security implications

| Concern | Decision |
| --- | --- |
| Who can write a state | Only its owner. Agent Control writes its own. The plugin host writes a plugin's states from that plugin's own declared projections. A plugin cannot write another plugin's state or Agent Control's. The provider id comes from the host, never from a payload. Phase 5 would add WebSocket publishers, with the same reserved-source rules events already have |
| New capability | None. No network, no files, no process. `capabilities` still fails closed |
| Execution | Feedback only chooses a look. No state or event rule runs an action, so shell and HTTP stay unreachable from plugins, the WebSocket and the bridge. Workflow chaining stays out of scope |
| Bounds | 16 states per plugin, 32 values per state, 16 fields per event, 32 rules per control, strings at most 64 characters (values at most 256 bytes), look names at most 40. The hub holds at most the declared states times their address combinations: 24 keys for Agent Control |
| Disclosure | A state value is shown on pads and in the app, as event payloads already are in the event log. Explain stays local. Nothing is persisted, so there is no new data at rest. Plugin log lines name the state and the field and never the value |
| Denial of service | Per-call event caps and action-call frequency bound plugins. A WebSocket flood is bounded by message size and connection count and is coalesced per key |
| Deadlock | The hub is a leaf lock; the order is runtime, then hub; writers never hold anything while calling out (8.4). A stress test guards it |
| Data loss | The migration backs up first and keeps anything it cannot convert |

## 18. Testing strategy

### 18.1 Characterization first

The nine probes of section 1 become permanent tests in `tests/state_characterization.rs`, passing against today's code in the first commit. The commit that fixes each behaviour flips the assertion in the same diff, so the history documents every behaviour change. P1, P2, P7, P8 flip in phase 1 or 2; P3 stays as it is (it must not regress); P4's arrival-order result flips for state rules; P5 flips (`1` equals `1.0`); P6 gains a report in explain; P9 becomes the benchmark.

### 18.2 What is covered

| Required area | Test | Layer |
| --- | --- | --- |
| Event-only rules | A rule on a declared event starts a hold, shows its look, then the derived look returns; `latch` stays until replaced; migrated `latch` matches old behaviour | Runtime, virtual device |
| Persistent state rules | A rule on the Counter's `level` follows `increment` and `reset` | Runtime, **real compiled plugin** |
| **Surface rehydration (mandatory)** | See 18.3 | Runtime, real plugin, virtual device |
| Page switch rehydration | State changes on another page and surface; switching shows the right look with no event | Runtime |
| Device reconnect | Unplug, change the state, replug: the new look is drawn (P3b kept) | Runtime |
| Provider reconnect | Disable the plugin: `unavailable` look. Enable: `unknown` look. Press: `known` look | Runtime, real plugin |
| Provider offline | Agent Control's adapter goes offline: the `offline` **value** look, not `unavailable` | Runtime, demo agents |
| Unknown state | After a restart from the saved file: rules for `has no value yet` show; with none, the pad rests | Runtime, real plugin, real store |
| Stale | Pins the decision: a value never expires on its own, and `updatedAt` is exposed | Unit |
| Multiple matching rules | First match wins; the shadow warning; the same result for either arrival order of two sources (P4 flipped) | Unit and runtime |
| Precedence between layers | Overlay over derived over rest; overlay expiry reveals the derived look | Unit and runtime |
| Condition operators | Table-driven: every operator against every status and type; `1` equals `1.0`; `is not` false for unknown; the Counter's numeric `value` through six comparisons | Unit and real plugin |
| Plugin state declaration | Schema 3 manifest valid and invalid cases (12.3); schema 1 and 2 still load; the Counter and Hello examples load; a missing field, an object and an out-of-list value are handled and logged without the value | Unit and real plugin |
| Agent Control state | The hub equals `Projection` after every scenario step; `command.state` equals `command_state()`; the same command's availability in the Action Library agrees with its pad; golden frames before and after migration | Runtime, demo agents |
| Persistence and migration | 15.5 | Unit |
| Renderer integration | Each role's default look reaches the LED, including on a device without native pulse and flash and a non-RGB device | Runtime and renderer |
| Bus lag | Flood the bus past capacity: hub-backed pads still show the right look (P8 flipped); one warning is logged | Runtime |
| Late or edited pad | A pad created or edited after the value shows it at once (P2 flipped) | Runtime |
| Locking | Concurrent hub writes from Agent Control while `snapshot()` and `catalog()` run, with timeouts | Runtime stress |
| Index and cost | 16.3 | Unit and benchmark |
| Interface | Zod parses the regenerated examples; the sentence builder offers the operators of each type; Fill from values; reorder by keyboard; the shadow warning; the explain texts; the suggestion line (shown, dismissed, never creates anything without a click); the missing-source row | Vitest |
| The real app | The production binary under Xvfb with the tauri-driver harness used for the Action Library: open a migrated v3 workspace, add Counter feedback by Fill from values, press until each level, restart the app (unknown look), disable the plugin in Plugins (dim look), reorder rules, read the explain line; screenshots | Real app |

### 18.3 The mandatory acceptance test

The strongest test in the brief, exactly:

1. Start a real runtime with the examples folder (the real compiled Counter, with its schema 3 manifest) and the starter workspace.
2. Run `counter.increment` five times through the real engine and sandbox. The hub now says `example.counter.level = high`, `known`.
3. Subscribe to the bus and record its sequence number. Create a surface whose pad has the rule "Counter level is High, show warning" and make it active. **No event is published after this point** (asserted).
4. Within one render tick the virtual device shows the warning colour on that pad.
5. Variants of the same test: with an Agent Control demo scenario instead of the plugin; with a page switch between steps 3 and 4; with a device disconnect and reconnect between steps 3 and 4; and after a restart from the saved file, where the same pad must show the `unknown` look.

It is mandatory because the current state is materialised.

## 19. Implementation phases

Each phase ends green on every existing check and adds its own tests. No phase starts before the previous one passes.

| Phase | Ships | Gate |
| --- | --- | --- |
| **0. Characterization** | `tests/state_characterization.rs` with the nine probes. Frozen v3 fixtures. No behaviour change | All existing and new tests pass |
| **1. Core, Rust only** | `observables` module and `Catalog.observables`; `StateHub`; `PluginContext` and Agent Control receive the hub; feedback model, pure evaluator, indexes, derived and overlay layers, event holds, explain; validation in `update_control`; Agent Control declares its three states and writes the hub in `publish_projection`; schema 4 and the migration; the default Agent Control surface built from rules; `republish` removed from navigation, lag logging added | Golden frames identical; the acceptance test with Agent Control; P2, P8 flipped; index counters; stress test |
| **2. Plugin SDK** | Manifest schema 3: `states`, `events[].label/fields`, `suggests`; host projection and lifecycle (`unknown`, `unavailable`); the demo plugin declares its states; Counter and Hello get schema 3 manifests; C Ping stays on schema 1; contract examples regenerated | The acceptance test with the real Counter; P7 flipped; compatibility matrix tests |
| **3. Interface** | Zod mirrors; the Feedback section, source picker, sentence builder, Fill from values, the unavailable rule, reorder, shadow warning, explain; the suggestion line; the Plugins screen shows each state's value, status and age; the old read-only bindings block is removed | Vitest suites; accessibility checks of section 14.5 |
| **4. Verification and docs** | `docs/feedback/{overview,states,rules,providers,diagnostics}.md`; updates to `docs/architecture.md`, `docs/surfaces.md`, `docs/agent-control/surfaces.md`, `docs/plugin-sdk/{manifest,versioning,security}.md`, `README.md`; real-app exercise with screenshots | Definition of Done |
| **5. Optional, separate decision** | A WebSocket `state` message so a local process can publish a state: `{ "type": "state", "source": "spotify", "id": "playback", "value": "playing" }`, rate and size limits as for events, states tied to the connection (`unavailable` on close, which is the Last Will analogue) with an explicit `keep` for one-shot publishers, and discovered states in the catalog. Not part of this milestone's Definition of Done | Only started if the product needs a feed other than Agent Control and installed plugins |

Order inside phase 1: model and evaluator first (pure, table-driven), then hub, then runtime wiring, then migration, then Agent Control. The evaluator can be finished and tested before anything else exists.

## 20. Risks

| Risk | Impact | Mitigation |
| --- | --- | --- |
| The word "state" now means two things (a look name in `Control.states`, and an observed value) | Confusion in code and docs | The interface says "look" for the first. Code uses `Hub`, `Observed` and `ObservableDefinition` for the second. The saved field keeps its name. A glossary opens `docs/feedback/overview.md`. Renaming the saved field is possible later in a schema bump and is not worth the churn now |
| Migration changes how the shipped Agent Control surface behaves | A visible regression in the flagship surface | Golden frames for every status and command state before and after; fixtures; backup; `legacyBindings` fallback |
| Users have edited their Agent Control surface | Edited bindings convert differently | The conversion is by shape, not by surface id; anything unrecognised is kept in `legacyBindings` |
| A plugin reload now shows `unknown` where it used to show a stale look | Looks like a regression to someone used to the lie | It is the honest answer; documented; an optional `state` export is the future fix (12.6) |
| Two layers (overlay, derived) are harder to reason about than one | Wrong mental model | One sentence in the editor, one line in explain, and layering is only ever event over state |
| The hub adds a lock | Deadlock | Leaf lock, fixed order, writers call nothing, a stress test, no `await` while held |
| Event rules still lose events on bus lag | A missed momentary flash | Logged once; documented as the one remaining loss; states are unaffected |
| Parameterised states exist for Agent Control only | Looks asymmetric | The model allows it everywhere; manifests get it when a plugin needs it, without breaking schema 3 |
| Scope creep into the WebSocket publisher, polling, TTL, AND/OR | A milestone that never lands | Each has a written trigger (10.4, 11.4, 12.6, phase 5). None is in the Definition of Done |
| Roles are a closed vocabulary | A provider wants a seventh intent | A new role comes with a schema bump. Six cover every example in the brief and in Agent Control |
| The explain panel grows into a debugger | Cost and noise | Fixed to three questions; no history, no event log |

## 21. Definition of Done for the implementation phase

1. Every existing check still passes: `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test`, `tsc --noEmit`, `eslint .`, Vitest, `npm run build`, `npx tauri build`.
2. `ObservableDefinition`, `ObservableRef`, `StateHub`, the evaluator, the indexes and `feedback_explain` exist as described; the hub is a leaf lock and no write goes through the bus.
3. `Catalog.observables` is populated from Agent Control, the demo plugin and installed plugins, carries no values, and parses in Zod; the contract examples are regenerated and guarded.
4. The characterization tests exist; P1, P2, P4 (state rules), P5, P7 and P8 are flipped to the new behaviour and P3 is unchanged.
5. The mandatory acceptance test passes with the real compiled Counter, and its four variants pass.
6. Workspace schema 4 loads v1, v2 and v3 files; the backup exists and is untouched after save; golden frames of the Agent Control surface are identical; nothing unconvertible is lost.
7. Manifest schema 3 is implemented with the validation of 12.3; schema 1 and 2 plugins, including the C Ping example, still load; ABI 1 and SDK 0.1.0 are unchanged and no `.wasm` is rebuilt.
8. A plugin's states are `unknown` after load or reload and `unavailable` while disabled, failed or missing; a pad depending on them follows within one render tick.
9. `1` equals `1.0`; `is not` is false for `unknown` and `unavailable`; first match wins; an overlay hides and then reveals the derived look; nothing depends on event arrival order for state rules.
10. An unrelated event evaluates nothing, a state change evaluates exactly its dependents, a burst evaluates each once, and the 2,669-control benchmark is within twice the starter workspace's.
11. Bus lag does not corrupt a state-backed pad and logs one warning.
12. The Feedback section lets a person, with no JSON and no ids, add a rule for a Counter state, fill it from values, add the unavailable rule, reorder, delete, and see the explain line; it works from the keyboard; every state is a word.
13. The suggestion line appears for an action that suggests a state, creates nothing without a click, and an action never writes a rule.
14. The Plugins screen shows each declared state's value, status and age.
15. Docs exist and are linked: `docs/feedback/{overview,states,rules,providers,diagnostics}.md`, with the updates listed in phase 4; the link checker reports no broken link; nothing is documented that is not implemented.
16. The production binary is exercised under the real-app harness (migrated workspace, add feedback, press, restart, disable the plugin, reorder, explain) with screenshots, and every defect that run finds is fixed or written down.
17. The final report states honestly what is not done: no `stale`, no AND/OR, no WebSocket publisher, no pull export, and that WebKitGTK on Linux is the only webview exercised.

## Deliberately out of scope

Marketplace, network and filesystem capabilities, new external integrations (Spotify, Home Assistant and similar), workflow chaining, macros, scripting, AI-generated rules, semantic search, historical analytics, remote state synchronisation, cloud persistence, complex rule graphs, an event debugging console, and a State Binding Editor beyond the Feedback section described here.
