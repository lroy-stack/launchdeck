// The editing model of a control's feedback. Pure functions, unit tested.
//
// Rust owns what a rule means: which observables exist, what a test accepts,
// which rule wins, what a draft would show. This module only edits the draft
// the Inspector already holds (a `Control`), offers the choices the catalog
// allows, and says what is still missing from a rule before it can be saved.
// It never evaluates a rule and never decides a look.

import { fold, isMissingIssue, normalizeArgs, tokensOf, withinOneEdit, type Args } from "./actions";
import { findObservable, readInstance } from "./observables";
import type {
  ActionDefinition,
  Control,
  ControlExplanation,
  EventField,
  EventRule,
  FeedbackLimits,
  FeedbackRule,
  IssueField,
  Observable,
  ObservableCatalog,
  ObservableKind,
  ObservedStatus,
  RuleIssue,
  Scalar,
  StateAddress,
  StateRule,
  StateTest,
  ValueOption,
  ValueSpec,
  ValueType,
  Visual,
} from "./schema";
import { isStateRule } from "./schema";

/** The look name that always means "the resting look" unless the control draws one called that. */
export const IDLE = "idle";

// ---------------------------------------------------------------------------
// Finding an observable
// ---------------------------------------------------------------------------

interface Indexed {
  o: Observable;
  name: string;
  words: string[];
  keywords: string[];
  category: string;
  provider: string;
  description: string;
  values: string[];
  id: string;
}

const indexes = new WeakMap<readonly Observable[], Indexed[]>();

function indexOf(catalog: ObservableCatalog): Indexed[] {
  let found = indexes.get(catalog.observables);
  if (!found) {
    found = catalog.observables.map((o): Indexed => {
      const name = fold(o.name);
      return {
        o,
        name,
        words: name.split(/[^a-z0-9]+/).filter(Boolean),
        keywords: o.keywords.map(fold),
        category: fold(o.category ?? ""),
        provider: fold(o.providerName),
        description: fold(o.description),
        values: [...(o.value?.values ?? []).map((v) => fold(v.label)), ...o.fields.map((f) => fold(f.label))],
        id: fold(o.id),
      };
    });
    indexes.set(catalog.observables, found);
  }
  return found;
}

function scoreToken(e: Indexed, t: string): number {
  let best = 0;
  const at = e.name.indexOf(t);
  if (at === 0) best = 14;
  else if (at > 0) best = e.words.some((w) => w.startsWith(t)) ? 11 : 6;
  if (e.keywords.includes(t)) best = Math.max(best, 9);
  else if (e.keywords.some((k) => k.startsWith(t))) best = Math.max(best, 7);
  else if (e.keywords.some((k) => k.includes(t))) best = Math.max(best, 3);
  if (e.provider.includes(t)) best = Math.max(best, 5);
  if (e.category.includes(t)) best = Math.max(best, 4);
  if (e.values.some((v) => v.includes(t))) best = Math.max(best, 4);
  if (e.description.includes(t)) best = Math.max(best, e.description.split(/[^a-z0-9]+/).some((w) => w.startsWith(t)) ? 2 : 1);
  if (e.id.includes(t)) best = Math.max(best, 1);
  if (best === 0 && t.length >= 4 && [...e.words, ...e.keywords].some((w) => withinOneEdit(w, t))) best = 3;
  return best;
}

/**
 * Observables of one kind that match a search, best first; all of them, in the
 * catalog's order of providers, when the search is empty. Every word has to
 * match somewhere in the name, provider, category, keywords, description or the
 * words of its values. Nothing is dropped for being unavailable.
 */
export function searchObservables(catalog: ObservableCatalog, query: string, kind: ObservableKind): Observable[] {
  const tokens = tokensOf(query);
  const order = new Map(catalog.providers.map((p, i) => [p.id, i] as const));
  const rank = (o: Observable): number => order.get(o.provider) ?? order.size;
  const byName = (a: Observable, b: Observable) => rank(a) - rank(b) || a.name.localeCompare(b.name) || a.id.localeCompare(b.id);
  const ofKind = indexOf(catalog).filter((e) => e.o.kind === kind);
  if (tokens.length === 0) return ofKind.map((e) => e.o).sort(byName);

  const phrase = tokens.join(" ");
  const hits: { o: Observable; score: number }[] = [];
  for (const e of ofKind) {
    let total = 0;
    for (const t of tokens) {
      const s = scoreToken(e, t);
      if (s === 0) {
        total = 0;
        break;
      }
      total += s;
    }
    if (total === 0) continue;
    if (e.name.startsWith(phrase)) total += 6;
    if (e.o.availability.status === "available") total += 2;
    hits.push({ o: e.o, score: total });
  }
  return hits.sort((a, b) => b.score - a.score || byName(a.o, b.o)).map((h) => h.o);
}

