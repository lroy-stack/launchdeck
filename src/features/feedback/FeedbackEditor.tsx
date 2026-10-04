// The FEEDBACK half of the Inspector: what the pad shows as things change. It
// edits the same draft as the rest of the Inspector (`Control`), so Save, Revert
// and "Not saved" are the ones that already exist. It decides nothing about what
// a rule means: Rust judges the draft (`useFeedbackReview`) and explains it.

import { useLayoutEffect, useRef, useState } from "react";
import { addressFor, addressKey, defaultTest, indexesOf, insertRule, newEventRule, newStateRule, suggestedObservables, type Problem } from "../../lib/feedbackEdit";
import type { ActionDefinition, Control, Observable, Scalar, Surface } from "../../lib/schema";
import { isStateRule } from "../../lib/schema";
import { useStore } from "../../stores/runtime";
import { EventRules } from "./EventRules";
import { LiveFeedback } from "./LiveFeedback";
import { LooksEditor } from "./LooksEditor";
import { ObservableFlow } from "./ObservablePicker";
import { StateRules } from "./StateRules";
import { Swatch, ruleSel, type EditorContext, type Focus } from "./parts";
import type { ReviewState } from "./useReview";

interface Props {
  /** The draft the Inspector edits. */
  control: Control;
  /** The control as saved; `null` for a pad that does not exist yet. */
  saved: Control | null;
  review: ReviewState;
  /** What is missing or wrong in the draft, from the local check and from Rust. */
  problems: Problem[];
  /** The actions on the pad, whose `suggests` offer a first rule. */
  actions: (ActionDefinition | undefined)[];
  surfaces: Surface[];
  currentSurface: Surface;
  onChange: (next: Control) => void;
}

type Flow = { kind: "state" | "event"; from?: { observable: Observable; args: Record<string, Scalar> } } | null;

export function FeedbackEditor({ control, saved, review, problems, actions, surfaces, currentSurface, onChange }: Props) {
  const catalog = useStore((s) => s.observables);
  const root = useRef<HTMLElement>(null);
  const focusing = useRef<Focus | null>(null);
  const [message, setMessage] = useState("");
  const [flow, setFlow] = useState<Flow>(null);

  const limits = review.review?.limits;
  const feedback = control.feedback;
  const full = !!limits && feedback.length >= limits.maxRules;
  const empty = feedback.length === 0;
  const dirty = JSON.stringify(control) !== JSON.stringify(saved);

  // The cursor goes where the last change asked, once the change is drawn.
  useLayoutEffect(() => {
    const wanted = focusing.current;
    if (!wanted) return;
    focusing.current = null;
    const target = root.current?.querySelector<HTMLElement>(wanted) ?? root.current?.querySelector<HTMLElement>(wanted.split(" ")[0]!);
    target?.focus();
  }, [control]);

  const ctx: EditorContext = {
    control,
    catalog,
    limits,
    problems,
    notes: review.review?.notes ?? [],
    surfaces,
    currentSurface,
    full,
    update: (next, focus) => {
      focusing.current = focus ?? null;
      setMessage("");
      onChange(next);
    },
    say: setMessage,
  };

  /** The first rule for an observable the person chose: a state joins the rules of the same state, an event the events. */
  const addFirst = (o: Observable, args: Record<string, Scalar>) => {
    setFlow(null);
    if (o.kind === "event") {
      const added = insertRule(feedback, newEventRule(o.provider, o.local));
      return ctx.update({ ...control, feedback: added.feedback }, ruleSel(added.index, "look"));
    }
    const address = addressFor(o, args);
    const key = addressKey(address);
    const sameState = indexesOf(feedback, "state").filter((i) => {
      const r = feedback[i]!;
      return isStateRule(r) && addressKey(r.when.state) === key;
    });
    const used = sameState.flatMap((i) => {
      const r = feedback[i]!;
      return isStateRule(r) && r.when.is.op === "equals" ? [r.when.is.value] : [];
    });
    const added = insertRule(feedback, newStateRule(address, defaultTest(o.value, used)), sameState[sameState.length - 1]);
    ctx.update({ ...control, feedback: added.feedback }, ruleSel(added.index, "condition"));
  };

  const suggestions = suggestedObservables(catalog, actions, feedback);
  const startSuggestion = (o: Observable) => (o.params.length > 0 ? setFlow({ kind: o.kind, from: { observable: o, args: {} } }) : addFirst(o, {}));

  const hasStateRules = indexesOf(feedback, "state").length > 0;

  return (
    <section ref={root} className="field-group feedback" aria-label="Feedback">
      <h3 className="section-title">Feedback</h3>
      <p className="hint">Make this pad follow something that changes. The pad shows its look from the first state rule that is true, and an event can show a look on top for a while.</p>
      <p className="rule-status" role="status" aria-live="polite">
        {message}
      </p>

      {empty && !flow && (
        <div className="feedback-empty">
          <p>
            <b>No feedback yet.</b> This pad shows its resting look and nothing else.
          </p>
          <button type="button" className="btn primary" data-part="add-feedback" onClick={() => setFlow({ kind: "state" })}>
            + Add feedback
          </button>
        </div>
      )}

      {!empty && (
        <>
          <LiveFeedback state={review} control={control} catalog={catalog} saved={saved !== null} dirty={dirty} />
          {hasStateRules && (
            <div className="rule-block">
              <h4>Follows a state</h4>
              <p className="hint">Rules are tried from 1 down. The first one that is true decides the look.</p>
              <StateRules {...ctx} />
              <p className="hint resting-note">
                <Swatch visual={control.visual} /> If none is true, the pad shows its resting look.
              </p>
            </div>
          )}
          {indexesOf(feedback, "event").length > 0 && (
            <div className="rule-block">
              <h4>Reacts to an event</h4>
              <p className="hint">The first rule that matches an event shows its look over the state rules for a while.</p>
              <EventRules {...ctx} />
            </div>
          )}
          {!flow && (
            <div>
              <button type="button" className="btn" data-part="add-feedback" disabled={full} onClick={() => setFlow({ kind: "state" })}>
                + Add feedback
              </button>
              {full && <p className="hint">A pad can have at most {limits?.maxRules} rules.</p>}
            </div>
          )}
        </>
      )}

      {flow && (
        <ObservableFlow
          catalog={catalog}
          title="Add feedback"
          kind={flow.kind}
          from={flow.from}
          doneLabel="Add the first rule"
          onDone={addFirst}
          onCancel={() => setFlow(null)}
          surfaces={surfaces}
          currentSurface={currentSurface}
        />
      )}

      {suggestions.length > 0 && !flow && (
        <div className="suggestions" role="group" aria-label="Suggested feedback">
          <h4>Suggested</h4>
          <p className="hint">What the actions on this pad usually show. Nothing is added until you choose it.</p>
          <ul>
            {suggestions.map((o) => (
              <li key={o.id}>
                <span>
                  <b>{o.name}</b> <span className="hint">· {o.providerName}</span>
                </span>
                <button type="button" className="btn small" disabled={full} aria-label={`Add feedback from ${o.name}`} onClick={() => startSuggestion(o)}>
                  Add
                </button>
              </li>
            ))}
          </ul>
        </div>
      )}

      <LooksEditor control={control} limits={limits} onChange={(next) => ctx.update(next)} say={setMessage} />
    </section>
  );
}
