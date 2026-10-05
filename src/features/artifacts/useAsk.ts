// Asking Rust again while the person is still choosing: the import plan with
// new decisions, the export preview with a new request. A question can wait
// for a pause in typing, and only the answer to the last question asked is
// taken; a slower answer to an earlier one never replaces it.

import { useCallback, useEffect, useRef, useState } from "react";
import { useStore } from "../../stores/runtime";

/** How long a typed change waits for the next keystroke before it is sent. */
export const TYPING_PAUSE_MS = 150;

export function useAsk<Q, A>(ask: (question: Q) => Promise<A>, take: (answer: A) => void) {
  const attempt = useStore((s) => s.attempt);
  const [pending, setPending] = useState(false);
  const latest = useRef(0);
  const timer = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);
  const handlers = useRef({ ask, take });

  useEffect(() => {
    handlers.current = { ask, take };
  });
  useEffect(() => () => clearTimeout(timer.current), []);

  /** Ask `question` after `wait` ms, unless another question is asked first. */
  const send = useCallback(
    (question: Q, wait = 0) => {
      clearTimeout(timer.current);
      const asked = ++latest.current;
      setPending(true);
      timer.current = setTimeout(() => {
        void attempt(async () => {
          try {
            const answer = await handlers.current.ask(question);
            if (asked === latest.current) handlers.current.take(answer);
          } finally {
            if (asked === latest.current) setPending(false);
          }
        });
      }, wait);
    },
    [attempt],
  );

  /** Forget the question waiting or on its way: an answer from elsewhere replaced it. */
  const drop = useCallback(() => {
    clearTimeout(timer.current);
    latest.current++;
    setPending(false);
  }, []);

  return { send, drop, pending };
}