export interface ProviderGroup {
  provider: string;
  name: string;
  items: Observable[];
}

/** The list cut into one group per provider, in the order each provider first appears. */
export function groupByProvider(items: Observable[]): ProviderGroup[] {
  const groups: ProviderGroup[] = [];
  const at = new Map<string, ProviderGroup>();
  for (const o of items) {
    let g = at.get(o.provider);
    if (!g) {
      g = { provider: o.provider, name: o.providerName, items: [] };
      at.set(o.provider, g);
      groups.push(g);
    }
    g.items.push(o);
  }
  return groups;
}

const TYPE_WORD: Record<ValueType, string> = { enum: "Choice", boolean: "Yes / No", number: "Number", string: "Text" };

/** What an observable holds, in a few words: its listed values, `Number`, or the fields an event carries. */
export function typeSummary(o: Observable): string {
  if (o.kind === "event") return o.fields.length > 0 ? `Event with ${o.fields.map((f) => f.label).join(", ")}` : "Event";
  const spec = o.value;
  if (!spec) return "State";
  const listed = spec.values.map((v) => v.label);
  if (listed.length > 0) return listed.length > 4 ? `${listed.slice(0, 4).join(", ")}, …` : listed.join(", ");
  return TYPE_WORD[spec.type];
}

/** What a state without scope reads now, for a list: `High`, `No value yet` or `Unavailable`. Scoped states have no single value. */
export function nowText(o: Observable): { status: ObservedStatus; text: string } | null {
  if (o.kind !== "state") return null;
  if (o.availability.status === "unavailable") return { status: "unavailable", text: "Unavailable" };
  if (o.params.length > 0) return o.instanceCount > 0 ? { status: "known", text: `${o.instanceCount === 1 ? "1 value" : `${o.instanceCount} values`} reported` } : { status: "unknown", text: "No value yet" };
  const read = readInstance(o, {});
  return read.status === "known" ? { status: "known", text: valueLabel(o.value, read.value) } : { status: read.status, text: read.status === "unknown" ? "No value yet" : "Unavailable" };
}

/** The observable a rule reads, if the catalog still lists it. */
export function observableOf(catalog: ObservableCatalog, rule: FeedbackRule): Observable | undefined {
  const [provider, id, kind] = isStateRule(rule) ? [rule.when.state.provider, rule.when.state.id, "state" as const] : [rule.when.event.provider, rule.when.event.id, "event" as const];
  const found = findObservable(catalog, `${provider}.${id}`);
  return found && found.kind === kind ? found : undefined;
}

export const addressKey = (a: StateAddress): string => `${a.provider}.${a.id}(${Object.entries(a.args ?? {}).sort(([x], [y]) => x.localeCompare(y)).map(([k, v]) => `${k}=${String(v)}`).join(",")})`;

/** Does any rule of the list read this observable (at any address)? */
export const readsObservable = (feedback: FeedbackRule[], o: Observable): boolean =>
  feedback.some((r) => (isStateRule(r) ? o.kind === "state" && r.when.state.provider === o.provider && `${r.when.state.provider}.${r.when.state.id}` === o.id : o.kind === "event" && `${r.when.event.provider}.${r.when.event.id}` === o.id));

/** The observables the pad's actions usually show, that the pad does not read yet. A hint, never applied. */
export function suggestedObservables(catalog: ObservableCatalog, actions: (ActionDefinition | undefined)[], feedback: FeedbackRule[]): Observable[] {
  const out: Observable[] = [];
  for (const def of actions) {
    for (const id of def?.suggests ?? []) {
      const o = findObservable(catalog, id);
      if (o && !out.includes(o) && !readsObservable(feedback, o)) out.push(o);
    }
  }
  return out;
}

// ---------------------------------------------------------------------------
// Values, in the words of the definition
// ---------------------------------------------------------------------------

export function optionOf(spec: ValueSpec | undefined, value: Scalar | null | undefined): ValueOption | undefined {
  if (value === null || value === undefined) return undefined;
  return spec?.values.find((o) => o.value === value);
}

