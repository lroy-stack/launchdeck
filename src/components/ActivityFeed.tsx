import { describeEvent, eventTone, formatTime } from "../lib/format";
import type { RuntimeEvent } from "../lib/schema";

/** Newest first. `control.pressed` is noisy, so it can be hidden. */
export function ActivityFeed({ events, limit = 40, empty = "Nothing yet. Press a pad." }: { events: RuntimeEvent[]; limit?: number; empty?: string }) {
  const rows = events.slice(-limit).reverse();
  if (rows.length === 0) return <p className="hint">{empty}</p>;
  return (
    <ol className="feed" aria-live="polite">
      {rows.map((e, i) => (
        <li key={`${e.ts}-${i}`} className={`tone-${eventTone(e)}`}>
          <time>{formatTime(e.ts)}</time>
          <span className="feed-text">{describeEvent(e)}</span>
          <span className="feed-source">{e.source}</span>
        </li>
      ))}
    </ol>
  );
}
