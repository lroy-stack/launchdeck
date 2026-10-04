import { describe, expect, it } from "vitest";
import fixture from "../../examples/observables.json";
import counterFixture from "../../examples/feedback-editor-counter.json";
import eventFixture from "../../examples/feedback-editor-event.json";
import {
  IDLE,
  addRulesForValues,
  addressFor,
  createAndShow,
  createLook,
  defaultConditionValue,
  defaultTest,
  groupByProvider,
  holdOf,
  indexesOf,
  insertRule,
  isSendable,
  lookKey,
  lookLabel,
  lookNameProblem,
  lookUsage,
  mergeIssues,
  moveRule,
  newEventRule,
  newStateRule,
  nowText,
  operatorsFor,
  problemsOf,
  readingsKey,
  removeLook,
  removeRule,
  renameLook,
  retargetEvent,
  retargetRule,
  ruleNumber,
  scopeOf,
  searchObservables,
  secondsText,
  showLook,
  stateRuns,
  suggestedObservables,
  testSentence,
  typeSummary,
  unusedValues,
  viewExplanation,
  withCondition,
  withHold,
  withOperator,
  withTest,
  withTestValue,
  withoutCondition,
  withLookTarget,
  guessLookColor,
} from "./feedbackEdit";
import { applyDelta, findObservable } from "./observables";
import {
  actionDefinitionSchema,
  controlExplanationSchema,
  controlSchema,
  feedbackRuleSchema,
  observableCatalogSchema,
  type Control,
  type FeedbackRule,
  type Observable,
  type StateRule,
} from "./schema";

const catalog = observableCatalogSchema.parse(fixture.catalog);
const get = (id: string): Observable => findObservable(catalog, id)!;
const level = get("example.counter.level");
const channels = get("example.channels.status");
const names = (found: Observable[]) => found.map((o) => o.name);

const blank = (over: Partial<Control> = {}): Control =>
  controlSchema.parse({ id: "g.1.1", pos: { kind: "grid", col: 1, row: 1 }, label: "Pad", visual: { color: "#2f81f7", animation: "none" }, ...over });

describe("finding an observable", () => {
  it("lists only the kind asked for, every provider in the catalog's order", () => {
    const states = searchObservables(catalog, "", "state");
    expect(states.every((o) => o.kind === "state")).toBe(true);
    const providers = groupByProvider(states).map((g) => g.provider);
    expect(providers).toEqual(["runtime", "demo", "agent-control", "example.channels", "example.counter"]);
    const events = searchObservables(catalog, "", "event");
    expect(events.every((o) => o.kind === "event")).toBe(true);
    expect(events.length).toBeGreaterThan(30);
  });

  it("finds by name, by provider, by a word of a value and by a keyword", () => {
    expect(names(searchObservables(catalog, "counter level", "state"))[0]).toBe("Counter level");
    expect(names(searchObservables(catalog, "channels", "state"))).toContain("Channel status");
    // “full” is a word of one of the Counter's values and a keyword of its level.
    expect(names(searchObservables(catalog, "full", "state"))).toContain("Counter level");
    expect(names(searchObservables(catalog, "health", "state"))).toEqual(["Channel status"]);
  });

  it("needs every word to match, and ignores case and accents", () => {
    expect(names(searchObservables(catalog, "agent status", "state"))).toContain("Agent track status");
    expect(searchObservables(catalog, "agent zzzz", "state")).toEqual([]);
    expect(names(searchObservables(catalog, "COUNTÉR", "state"))).toContain("Counter level");
  });

  it("forgives one slip of the keys", () => {
    expect(names(searchObservables(catalog, "levle", "state"))).toContain("Counter level");
  });

  it("puts the best match first and keeps a state from a stopped provider in the list", () => {
    const hello = searchObservables(catalog, "hello", "event");
    expect(hello).toHaveLength(1);
    expect(hello[0]!.availability.status).toBe("unavailable");
    const ping = searchObservables(catalog, "ping", "event");
    // The one that works comes before the one that does not.
    expect(ping.map((o) => o.availability.status)).toEqual(["available", "unavailable"]);
  });

  it("groups by provider keeping the order of first appearance", () => {
    const groups = groupByProvider(searchObservables(catalog, "agent", "event"));
    expect(groups).toHaveLength(1);
    expect(groups[0]).toMatchObject({ provider: "agent-control", name: "Agent Control" });
    expect(groups[0]!.items.length).toBeGreaterThan(10);
  });

  it("says what an observable holds in the words of its definition", () => {
    expect(typeSummary(level)).toBe("Empty, Low, High, Full");
    expect(typeSummary(get("agent-control.slot.focused"))).toBe("Selected, Not selected");
    expect(typeSummary(get("example.counter.counter.changed"))).toBe("Event with Value, Level");
    expect(typeSummary(get("runtime.surface.active"))).toBe("Text");
  });

  it("says what a state reads now without inventing a value for a scoped one", () => {
    expect(nowText(level)).toEqual({ status: "known", text: "Low" });
    expect(nowText(channels)).toEqual({ status: "known", text: "2 values reported" });
    expect(nowText(get("example.counter.counter.changed"))).toBeNull();
    const off = { ...level, availability: { status: "unavailable" as const, cause: "provider" as const, reason: "The plugin is turned off in Plugins." } };
    expect(nowText(off)).toEqual({ status: "unavailable", text: "Unavailable" });
  });

  it("suggests what the pad's actions say they usually show, once, never what is already read", () => {
    const increment = actionDefinitionSchema.parse({
      id: "example.counter/counter.increment",
      provider: "example.counter",
      action: "counter.increment",
      name: "Increment",
      description: "",
      category: "Counter Plugin",
      availability: { status: "available" },
      suggests: ["example.counter.level", "example.counter.level", "nothing.here"],
    });
    expect(names(suggestedObservables(catalog, [increment, undefined], []))).toEqual(["Counter level"]);
    const reads = [newStateRule(addressFor(level, {}), { op: "hasValue" })];
    expect(suggestedObservables(catalog, [increment], reads)).toEqual([]);
  });
});

