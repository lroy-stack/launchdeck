// State rules: "when this state is …, show this look", in the order the runtime
// tries them. The first rule that is true decides, so the order is shown as
// numbers and changed only by the person, with buttons or Alt+↑ / Alt+↓.

import { useId, useState, type KeyboardEvent } from "react";
import { readInstance } from "../../lib/observables";
import {
  addRulesForValues,
  choicesOf,
  createAndShow,
  defaultTest,
  insertRule,
  isNumericOp,
  lookKey,
  lookLabel,
  moveRule,
  newStateRule,
  observableOf,
  OPERATOR_LABEL,
  operatorsFor,
  removeRule,
  replaceRule,
  retargetRule,
  ruleNumber,
  showLook,
  stateRuns,
  takesValue,
  testSentence,
  unusedValues,
  valueLabel,
  withOperator,
  withTest,
  withTestValue,
  type Run,
  type TestOp,
} from "../../lib/feedbackEdit";
import type { FeedbackRule, Observable, Scalar, StateRule, StateTest, ValueSpec } from "../../lib/schema";
import { ObservableFlow } from "./ObservablePicker";
import { LookSelect } from "./LookSelect";
import { Messages, NumberField, RuleTools, StatusTag, ruleSel, type EditorContext } from "./parts";

/** Keys on a rule's card: Alt+↑ and Alt+↓ move the rule and the cursor goes with it. */
export function altArrows(move: (delta: -1 | 1, part: string) => void) {
  return (e: KeyboardEvent<HTMLElement>) => {
    if (!e.altKey || e.ctrlKey || e.metaKey || (e.key !== "ArrowUp" && e.key !== "ArrowDown")) return;
    e.preventDefault();
    const part = (e.target as HTMLElement).closest<HTMLElement>("[data-part]")?.dataset.part ?? "card";
    move(e.key === "ArrowUp" ? -1 : 1, part);
  };
}

export function StateRules(ctx: EditorContext) {
  const runs = stateRuns(ctx.control.feedback);
  if (runs.length === 0) return null;
  return (
    <div className="rule-runs">
      {runs.map((run) => (
        <RunBlock key={`${run.key}#${run.indexes[0]}`} run={run} ctx={ctx} />
      ))}
    </div>
  );
}

