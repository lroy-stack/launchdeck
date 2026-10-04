// Small pieces the feedback editor shares. State is always said in words, never by colour alone.

import { useEffect, useState } from "react";
import { IDLE, type Problem } from "../../lib/feedbackEdit";
import type { Control, FeedbackLimits, ObservableCatalog, ObservedStatus, RuleNote, Surface, Visual } from "../../lib/schema";

const STATUS_WORD: Record<ObservedStatus, string> = { known: "Known", unknown: "Unknown", unavailable: "Unavailable" };

/** `Known`, `Unknown` or `Unavailable`, as Rust reports the state. */
export function StatusTag({ status, title }: { status: ObservedStatus; title?: string }) {
  return (
    <span className={`tag status ${status}`} title={title}>
      {STATUS_WORD[status]}
    </span>
  );
}

/** What a look looks like on the pad, from the control's own looks. The resting look is the control's own colour. */
export function visualOfLook(control: Control, key: string | null): Visual {
  if (key === null || (key === IDLE && !(IDLE in control.states))) return control.visual;
  return control.states[key] ?? control.visual;
}

export function Swatch({ visual, label }: { visual: Visual; label?: string }) {
  return (
    <span
      className={`look-swatch${visual.animation !== "none" ? ` anim-${visual.animation}` : ""}`}
      style={{ background: visual.color, ["--glow" as string]: visual.color }}
      role={label ? "img" : undefined}
      aria-label={label}
      aria-hidden={label ? undefined : true}
    />
  );
}

// ---------------------------------------------------------------------------
// What the pieces of the rule editor share
// ---------------------------------------------------------------------------

/** A CSS selector for where the cursor goes after the draft changes. */
export type Focus = string;

/** The selector of a rule's card, or of one control inside it. */
export const ruleSel = (index: number, part?: string): string => (part ? `[data-rule="${index}"] [data-part="${part}"]` : `[data-rule="${index}"]`);

/** Everything the rule lists need from the editor that holds the draft. */
export interface EditorContext {
  control: Control;
  catalog: ObservableCatalog;
  /** The runtime's numbers; unknown until it has answered once. */
  limits: FeedbackLimits | undefined;
  /** What is missing or wrong, from the local check and from Rust. */
  problems: Problem[];
  /** What Rust can prove about each rule. */
  notes: RuleNote[];
  surfaces: Surface[];
  currentSurface: Surface;
  /** The draft changed; the cursor goes to `focus` once it is drawn. */
  update: (next: Control, focus?: Focus) => void;
  /** A sentence about what just happened, shown and announced. */
  say: (message: string) => void;
  /** More rules cannot be added. */
  full: boolean;
}

/** Messages about one place in a rule: a prompt when something is only missing, an error when it is wrong. */
export function Messages({ id, items }: { id: string; items: Problem[] }) {
  if (items.length === 0) return null;
  return (
    <div id={id} className="rule-messages">
      {items.map((p, i) => (
        <small key={i} className={p.needed ? "needed" : "field-error"}>
          {p.message}
        </small>
      ))}
    </div>
  );
}

/** A number as it is typed. Empty or half-typed text is `NaN` for the draft, which cannot be saved until it is a number. */
export function NumberField({ label, value, onChange, invalid, describedBy, min, max, step, dataPart }: { label: string; value: number; onChange: (n: number) => void; invalid?: boolean; describedBy?: string; min?: number; max?: number; step?: number | "any"; dataPart?: string }) {
  const external = Number.isFinite(value) ? String(value) : "";
  const [text, setText] = useState(external);
  // Follow the value when something else changes it (another rule, a revert), not while it is being typed.
  useEffect(() => {
    setText((t) => (t.trim() !== "" && Number(t) === value ? t : external));
  }, [value, external]);
  return (
    <input
      type="number"
      aria-label={label}
      aria-invalid={invalid || undefined}
      aria-describedby={describedBy}
      data-part={dataPart}
      inputMode="decimal"
      min={min}
      max={max}
      step={step ?? "any"}
      value={text}
      onChange={(e) => {
        setText(e.target.value);
        const t = e.target.value.trim();
        onChange(t !== "" && Number.isFinite(Number(t)) ? Number(t) : Number.NaN);
      }}
    />
  );
}

/** Move up, move down and remove, for one rule. A button with nowhere to go says so and keeps the cursor. */
export function RuleTools({ number, kind, first, last, onMove, onRemove }: { number: number; kind: string; first: boolean; last: boolean; onMove: (delta: -1 | 1, part: string) => void; onRemove: () => void }) {
  const name = kind === "event" ? "event rule" : "rule";
  return (
    <div className="rule-tools">
      <button type="button" className="icon-btn" data-part="up" aria-disabled={first || undefined} aria-label={`Move ${name} ${number} up`} title="Move up (Alt+↑)" onClick={() => onMove(-1, "up")}>
        ↑
      </button>
      <button type="button" className="icon-btn" data-part="down" aria-disabled={last || undefined} aria-label={`Move ${name} ${number} down`} title="Move down (Alt+↓)" onClick={() => onMove(1, "down")}>
        ↓
      </button>
      <button type="button" className="icon-btn" data-part="remove" aria-label={`Remove ${name} ${number}`} title="Remove this rule" onClick={onRemove}>
        ×
      </button>
    </div>
  );
}