describe("tests: only the nine operators, offered by type", () => {
  it("offers numeric comparisons to numbers and equality to everything with a value", () => {
    expect(operatorsFor("number")).toEqual(["equals", "notEquals", "lessThan", "atMost", "greaterThan", "atLeast", "hasValue", "noValue", "unavailable"]);
    expect(operatorsFor("enum")).toEqual(["equals", "notEquals", "hasValue", "noValue", "unavailable"]);
    expect(operatorsFor("boolean")).toEqual(operatorsFor("string"));
    // The provider is gone: only what can be said without its definition.
    expect(operatorsFor(undefined)).toEqual(["hasValue", "noValue", "unavailable"]);
  });

  it("starts a new test with the first value nobody tests yet", () => {
    expect(defaultTest(level.value)).toEqual({ op: "equals", value: "zero" });
    expect(defaultTest(level.value, ["zero", "low"])).toEqual({ op: "equals", value: "high" });
    expect(defaultTest({ type: "number", values: [] })).toEqual({ op: "atLeast", value: 0 });
    expect(defaultTest(undefined)).toEqual({ op: "hasValue" });
  });

  it("changes the operator and keeps the value only when it still makes sense", () => {
    const spec = level.value;
    expect(withOperator({ op: "equals", value: "low" }, "notEquals", spec)).toEqual({ op: "notEquals", value: "low" });
    expect(withOperator({ op: "equals", value: "low" }, "hasValue", spec)).toEqual({ op: "hasValue" });
    expect(withOperator({ op: "hasValue" }, "equals", spec)).toEqual({ op: "equals", value: "zero" });
    expect(withOperator({ op: "equals", value: 3 }, "atLeast", { type: "number", values: [] })).toEqual({ op: "atLeast", value: 3 });
    expect(withOperator({ op: "hasValue" }, "lessThan", { type: "number", values: [] })).toEqual({ op: "lessThan", value: 0 });
  });

  it("sets a value only on a test that takes one", () => {
    expect(withTestValue({ op: "equals", value: "low" }, "high")).toEqual({ op: "equals", value: "high" });
    expect(withTestValue({ op: "noValue" }, "high")).toEqual({ op: "noValue" });
  });

  it("writes a test in the words of the definition", () => {
    expect(testSentence({ op: "equals", value: "max" }, level.value)).toBe("is Full");
    expect(testSentence({ op: "atLeast", value: 3 })).toBe("is at least 3");
    expect(testSentence({ op: "noValue" })).toBe("has no value yet");
    expect(testSentence({ op: "unavailable" })).toBe("is not available");
  });

  it("drops a look named after the value when the test has no value", () => {
    const rule: StateRule = { ...newStateRule(addressFor(level, {}), { op: "hasValue" }), showFromValue: true };
    expect(withTest(rule, { op: "noValue" }).showFromValue).toBeUndefined();
    expect(withTest(rule, { op: "equals", value: "low" }).showFromValue).toBe(true);
  });
});

