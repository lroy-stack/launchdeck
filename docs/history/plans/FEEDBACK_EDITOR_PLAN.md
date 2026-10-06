# Visual Feedback Editor: plan

Goal: make the feedback model authorable in the Inspector, with no JSON and no
event-bus vocabulary. The runtime is not redesigned. Rust keeps every
decision; React edits a draft and draws what Rust says.

## What exists and is reused

| Piece | Where | Reuse |
| --- | --- | --- |
| Draft and save lifecycle | `Inspector.tsx` (`ControlEditor`: local draft, `Not saved`, Save/Revert, `update_control`) | Feedback is part of the same `Control` draft. No second editing model. Pads change when Save applies it, exactly like an action. |
| Rule model | `state/feedback.rs`, `schema.ts` (`stateRuleSchema`, `eventRuleSchema`, `stateTestSchema`) | Unchanged. Schema 4 holds everything the editor writes. |
| Observable Catalog | `state/catalog.rs`, `lib/observables.ts`, store `observables` kept current by `observables-changed` | The only source of the picker. Definitions, value labels, roles, scope params, event fields, availability, current readings. |
| Parameter vocabulary | `ParamDef`, `ParamForm.tsx`, `normalizeArgs` | Scope fields are `ObservableDefinition.params` drawn by `ParamForm`. |
| Evaluator and explanation | `state/eval.rs`, `core/explain.rs`, `explain_control` | The Live Feedback panel. Never reimplemented. |
| Looks | `Control.states` (name to `{color, animation}`), `visual` is the resting look, `idle` means rest | Look editor edits `states`. No new visual model, no brightness. |
| Library patterns | `ActionLibrary.tsx` (combobox, listbox, local search, `tokensOf`) | Observable picker uses the same pattern and tokenizer. |
| Suggestions | `ActionDefinition.suggests` | Suggested feedback, added only on request. |
| Contract fixtures | `examples/observables.json` + `observables_contract.rs` | Extended with the new review reply. |

## Minimal backend and Tauri additions

The draft cannot be evaluated without saving, and saving is the only place
rules are validated. One read-only command closes both gaps.

1. `review_feedback(surfaceId, pageId, control) -> FeedbackReview`, nothing is stored.
   - `issues`: exactly what `update_control` would refuse (the existing
     `prepare_rules`), now with a `field` (`look|state|args|test|value|event|where|hold|rules`)
     and `param` so the UI puts the message next to the input.
   - `notes`: diagnostics Rust can prove: `missingLook`, `shadowed`,
     `missingObservable`, `providerUnavailable`.
   - `limits`: rule count, hold range and default, look-name length, so the UI
     does not copy constants.
   - `preview`: the `ControlExplanation` of the draft against the live hub
     (same `explain()` code), present only when rules or looks differ from the
     saved control. When they do not differ, the UI calls the existing
     `explain_control`, which also knows the temporary overlay.
2. `state::lint`: pure `shadowed(rules)`. Only two cases are claimed: an identical
   earlier test on the same address, and an earlier `hasValue` over any test that
   needs a value. No symbolic analysis.
3. Contract: the scenario behind `examples/observables.json` also captures a review.

No schema bump, no new runtime state, no change to evaluation.

## Frontend structure

```
src/lib/feedbackEdit.ts        pure editing model (unit tested)
src/features/feedback/
  FeedbackEditor.tsx           the FEEDBACK section: composition and empty state
  AddFeedback.tsx              State or Event, picker, scope, first rule
  ObservablePicker.tsx         searchable combobox over the catalog
  StateRules.tsx               grouped, numbered, reorderable state rules
  EventRules.tsx               event overlays: event, conditions, look, hold
  LookSelect.tsx               choose a look or create one
  LooksEditor.tsx              rename, colour, animation, remove
  LiveFeedback.tsx             the explanation panel
  useReview.ts                 review + explain, refreshed on relevant changes only
```

`Inspector.tsx` gains two headed sections, ACTION (what happens on press) and
FEEDBACK (what external state this control represents), replaces the read-only
rule list and the "State appearances" block, and blocks Save while Rust reports
issues for the draft.

## Rule-editing model

* The control keeps one ordered list. The UI shows it as two lists because the
  evaluations are independent: state rules (first true wins) and event rules
  (first match starts the overlay). Numbers are per list and always visible.
* Consecutive state rules on one observable share a header with its live value.
* Reordering swaps with the previous or next rule *of the same kind*; other
  rules do not move. Buttons and `Alt+Arrow` both work. Nothing is reordered on its own.
* Operators come from the definition's type: enum, string and boolean get
  is / is not; number adds below, at most, above, at least; every state adds
  has a value, has no value yet, is not available. These are the runtime's
  nine `op`s and nothing else.
* Values: enum and boolean from the definition's labelled values (stored value
  is the stable one), number inputs, free text only for a string state.
* Look target: resting look (`idle`), an existing look, a new look created in
  place, or "the look named after the value" for rules that already use it.
* Resting look is a property of the pad, shown in its own section. No wildcard rule.
* A rule whose observable is gone keeps its data; it can change look, be
  replaced by another observable (tests that no longer fit are cleared and
  flagged) or removed.

## Validation flow

1. The UI only offers valid combinations and marks incomplete ones (no value,
   no look, missing scope) as `(needed)`.
2. On every relevant draft change (debounced 250 ms) the UI calls
   `review_feedback`; `issues` appear under the field, `notes` under the rule.
3. Save is disabled while issues exist; `update_control` still validates, so a
   stale review cannot save something invalid.
4. Catalog changes re-run the review only when a value, availability or
   definition that a rule reads changed.

## Test strategy

* Pure model (`feedbackEdit.test.ts`): search, grouping, operators by type,
  rule construction, move/remove/retarget, look create/rename/remove, numbering,
  suggestions, JSON round trip against `stateRuleSchema`.
* Components (jsdom, preview transport with Rust-captured fixtures): picker,
  add flow, composer, scoped fields, ordering by keyboard, looks, missing
  observable, explanation in every state (known, unknown, unavailable, matched
  rule, no match, missing look, overlay).
* Rust: `lint`, `review` (issues by field, notes), contract fixture.
* End to end, real Counter WASM and virtual Launchpad (`tests/feedback_editor.rs`):
  the control is `examples/feedback-editor-counter.json`, which a TypeScript test
  proves is exactly what the editor's functions produce. Rust then runs the
  workflow: Action Library entry, review, save, Live, presses, look per level,
  surface away and back, disable (unavailable and explanation), enable (unknown), invoke (known).
* Event acceptance: derived Active, real `counter.changed` event, Error overlay,
  expiry, Active again, state untouched.
* Real desktop: the Tauri app under Xvfb, driven through the real WebView.

## Acceptance flow (what a person does)

Edit, select an empty pad, Library: Counter / Increment, Feedback, Add feedback,
State, Counter / Level, add a rule for each value, choose
zero to Default, low to a new "Active", high to a new "Warning", max to a new
"Error", watch Live Feedback say what Rust evaluates, Save, switch to Live,
press the pad and watch it change, leave the surface and return (look is
immediate), disable the plugin (explanation says unavailable and why), enable it
(unknown), press again (known, correct look). Then add an event overlay with a
hold and watch it expire back to the state's look.