/** `High` for `"high"`; `Yes` for `true` when the definition names no words for it. */
export function valueLabel(spec: ValueSpec | undefined, value: Scalar | null | undefined): string {
  if (value === null || value === undefined) return "";
  const option = optionOf(spec, value);
  if (option) return option.label;
  if (typeof value === "boolean") return value ? "Yes" : "No";
  return String(value);
}

/** The choices a state's value can be compared with: its listed values, or yes and no for a boolean without words. */
export function choicesOf(spec: ValueSpec | undefined): ValueOption[] {
  if (!spec) return [];
  if (spec.values.length > 0) return spec.values;
  return spec.type === "boolean"
    ? [
        { value: true, label: "Yes" },
        { value: false, label: "No" },
      ]
    : [];
}

export function fieldChoices(field: EventField): ValueOption[] {
  if (field.values.length > 0) return field.values;
  return field.type === "boolean"
    ? [
        { value: true, label: "Yes" },
        { value: false, label: "No" },
      ]
    : [];
}

// ---------------------------------------------------------------------------
// Tests: the nine operators the runtime has, offered by type
// ---------------------------------------------------------------------------

export type TestOp = StateTest["op"];

export const OPERATOR_LABEL: Record<TestOp, string> = {
  equals: "is",
  notEquals: "is not",
  lessThan: "is below",
  atMost: "is at most",
  greaterThan: "is above",
  atLeast: "is at least",
  hasValue: "has a value",
  noValue: "has no value yet",
  unavailable: "is not available",
};

export const isStatusOp = (op: TestOp): boolean => op === "hasValue" || op === "noValue" || op === "unavailable";
export const isNumericOp = (op: TestOp): boolean => op === "lessThan" || op === "atMost" || op === "greaterThan" || op === "atLeast";
/** Does the test carry a value to compare with? */
export const takesValue = (op: TestOp): boolean => !isStatusOp(op);
/** Can a look be named after the value the state holds when this test is true? */
export const hasValueWhenTrue = (op: TestOp): boolean => op !== "noValue" && op !== "unavailable";

/**
 * The operators that make sense for a state of this type, in the order they are
 * shown: first about the value, then about whether there is one. Without a
 * definition (the provider is gone) only the status ones are offered.
 */
export function operatorsFor(type: ValueType | undefined): TestOp[] {
  const status: TestOp[] = ["hasValue", "noValue", "unavailable"];
  switch (type) {
    case "number":
      return ["equals", "notEquals", "lessThan", "atMost", "greaterThan", "atLeast", ...status];
    case "enum":
    case "string":
    case "boolean":
      return ["equals", "notEquals", ...status];
    default:
      return status;
  }
}

/** The value a new comparison starts with: the first listed value that nothing uses yet. */
export function defaultValue(spec: ValueSpec | undefined, avoid: Scalar[] = []): Scalar {
  const listed = choicesOf(spec);
  if (listed.length > 0) return (listed.find((o) => !avoid.includes(o.value)) ?? listed[0]!).value;
  return spec?.type === "number" ? 0 : "";
}

export function defaultTest(spec: ValueSpec | undefined, avoid: Scalar[] = []): StateTest {
  if (!spec) return { op: "hasValue" };
  return spec.type === "number" ? { op: "atLeast", value: 0 } : { op: "equals", value: defaultValue(spec, avoid) };
}

/** The same test with another operator, keeping the value when it still makes sense. */
export function withOperator(test: StateTest, op: TestOp, spec: ValueSpec | undefined): StateTest {
  if (test.op === op) return test;
  if (isStatusOp(op)) return { op } as StateTest;
  const current = "value" in test ? test.value : undefined;
  if (isNumericOp(op)) return { op, value: typeof current === "number" ? current : 0 } as StateTest;
  const fits = current !== undefined && (spec?.type === "number" ? typeof current === "number" : typeof current === (spec?.type === "boolean" ? "boolean" : "string"));
  return { op, value: fits ? current : defaultValue(spec) } as StateTest;
}

export const withTestValue = (test: StateTest, value: Scalar): StateTest => (takesValue(test.op) ? ({ ...test, value } as StateTest) : test);

/** `is High`, `is below 5`, `has a value`: a test in words, with the labels of the definition. */
export function testSentence(test: StateTest, spec?: ValueSpec): string {
  const label = OPERATOR_LABEL[test.op];
  if (!("value" in test)) return label;
  const shown = isNumericOp(test.op) || typeof test.value === "number" ? String(test.value) : valueLabel(spec, test.value) || "…";
  return `${label} ${shown}`;
}

// ---------------------------------------------------------------------------
// Rules: building, and what a rule shows
// ---------------------------------------------------------------------------