describe("rules: building, numbering, ordering", () => {
  const rule = (value: string, show?: string): StateRule => ({ ...newStateRule(addressFor(level, {}), { op: "equals", value }), ...(show ? { show } : {}) });
  const event = (): FeedbackRule => newEventRule("example.counter", "counter.changed");

  it("builds a scoped rule with its arguments and a plain one without an empty args", () => {
    expect(newStateRule(addressFor(channels, { channel: "alpha" }), { op: "hasValue" }).when.state).toEqual({ provider: "example.channels", id: "status", args: { channel: "alpha" } });
    expect(newStateRule(addressFor(level, {}), { op: "hasValue" }).when.state).toEqual({ provider: "example.counter", id: "level" });
  });

  it("adds after the last rule of its own kind and never moves another", () => {
    const list: FeedbackRule[] = [rule("zero"), event(), rule("low")];
    const added = insertRule(list, rule("high"));
    expect(added.index).toBe(3);
    expect(added.feedback.map((r) => ("state" in r.when ? "s" : "e"))).toEqual(["s", "e", "s", "s"]);
    // An event goes after the event, not at the end.
    const evented = insertRule([event(), rule("zero")], event());
    expect(evented.index).toBe(1);
    // A first rule of a kind goes to the end.
    expect(insertRule([rule("zero")], event()).index).toBe(1);
    // After a given rule.
    expect(insertRule(list, rule("high"), 0).index).toBe(1);
  });

  it("numbers rules among those of their own kind, from 1", () => {
    const list: FeedbackRule[] = [rule("zero"), event(), rule("low"), event()];
    expect([0, 1, 2, 3].map((i) => ruleNumber(list, i))).toEqual([1, 1, 2, 2]);
    expect(indexesOf(list, "event")).toEqual([1, 3]);
  });

  it("gathers neighbours reading the same state, and repeats the state when something else sits between", () => {
    const list: FeedbackRule[] = [rule("zero"), rule("low"), newStateRule(addressFor(channels, { channel: "alpha" }), { op: "hasValue" }), rule("high")];
    const runs = stateRuns(list);
    expect(runs.map((r) => r.indexes)).toEqual([[0, 1], [2], [3]]);
  });

  it("moves a rule past its neighbour of the same kind, and says where it went", () => {
    const list: FeedbackRule[] = [rule("zero"), event(), rule("low"), rule("high")];
    const up = moveRule(list, 2, -1)!;
    expect(up.index).toBe(0);
    expect(up.feedback.map((r) => ("is" in r.when ? r.when.is : "event"))).toEqual([{ op: "equals", value: "low" }, "event", { op: "equals", value: "zero" }, { op: "equals", value: "high" }]);
    expect(moveRule(list, 0, -1)).toBeNull();
    expect(moveRule(list, 3, 1)).toBeNull();
    expect(moveRule(list, 1, 1)).toBeNull(); // the only event
    // The input is not changed.
    expect(list[0]).toEqual(rule("zero"));
  });

  it("removes and replaces without touching the rest", () => {
    const list: FeedbackRule[] = [rule("zero"), rule("low"), rule("high")];
    expect(removeRule(list, 1)).toEqual([rule("zero"), rule("high")]);
  });

  it("finds the values of an enum that no rule of this address tests", () => {
    const list: FeedbackRule[] = [rule("zero"), rule("high")];
    expect(unusedValues(list, { provider: "example.counter", id: "level" }, level.value).map((o) => o.value)).toEqual(["low", "max"]);
    // Another address is not the same state.
    expect(unusedValues(list, { provider: "example.counter", id: "other" }, level.value)).toHaveLength(4);
    expect(unusedValues([], { provider: "x", id: "y" }, { type: "boolean", values: [] }).map((o) => o.value)).toEqual([true, false]);
  });

  it("adds one rule per missing value with a look of its name, in a colour its role suggests, and reuses what exists", () => {
    const start = blank({ states: { low: { color: "#123456", animation: "pulse" } }, feedback: [rule("zero", IDLE)] });
    const made = addRulesForValues(start, { provider: "example.counter", id: "level" }, unusedValues(start.feedback, { provider: "example.counter", id: "level" }, level.value), 0);
    expect(made.added).toEqual([1, 2, 3]);
    // The look is named after the value's words (“Full”), not after its stored value.
    expect(made.control.feedback.map((r) => r.show)).toEqual(["idle", "low", "high", "full"]);
    // The look that was there is untouched; the others are new.
    expect(made.control.states.low).toEqual({ color: "#123456", animation: "pulse" });
    expect(Object.keys(made.control.states).sort()).toEqual(["full", "high", "low"]);
    expect(made.control.states.high!.color).toBe("#f59e0b");
    // The first rule did not move.
    expect(made.control.feedback[0]).toEqual(rule("zero", IDLE));
  });

  it("never turns the resting look into a look of the pad", () => {
    const start = blank();
    const made = addRulesForValues(start, { provider: "p", id: "s" }, [{ value: "idle", label: "Idle", role: "inactive" }], -1);
    expect(made.control.states).toEqual({});
    expect(made.control.feedback[0]!.show).toBe("idle");
  });
});

