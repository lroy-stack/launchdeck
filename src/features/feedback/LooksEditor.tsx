// The looks of one pad: the named appearances its rules can show. The pad's own
// colour (set above, under Resting look) is the look it rests in; nothing here
// pretends to be a rule that is always true.

import { useId, useState } from "react";
import { IDLE, kindOfRule, lookKey, lookLabel, lookNameProblem, lookUsage, removeLook, renameLook, ruleNumber, setLookVisual } from "../../lib/feedbackEdit";
import type { Animation, Control, FeedbackLimits } from "../../lib/schema";
import { ColorField } from "../editor/ColorField";
import { NewLook } from "./LookSelect";
import { Swatch } from "./parts";

interface Props {
  control: Control;
  limits: FeedbackLimits | undefined;
  onChange: (next: Control) => void;
  say: (message: string) => void;
}

const ANIMATION: Record<Animation, string> = { none: "Solid", pulse: "Pulse", flash: "Flash" };

/** `Rule 1, Event rule 2`: where a look is shown from. */
function usageText(control: Control, key: string): string {
  const used = lookUsage(control, key).map((i) => {
    const rule = control.feedback[i]!;
    const n = ruleNumber(control.feedback, i);
    return kindOfRule(rule) === "event" ? `event rule ${n}` : `rule ${n}`;
  });
  // Two names mean something by themselves: the runtime shows them for the pad's own action.
  if (used.length === 0 && key === "pending") return "Shown while this pad’s action is running";
  if (used.length === 0 && key === "failed") return "Shown for a moment when this pad’s action fails";
  if (used.length === 0) return "Not used by any rule";
  const text = used.join(", ");
  return `Shown by ${text.charAt(0).toUpperCase()}${text.slice(1)}`;
}

export function LooksEditor({ control, limits, onChange, say }: Props) {
  const keys = Object.keys(control.states);
  const [open, setOpen] = useState<string | null>(null);
  const [adding, setAdding] = useState(false);

  return (
    <section className="field-group looks" aria-label="Looks">
      <h4>Looks</h4>
      <p className="hint">
        A look is a colour and an animation this pad can show. Rules choose one by name. Two names work by themselves: a look called <b>Pending</b> shows while
        this pad’s action is running, and one called <b>Failed</b> for a moment when it fails.
      </p>
      <ul className="look-list">
        <li className="look-row">
          <Swatch visual={control.visual} />
          <span className="look-name">
            <b>{lookLabel(IDLE)}</b>
            <span className="hint">The pad’s own colour. Shown when no rule applies. Edit it under Resting look.</span>
          </span>
        </li>
        {keys.map((key) => (
          <LookRow
            key={key}
            control={control}
            name={key}
            limits={limits}
            open={open === key}
            onToggle={() => setOpen(open === key ? null : key)}
            onChange={onChange}
            onRenamed={(to) => setOpen(to)}
            say={say}
          />
        ))}
      </ul>
      {adding ? (
        <NewLook
          control={control}
          maxName={limits?.maxLookName}
          onCancel={() => setAdding(false)}
          onCreate={(name, visual) => {
            const key = lookKey(name, limits?.maxLookName);
            const made = { ...control, states: { ...control.states, [key]: visual } };
            setAdding(false);
            setOpen(key);
            onChange(made);
            say(`Created the look “${lookLabel(key)}”.`);
          }}
        />
      ) : (
        <div>
          <button type="button" className="btn small" onClick={() => setAdding(true)}>
            + New look
          </button>
        </div>
      )}
    </section>
  );
}