function RunBlock({ run, ctx }: { run: Run; ctx: EditorContext }) {
  const { control, catalog } = ctx;
  const first = control.feedback[run.indexes[0]!] as StateRule;
  const entry = observableOf(catalog, first);
  const spec = entry?.value;
  const [editing, setEditing] = useState<"state" | "scope" | null>(null);
  const headId = useId();

  const reading = entry ? readInstance(entry, run.address.args ?? {}) : null;
  const scope = Object.entries(run.address.args ?? {})
    .map(([k, v]) => `${k} ${String(v)}`)
    .join(", ");
  const last = run.indexes[run.indexes.length - 1]!;
  const stopped = ctx.notes.find((n) => n.kind === "providerUnavailable" && run.indexes.includes(n.rule));
  const scopeProblems = ctx.problems.filter((p) => run.indexes.includes(p.rule) && p.field === "args");
  const stateProblems = ctx.problems.filter((p) => run.indexes.includes(p.rule) && p.field === "state");
  const dedupe = (items: typeof scopeProblems) => items.filter((p, i) => items.findIndex((q) => q.message === p.message) === i);

  const title = entry ? entry.name : "A state that is no longer offered";
  const provider = entry?.providerName ?? run.address.provider;

  const used = run.indexes.flatMap((i) => {
    const t = (control.feedback[i] as StateRule).when.is;
    return t.op === "equals" ? [t.value] : [];
  });
  const others = entry ? unusedValues(control.feedback, run.address, spec) : [];
  const hasOp = (op: TestOp) => run.indexes.some((i) => (control.feedback[i] as StateRule).when.is.op === op);

  const add = (test: StateTest) => {
    const at = insertRule(control.feedback, newStateRule(run.address, test), last);
    ctx.update({ ...control, feedback: at.feedback }, ruleSel(at.index, "condition"));
  };

  const replaceState = (to: Observable, args: Record<string, Scalar>) => {
    let reset = 0;
    const feedback = control.feedback.map((r, i) => {
      if (!run.indexes.includes(i)) return r;
      const moved = retargetRule(r as StateRule, to, args);
      if (moved.testReset) reset += 1;
      return moved.rule;
    });
    setEditing(null);
    ctx.update({ ...control, feedback }, ruleSel(run.indexes[0]!, "condition"));
    ctx.say(reset > 0 ? `Now following “${to.name}”. ${reset === 1 ? "One condition" : `${reset} conditions`} did not fit it and ${reset === 1 ? "was" : "were"} reset: check ${reset === 1 ? "it" : "them"}.` : `Now following “${to.name}”.`);
  };

  return (
    <section className="rule-run" aria-labelledby={headId}>
      <header className="run-head">
        <div className="run-title">
          <h5 id={headId}>{title}</h5>
          {reading && <StatusTag status={reading.status} />}
        </div>
        <p className="hint">
          {provider}
          {scope && ` · ${scope}`}
          {reading?.status === "known" && ` · now ${valueLabel(spec, reading.value)}`}
        </p>
        {!entry && (
          <p className="rule-note warn" role="note">
            No installed provider offers “{run.address.provider} · {run.address.id}” any more. The rules are kept as they are. Choose another state to replace it, or remove them.
          </p>
        )}
        {entry && stopped && stopped.kind === "providerUnavailable" && (
          <p className="rule-note" role="note">
            {stopped.providerName} is not running: {stopped.reason}
          </p>
        )}
        <Messages id={`${headId}-state`} items={dedupe(stateProblems)} />
        <Messages id={`${headId}-scope`} items={dedupe(scopeProblems)} />
        <div className="run-buttons">
          <button type="button" className="btn small" onClick={() => setEditing(editing === "state" ? null : "state")} aria-expanded={editing === "state"}>
            {entry ? "Change state…" : "Replace state…"}
          </button>
          {entry && entry.params.length > 0 && (
            <button type="button" className="btn small" onClick={() => setEditing(editing === "scope" ? null : "scope")} aria-expanded={editing === "scope"}>
              Change {entry.params.map((p) => p.label.toLowerCase()).join(" and ")}…
            </button>
          )}
          {!entry && (
            <button
              type="button"
              className="btn small danger"
              onClick={() => {
                const gone = new Set(run.indexes);
                ctx.update({ ...control, feedback: control.feedback.filter((_, i) => !gone.has(i)) }, '[data-part="add-feedback"]');
                ctx.say(`Removed ${run.indexes.length === 1 ? "the rule" : `${run.indexes.length} rules`} for a state that is no longer offered.`);
              }}
            >
              Remove these rules
            </button>
          )}
        </div>
      </header>

      {editing === "state" && (
        <ObservableFlow catalog={catalog} title="Change the state" kind="state" locked doneLabel="Follow this one" onDone={replaceState} onCancel={() => setEditing(null)} surfaces={ctx.surfaces} currentSurface={ctx.currentSurface} />
      )}
      {editing === "scope" && entry && (
        <ObservableFlow
          catalog={catalog}
          title={`Change ${entry.params.map((p) => p.label.toLowerCase()).join(" and ")}`}
          kind="state"
          locked
          from={{ observable: entry, args: run.address.args ?? {} }}
          doneLabel="Use this"
          onDone={replaceState}
          onCancel={() => setEditing(null)}
          surfaces={ctx.surfaces}
          currentSurface={ctx.currentSurface}
        />
      )}

      <ol className="rules" aria-label={`Rules for ${title}`}>
        {run.indexes.map((i) => (
          <StateRuleCard key={i} index={i} entry={entry} ctx={ctx} />
        ))}
      </ol>

      <div className="run-buttons">
        <button type="button" className="btn small" disabled={ctx.full} onClick={() => add(defaultTest(spec, used))}>
          + Add a rule
        </button>
        {entry && others.length > 0 && (
          <button
            type="button"
            className="btn small"
            disabled={ctx.full}
            onClick={() => {
              const room = ctx.limits ? Math.max(0, ctx.limits.maxRules - control.feedback.length) : others.length;
              const made = addRulesForValues(control, run.address, others.slice(0, room), last);
              ctx.update(made.control, ruleSel(made.added[0] ?? last, "look"));
              ctx.say(`Added ${made.added.length === 1 ? "a rule" : `${made.added.length} rules`}, each with a look of the value's name. Change a look if you like.`);
            }}
          >
            Add a rule for {others.length === 1 ? "the other value" : `each of the ${others.length} other values`}
          </button>
        )}
        {entry && !hasOp("noValue") && (
          <button type="button" className="btn small" disabled={ctx.full} onClick={() => add({ op: "noValue" })}>
            + When it has no value yet
          </button>
        )}
        {entry && !hasOp("unavailable") && (
          <button type="button" className="btn small" disabled={ctx.full} onClick={() => add({ op: "unavailable" })}>
            + When it is not available
          </button>
        )}
      </div>
      {ctx.full && <p className="hint">A pad can have at most {ctx.limits?.maxRules} rules.</p>}
    </section>
  );
}