describe("scope: the arguments that pick one instance", () => {
  const slot = get("agent-control.slot.status");

  it("needs the arguments the definition requires and says which", () => {
    const empty = scopeOf(slot, {});
    expect(empty.args).toEqual({});
    expect(empty.issues).toEqual([expect.objectContaining({ param: "slot", message: "Track is required" })]);
    expect(scopeOf(slot, { slot: 2 })).toEqual({ args: { slot: 2 }, issues: [] });
  });

  it("refuses a value outside the definition's range and a name it does not have", () => {
    expect(scopeOf(slot, { slot: 9 }).issues[0]!.message).toMatch(/between 0 and 7/);
    expect(scopeOf(slot, { slot: 1, extra: 1 }).issues.map((i) => i.param)).toContain("extra");
    expect(scopeOf(channels, { channel: "" }).issues).toHaveLength(1);
  });

  it("stores only scalars, blank ones left out", () => {
    expect(scopeOf(channels, { channel: "alpha" }).args).toEqual({ channel: "alpha" });
    expect(addressFor(channels, {})).toEqual({ provider: "example.channels", id: "status" });
  });

  it("points rules at another state, keeping a test that still fits and resetting one that does not", () => {
    const equalsLow = rule("low");
    const kept = retargetRule(equalsLow, level, {});
    expect(kept).toEqual({ rule: equalsLow, testReset: false });
    const moved = retargetRule(equalsLow, channels, { channel: "beta" });
    // “low” is not a Channel status: the value is replaced by one that is, and the caller is told.
    expect(moved.testReset).toBe(true);
    expect(moved.rule.when.state).toEqual({ provider: "example.channels", id: "status", args: { channel: "beta" } });
    expect(moved.rule.when.is).toEqual({ op: "equals", value: "up" });
    // Only the scope changes: nothing is reset.
    const rescoped = retargetRule(newStateRule(addressFor(channels, { channel: "alpha" }), { op: "equals", value: "down" }), channels, { channel: "beta" });
    expect(rescoped.testReset).toBe(false);
    expect(rescoped.rule.when.is).toEqual({ op: "equals", value: "down" });
    // A status test fits anything.
    expect(retargetRule(newStateRule(addressFor(level, {}), { op: "noValue" }), channels, { channel: "alpha" }).testReset).toBe(false);
    // The look is the person's and stays.
    expect(retargetRule({ ...equalsLow, show: "x" }, channels, { channel: "alpha" }).rule.show).toBe("x");
  });

  function rule(value: string): StateRule {
    return newStateRule(addressFor(level, {}), { op: "equals", value });
  }
});

