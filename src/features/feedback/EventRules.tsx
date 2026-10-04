// Event rules: "when this happens, show this look for a while". An event passes;
// the look it puts on the pad sits over what the state rules say and then goes.

import { useId, useState } from "react";
import {
  createAndShow,
  defaultConditionValue,
  fieldChoices,
  holdOf,
  indexesOf,
  lookKey,
  lookLabel,
  moveRule,
  observableOf,
  removeRule,
  replaceRule,
  retargetEvent,
  ruleNumber,
  secondsText,
  showLook,
  withCondition,
  withHold,
  withoutCondition,
} from "../../lib/feedbackEdit";
import type { EventField, EventRule, Observable, Scalar } from "../../lib/schema";
import { LookSelect } from "./LookSelect";
import { ObservableFlow } from "./ObservablePicker";
import { Messages, NumberField, RuleTools, StatusTag, ruleSel, type EditorContext } from "./parts";
import { altArrows } from "./StateRules";

export function EventRules(ctx: EditorContext) {
  const indexes = indexesOf(ctx.control.feedback, "event");
  if (indexes.length === 0) return null;
  return (
    <ol className="rules event-rules" aria-label="Event rules">
      {indexes.map((i) => (
        <EventRuleCard key={i} index={i} ctx={ctx} />
      ))}
    </ol>
  );
}

function EventRuleCard({ index, ctx }: { index: number; ctx: EditorContext }) {
  const { control, catalog, limits } = ctx;
  const feedback = control.feedback;
  const rule = feedback[index] as EventRule;
  const entry = observableOf(catalog, rule);
  const number = ruleNumber(feedback, index);
  const id = useId();
  const [changing, setChanging] = useState(false);

  const mine = ctx.problems.filter((p) => p.rule === index);
  const at = (...fields: string[]) => mine.filter((p) => fields.includes(p.field));
  const notes = ctx.notes.filter((n) => n.rule === index);
  const noLook = notes.find((n) => n.kind === "missingLook");
  const stopped = notes.find((n) => n.kind === "providerUnavailable");

  const same = indexesOf(feedback, "event");
  const first = same[0] === index;
  const last = same[same.length - 1] === index;

  const set = (r: EventRule, focus?: string) => ctx.update({ ...control, feedback: replaceRule(feedback, index, r) }, focus);

  const move = (delta: -1 | 1, part: string) => {
    const moved = moveRule(feedback, index, delta);
    if (!moved) return ctx.say(delta < 0 ? `Event rule ${number} is already the first.` : `Event rule ${number} is already the last.`);
    ctx.update({ ...control, feedback: moved.feedback }, ruleSel(moved.index, part));
    ctx.say(`Event rule moved ${delta < 0 ? "up" : "down"}: it is now event rule ${ruleNumber(moved.feedback, moved.index)} of ${same.length}.`);
  };

  const remove = () => {
    const next = removeRule(feedback, index);
    const stay = same.filter((i) => i !== index).map((i) => (i > index ? i - 1 : i));
    const near = stay.find((i) => i >= index) ?? stay[stay.length - 1];
    ctx.update({ ...control, feedback: next }, near === undefined ? '[data-part="add-feedback"]' : ruleSel(near, "remove"));
    ctx.say(`Event rule ${number} removed.`);
  };

  const replaceEvent = (to: Observable) => {
    const moved = retargetEvent(rule, to);
    setChanging(false);
    set(moved.rule, ruleSel(index, "change"));
    ctx.say(moved.dropped ? `Now listening to “${to.name}”. Conditions or a look from fields it does not have were removed: check them.` : `Now listening to “${to.name}”.`);
  };

  const fields = entry?.fields ?? [];
  const usable = fields.filter((f) => !rule.when.where.some((w) => w.field === f.name));
  const room = !limits || rule.when.where.length < limits.maxWhere;
  const hold = holdOf(rule, limits);

  return (
    <li className="rule event" data-rule={index} data-kind="event" aria-label={`Event rule ${number}`} tabIndex={-1} onKeyDown={altArrows(move)}>
      <div className="rule-head">
        <span className="rule-num" aria-hidden="true">
          {number}
        </span>
        <div className="rule-title">
          <b>{entry ? entry.name : "An event that is no longer offered"}</b>
          <span className="hint">{entry?.providerName ?? rule.when.event.provider}</span>
          {entry && entry.availability.status === "unavailable" && <StatusTag status="unavailable" title={entry.availability.reason} />}
        </div>
        <RuleTools number={number} kind="event" first={first} last={last} onMove={move} onRemove={remove} />
      </div>

      {!entry && (
        <p className="rule-note warn" role="note">
          No installed provider offers “{rule.when.event.provider} · {rule.when.event.id}” any more. The rule is kept as it is. Choose another event to replace it, or remove it.
        </p>
      )}
      {entry && stopped && stopped.kind === "providerUnavailable" && (
        <p className="rule-note" role="note">
          {stopped.providerName} is not running: {stopped.reason}
        </p>
      )}
      <Messages id={`${id}-event`} items={at("event")} />
      <div className="run-buttons">
        <button type="button" className="btn small" data-part="change" aria-expanded={changing} onClick={() => setChanging(!changing)}>
          {entry ? "Change event…" : "Replace event…"}
        </button>
      </div>
      {changing && <ObservableFlow catalog={catalog} title="Change the event" kind="event" locked doneLabel="Listen to this" onDone={replaceEvent} onCancel={() => setChanging(false)} surfaces={ctx.surfaces} currentSurface={ctx.currentSurface} />}

      {(rule.when.where.length > 0 || (entry && usable.length > 0)) && (
        <div className="rule-conditions">
          <span className="rule-word">Only if</span>
          {rule.when.where.map((w) => {
            const field = fields.find((f) => f.name === w.field);
            return (
              <Condition
                key={w.field}
                label={`Event rule ${number} condition`}
                field={field}
                name={w.field}
                value={w.equals}
                invalid={at("where").some((p) => !p.needed)}
                onChange={(v) => set(withCondition(rule, w.field, v))}
                onRemove={() => set(withoutCondition(rule, w.field), ruleSel(index, "add-condition"))}
              />
            );
          })}
          <Messages id={`${id}-where`} items={at("where")} />
          {entry && usable.length > 0 && (
            <select
              data-part="add-condition"
              aria-label={`Event rule ${number}: add a condition`}
              value=""
              disabled={!room}
              onChange={(e) => {
                const field = usable.find((f) => f.name === e.target.value);
                if (field) set(withCondition(rule, field.name, defaultConditionValue(field)));
              }}
            >
              <option value="">+ Add a condition on…</option>
              {usable.map((f) => (
                <option key={f.name} value={f.name}>
                  {f.label}
                </option>
              ))}
            </select>
          )}
          {!room && <p className="hint">At most {limits?.maxWhere} conditions.</p>}
        </div>
      )}

      <div className="rule-line">
        <span className="rule-word">Show</span>
        <LookSelect
          label={`Event rule ${number} look`}
          control={control}
          rule={rule}
          entry={entry}
          maxName={limits?.maxLookName}
          invalid={at("look").some((p) => !p.needed)}
          describedBy={at("look").length ? `${id}-look` : undefined}
          onChange={(target) => ctx.update(showLook(control, index, target), ruleSel(index, "look"))}
          onCreate={(name, visual) => {
            ctx.update(createAndShow(control, index, name, visual), ruleSel(index, "look"));
            ctx.say(`Created the look “${lookLabel(lookKey(name, limits?.maxLookName))}” and showed it from event rule ${number}.`);
          }}
        />
      </div>
      <Messages id={`${id}-look`} items={at("look")} />
      {noLook && noLook.kind === "missingLook" && (
        <p className="rule-note warn" role="note">
          This pad has no look called “{noLook.look}”, so the event will show nothing. Create it above or choose another.
        </p>
      )}

      <div className="rule-line">
        <span className="rule-word">Keep</span>
        <select
          data-part="hold"
          aria-label={`Event rule ${number} duration`}
          value={hold.kind}
          onChange={(e) => set(withHold(rule, e.target.value === "latch" ? { kind: "latch" } : { kind: "for", ms: limits?.defaultHoldMs ?? 3000 }))}
        >
          <option value="for">For a time</option>
          <option value="latch">Until another event</option>
        </select>
        {hold.kind === "for" && (
          <>
            <NumberField
              label={`Event rule ${number} seconds`}
              dataPart="seconds"
              value={hold.ms / 1000}
              min={limits ? limits.minHoldMs / 1000 : undefined}
              max={limits ? limits.maxHoldMs / 1000 : undefined}
              invalid={at("hold").some((p) => !p.needed)}
              describedBy={`${id}-hold`}
              onChange={(s) => set(withHold(rule, { kind: "for", ms: s * 1000 }))}
            />
            <span className="static">seconds</span>
          </>
        )}
      </div>
      <Messages id={`${id}-hold`} items={at("hold")} />
      <p className="hint">
        {hold.kind === "latch" ? "The look stays until another event replaces it, or a rule showing the resting look clears it." : `Shown for ${secondsText(hold.ms)}, then the pad goes back to what its state rules say.`}
      </p>
    </li>
  );
}

