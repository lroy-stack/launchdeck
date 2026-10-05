import { useMemo } from "react";
import { describeEvent, eventNames, eventTone, formatTime } from "../lib/format";
import type { RuntimeEvent } from "../lib/schema";
import { useStore } from "../stores/runtime";

/** Newest first, in names rather than ids. `control.pressed` is noisy, so it can be hidden. */
export function ActivityFeed({ events, limit = 40, empty = "Nothing yet. Press a pad." }: { events: RuntimeEvent[]; limit?: number; empty?: string }) {
  const workspace = useStore((s) => s.workspace);
  const actions = useStore((s) => s.catalog.catalog.actions);
  const names = useMemo(() => eventNames(workspace, actions), [workspace, actions]);
  const rows = events.slice(-limit).reverse();
  if (rows.length === 0) return <p className="hint">{empty}</p>;
  return (
    <ol className="feed" aria-live="polite">
      {rows.map((e, i) => (
        <li key={`${e.ts}-${i}`} className={`tone-${eventTone(e)}`}>
          <time>{formatTime(e.ts)}</time>
          <span className="feed-text">{describeEvent(e, names)}</span>
          <span className="feed-source">{e.source}</span>
        </li>
      ))}
    </ol>
  );
}