function LookRow({ control, name, limits, open, onToggle, onChange, onRenamed, say }: { control: Control; name: string; limits: FeedbackLimits | undefined; open: boolean; onToggle: () => void; onChange: (next: Control) => void; onRenamed: (to: string) => void; say: (message: string) => void }) {
  const look = control.states[name]!;
  const used = lookUsage(control, name);
  const nameId = useId();
  const [text, setText] = useState(lookLabel(name));
  const [confirm, setConfirm] = useState(false);
  const typed = text.trim();
  const problem = typed === "" || lookKey(typed, limits?.maxLookName) === name ? null : lookNameProblem(control, typed, name, limits?.maxLookName);

  const commit = () => {
    if (typed === "" || lookKey(typed, limits?.maxLookName) === name) return setText(lookLabel(name));
    if (problem) return;
    const renamed = renameLook(control, name, typed);
    if (renamed.key === name) return setText(lookLabel(name));
    onChange(renamed.control);
    onRenamed(renamed.key);
    setText(lookLabel(renamed.key));
    say(used.length > 0 ? `Renamed to “${lookLabel(renamed.key)}”. The rules that showed it show the new name.` : `Renamed to “${lookLabel(renamed.key)}”.`);
  };

  return (
    <li className="look-row" data-look={name}>
      <Swatch visual={look} />
      <span className="look-name">
        <b>{lookLabel(name)}</b>
        <span className="hint">
          {ANIMATION[look.animation]} · {usageText(control, name)}
        </span>
      </span>
      <button type="button" className="btn small" aria-expanded={open} aria-label={`${open ? "Close" : "Edit"} the look ${lookLabel(name)}`} onClick={onToggle}>
        {open ? "Done" : "Edit"}
      </button>
      {open && (
        <div className="look-edit">
          <div className="field">
            <label htmlFor={`${nameId}-name`}>
              <span>Name</span>
            </label>
            <input
              id={`${nameId}-name`}
              value={text}
              aria-label={`Name of the look ${lookLabel(name)}`}
              aria-invalid={problem ? true : undefined}
              aria-describedby={problem ? `${nameId}-problem` : undefined}
              onChange={(e) => setText(e.target.value)}
              onBlur={commit}
              onKeyDown={(e) => {
                if (e.key === "Enter") {
                  e.preventDefault();
                  commit();
                } else if (e.key === "Escape") {
                  e.preventDefault();
                  e.stopPropagation();
                  setText(lookLabel(name));
                }
              }}
            />
            {problem && (
              <small id={`${nameId}-problem`} className="field-error">
                {problem}
              </small>
            )}
          </div>
          <ColorField label={`${lookLabel(name)} colour`} value={look.color} onChange={(color) => onChange(setLookVisual(control, name, { color }))} />
          <label className="field">
            <span>Animation</span>
            <select aria-label={`${lookLabel(name)} animation`} value={look.animation} onChange={(e) => onChange(setLookVisual(control, name, { animation: e.target.value as Animation }))}>
              <option value="none">Solid</option>
              <option value="pulse">Pulse</option>
              <option value="flash">Flash</option>
            </select>
          </label>
          {confirm ? (
            <div className="notice" role="alert">
              <p>
                {used.length === 1 ? "One rule shows" : `${used.length} rules show`} this look. If you remove it, {used.length === 1 ? "that rule keeps" : "they keep"} the name and the pad rests until you create it again or choose another look.
              </p>
              <div className="foot-buttons">
                <button
                  type="button"
                  className="btn danger small"
                  onClick={() => {
                    setConfirm(false);
                    onChange(removeLook(control, name));
                    say(`Removed the look “${lookLabel(name)}”.`);
                  }}
                >
                  Remove it anyway
                </button>
                <button type="button" className="btn small" onClick={() => setConfirm(false)}>
                  Keep it
                </button>
              </div>
            </div>
          ) : (
            <div>
              <button
                type="button"
                className="btn danger small"
                aria-label={`Remove the look ${lookLabel(name)}`}
                onClick={() => {
                  if (used.length > 0) return setConfirm(true);
                  onChange(removeLook(control, name));
                  say(`Removed the look “${lookLabel(name)}”.`);
                }}
              >
                Remove look
              </button>
            </div>
          )}
        </div>
      )}
    </li>
  );
}