describe("what is still missing, and who says so", () => {
  const control = (feedback: FeedbackRule[]) => blank({ feedback });

  it("marks a rule without a look as needed, not wrong", () => {
    const list: FeedbackRule[] = [newStateRule(addressFor(level, {}), { op: "equals", value: "low" })];
    expect(problemsOf(list, catalog)).toEqual([{ rule: 0, field: "look", message: "Choose the look to show.", needed: true }]);
    expect(problemsOf([{ ...list[0]!, show: "x" }], catalog)).toEqual([]);
  });

  it("marks an empty or half-typed number and an empty hold", () => {
    const n = { ...newStateRule(addressFor(level, {}), { op: "atLeast", value: Number.NaN }), show: "x" };
    expect(problemsOf([n], catalog).map((p) => [p.field, p.needed])).toEqual([["value", true]]);
    const e = { ...newEventRule("example.counter", "counter.changed"), show: "x", hold: { ms: Number.NaN } };
    expect(problemsOf([e], catalog).map((p) => p.field)).toEqual(["hold"]);
    expect(isSendable([n])).toBe(false);
    expect(isSendable([e])).toBe(false);
    expect(isSendable([{ ...e, hold: "latch" as const }])).toBe(true);
  });

  it("marks a scope that is not filled in, with the setting it is about", () => {
    const r = { ...newStateRule({ provider: "agent-control", id: "slot.status" }, { op: "hasValue" }), show: "x" };
    expect(problemsOf([r], catalog)).toEqual([{ rule: 0, field: "args", param: "slot", message: "Track is required", needed: true }]);
  });

  it("marks an event condition left empty", () => {
    const e = withCondition({ ...newEventRule("example.channels", "channel.changed"), show: "x" }, "channel", "");
    expect(problemsOf([e], catalog).map((p) => p.field)).toEqual(["where"]);
  });

  it("lets Rust's wording replace the local one about the same place, and keeps both when they differ", () => {
    const local = [{ rule: 0, field: "look" as const, message: "Choose the look to show.", needed: true }, { rule: 1, field: "value" as const, message: "Choose a value.", needed: true }];
    const merged = mergeIssues(local, [{ rule: 0, field: "look", message: "choose exactly one look to show" }, { rule: 2, field: "state", message: "gone" }]);
    expect(merged.map((m) => [m.rule, m.field, m.message, m.needed])).toEqual([
      [1, "value", "Choose a value.", true],
      [0, "look", "choose exactly one look to show", undefined],
      [2, "state", "gone", undefined],
    ]);
    expect(control([]).feedback).toEqual([]);
  });
});

