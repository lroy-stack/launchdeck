# Surfaces: how agent state becomes light

The Agent Control surface is built for one thing: knowing what your agents are doing at a glance, and being able to step in without thinking about where to press. The same surface runs on the virtual Launchpad X and on a physical one, and it is a normal LaunchDeck surface. You can open it in the editor, move pads around and change colours.

## Design principles

* **Glanceable.** One pad per session, coloured by status. Nothing needs reading.
* **Low cognitive load.** Few states, each visually distinct. Urgent states move; calm states do not.
* **Fast intervention.** The two decisions that matter most, Approve and Reject, are the largest pads on the surface.
* **Clear state, never colour alone.** Urgency is carried by motion as well as colour (flash, pulse), and the desktop UI always prints the status name next to its dot.
* **No lies.** A command that cannot work right now is dim. It does not light up and then fail.

## Layout

```
 col   0       1       2       3       4       5       6       7
row 0  [ 1 ]   [ 2 ]   [ 3 ]   [ 4 ]   [ 5 ]   [ 6 ]   [ 7 ]   [ 8 ]      one track per pad
row 1  [ ^ ]   [ ^ ]   [ ^ ]   [ ^ ]   [ ^ ]   [ ^ ]   [ ^ ]   [ ^ ]      marker under the selected track
row 2  [   APPROVE   ] [   REJECT    ] [Pause] [Resume] [Retry] [Stop]
row 3  [     (2x2)   ] [    (2x2)    ] [Cont.]
row 4  [Run tests] [Review diff] [Summarize]                               start a task
 ...
row 7  [  Run Agent Demo  ] [Reset]                    [  Open Agents  ]
top row (round buttons): surface switchers
```

* **Tracks (row 0).** A track is a session, or an agent that has no session yet. Press one to select it. Long-press dismisses a finished session.
* **Markers (row 1).** The selected track is white. The command pads act on the selected track.
* **Approve and Reject.** Large, because they are the pads you most often hit in a hurry. Approve flashes while the selected session is waiting for a decision.
* **Pause, Resume, Retry, Stop, Continue.** Each lights up only when it is possible for the selected session.
* **Task pads.** Start a task on the selected agent (or the first agent that can). Edit the text in the inspector.
* **Run Agent Demo, Reset, Open Agents.** Start and clear the simulated agents; bring up the Agents section of the app.

Selection never jumps while it points at something real. If nothing is selected, the first approval that arrives is selected automatically, so APPROVE is ready.

## The mapping: state to appearance

One module owns it: `src-tauri/src/agents/visual.rs`. The surface builder, the palette shown in the desktop UI and the tests all read the same functions, so the app's dots cannot drift from the pads.

### Session status on a track

| Status | Colour | Motion | Why |
| --- | --- | --- | --- |
| `idle` | dim blue | none | Online and calm. |
| `starting` | yellow | none | About to run. |
| `running` | green | pulse | Alive and working. |
| `tool_use` | cyan | pulse | Working, using a tool. |
| `waiting` | purple | none | Waiting on input that is not a decision. |
| `approval_required` | white | **flash** | Needs you, now. |
| `paused` | orange | none | Held by you. |
| `completed` | green | none | Done. |
| `failed` | red | **flash** | Needs attention. |
| `cancelled` | dim red | none | Stopped. |
| `offline` | very dim grey | none | Not connected. |
| (empty slot) | near black | none | Nothing here. |

A test asserts that every status looks different from every other, that only the urgent states flash, and that calm states do not animate.

### Command pads

A command pad has a resting colour (green for Approve, red for Reject and Stop, orange for Pause, and so on) and shows one of six states.

| Command state | Appearance | Meaning |
| --- | --- | --- |
| `unavailable` | the colour at about 10% | Cannot be issued now. Includes unsupported, read-only, nothing to act on. |
| `available` | the colour | Can be issued. |
| `attention` | the colour, flashing | Available, and a decision is waiting for exactly this command. |
| `pending` | yellow pulse | Sent to the agent, no result yet. |
| `succeeded` | green | Confirmed. Shown for about 2.5 seconds. |
| `failed` | red flash | Refused or errored. Shown for about 4 seconds. |

