// Asking Rust about the draft of a control's feedback.
//
// Two answers, both from the runtime:
//   * `review_feedback`: what saving would refuse (issues), what Rust can prove
//     about each rule (notes), the limits, and, when the draft differs from what
//     is saved, how the draft would look right now (preview);
//   * `explain_control`: for a draft that is the saved control, the explanation
//     that also knows the temporary look an event put on the pad.
//
// Nothing is derived here. The questions are asked again only when the draft
// changes or when a value, availability or definition that one of its rules
// reads changes, never on every unrelated update of the catalog.

import { useEffect, useState } from "react";
import { api, toProblem, type Problem } from "../../lib/ipc";
import { isSendable, readingsKey } from "../../lib/feedbackEdit";
import type { Control, ControlExplanation, FeedbackReview } from "../../lib/schema";
import { controlState, useStore } from "../../stores/runtime";

export interface ReviewState {
  /** Rust's judgement of the draft; `null` until the first answer, or when there is nothing to judge. */
  review: FeedbackReview | null;
  /** Why the pad looks as it does. Of the saved control when `live`, of the draft otherwise. */
  explanation: ControlExplanation | null;
  /** The explanation is of the saved control, so it includes the temporary look. */
  live: boolean;
  /** Why no explanation could be had (a browser preview has no runtime). */
  unavailable: Problem | null;
}

const NOTHING: ReviewState = { review: null, explanation: null, live: false, unavailable: null };
const DEBOUNCE_MS = 150;

export function useFeedbackReview(surfaceId: string, pageId: string, draft: Control, saved: Control | null): ReviewState {
  const catalog = useStore((s) => s.observables);
  // The look on the pad changes when an event starts or ends: that is also a reason to ask again.
  const shown = useStore((s) => controlState(s, draft.id));
  const [state, setState] = useState<ReviewState>(NOTHING);

  const draftKey = JSON.stringify([draft.feedback, draft.states]);
  const savedKey = JSON.stringify(saved ? [saved.feedback, saved.states] : null);
  const readings = readingsKey(catalog, draft.feedback);
  const anything = draft.feedback.length > 0 || (saved?.feedback.length ?? 0) > 0;

  useEffect(() => {
    if (!anything) {
      setState(NOTHING);
      return;
    }
    // A number still being typed is not a number yet: keep the last answer until it is.
    if (!isSendable(draft.feedback)) return;
    let current = true;
    const timer = setTimeout(() => {
      void (async () => {
        try {
          const review = await api.reviewFeedback(surfaceId, pageId, draft);
          let explanation: ControlExplanation | null = review.preview ?? null;
          let live = false;
          let unavailable: Problem | null = null;
          if (!explanation && saved) {
            try {
              explanation = await api.explainControl(surfaceId, pageId, draft.id);
              live = true;
            } catch (e) {
              unavailable = toProblem(e);
            }
          }
          if (current) setState({ review, explanation, live, unavailable });
        } catch (e) {
          if (current) setState({ review: null, explanation: null, live: false, unavailable: toProblem(e) });
        }
      })();
    }, DEBOUNCE_MS);
    return () => {
      current = false;
      clearTimeout(timer);
    };
    // `draft` and `saved` are covered by their keys: a new object with the same content asks nothing.
  }, [surfaceId, pageId, draft.id, draftKey, savedKey, readings, shown, anything]); // eslint-disable-line react-hooks/exhaustive-deps

  return state;
}