export type LookTarget = { kind: "none" } | { kind: "look"; name: string } | { kind: "value" } | { kind: "field"; field: string };

export function lookTargetOf(rule: FeedbackRule): LookTarget {
  if (rule.show) return { kind: "look", name: rule.show };
  if (rule.showFromValue) return { kind: "value" };
  if (rule.showFromField) return { kind: "field", field: rule.showFromField };
  return { kind: "none" };
}

/** The rule showing something else; exactly one way of naming a look is ever set. */
export function withLookTarget<R extends FeedbackRule>(rule: R, target: LookTarget): R {
  const rest: Record<string, unknown> = { ...rule };
  delete rest.show;
  delete rest.showFromValue;
  delete rest.showFromField;
  switch (target.kind) {
    case "none":
      return rest as R;
    case "look":
      return { ...rest, show: target.name } as R;
    case "value":
      return isStateRule(rule) ? ({ ...rest, showFromValue: true } as R) : (rest as R);
    case "field":
      return isStateRule(rule) ? (rest as R) : ({ ...rest, showFromField: target.field } as R);
  }
}

export function newStateRule(address: StateAddress, test: StateTest): StateRule {
  const args = Object.keys(address.args ?? {}).length > 0 ? { args: address.args } : {};
  return { when: { state: { provider: address.provider, id: address.id, ...args }, is: test } };
}

export function newEventRule(provider: string, id: string): EventRule {
  return { when: { event: { provider, id }, where: [] } };
}

/** A state rule's test changed. A look named after the value cannot survive a test that has no value. */
export function withTest(rule: StateRule, test: StateTest): StateRule {
  const next: StateRule = { ...rule, when: { ...rule.when, is: test } };
  return rule.showFromValue && !hasValueWhenTrue(test.op) ? withLookTarget(next, { kind: "none" }) : next;
}

export function withAddress(rule: StateRule, address: StateAddress): StateRule {
  const args = Object.keys(address.args ?? {}).length > 0 ? { args: address.args } : {};
  return { ...rule, when: { ...rule.when, state: { provider: address.provider, id: address.id, ...args } } };
}

/** One rule showing another look (or none yet). */
export const showLook = (control: Control, index: number, target: LookTarget): Control => {
  const rule = control.feedback[index];
  return rule ? { ...control, feedback: control.feedback.map((r, i) => (i === index ? withLookTarget(r, target) : r)) } : control;
};

/** A new look, made and shown by one rule, as a single change of the draft. */
export function createAndShow(control: Control, index: number, name: string, visual?: Partial<Visual>): Control {
  const made = createLook(control, name, visual);
  return showLook(made.control, index, { kind: "look", name: made.key });
}

// ---------------------------------------------------------------------------
// The list: numbers, runs, ordering
// ---------------------------------------------------------------------------

export type RuleKind = "state" | "event";
export const kindOfRule = (r: FeedbackRule): RuleKind => (isStateRule(r) ? "state" : "event");

/** Where the rules of one kind sit in the control's list. */
export const indexesOf = (feedback: FeedbackRule[], kind: RuleKind): number[] => feedback.flatMap((r, i) => (kindOfRule(r) === kind ? [i] : []));

/** The number a person sees: the rule's place among rules of its own kind, from 1. First match wins in that order. */
export const ruleNumber = (feedback: FeedbackRule[], index: number): number => {
  const rule = feedback[index];
  return rule ? indexesOf(feedback, kindOfRule(rule)).indexOf(index) + 1 : 0;
};

export interface Run {
  key: string;
  address: StateAddress;
  /** Positions in the control's list. */
  indexes: number[];
}

/** State rules in order, with neighbours that read the same address gathered under one header. */
export function stateRuns(feedback: FeedbackRule[]): Run[] {
  const runs: Run[] = [];
  for (const i of indexesOf(feedback, "state")) {
    const rule = feedback[i] as StateRule;
    const key = addressKey(rule.when.state);
    const last = runs[runs.length - 1];
    if (last && last.key === key) last.indexes.push(i);
    else runs.push({ key, address: rule.when.state, indexes: [i] });
  }
  return runs;
}

/**
 * Swap a rule with its neighbour of the same kind, up (`-1`) or down (`1`).
 * Rules of the other kind stay where they are. `null` when there is no neighbour.
 */
