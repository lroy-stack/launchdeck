// Choosing the look a rule shows, and making a new one without leaving the rule.

import { useId, useState } from "react";
import { guessLookColor, hasValueWhenTrue, lookKey, lookLabel, lookNameProblem, lookTargetOf, IDLE, type LookTarget } from "../../lib/feedbackEdit";
import type { Animation, Control, FeedbackRule, Observable, Visual } from "../../lib/schema";
import { isStateRule } from "../../lib/schema";
import { ColorField } from "../../components/forms/ColorField";
import { Swatch, visualOfLook } from "./parts";

const NEW = "__new__";
const NONE = "";

const encode = (t: LookTarget): string => {
  switch (t.kind) {
    case "none":
      return NONE;
    case "look":
      return `look:${t.name}`;
    case "value":
      return "value";
    case "field":
      return `field:${t.field}`;
  }
};

const decode = (v: string): LookTarget => {
  if (v === NONE) return { kind: "none" };
  if (v === "value") return { kind: "value" };
  if (v.startsWith("field:")) return { kind: "field", field: v.slice(6) };
  return { kind: "look", name: v.slice(5) };
};

interface Props {
  /** The accessible name, e.g. `Rule 2 look`. */
  label: string;
  control: Control;
  rule: FeedbackRule;
  /** What the rule reads, when the catalog still lists it: it decides which "named after" choices make sense. */
  entry: Observable | undefined;
  maxName?: number;
  onChange: (target: LookTarget) => void;
  /** Make a look and show it from this rule: one step for the draft. */
  onCreate: (name: string, visual: Visual) => void;
  /** Ids of the messages that describe this field. */
  describedBy?: string;
  invalid?: boolean;
}

/** The look a rule shows: one of the pad's looks, the resting look, one named after the value, or a new one. */
export function LookSelect({ label, control, rule, entry, maxName, onChange, onCreate, describedBy, invalid }: Props) {
  const [creating, setCreating] = useState(false);
  const target = lookTargetOf(rule);
  const current = encode(target);

  const looks = Object.keys(control.states).filter((k) => k !== IDLE);
  const state = isStateRule(rule);
  const namedByValue = state && (target.kind === "value" || (hasValueWhenTrue(rule.when.is.op) && (entry?.value?.type === "enum" || entry?.value?.type === "string")));
  const fields = state ? [] : (entry?.fields ?? []).filter((f) => f.type === "enum" || f.type === "string");
  const missing = target.kind === "look" && target.name !== IDLE && !(target.name in control.states);
  const staleField = target.kind === "field" && !fields.some((f) => f.name === target.field);

  const shown = target.kind === "look" ? visualOfLook(control, target.name) : null;

  return (
    <div className="look-select">
      <div className="look-pick">
        {shown && <Swatch visual={shown} />}
        <select
          aria-label={label}
          data-part="look"
          aria-invalid={invalid || undefined}
          aria-describedby={describedBy}
          value={current}
          onChange={(e) => {
            if (e.target.value === NEW) return setCreating(true);
            setCreating(false);
            onChange(decode(e.target.value));
          }}
        >
          {target.kind === "none" && (
            <option value={NONE} disabled>
              Choose a look…
            </option>
          )}
          <option value={`look:${IDLE}`}>{lookLabel(IDLE)}</option>
          {looks.map((k) => (
            <option key={k} value={`look:${k}`}>
              {lookLabel(k)}
            </option>
          ))}
          {missing && <option value={current}>{lookLabel(target.kind === "look" ? target.name : "")} (this pad has no such look)</option>}
          {namedByValue && <option value="value">The look named after the value</option>}
          {fields.map((f) => (
            <option key={f.name} value={`field:${f.name}`}>
              The look named in “{f.label}”
            </option>
          ))}
          {staleField && target.kind === "field" && <option value={current}>The look named in “{target.field}” (not a field of this event)</option>}
          <option value={NEW}>+ Create a new look…</option>
        </select>
      </div>
      {creating && (
        <NewLook
          control={control}
          maxName={maxName}
          onCancel={() => setCreating(false)}
          onCreate={(name, visual) => {
            setCreating(false);
            onCreate(name, visual);
          }}
        />
      )}
    </div>
  );
}

export function NewLook({ control, maxName, onCreate, onCancel }: { control: Control; maxName?: number; onCreate: (name: string, visual: Visual) => void; onCancel: () => void }) {
  const id = useId();
  const [name, setName] = useState("");
  const [color, setColor] = useState<string | null>(null);
  const [animation, setAnimation] = useState<Animation>("none");
  const problem = name === "" ? null : lookNameProblem(control, name, undefined, maxName);
  const ready = name.trim() !== "" && !lookNameProblem(control, name, undefined, maxName);
  const chosen = color ?? guessLookColor(name);

  const submit = () => {
    if (ready) onCreate(lookKey(name, maxName), { color: chosen, animation });
  };

  return (
    <div className="new-look" role="group" aria-label="New look">
      <div className="field">
        <label htmlFor={`${id}-name`}>
          <span>Name of the new look</span>
        </label>
        <input
          id={`${id}-name`}
          autoFocus
          value={name}
          maxLength={maxName ? maxName + 10 : undefined}
          aria-invalid={problem ? true : undefined}
          aria-describedby={problem ? `${id}-problem` : undefined}
          placeholder="e.g. Warning"
          onChange={(e) => setName(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter") {
              e.preventDefault();
              submit();
            } else if (e.key === "Escape") {
              e.preventDefault();
              e.stopPropagation();
              onCancel();
            }
          }}
        />
        {problem && (
          <small id={`${id}-problem`} className="field-error">
            {problem}
          </small>
        )}
      </div>
      <ColorField label="New look colour" value={chosen} onChange={setColor} />
      <label className="field">
        <span>Animation</span>
        <select aria-label="New look animation" value={animation} onChange={(e) => setAnimation(e.target.value as Animation)}>
          <option value="none">Solid</option>
          <option value="pulse">Pulse</option>
          <option value="flash">Flash</option>
        </select>
      </label>
      <div className="foot-buttons">
        <button type="button" className="btn primary small" disabled={!ready} onClick={submit}>
          Create look
        </button>
        <button type="button" className="btn small" onClick={onCancel}>
          Cancel
        </button>
      </div>
    </div>
  );
}
