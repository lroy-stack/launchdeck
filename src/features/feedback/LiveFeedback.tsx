// Why the pad looks as it does, in words. Every fact on this panel is what Rust
// answered (`explain_control`, or `review_feedback`'s preview for a draft): the
// values, their status and the reason for it, the rule that decided, the look,
// the temporary look an event put on top. Nothing here is worked out again.

import { useMemo } from "react";
import { lookLabel, secondsText, viewExplanation } from "../../lib/feedbackEdit";
import type { Control, ObservableCatalog } from "../../lib/schema";
import { StatusTag, Swatch, visualOfLook } from "./parts";
import type { ReviewState } from "./useReview";

interface Props {
  state: ReviewState;
  control: Control;
  catalog: ObservableCatalog;
  /** The control exists in the workspace (an explanation of the saved pad can be had). */
  saved: boolean;
  /** The draft differs from what is saved. */
  dirty: boolean;
}

const lookText = (key: string | null): string => (key === null ? "Resting look" : lookLabel(key));

export function LiveFeedback({ state, control, catalog, saved, dirty }: Props) {
  const { explanation, live, unavailable } = state;
  const view = useMemo(() => (explanation ? viewExplanation(explanation, control.feedback, catalog) : null), [explanation, control.feedback, catalog]);

  // For a draft that differs from what is saved, "live" would be a lie: the pad on the surface still has its saved rules.
  const draft = !!view && !live;
  const heading = draft ? "Feedback if you save now" : "Live feedback";
  const note = draft && (dirty || !saved) ? "This is how the pad would look with the values it reads now. Temporary looks from events are not part of it." : null;

  return (
    <section className="live-feedback" aria-label={heading} aria-busy={!view && !unavailable ? true : undefined}>
      <header className="live-head">
        <h5>{heading}</h5>
        {live && <span className="tag status known">Live</span>}
        {draft && <span className="tag unsaved">Draft</span>}
      </header>

      {!view && unavailable && (
        <p className="hint" role="status">
          LaunchDeck could not explain this pad right now. {unavailable}
        </p>
      )}
      {!view && !unavailable && (
        <p className="hint" role="status">
          Asking LaunchDeck…
        </p>
      )}

      {view && (
        <div role="status" aria-live="polite">
          {note && <p className="hint">{note}</p>}
          <dl className="live-list">
            {view.readings.length === 0 && (
              <>
                <dt>Reads</dt>
                <dd>No state: only events decide this pad.</dd>
              </>
            )}
            {view.readings.map((r) => (
              <div key={r.address + r.scope} className="live-reading">
                <dt>
                  {r.title}
                  {r.scope && <span className="hint"> · {r.scope}</span>}
                </dt>
                <dd>
                  <StatusTag status={r.status} /> {r.status === "known" ? <b>{r.value}</b> : <span>{r.reason}</span>}
                </dd>
              </div>
            ))}

            <dt>Rule that decided</dt>
            <dd>{view.matched ? `Rule ${view.matched.number}: it ${view.matched.text}` : view.readings.length > 0 ? "None is true, so the pad rests." : "None"}</dd>

            <dt>Look from the state rules</dt>
            <dd>
              <span className="look-chip">
                <Swatch visual={visualOfLook(control, view.derived)} />
                {lookText(view.derived)}
              </span>
            </dd>

            {view.overlay && (
              <>
                <dt>Temporary look</dt>
                <dd>
                  <span className="look-chip">
                    <Swatch visual={visualOfLook(control, view.overlay.look)} />
                    {lookText(view.overlay.look)}
                  </span>{" "}
                  <span className="hint">
                    {view.overlay.source === "action" ? "from the action that just ran" : view.overlay.rule ? `from event rule ${view.overlay.rule}` : "from an event"}
                    {view.overlay.latched ? ", until another event" : view.overlay.remainingMs != null ? `, ${secondsText(Math.max(0, Math.round(view.overlay.remainingMs / 100) * 100))} left` : ""}
                  </span>
                </dd>
              </>
            )}

            <dt>On the pad</dt>
            <dd>
              <span className="look-chip">
                <Swatch visual={visualOfLook(control, view.shown)} />
                {lookText(view.shown)}
              </span>
              {view.layer === "overlay" && <span className="hint"> · the temporary look is over the state rules</span>}
            </dd>
          </dl>

          {view.missing.map((m) => (
            <p key={`${m.rule}:${m.look}`} className="rule-note warn" role="note">
              {m.label} chose the look “{m.look}”, which this pad does not have, so the pad rests.
            </p>
          ))}

          {view.readings.length > 0 && (
            <details className="live-details">
              <summary>Technical details</summary>
              <ul>
                {view.readings.map((r) => (
                  <li key={r.address + r.scope}>
                    <code>{r.address}</code>
                  </li>
                ))}
              </ul>
            </details>
          )}
        </div>
      )}
    </section>
  );
}