function StateRuleCard({ index, entry, ctx }: { index: number; entry: Observable | undefined; ctx: EditorContext }) {
  const { control } = ctx;
  const feedback = control.feedback;
  const rule = feedback[index] as StateRule;
  const spec = entry?.value;
  const number = ruleNumber(feedback, index);
  const id = useId();
  const test = rule.when.is;

  const mine = ctx.problems.filter((p) => p.rule === index);
  const when = mine.filter((p) => p.field === "test" || p.field === "value");
  const look = mine.filter((p) => p.field === "look");
  const notes = ctx.notes.filter((n) => n.rule === index);
  const shadow = notes.find((n) => n.kind === "shadowed");
  const noLook = notes.find((n) => n.kind === "missingLook");

  const same = feedback.flatMap((r, i) => ("state" in r.when ? [i] : []));
  const first = same[0] === index;
  const last = same[same.length - 1] === index;

  const set = (r: FeedbackRule, focus?: string) => ctx.update({ ...control, feedback: replaceRule(feedback, index, r) }, focus);

  const move = (delta: -1 | 1, part: string) => {
    const moved = moveRule(feedback, index, delta);
    if (!moved) return ctx.say(delta < 0 ? `Rule ${number} is already the first.` : `Rule ${number} is already the last.`);
    ctx.update({ ...control, feedback: moved.feedback }, ruleSel(moved.index, part));
    ctx.say(`Rule moved ${delta < 0 ? "up" : "down"}: it is now rule ${ruleNumber(moved.feedback, moved.index)} of ${same.length}.`);
  };

  const remove = () => {
    const next = removeRule(feedback, index);
    const stay = same.filter((i) => i !== index).map((i) => (i > index ? i - 1 : i));
    const near = stay.find((i) => i >= index) ?? stay[stay.length - 1];
    ctx.update({ ...control, feedback: next }, near === undefined ? '[data-part="add-feedback"]' : ruleSel(near, "remove"));
    ctx.say(`Rule ${number} removed.`);
  };

  const ops = operatorsFor(spec?.type);
  const hasCurrent = ops.includes(test.op);

  return (
    <li className="rule" data-rule={index} data-kind="state" aria-label={`Rule ${number}`} tabIndex={-1} onKeyDown={altArrows(move)}>
      <div className="rule-head">
        <span className="rule-num" aria-hidden="true">
          {number}
        </span>
        <RuleTools number={number} kind="state" first={first} last={last} onMove={move} onRemove={remove} />
      </div>

      <div className="rule-line">
        <span className="rule-word">When</span>
        {entry ? (
          <>
            <select
              data-part="condition"
              aria-label={`Rule ${number} condition`}
              aria-invalid={when.some((p) => !p.needed) || undefined}
              aria-describedby={when.length ? `${id}-when` : undefined}
              value={test.op}
              onChange={(e) => set(withTest(rule, withOperator(test, e.target.value as TestOp, spec)))}
            >
              {ops.map((op) => (
                <option key={op} value={op}>
                  {OPERATOR_LABEL[op]}
                </option>
              ))}
              {!hasCurrent && <option value={test.op}>{OPERATOR_LABEL[test.op]} (does not fit this state)</option>}
            </select>
            {takesValue(test.op) && "value" in test && (
              <ValueCell
                label={`Rule ${number} value`}
                test={test}
                spec={spec}
                invalid={when.some((p) => p.field === "value" && !p.needed)}
                describedBy={when.length ? `${id}-when` : undefined}
                onChange={(v) => set(withTest(rule, withTestValue(test, v)))}
              />
            )}
          </>
        ) : (
          <span className="static" data-part="condition" tabIndex={0}>
            {testSentence(test)}
          </span>
        )}
      </div>
      <Messages id={`${id}-when`} items={when} />

      <div className="rule-line">
        <span className="rule-word">Show</span>
        <LookSelect
          label={`Rule ${number} look`}
          control={control}
          rule={rule}
          entry={entry}
          maxName={ctx.limits?.maxLookName}
          invalid={look.some((p) => !p.needed)}
          describedBy={look.length ? `${id}-look` : undefined}
          onChange={(target) => ctx.update(showLook(control, index, target), ruleSel(index, "look"))}
          onCreate={(name, visual) => {
            ctx.update(createAndShow(control, index, name, visual), ruleSel(index, "look"));
            ctx.say(`Created the look “${lookLabel(lookKey(name, ctx.limits?.maxLookName))}” and showed it from rule ${number}.`);
          }}
        />
      </div>
      <Messages id={`${id}-look`} items={look} />

      {noLook && noLook.kind === "missingLook" && (
        <p className="rule-note warn" role="note">
          This pad has no look called “{noLook.look}”, so it rests instead. Create it above or choose another.
        </p>
      )}
      {shadow && shadow.kind === "shadowed" && (
        <p className="rule-note warn" role="note">
          Never used: rule {ruleNumber(feedback, shadow.by)} is tried first and is true whenever this one is.
        </p>
      )}
    </li>
  );
}