export function moveRule(feedback: FeedbackRule[], index: number, delta: -1 | 1): { feedback: FeedbackRule[]; index: number } | null {
  const rule = feedback[index];
  if (!rule) return null;
  const same = indexesOf(feedback, kindOfRule(rule));
  const at = same.indexOf(index) + delta;
  if (at < 0 || at >= same.length) return null;
  const other = same[at]!;
  const next = [...feedback];
  next[index] = feedback[other]!;
  next[other] = rule;
  return { feedback: next, index: other };
}

export const removeRule = (feedback: FeedbackRule[], index: number): FeedbackRule[] => feedback.filter((_, i) => i !== index);

export function replaceRule(feedback: FeedbackRule[], index: number, rule: FeedbackRule): FeedbackRule[] {
  const next = [...feedback];
  next[index] = rule;
  return next;
}

/**
 * Add a rule after another, or after the last rule of its kind (the end of the
 * list when it has none). Never moves an existing rule relative to another.
 */
export function insertRule(feedback: FeedbackRule[], rule: FeedbackRule, after?: number): { feedback: FeedbackRule[]; index: number } {
  const same = indexesOf(feedback, kindOfRule(rule));
  const at = (after ?? same[same.length - 1] ?? feedback.length - 1) + 1;
  const next = [...feedback];
  next.splice(at, 0, rule);
  return { feedback: next, index: at };
}

/** One rule for each value of an enum or boolean state that no rule of this address tests yet. */
export function unusedValues(feedback: FeedbackRule[], address: StateAddress, spec: ValueSpec | undefined): ValueOption[] {
  const key = addressKey(address);
  const used = feedback.flatMap((r) => (isStateRule(r) && addressKey(r.when.state) === key && r.when.is.op === "equals" ? [r.when.is.value] : []));
  return choicesOf(spec).filter((o) => !used.includes(o.value));
}

// ---------------------------------------------------------------------------
// Scope: the arguments that pick one instance of a state
// ---------------------------------------------------------------------------

/** What a scope form holds, as the scalars an address stores. Blanks are left out. */
export function scalarArgs(args: Args): Record<string, Scalar> {
  const out: Record<string, Scalar> = {};
  for (const [k, v] of Object.entries(args)) if (typeof v === "boolean" || typeof v === "number" || (typeof v === "string" && v !== "")) out[k] = v;
  return out;
}

/** The arguments as they would be stored (defaults filled in), and what is still wrong with them. */
export function scopeOf(o: Observable, args: Args): { args: Record<string, Scalar>; issues: { param?: string; message: string }[] } {
  const normalized = normalizeArgs(o.params, args);
  return { args: scalarArgs(normalized.args), issues: normalized.issues };
}

export const addressFor = (o: Observable, args: Record<string, Scalar>): StateAddress => ({ provider: o.provider, id: o.local, ...(Object.keys(args).length > 0 ? { args } : {}) });

/** Point a rule at another state. A test that no longer fits is replaced by one that does, and said so. */
export function retargetRule(rule: StateRule, to: Observable, args: Record<string, Scalar>): { rule: StateRule; testReset: boolean } {
  const address = addressFor(to, args);
  const moved = withAddress(rule, address);
  const test = rule.when.is;
  const spec = to.value;
  const fits = isStatusOp(test.op) || (spec !== undefined && operatorsFor(spec.type).includes(test.op) && (!("value" in test) || valueFits(spec, test.value)));
  if (fits) return { rule: moved, testReset: false };
  return { rule: withTest(moved, defaultTest(spec)), testReset: true };
}

function valueFits(spec: ValueSpec, value: Scalar): boolean {
  switch (spec.type) {
    case "number":
      return typeof value === "number";
    case "boolean":
      return typeof value === "boolean";
    case "enum":
      return spec.values.some((o) => o.value === value);
    case "string":
      return typeof value === "string";
  }
}

/** Point an event rule at another event. Conditions and a look from a field that the new event lacks are dropped, and said so. */
export function retargetEvent(rule: EventRule, to: Observable): { rule: EventRule; dropped: boolean } {
  const fields = new Map(to.fields.map((f) => [f.name, f] as const));
  const where = rule.when.where.filter((w) => fields.has(w.field));
  let next: EventRule = { ...rule, when: { event: { provider: to.provider, id: to.local }, where } };
  let dropped = where.length !== rule.when.where.length;
  if (rule.showFromField && !fields.has(rule.showFromField)) {
    next = withLookTarget(next, { kind: "none" });
    dropped = true;
  }
  return { rule: next, dropped };
}