describe("events: conditions and how long the look stays", () => {
  const event = () => newEventRule("example.counter", "counter.changed");

  it("adds, replaces and removes a condition on a field", () => {
    let rule = withCondition(event(), "level", "high");
    expect(rule.when.where).toEqual([{ field: "level", equals: "high" }]);
    rule = withCondition(rule, "level", "max");
    expect(rule.when.where).toEqual([{ field: "level", equals: "max" }]);
    expect(withoutCondition(rule, "level").when.where).toEqual([]);
    const field = get("example.counter.counter.changed").fields;
    expect(defaultConditionValue(field[0]!)).toBe(0);
    expect(defaultConditionValue(field[1]!)).toBe("zero");
  });

  it("holds for a time or until another event, with the runtime's default when none is set", () => {
    expect(holdOf(event())).toEqual({ kind: "for", ms: 3000 });
    expect(holdOf(event(), { maxRules: 32, maxLookName: 40, maxWhere: 8, defaultHoldMs: 5000, minHoldMs: 200, maxHoldMs: 60000 })).toEqual({ kind: "for", ms: 5000 });
    expect(holdOf(withHold(event(), { kind: "latch" }))).toEqual({ kind: "latch" });
    expect(withHold(event(), { kind: "for", ms: 700.4 }).hold).toEqual({ ms: 700 });
    expect(secondsText(700)).toBe("0.7 seconds");
    expect(secondsText(1000)).toBe("1 second");
    expect(secondsText(3000)).toBe("3 seconds");
  });

  it("points an event rule at another event and drops what the new event does not have", () => {
    const rule = { ...withCondition(event(), "level", "high"), showFromField: "level" };
    const same = retargetEvent(rule, get("example.counter.counter.changed"));
    expect(same.dropped).toBe(false);
    const other = retargetEvent(rule, get("example.channels.channel.changed"));
    expect(other.dropped).toBe(true);
    expect(other.rule.when).toEqual({ event: { provider: "example.channels", id: "channel.changed" }, where: [] });
    expect(other.rule.showFromField).toBeUndefined();
  });

  it("names a look one way only", () => {
    const rule = withLookTarget({ ...event(), show: "x" }, { kind: "field", field: "level" });
    expect(rule.show).toBeUndefined();
    expect(rule.showFromField).toBe("level");
    const state = withLookTarget(newStateRule(addressFor(level, {}), { op: "hasValue" }), { kind: "value" });
    expect(state.showFromValue).toBe(true);
    expect(withLookTarget(state, { kind: "look", name: "y" })).toMatchObject({ show: "y" });
    expect(withLookTarget(state, { kind: "look", name: "y" }).showFromValue).toBeUndefined();
    // A state rule has no payload field and an event no value: asking is refused quietly.
    expect(withLookTarget(newStateRule(addressFor(level, {}), { op: "hasValue" }), { kind: "field", field: "f" }).showFromField).toBeUndefined();
  });
});

describe("looks: the pad's named appearances", () => {
  it("turns a typed name into a key and a label back", () => {
    expect(lookKey("Needs approval!")).toBe("needs-approval");
    expect(lookKey("  Ünder  Review ")).toBe("under-review");
    expect(lookKey("x".repeat(80), 40)).toHaveLength(40);
    expect(lookLabel("needs-approval")).toBe("Needs approval");
    expect(lookLabel("approval_required")).toBe("Approval required");
    expect(lookLabel(IDLE)).toBe("Resting look");
  });

  it("refuses a name that is empty, taken, too long or the resting look's", () => {
    const control = blank({ states: { warning: { color: "#f59e0b", animation: "none" } } });
    expect(lookNameProblem(control, "  ")).toMatch(/Give the look a name/);
    expect(lookNameProblem(control, "Warning")).toMatch(/already a look called “Warning”/);
    expect(lookNameProblem(control, "Warning", "warning")).toBeNull(); // renaming it to itself
    expect(lookNameProblem(control, "Idle")).toMatch(/resting look/);
    expect(lookNameProblem(control, "y".repeat(41), undefined, 40)).toMatch(/at most 40/);
    expect(lookNameProblem(control, "Calm")).toBeNull();
  });

  it("starts a look in the colour its name says, and never overwrites one that exists", () => {
    expect(guessLookColor("Error")).toBe("#ef4444");
    expect(guessLookColor("Warning")).toBe("#f59e0b");
    expect(guessLookColor("Needs approval")).toBe("#a855f7");
    expect(guessLookColor("Offline")).toBe("#64748b");
    expect(guessLookColor("Active")).toBe("#22c55e");
    expect(guessLookColor("Something")).toBe("#3b82f6");
    const control = blank({ states: { warning: { color: "#111111", animation: "flash" } } });
    expect(createLook(control, "Warning").control.states.warning).toEqual({ color: "#111111", animation: "flash" });
    expect(createLook(control, "Calm", { animation: "pulse" }).control.states.calm).toEqual({ color: "#3b82f6", animation: "pulse" });
  });

  it("creates a look and shows it from one rule in one step", () => {
    const control = blank({ feedback: [newStateRule(addressFor(level, {}), { op: "equals", value: "max" })] });
    const next = createAndShow(control, 0, "Full tank", { color: "#abcdef" });
    expect(next.states["full-tank"]).toEqual({ color: "#abcdef", animation: "none" });
    expect(next.feedback[0]!.show).toBe("full-tank");
    expect(control.states).toEqual({}); // the draft it came from is untouched
    expect(showLook(next, 0, { kind: "none" }).feedback[0]!.show).toBeUndefined();
  });

  it("renames a look and the rules that show it, so nothing is left pointing at the old name", () => {
    const control = blank({
      states: { warn: { color: "#f59e0b", animation: "none" }, ok: { color: "#22c55e", animation: "none" } },
      feedback: [{ ...newStateRule(addressFor(level, {}), { op: "equals", value: "high" }), show: "warn" }, { ...newEventRule("example.counter", "counter.changed"), show: "warn" }, { ...newStateRule(addressFor(level, {}), { op: "equals", value: "low" }), show: "ok" }],
    });
    expect(lookUsage(control, "warn")).toEqual([0, 1]);
    const renamed = renameLook(control, "warn", "High fill");
    expect(renamed.key).toBe("high-fill");
    expect(Object.keys(renamed.control.states)).toEqual(["high-fill", "ok"]); // order kept
    expect(renamed.control.feedback.map((r) => r.show)).toEqual(["high-fill", "high-fill", "ok"]);
    // A name that is taken changes nothing.
    expect(renameLook(control, "warn", "ok")).toEqual({ control, key: "warn" });
  });

  it("removes a look and leaves the rules that showed it as they were, for the editor to report", () => {
    const control = blank({ states: { warn: { color: "#f59e0b", animation: "none" } }, feedback: [{ ...newStateRule(addressFor(level, {}), { op: "equals", value: "high" }), show: "warn" }] });
    const next = removeLook(control, "warn");
    expect(next.states).toEqual({});
    expect(next.feedback[0]!.show).toBe("warn");
  });
});

