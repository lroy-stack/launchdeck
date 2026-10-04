# The feedback editor

A pad can follow something that changes: the Counter's level, the status of an agent's track, a channel that goes down, a build that finishes. You set that up in the Inspector, with the mouse or the keyboard, without writing JSON and without knowing how events travel inside LaunchDeck.

This page describes what is built. How the rules behave at run time is in [state-and-feedback.md](state-and-feedback.md); what can be observed, and how, is in [observables.md](observables.md).

## Where it is

Select a pad in Edit mode. The Inspector is divided by what the pad **does** and what it **shows**:

| Section | What it holds |
| --- | --- |
| **Action** | What happens when the pad is pressed (and long-pressed). The Action Library gives the pad one; the form below it is the action's own settings. Nothing here decides what the pad shows. |
| **Resting look** | The pad's own icon, colour and animation: how it looks when no feedback rule applies. |
| **Feedback** | What the pad shows as things change: a live explanation, the rules, the looks they use, and suggestions. |

Everything is edited in the same draft the rest of the Inspector edits. "Not saved", Save, Revert and Delete are the ones that were already there, and nothing reaches the pad or the file until Save.

## Adding feedback

A pad with no feedback says so and offers **+ Add feedback**. The flow has three steps, and the third exists only when it is needed:

1. **A state or an event.** A state has a value for as long as it lasts (a level, a status): the pad follows it. An event happens once and passes (a build finished, an approval asked): the pad shows a look for a while.
2. **Which one.** One searchable list for every source. See the picker below.
3. **Which instance**, for a state that exists once per track, channel and so on. The form is drawn from the same parameter definitions as an action's settings (a number with a range, a choice, a text), and the button stays disabled, with the message next to the field, until the arguments are valid.

The result is a first rule with the state's first value already chosen. It asks you for the look and nothing else; the cursor lands on its first field.

### The picker

* **One list for all providers.** Plugins, Agent Control, LaunchDeck itself (the active surface and page, actions, devices) and the demo appear side by side, grouped under the name of the provider that offers them.
* **Words, not ids.** Each row shows the name, what the state can hold (`Empty, Low, High, Full`; `Number`; `Event with Value, Level`), and, for a state, its status and value now. A state that exists once for each track says `Per track`. Technical ids are not shown here.
* **Search** covers names, provider names, categories, keywords, the words of a state's values and the descriptions, and every word has to match. It forgives one slip of the keys. The box has the cursor when the picker opens: type, move with ↑ and ↓, take with Enter or the Use button, clear with Escape.
* **Nothing is hidden for being unavailable.** An event of a plugin that is turned off is listed, tagged `Unavailable`, with the reason as its tooltip. You can write a rule for it; it will work when the plugin runs.
* **An empty catalog says so** and points to Plugins.

## State rules

A rule reads as a sentence: **When** *it* `is` `High`, **show** `Warning`.

* **Operators** are the nine the runtime has, and the editor offers the ones that make sense for the state's type:

  | State holds | Operators offered |
  | --- | --- |
  | A choice, a text, yes/no | `is`, `is not`, `has a value`, `has no value yet`, `is not available` |
  | A number | the above, plus `is below`, `is at most`, `is above`, `is at least` |
  | Unknown to the catalog (its provider is gone) | `has a value`, `has no value yet`, `is not available` |

  Changing the operator keeps the value when it still fits and takes it away when the operator has none. A comparison that does not fit the state (a number test on text) cannot be made here; if one is already saved, it is shown as `(does not fit this state)` and Rust refuses to save it.
* **Values** come from the state's definition, with its labels (`Empty`, `Full`), not from what you remember. A number is a number field; a half-typed or empty number is not sent anywhere and blocks Save until it is a number.
* **+ Add a rule** adds the next value nobody tests yet, after the last rule of that state.
* **Add a rule for each of the N other values** adds one rule per value that has none, each showing a look named after the value. A look that exists is reused, a missing one is created in a colour the value's role suggests (`ok`, `warning`, `error` ...). It tells you what it did, and it never changes a rule or a look you already have.
* **+ When it has no value yet** and **+ When it is not available** add the optional rules for a state that is `unknown` or `unavailable`. They are offered once each.

### Order

The runtime tries the state rules from the first down and the first one that is true decides. So the rules are **numbered**, and the number is the order.