/**
 * One rule for each listed value that no rule of this address tests yet, placed
 * after `after`. Each value gets the look of its own name, created when the pad
 * does not have it, in a colour its role suggests. Nothing that exists is changed.
 */
export function addRulesForValues(control: Control, address: StateAddress, values: ValueOption[], after: number): { control: Control; added: number[] } {
  let next = control;
  let at = after;
  const added: number[] = [];
  for (const option of values) {
    const key = lookKey(String(option.label || option.value)) || "look";
    if (key !== IDLE && !(key in next.states)) next = createLook(next, key, { color: guessLookColor(option.role ?? String(option.label)) }).control;
    const inserted = insertRule(next.feedback, { ...newStateRule(address, { op: "equals", value: option.value }), show: key }, at);
    next = { ...next, feedback: inserted.feedback };
    at = inserted.index;
    added.push(at);
  }
  return { control: next, added };
}

// ---------------------------------------------------------------------------
// What is still missing from a rule
// ---------------------------------------------------------------------------

export interface Problem {
  rule: number;
  field: IssueField;
  param?: string;
  message: string;
  /** Not wrong, only not filled in yet: shown as a prompt, not as an error. */
  needed?: boolean;
}

/**
 * What a person has not filled in yet, from the rule and its definition alone.
 * Rust has the last word (`review_feedback`); this is only so a rule that is
 * obviously unfinished is marked at once and cannot be saved by mistake.
 */
export function problemsOf(feedback: FeedbackRule[], catalog: ObservableCatalog): Problem[] {
  const out: Problem[] = [];
  feedback.forEach((rule, i) => {
    const entry = observableOf(catalog, rule);
    if (lookTargetOf(rule).kind === "none") out.push({ rule: i, field: "look", message: "Choose the look to show.", needed: true });
    if (isStateRule(rule)) {
      const test = rule.when.is;
      if ("value" in test && (test.value === "" || (typeof test.value === "number" && !Number.isFinite(test.value)))) out.push({ rule: i, field: "value", message: "Choose a value.", needed: true });
      if (entry) {
        for (const issue of scopeOf(entry, rule.when.state.args ?? {}).issues) out.push({ rule: i, field: "args", param: issue.param, message: issue.message, needed: isMissingIssue(issue) });
      }
    } else {
      if (typeof rule.hold === "object" && !Number.isFinite(rule.hold.ms)) out.push({ rule: i, field: "hold", message: "Enter how many seconds.", needed: true });
    }
    if (!isStateRule(rule) && entry) {
      for (const w of rule.when.where) {
        const field = entry.fields.find((f) => f.name === w.field);
        if (field && (w.equals === "" || (typeof w.equals === "number" && !Number.isFinite(w.equals)))) out.push({ rule: i, field: "where", message: `Choose a value for “${field.label}”.`, needed: true });
      }
    }
  });
  return out;
}

/**
 * Problems from the local check and from Rust, one per place. Rust's wording wins
 * where both speak about the same thing, because it is the one that refuses a save.
 */
export function mergeIssues(local: Problem[], remote: RuleIssue[]): Problem[] {
  const place = (p: { rule: number; field?: IssueField; param?: string }): string => `${p.rule}|${p.field ?? ""}|${p.param ?? ""}`;
  const fromRust = new Map(remote.map((r) => [place(r), r] as const));
  const out: Problem[] = [];
  for (const p of local) if (!fromRust.has(place(p))) out.push(p);
  for (const r of remote) out.push({ rule: r.rule, field: r.field ?? "rules", param: r.param, message: r.message });
  return out;
}

/** Can the draft be sent to Rust? A number being typed (still empty) is not a number yet. */
export function isSendable(feedback: FeedbackRule[]): boolean {
  return feedback.every((rule) => {
    if (isStateRule(rule)) return !("value" in rule.when.is) || typeof rule.when.is.value !== "number" || Number.isFinite(rule.when.is.value);
    const hold = rule.hold;
    return (hold === undefined || hold === "latch" || Number.isFinite(hold.ms)) && rule.when.where.every((w) => typeof w.equals !== "number" || Number.isFinite(w.equals));
  });
}

// ---------------------------------------------------------------------------
// Events: conditions and how long the look stays
// ---------------------------------------------------------------------------

export type HoldChoice = { kind: "for"; ms: number } | { kind: "latch" };

export function holdOf(rule: EventRule, limits?: FeedbackLimits): HoldChoice {
  if (rule.hold === "latch") return { kind: "latch" };
  return { kind: "for", ms: rule.hold?.ms ?? limits?.defaultHoldMs ?? 3000 };
}