describe("what a refresh depends on", () => {
  const feedback: FeedbackRule[] = [{ ...newStateRule(addressFor(level, {}), { op: "equals", value: "low" }), show: "x" }];

  it("changes when a value that the rules read changes", () => {
    const before = readingsKey(catalog, feedback);
    const changed = applyDelta(catalog, {
      catalog: false,
      providers: [],
      observables: [{ id: level.id, availability: level.availability, instances: [{ args: {}, status: "known", value: "high", updatedAt: 1 }], instanceCount: 1 }],
    });
    expect(readingsKey(changed, feedback)).not.toBe(before);
  });

  it("does not change when something the rules do not read changes", () => {
    const before = readingsKey(catalog, feedback);
    const other = get("example.channels.status");
    const changed = applyDelta(catalog, {
      catalog: false,
      providers: [],
      observables: [{ id: other.id, availability: other.availability, instances: [{ args: { channel: "alpha" }, status: "known", value: "down", updatedAt: 9 }], instanceCount: 1 }],
    });
    expect(readingsKey(changed, feedback)).toBe(before);
  });

  it("notices a provider going away", () => {
    const before = readingsKey(catalog, feedback);
    const off = applyDelta(catalog, { catalog: false, providers: [], observables: [{ id: level.id, availability: { status: "unavailable", cause: "provider", reason: "off" }, instances: [], instanceCount: 0 }] });
    expect(readingsKey(off, feedback)).not.toBe(before);
    expect(readingsKey({ ...catalog, observables: [] }, feedback)).toBe("missing");
  });
});