* Move a rule with its **↑** and **↓** buttons, or press **Alt+↑** / **Alt+↓** from any field of the rule. The cursor goes with the rule, and a line says where it went (`Rule moved up: it is now rule 2 of 4`). At either end the button says so and nothing moves.
* **Nothing reorders by itself.** Adding a rule puts it after the last rule of its state. Removing one renumbers the rest and puts the cursor on the rule that took its place. Rules of the same state sit under one header while they are neighbours; if you move another state's rule between them, the header repeats, because the order is what it is.
* Event rules are numbered apart (`Event rule 1`) and keep their own order: the first one that matches an event decides its look. Moving one never crosses a state rule.
* A rule that can never be reached is marked `Never used: rule 1 is tried first and is true whenever this one is`. This is said only when it is provable (see [What Rust says](#what-rust-says-about-a-draft)).

## Scope

For a state with arguments the run header names the instance (`channel alpha`, `track 2`) and has a button, **Change track…** or **Change channel…**, that opens the same form as the first time and applies the new instance to every rule under that header. **Change state…** swaps the state: each rule keeps its test if the new state can take it, and says if it had to reset one (`One condition did not fit it and was reset: check it.`).

## Looks

A **look** is a colour and an animation with a name, kept in the control's `states`. The pad's own colour is its **resting look**.

* **Choosing.** Each rule has a look field: *Resting look*, each look of the pad, *The look named after the value* (for a state whose value has names, and not for a test that has no value), *The look named in "Level"* (for an event whose payload field has names), and *+ Create a new look…*.
* **The resting look is a look, not a rule.** Choosing it saves `"show": "idle"`. There is no catch-all rule and no look called `idle` is made: the runtime already reads `idle` as the resting look unless the pad has one by that name, and a rule that is simply not true also leaves the pad at rest.
* **Creating in place.** *+ Create a new look…* opens a small form under the rule: a name, a colour (it starts from what the name says: *Warning* amber, *Error* red) and an animation. *Create look* makes the look and shows it from that rule in one step. A name must be new, at most the length Rust allows, and cannot be `idle`.
* **The Looks list** (under the rules) shows each look with its animation and which rules show it. *Edit* changes its name, colour and animation, and removes it.
  * **Renaming** rewrites every rule that showed the old name, so nothing is left pointing at it.
  * **Removing** a look nothing shows is immediate. Removing one that rules show asks first, and says what happens: the rules keep their choice, the choice reads `(this pad has no such look)`, and the pad rests until you create the look again or choose another.
* A rule that names a look the pad does not have is **not an error and not silent**: the field says so and the note says what the pad does.

## Event rules

An event rule reads: **Only if** *conditions*, **show** a look, **keep** it for a time.

* **Only if** is optional and lists conditions on the event's payload, one per field (`Level is High`). A field with named values is a list, a number is a number, anything else is text. The most the runtime allows is the limit in `review_feedback`.
* **Show** works as for state rules. *The look named in "Level"* shows whichever look the event's payload names.
* **Keep** is *For a time* (seconds, between the limits Rust gives; the default is the runtime's) or *Until another event*. Under it a sentence says what that means: `Shown for 0.7 seconds, then the pad goes back to what its state rules say.`
* **Change event…** points the rule at another event; conditions and a field-named look the new event does not have are dropped, and the editor says so.

An event's look sits over the look the state rules give; when it ends the state rules decide again.

## When something is missing

Saved rules are kept as they are, whatever happens to the things they name. Nothing is deleted or rewritten behind your back.

| Situation | What you see | What you can do |
| --- | --- | --- |
| The provider is stopped or disabled | The state's status reads `Unavailable`, and a note gives Rust's reason (`Counter Plugin is not running: The plugin is turned off in Plugins.`) | Nothing needed: the rules keep working when it runs. |
| No installed provider offers the state or event any more | `A state that is no longer offered`, with what the rule named | **Replace state…** (or **Replace event…**), **Remove these rules**; or leave it. The rule's look, move and remove still work. |
| A rule names a look the pad does not have | The look field and a note | Create it, choose another. |

You can still rename, recolour or move a pad whose plugin was uninstalled: saving does not re-check rules that have not changed.

## Live feedback

The **Live feedback** panel is the first thing in Feedback once the pad has rules. It does not work anything out. Every fact on it is what Rust answered:

* each state the pad reads, with its status (`Known`, `Unknown`, `Unavailable`) and, for `Known`, its value in the definition's words; for the others, **Rust's own sentence** for why (`Channels is running but has not reported status for these arguments yet.`);
* the **rule that decided**, by its number and its test (`Rule 3: it is High`), or that none is true and the pad rests;
* the **look from the state rules**, the **temporary look** an event put on top (which event rule, and how long is left, or that it stays until another event), and the look **on the pad**;
* any rule that decided on a look the pad does not have;
* **Technical details**, closed by default, with the addresses.

When the draft is what is saved, the panel is the saved pad's explanation (`explain_control`), and the title says *Live feedback*. When you have changed rules or looks, it becomes *Feedback if you save now*: Rust evaluates the draft against the values it holds now. A temporary look from an event is not part of a draft, and the panel says so. It is asked for again when the draft changes or when a value, availability or definition that this pad reads changes, and not otherwise: a Counter value changing does not refresh a pad that reads a channel. In a browser without the desktop app there is no runtime to ask, and the panel says `Explanations come from the desktop app.`

## Suggested feedback

An action can say which states a pad for it usually shows (the Counter's actions suggest *Counter level*). When the pad has such an action and does not read that state yet, **Suggested** lists it with an **Add** button. Nothing is ever added by itself.

## What Rust says about a draft

`review_feedback(surfaceId, pageId, control)` judges the unsaved control and changes nothing. It runs the same checks `update_control` runs on save, so what it reports is what saving would do.

| Part of the answer | Meaning | Effect |
| --- | --- | --- |
| `issues` | What saving would refuse, each with the rule and the part of it (`state`, `args`, `test`, `value`, `look`, `event`, `where`, `hold`, `rules`) | Shown next to that field. **Save is disabled** while any exists. |
| `notes` | What Rust can prove without refusing: `missingLook`, `missingObservable`, `providerUnavailable`, `shadowed` | Shown as notes on the rule or its header. Never block saving. |
| `limits` | Most rules, longest look name, most conditions, the hold's default and bounds | Used for the form, never copied into the interface. |
| `preview` | The draft's explanation, when its rules or looks differ from the saved ones | The Live feedback panel. |

`shadowed` is deliberately narrow. A rule is reported only when an earlier rule tests the same state at the same address with the same test, or when an earlier `has a value` covers it, and the earlier rule always names a look. Two overlapping ranges are not analysed: a note that might be wrong would be worse than none.

The editor also marks what is plainly unfinished at once, before Rust answers (a rule without a look, an empty number, an argument that is required). Those are shown as prompts (`Choose the look to show.`), not as errors, because nothing is wrong yet. Rust's message replaces the local one about the same place.

## What is saved

The workspace schema is unchanged (**4**). The editor writes the rules in `feedback` (in the order shown) and the looks in `states`, and the pad's own colour in `visual`. It never writes a value, a status, a reason, a catalog entry or an explanation: those belong to the runtime and are asked for again each time. Reopening a pad shows the rules exactly as saved, with their order and instances.

## Keyboard and screen readers

* Every control has a name that includes its rule (`Rule 2 condition`, `Event rule 1 seconds`). Messages are tied to their field with `aria-describedby`, and a field with an error is `aria-invalid`.
* The picker is a combobox with a listbox of options in groups; the highlighted option is `aria-activedescendant`.
* Moves, removals, replaced states and created looks are announced in a status line, which is also visible.
* Status is always a word (`Known`, `Unknown`, `Unavailable`), never only a colour. Moving a rule keeps the cursor on the same control of the same rule.
* Everything described here can be done without a mouse. Alt+↑ / Alt+↓ move a rule from any of its fields.
* The Launchpad surface is one tab stop: Tab enters it on one pad and leaves it, the arrow keys (and Home, End, Ctrl+Home, Ctrl+End) move between pads, Enter or Space selects a pad in Edit mode and presses it in Live mode for as long as the key is held.

## How it is built

* **Rust stays the authority** on what exists, what a value is, whether a rule is acceptable, which rule wins and what a pad shows. The editor adds one read-only command, `review_feedback` (`core/review.rs`), and a lint (`state/lint.rs`) for the provable `shadowed` case. `explain_control` is reused as it was.
* **React edits, searches and renders.** `src/lib/feedbackEdit.ts` holds the editing model as pure functions (search, operators by type, building and moving rules, scope, looks, problems). It never evaluates a rule and never decides a look; the components in `src/features/feedback/` draw it.
* **The catalog is current without polling.** `observables-changed` keeps it up to date, and the live panel asks again only when something this pad reads changes.

### Tests

* Pure model: `src/lib/feedbackEdit.test.ts`.
* Components, on the preview transport with the catalog and workspace Rust emitted: the picker (`picker.test.tsx`), rules, order, events, missing providers and saving (`rules.test.tsx`), looks (`looks.test.tsx`), the live panel and when it asks again (`explain.test.tsx`).
* Contract: `examples/observables.json` carries the real `review_feedback` replies and is checked from both sides.
* **The real window.** `tools/desktop-e2e` drives the desktop app itself (the WebKitGTK window under a virtual display, through WebDriver) with the real Counter and Channels plugins: authoring from an empty pad, saving, pressing the virtual Launchpad, switching surface, turning a plugin off and on and taking its folder away, an event overlay, scoped states, reordering by button and by Alt+↑/↓, and a run done with the keyboard only. It is slow and needs a Linux desktop stack, so it is run on request (`python3 tools/desktop-e2e/run.py feedback`) and is not part of `cargo test` or `npm test`. See [tools/desktop-e2e/README.md](../tools/desktop-e2e/README.md).
* **Shared acceptance controls.** `examples/feedback-editor-counter.json` and `examples/feedback-editor-event.json` are the exact controls the editor produces for the Counter and event scenarios. A TypeScript test proves the editor's functions generate them, and `src-tauri/tests/feedback_editor.rs` runs them against the real Counter and Channels plugins and the virtual Launchpad: the pad changes as the counter goes up, survives switching surface and back, goes dim when the plugin is disabled, `unknown` when it comes back and correct once it reports; and an event overlays Error for 0.7 seconds and returns to the state's look with the state untouched.

## Not built

AND / OR and nested conditions, staleness timers, scripting, aggregate states ("anything needs me"), mappings that call a model, copying rules between pads, and a plugin-drawn editor. A rule is one comparison on one state, or one event.