function ValueCell({ label, test, spec, invalid, describedBy, onChange }: { label: string; test: StateTest & { value: Scalar }; spec: ValueSpec | undefined; invalid: boolean; describedBy?: string; onChange: (v: Scalar) => void }) {
  const value = test.value;
  const listed = choicesOf(spec);
  if (isNumericOp(test.op) || spec?.type === "number") {
    return <NumberField label={label} dataPart="value" value={typeof value === "number" ? value : Number.NaN} invalid={invalid} describedBy={describedBy} onChange={onChange} />;
  }
  if (listed.length > 0) {
    const known = listed.some((o) => o.value === value);
    return (
      <select data-part="value" aria-label={label} aria-invalid={invalid || undefined} aria-describedby={describedBy} value={String(value)} onChange={(e) => onChange(listed.find((o) => String(o.value) === e.target.value)!.value)}>
        {value === "" && (
          <option value="" disabled>
            Choose…
          </option>
        )}
        {!known && value !== "" && <option value={String(value)}>{valueLabel(spec, value)} (not a value of this state)</option>}
        {listed.map((o) => (
          <option key={String(o.value)} value={String(o.value)}>
            {o.label}
          </option>
        ))}
      </select>
    );
  }
  return <input data-part="value" aria-label={label} aria-invalid={invalid || undefined} aria-describedby={describedBy} value={typeof value === "string" ? value : String(value)} onChange={(e) => onChange(e.target.value)} />;
}