function Condition({ label, field, name, value, invalid, onChange, onRemove }: { label: string; field: EventField | undefined; name: string; value: Scalar; invalid: boolean; onChange: (v: Scalar) => void; onRemove: () => void }) {
  const listed = field ? fieldChoices(field) : [];
  const title = field?.label ?? name;
  return (
    <div className="condition">
      <span className="condition-name">{title} is</span>
      {field && listed.length > 0 ? (
        <select aria-label={`${label}: ${title}`} aria-invalid={invalid || undefined} value={String(value)} onChange={(e) => onChange(listed.find((o) => String(o.value) === e.target.value)!.value)}>
          {!listed.some((o) => o.value === value) && <option value={String(value)}>{String(value) || "Choose…"}</option>}
          {listed.map((o) => (
            <option key={String(o.value)} value={String(o.value)}>
              {o.label}
            </option>
          ))}
        </select>
      ) : field?.type === "number" ? (
        <NumberField label={`${label}: ${title}`} value={typeof value === "number" ? value : Number.NaN} invalid={invalid} onChange={onChange} />
      ) : (
        <input aria-label={`${label}: ${title}`} aria-invalid={invalid || undefined} value={String(value)} onChange={(e) => onChange(e.target.value)} />
      )}
      <button type="button" className="icon-btn" aria-label={`Remove the condition on ${title}`} onClick={onRemove}>
        ×
      </button>
    </div>
  );
}