describe("the explanation, arranged for the panel without adding anything", () => {
  const explain = (name: "missingLook" | "overlay" | "unknown") => controlExplanationSchema.parse(fixture.explain[name]);
  const counterRule = (value: string, show: string): StateRule => ({ ...newStateRule(addressFor(level, {}), { op: "equals", value }), show });

  it("reads a known value in the definition's words, the rule that decided and the missing look Rust found", () => {
    const view = viewExplanation(explain("missingLook"), [counterRule("low", "nope")], catalog);
    expect(view.readings).toEqual([expect.objectContaining({ title: "Counter Plugin / Counter level", status: "known", value: "Low", reason: "" })]);
    expect(view.matched).toEqual({ number: 1, text: "is Low" });
    expect(view.derived).toBe("nope");
    expect(view.missing).toEqual([{ rule: 1, look: "nope", label: "Rule 1" }]);
  });

  it("puts the temporary look over the state rules and counts its rule among the events", () => {
    const feedback: FeedbackRule[] = [counterRule("low", "low"), { ...newEventRule("example.counter", "counter.changed"), show: "blink" }];
    const view = viewExplanation(explain("overlay"), feedback, catalog);
    expect(view.layer).toBe("overlay");
    expect(view.overlay).toEqual({ look: "blink", rule: 1, remainingMs: 29743, latched: false, source: "event" });
    expect(view.shown).toBe("blink");
    expect(view.derived).toBe("low");
    expect(view.missing[0]).toMatchObject({ label: "Rule 1" });
  });

  it("gives the reason Rust gave for a state that is not known, and shows no value for it", () => {
    const feedback: FeedbackRule[] = [
      { ...newStateRule({ provider: "acme.missing", id: "thing" }, { op: "hasValue" }), show: "x" },
      { ...newStateRule(addressFor(channels, { channel: "zeta" }), { op: "hasValue" }), show: "x" },
      { ...newStateRule({ provider: "example.hello", id: "never" }, { op: "hasValue" }), show: "x" },
    ];
    const view = viewExplanation(explain("unknown"), feedback, catalog);
    expect(view.readings.map((r) => [r.status, r.value, r.reason])).toEqual([
      ["unavailable", "", "Nothing named `acme.missing` is installed."],
      ["unknown", "", "Channels is running but has not reported `status` for these arguments yet."],
      ["unavailable", "", "The plugin is turned off in Plugins."],
    ]);
    expect(view.readings[1]!.scope).toBe("channel zeta");
    expect(view.matched).toBeNull();
    expect(view.shown).toBeNull();
    expect(view.layer).toBe("rest");
  });
});

describe("what the editor produces is what the runtime was tested with", () => {
  // These two controls are also run by Rust against the real Counter and Channels plugins
  // (src-tauri/tests/feedback_editor.rs). If an editing function changes what it writes, one
  // side stops matching the other and a test here or there fails.
  const roundTrip = (c: Control) => controlSchema.parse(JSON.parse(JSON.stringify(c)));

  it("a person builds the Counter pad: Counter level → Resting look, Active, Warning, Error", () => {
    const address = addressFor(level, {});
    let draft: Control = controlSchema.parse({ ...counterFixture, states: {}, feedback: [] });
    const used: (string | number | boolean)[] = [];
    const look = ["idle", "Active", "Warning", "Error"];
    look.forEach((name, i) => {
      const test = defaultTest(level.value, used);
      used.push((test as { value: string }).value);
      const added = insertRule(draft.feedback, newStateRule(address, test), i === 0 ? undefined : i - 1);
      draft = { ...draft, feedback: added.feedback };
      draft = name === IDLE ? showLook(draft, added.index, { kind: "look", name: IDLE }) : createAndShow(draft, added.index, name);
    });
    expect(roundTrip(draft)).toEqual(controlSchema.parse(counterFixture));
    // And it is exactly what the schema takes: nothing the draft holds is lost on the way to a file.
    expect(draft.feedback.map((r) => feedbackRuleSchema.parse(r))).toEqual(draft.feedback);
  });

  it("a person builds the event pad: a scoped state, and an event that overlays Error for 0.7 seconds", () => {
    let draft: Control = controlSchema.parse({ ...eventFixture, states: {}, feedback: [] });
    const first = insertRule(draft.feedback, newStateRule(addressFor(channels, { channel: "alpha" }), defaultTest(channels.value)));
    draft = createAndShow({ ...draft, feedback: first.feedback }, first.index, "Active");
    const second = insertRule(draft.feedback, newEventRule("example.counter", "counter.changed"));
    draft = { ...draft, feedback: second.feedback };
    draft = { ...draft, feedback: draft.feedback.map((r, i) => (i === second.index ? withHold(withCondition(r as never, "level", "high"), { kind: "for", ms: 700 }) : r)) };
    draft = createAndShow(draft, second.index, "Error", { animation: "flash" });
    expect(roundTrip(draft)).toEqual(controlSchema.parse(eventFixture));
  });
});