export function withHold(rule: EventRule, hold: HoldChoice): EventRule {
  return { ...rule, hold: hold.kind === "latch" ? "latch" : { ms: Math.round(hold.ms) } };
}

/** `2 seconds`, `0.5 seconds`, `1 second`. */
export function secondsText(ms: number): string {
  const s = ms / 1000;
  const shown = Number.isInteger(s) ? String(s) : String(Math.round(s * 100) / 100);
  return `${shown} ${s === 1 ? "second" : "seconds"}`;
}

export function withCondition(rule: EventRule, field: string, equals: Scalar): EventRule {
  const where = rule.when.where.some((w) => w.field === field) ? rule.when.where.map((w) => (w.field === field ? { ...w, equals } : w)) : [...rule.when.where, { field, equals }];
  return { ...rule, when: { ...rule.when, where } };
}

export const withoutCondition = (rule: EventRule, field: string): EventRule => ({ ...rule, when: { ...rule.when, where: rule.when.where.filter((w) => w.field !== field) } });

export function defaultConditionValue(field: EventField): Scalar {
  const listed = fieldChoices(field);
  if (listed.length > 0) return listed[0]!.value;
  return field.type === "number" ? 0 : "";
}

// ---------------------------------------------------------------------------
// Looks: the control's named appearances (`states`); `visual` is the resting one
// ---------------------------------------------------------------------------

/** `approval_required` and `approval-required` are shown as “Approval required”. */
export function lookLabel(key: string): string {
  if (key === IDLE) return "Resting look";
  const words = key.replace(/[_-]+/g, " ").trim();
  return words.charAt(0).toUpperCase() + words.slice(1);
}

/** The key a typed name is stored under: lower case, letters, digits, `_` and `-`. */
export function lookKey(name: string, max = 40): string {
  return fold(name)
    .replace(/[^a-z0-9_-]+/g, "-")
    .replace(/^-+|-+$/g, "")
    .slice(0, max)
    .replace(/-+$/g, "");
}

/** Why a name cannot be a new look, or `null`. `except` is the look being renamed. */
export function lookNameProblem(control: Control, name: string, except?: string, max = 40): string | null {
  const key = lookKey(name, max);
  if (!name.trim() || !key) return "Give the look a name.";
  if (name.trim().length > max) return `A name has at most ${max} characters.`;
  if (key === IDLE) return "“idle” is the resting look: change it in Resting look.";
  if (key !== except && key in control.states) return `There is already a look called “${lookLabel(key)}”.`;
  return null;
}

const COLOR_WORDS: [RegExp, string][] = [
  [/err|fail|danger|critical|alarm|bad|stop/, "#ef4444"],
  [/warn|high|caution|hot|full/, "#f59e0b"],
  [/attention|approv|needs|alert|ask/, "#a855f7"],
  [/offline|inactive|off|down|idle|empty|none|disabled/, "#64748b"],
  [/active|ok|good|success|done|run|up|ready|on|low|fine/, "#22c55e"],
];

/** A colour a new look starts with, from what its name says. Only a starting point. */
export function guessLookColor(name: string): string {
  const n = fold(name);
  return COLOR_WORDS.find(([re]) => re.test(n))?.[1] ?? "#3b82f6";
}

export function createLook(control: Control, name: string, visual?: Partial<Visual>): { control: Control; key: string } {
  const key = lookKey(name);
  if (key in control.states) return { control, key };
  const look: Visual = { color: visual?.color ?? guessLookColor(name), animation: visual?.animation ?? "none" };
  return { control: { ...control, states: { ...control.states, [key]: look } }, key };
}

export const setLookVisual = (control: Control, key: string, patch: Partial<Visual>): Control =>
  key in control.states ? { ...control, states: { ...control.states, [key]: { ...control.states[key]!, ...patch } } } : control;

/** Positions of the rules that show this look by name. */
export const lookUsage = (control: Control, key: string): number[] => control.feedback.flatMap((r, i) => (r.show === key ? [i] : []));

/** A look under another name. The rules that showed it show the new name, so nothing is orphaned. */
export function renameLook(control: Control, from: string, name: string): { control: Control; key: string } {
  const key = lookKey(name);
  if (key === from || !(from in control.states) || key in control.states) return { control, key: from };
  const states: Record<string, Visual> = {};
  for (const [k, v] of Object.entries(control.states)) states[k === from ? key : k] = v;
  const feedback = control.feedback.map((r) => (r.show === from ? { ...r, show: key } : r));
  return { control: { ...control, states, feedback }, key };
}