## How it is wired

A surface never talks to agents. It binds to projection events that Agent Control publishes and calls plugin actions when pressed.

```
agents --> AgentControl --publishes--> slot.changed / focus.changed / command.state
                                                |  (event bus, source "agent-control")
        surface controls bind to them <---------+
                |
        renderer adapts the visual to the device  -->  virtual Launchpad  and  physical Launchpad
```

A slot pad's binding, as it appears in the workspace file:

```json
{ "source": "agent-control",
  "rules": [ { "event": "slot.changed", "when": [ { "field": "slot", "equals": 0 } ], "setState": "$payload.status" } ] }
```

`$payload.status` means "use the status in the event as the control's state name". The control has a visual for each status name (`running`, `failed`, and so on), so the look follows without any agent-specific code in the device layer. An empty slot arrives as the state `empty`.

Pressing a pad runs `{"provider":"agent-control","action":"approve"}` or one of: `reject`, `pause`, `resume`, `cancel`, `retry`, `continue` (send "continue", or the text you set), `newTask` (task text in the args), `focusSlot`, `clearFocus`, `dismissSlot`, `runDemo`, `resetDemo`, `openApp`. The plugin turns an action into a command on the selected track and the service decides whether it is allowed. If it is not, the pad was already dim.

### One state, three views

The desktop app, the virtual Launchpad and a physical Launchpad all derive from the single `AgentControl` state through the same events and renderer. They cannot disagree. State survives UI rerenders and switching surfaces because it lives in Rust: switch to another surface and back and the tracks are exactly where you left them.

## Device capabilities

The surface is designed once and the renderer adapts it.

| If the device... | then... |
| --- | --- |
| supports native pulse and flash (the virtual Launchpad, which animates in the app) | They are passed through as effects. |
| does not support an animation (the physical Launchpad X, whose hardware pulse and flash use palette colours that LaunchDeck does not rely on) | It is synthesised from timed RGB frames at 25 per second: pulse swings brightness between 25% and 100% over 1.4 s, flash toggles every 0.7 s. Only changed pads are sent, so calm pads cost nothing. |
| is not RGB | Colours become brightness only. Status is then carried by motion and by position, which is why urgent states flash. |
| has fewer pads | Positions the device does not have are dropped; pads that remain keep working. |

The surface is laid out for an 8x8 grid. There is no dedicated small-grid layout yet, so on a smaller device some command pads fall off the edge. The desktop app is complete on any hardware.

## Make your own

Copy the surface in the editor, or write one as a surface file. A control binds to events and picks its look by state name. The editor changes positions, actions and looks; the `bindings` list itself is written in the surface file and the inspector shows it read-only. Two recipes cover most needs.

**A pad that mirrors one track.** Give the control a visual for each status name you care about (`running`, `approval_required`, `failed`, plus a default look). Add a binding with `source: "agent-control"`, event `slot.changed`, a `when` of `slot` equals the track number, and `setState` of `$payload.status`. This is exactly how the built-in track pads work, and it follows the track through every state, including `empty`.

**A pad that reacts to a moment.** Bind to a [semantic event](domain-model.md#events) such as `session.failed` or `approval.created`, set a state, and clear it with a second rule on the event that ends it (`approval.resolved`). Pressing can run `plugin: "agent-control"`, action `openApp`. Keep these for a single known agent: with several sessions, one session finishing would clear a light another still needs.

An aggregate "anything needs me" light is not published yet. Surfaces should not count sessions themselves, and the projection events (`slot.changed`, `focus.changed`, `command.state`) describe tracks and the selected command state, not totals. See [Known limitations](overview.md#known-limitations).

## Editing and accessibility

* The labels and icons are meant to be read at a distance; turn labels off in the editor if you prefer symbols.
* Colour is never the only channel. If colours are hard for you to tell apart, rely on motion and position, or on the desktop view, which prints every status name.
* Reduced-motion preferences slow the on-screen pad animations in the app. Physical LEDs are driven by the device.