/** The look removed. Rules that showed it keep their choice and are reported as missing a look. */
export function removeLook(control: Control, key: string): Control {
  const states = { ...control.states };
  delete states[key];
  return { ...control, states };
}

// ---------------------------------------------------------------------------
// What a refresh depends on
// ---------------------------------------------------------------------------

/**
 * A short fingerprint of what the rules read: for each, whether it can be
 * observed and what it reads now. It changes when a value or availability that
 * matters to this control changes and not otherwise, so the explanation is asked
 * for again only then.
 */
export function readingsKey(catalog: ObservableCatalog, feedback: FeedbackRule[]): string {
  return feedback
    .map((rule) => {
      const entry = observableOf(catalog, rule);
      if (!entry) return "missing";
      if (!isStateRule(rule)) return `event:${entry.availability.status}`;
      const read = readInstance(entry, rule.when.state.args ?? {});
      return `${entry.availability.status}:${read.status}:${String(read.value ?? "")}:${read.updatedAt ?? ""}`;
    })
    .join("|");
}

// ---------------------------------------------------------------------------
// Explanation, for display
// ---------------------------------------------------------------------------

export interface Reading {
  /** The state, in words (`Counter Plugin / Counter level`). */
  title: string;
  /** With its arguments: `slot 2`. Empty for a state without any. */
  scope: string;
  status: ObservedStatus;
  /** The value in the definition's words; empty unless the status is `known`. */
  value: string;
  /** Rust's sentence for why it is not known; empty when it is. */
  reason: string;
  /** The technical address (`example.counter.level`), for the details nobody needs by default. */
  address: string;
}

export interface ExplanationView {
  readings: Reading[];
  /** The state rule that decided the look, as the number a person sees and its words. */
  matched: { number: number; text: string } | null;
  /** The look the state rules give, in words; `null` is the resting look. */
  derived: string | null;
  overlay: { look: string; rule: number | null; remainingMs: number | null; latched: boolean; source: "event" | "action" } | null;
  /** The look on the pad. */
  shown: string | null;
  layer: "overlay" | "state" | "rest";
  /** Rules that decided on a look the control does not have. */
  missing: { rule: number; look: string; label: string }[];
}

const scopeText = (args: Record<string, Scalar> | undefined): string => Object.entries(args ?? {}).map(([k, v]) => `${k} ${String(v)}`).join(", ");

/** The explanation Rust gave, arranged for the panel. Nothing is inferred: every word comes from the reply or the definitions. */
export function viewExplanation(ex: ControlExplanation, feedback: FeedbackRule[], catalog: ObservableCatalog): ExplanationView {
  const seen = new Set<string>();
  const readings: Reading[] = [];
  for (const r of ex.rules) {
    if (r.kind !== "state" || !r.observed || !r.observable) continue;
    if (seen.has(r.observable.address)) continue;
    seen.add(r.observable.address);
    const rule = feedback[r.index];
    const args = rule && isStateRule(rule) ? rule.when.state.args : undefined;
    readings.push({
      title: r.observable.name ? `${r.observable.providerName ?? r.observable.provider} / ${r.observable.name}` : r.observable.address,
      scope: scopeText(args),
      status: r.observed.status,
      value: r.observed.status === "known" ? (r.valueLabel ?? String(r.observed.value ?? "")) : "",
      reason: r.reason ?? "",
      address: r.observable.address,
    });
  }
  let matched: ExplanationView["matched"] = null;
  if (ex.winner !== null) {
    const rule = feedback[ex.winner];
    if (rule && isStateRule(rule)) {
      const entry = observableOf(catalog, rule);
      matched = { number: ruleNumber(feedback, ex.winner), text: testSentence(rule.when.is, entry?.value) };
    }
  }
  return {
    readings,
    matched,
    derived: ex.derived,
    overlay: ex.overlay
      ? { look: ex.overlay.look, rule: ex.overlay.rule === undefined ? null : ruleNumber(feedback, ex.overlay.rule), remainingMs: ex.overlay.remainingMs ?? null, latched: ex.overlay.latched, source: ex.overlay.source }
      : null,
    shown: ex.shown,
    layer: ex.layer,
    missing: ex.diagnostics.map((d) => {
      const rule = feedback[d.rule];
      const number = ruleNumber(feedback, d.rule);
      return { rule: number, look: d.look, label: rule && !isStateRule(rule) ? `Event rule ${number}` : `Rule ${number}` };
    }),
  };
}
