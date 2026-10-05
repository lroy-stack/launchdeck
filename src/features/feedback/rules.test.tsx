// @vitest-environment jsdom
import { act, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it } from "vitest";
import counterFixture from "../../../examples/feedback-editor-counter.json";
import eventFixture from "../../../examples/feedback-editor-event.json";
import { api } from "../../lib/ipc";
import { controlSchema, type Control, type FeedbackRule, type Observable, type ObservableCatalog } from "../../lib/schema";
import { useStore } from "../../stores/runtime";
import { FREE, clean, eventRule, inFeedback, openPad, review, rule, runtimeAnswers, savedAt, installEditorHarness } from "./testkit";

installEditorHarness();

const addFeedback = () => screen.getByRole("button", { name: /^\+ add feedback$/i });
const names = (select: HTMLElement) => within(select).getAllByRole("option").map((o) => o.textContent);
const select = (scope: HTMLElement, name: string) => within(scope).getByRole("combobox", { name }) as HTMLSelectElement;
const save = () => screen.getByRole("button", { name: /^(Add control|Save)$/ }) as HTMLButtonElement;

/** Add feedback from an observable found by its name, the way a person does it. */
async function addFrom(user: ReturnType<typeof userEvent.setup>, search: string, kind: "state" | "event" = "state") {
  await user.click(addFeedback());
  if (kind === "event") await user.click(screen.getByRole("button", { name: "An event" }));
  await user.type(screen.getByRole("combobox", { name: kind === "state" ? "Search states" : "Search events" }), search);
  await user.keyboard("{Enter}");
}

/** Choose “+ Create a new look…” on a rule, name it and make it. */
async function createLook(user: ReturnType<typeof userEvent.setup>, ruleEl: HTMLElement, label: string, name: string, extra?: (form: HTMLElement) => Promise<void>) {
  await user.selectOptions(select(ruleEl, label), "+ Create a new look…");
  const form = within(ruleEl).getByRole("group", { name: "New look" });
  await user.type(within(form).getByLabelText("Name of the new look"), name);
  await extra?.(form);
  await user.click(within(form).getByRole("button", { name: "Create look" }));
}

/** An observable that holds a number: nothing in the catalog does today, and the composer has to take one. */
function withNumberState(): Observable {
  const base = useStore.getState().observables;
  const level = base.observables.find((o) => o.id === "example.counter.level")!;
  const temperature: Observable = {
    ...level,
    id: "acme.thermo.celsius",
    provider: "acme.thermo",
    providerName: "Thermostat",
    local: "celsius",
    name: "Temperature",
    keywords: ["heat"],
    value: { type: "number", values: [] },
    instances: [{ args: {}, status: "known", value: 21, updatedAt: 1 }],
    instanceCount: 1,
  };
  const next: ObservableCatalog = {
    ...base,
    providers: [...base.providers, { id: "acme.thermo", name: "Thermostat", kind: "plugin", version: "1", description: "", status: "ready" }],
    observables: [...base.observables, temperature],
  };
  useStore.setState({ observables: next });
  return temperature;
}

describe("a state rule, as a sentence a person fills in", () => {
  it("starts from the state's first value and asks for the look, as something missing and not as an error", async () => {
    const user = userEvent.setup();
    openPad();
    await addFrom(user, "counter level");
    const r1 = rule(1);
    expect(select(r1, "Rule 1 condition").value).toBe("equals");
    expect(select(r1, "Rule 1 value").value).toBe("zero");
    expect(names(select(r1, "Rule 1 value"))).toEqual(["Empty", "Low", "High", "Full"]);
    // A prompt, not an alarm: nothing is marked invalid before the person had a chance.
    expect(within(r1).getByText("Choose the look to show.").className).toBe("needed");
    expect(select(r1, "Rule 1 look").getAttribute("aria-invalid")).toBeNull();
    expect(save().disabled).toBe(true);
    expect(screen.getByText("Finish the feedback rules marked (needed) to save.")).toBeTruthy();
  });

  it("shows the header of the state with what it reads now, and nothing technical", async () => {
    const user = userEvent.setup();
    openPad();
    await addFrom(user, "counter level");
    const head = inFeedback().getByRole("heading", { name: "Counter level" });
    const run = head.closest("section")!;
    expect(within(run).getByText("Known")).toBeTruthy();
    expect(run.textContent).toContain("Counter Plugin");
    expect(run.textContent).toContain("now Low");
    expect(run.textContent).not.toContain("example.counter");
  });

  it("offers exactly the runtime's operators that fit an enum, and none that compare sizes", async () => {
    const user = userEvent.setup();
    openPad();
    await addFrom(user, "counter level");
    expect(names(select(rule(1), "Rule 1 condition"))).toEqual(["is", "is not", "has a value", "has no value yet", "is not available"]);
  });

  it("takes the value away from an operator that has none and gives it back", async () => {
    const user = userEvent.setup();
    openPad();
    await addFrom(user, "counter level");
    await user.selectOptions(select(rule(1), "Rule 1 value"), "High");
    await user.selectOptions(select(rule(1), "Rule 1 condition"), "has a value");
    expect(within(rule(1)).queryByRole("combobox", { name: "Rule 1 value" })).toBeNull();
    await user.selectOptions(select(rule(1), "Rule 1 condition"), "is not");
    expect(select(rule(1), "Rule 1 value").value).toBe("zero");
  });

  it("takes a number for a state that holds one, offers the size comparisons, and will not save an empty one", async () => {
    const user = userEvent.setup();
    withNumberState();
    const answers = runtimeAnswers(() => clean);
    openPad();
    await addFrom(user, "temperature");
    const r1 = rule(1);
    expect(names(select(r1, "Rule 1 condition"))).toEqual(["is", "is not", "is below", "is at most", "is above", "is at least", "has a value", "has no value yet", "is not available"]);
    expect(select(r1, "Rule 1 condition").value).toBe("atLeast");
    const value = within(r1).getByRole("spinbutton", { name: "Rule 1 value" }) as HTMLInputElement;
    expect(value.value).toBe("0");
    await user.clear(value);
    expect(within(r1).getByText("Choose a value.")).toBeTruthy();
    expect(save().disabled).toBe(true);
    await user.type(value, "30");
    expect(value.value).toBe("30");
    await user.selectOptions(select(r1, "Rule 1 look"), "Resting look");
    await waitFor(() => expect(save().disabled).toBe(false));
    await user.click(save());
    await waitFor(() => expect(savedAt(FREE)).toBeTruthy());
    expect(savedAt(FREE)!.feedback[0]).toEqual({ when: { state: { provider: "acme.thermo", id: "celsius" }, is: { op: "atLeast", value: 30 } }, show: "idle" });
    // A number that was not a number yet never went to Rust.
    for (const [, , control] of answers.reviewFeedback.mock.calls) {
      for (const r of (control as Control).feedback) if ("is" in r.when && "value" in r.when.is) expect(Number.isFinite(r.when.is.value as number)).toBe(true);
    }
  });

  it("changes a comparison to a size comparison with a number the person can type", async () => {
    const user = userEvent.setup();
    withNumberState();
    openPad();
    await addFrom(user, "temperature");
    await user.selectOptions(select(rule(1), "Rule 1 condition"), "is below");
    const value = within(rule(1)).getByRole("spinbutton", { name: "Rule 1 value" });
    await user.clear(value);
    await user.type(value, "-3.5");
    await user.selectOptions(select(rule(1), "Rule 1 look"), "Resting look");
    await user.click(save());
    await waitFor(() => expect(savedAt(FREE)).toBeTruthy());
    expect(savedAt(FREE)!.feedback[0]).toMatchObject({ when: { is: { op: "lessThan", value: -3.5 } } });
  });
});

describe("building the Counter pad with the editor alone", () => {
  it("assigns Increment, adds the feedback the action suggests, shows four looks, and saves exactly the rules the runtime was tested with", async () => {
    const user = userEvent.setup();
    useStore.getState().select(FREE);
    openPad();
    // The Library gives the action to the pad; the Inspector puts it in the draft.
    act(() => {
      useStore.getState().requestAssign({ definitionId: "example.counter.counter.increment", slot: "press" });
    });
    await screen.findByText("Increment", { selector: "b, h4, span, strong" });
    // The action says what it usually shows, and nothing is added until the person chooses.
    const suggested = await screen.findByRole("group", { name: "Suggested feedback" });
    expect(within(suggested).getByText("Counter level")).toBeTruthy();
    expect(inFeedback().getByText("No feedback yet.")).toBeTruthy();
    await user.click(within(suggested).getByRole("button", { name: "Add feedback from Counter level" }));
    expect(screen.queryByRole("group", { name: "Suggested feedback" })).toBeNull(); // read now: no longer a suggestion

    await user.selectOptions(select(rule(1), "Rule 1 look"), "Resting look"); // Empty → the resting look
    for (let n = 2; n <= 4; n += 1) await user.click(inFeedback().getByRole("button", { name: "+ Add a rule" }));
    expect(inFeedback().getAllByRole("listitem", { name: /^Rule \d$/ })).toHaveLength(4);
    expect([1, 2, 3, 4].map((n) => select(rule(n), `Rule ${n} value`).value)).toEqual(["zero", "low", "high", "max"]);
    await createLook(user, rule(2), "Rule 2 look", "Active");
    await createLook(user, rule(3), "Rule 3 look", "Warning");
    await createLook(user, rule(4), "Rule 4 look", "Error");
    expect(screen.queryByText(/Finish the feedback rules/)).toBeNull();
    await waitFor(() => expect(save().disabled).toBe(false));
    await user.click(save());
    await waitFor(() => expect(savedAt(FREE)).toBeTruthy());
    const saved = savedAt(FREE)!;
    expect(saved.press).toEqual(counterFixture.press);
    expect(saved.feedback).toEqual(counterFixture.feedback);
    expect(saved.states).toEqual(counterFixture.states);
  });
});

/** The names of the states the rules read, in order: the headers of the runs, not of the live panel. */
const runHeads = () => [...feedbackRegion().querySelectorAll(".run-title h5")].map((h) => h.textContent);

function feedbackRegion() {
  return screen.getByRole("region", { name: "Feedback" });
}

describe("adding rules without disturbing the others", () => {
  it("adds a rule after the last of its state with the next value nobody tests, and leaves the order alone", async () => {
    const user = userEvent.setup();
    openPad();
    await addFrom(user, "counter level");
    await user.selectOptions(select(rule(1), "Rule 1 value"), "High");
    await user.click(inFeedback().getByRole("button", { name: "+ Add a rule" }));
    expect(select(rule(2), "Rule 2 value").value).toBe("zero"); // the first value nobody uses
    expect(select(rule(1), "Rule 1 value").value).toBe("high"); // still first
    await user.click(inFeedback().getByRole("button", { name: "+ Add a rule" }));
    expect(select(rule(3), "Rule 3 value").value).toBe("low");
    expect(select(rule(2), "Rule 2 value").value).toBe("zero");
  });

  it("adds a rule for each value that has none, with a look named after it, and says so", async () => {
    const user = userEvent.setup();
    openPad();
    await addFrom(user, "counter level");
    await user.selectOptions(select(rule(1), "Rule 1 look"), "Resting look");
    await user.click(inFeedback().getByRole("button", { name: "Add a rule for each of the 3 other values" }));
    expect([1, 2, 3, 4].map((n) => select(rule(n), `Rule ${n} value`).value)).toEqual(["zero", "low", "high", "max"]);
    expect([1, 2, 3, 4].map((n) => select(rule(n), `Rule ${n} look`).value)).toEqual(["look:idle", "look:low", "look:high", "look:full"]);
    expect(inFeedback().getByText(/Added 3 rules, each with a look of the value's name/)).toBeTruthy();
    // Nothing left to add: the button is gone.
    expect(inFeedback().queryByRole("button", { name: /Add a rule for/ })).toBeNull();
    // The new looks are listed and the first rule did not change.
    const looks = within(screen.getByRole("region", { name: "Looks" }));
    for (const name of ["Low", "High", "Full"]) expect(looks.getByText(name)).toBeTruthy();
  });

  it("adds the two rules about missing values once each, and each needs a look", async () => {
    const user = userEvent.setup();
    openPad();
    await addFrom(user, "counter level");
    await user.click(inFeedback().getByRole("button", { name: "+ When it has no value yet" }));
    expect(select(rule(2), "Rule 2 condition").value).toBe("noValue");
    expect(inFeedback().queryByRole("button", { name: "+ When it has no value yet" })).toBeNull();
    await user.click(inFeedback().getByRole("button", { name: "+ When it is not available" }));
    expect(select(rule(3), "Rule 3 condition").value).toBe("unavailable");
    expect(inFeedback().queryByRole("button", { name: "+ When it is not available" })).toBeNull();
    // Neither takes a value.
    expect(within(rule(3)).queryByRole("combobox", { name: "Rule 3 value" })).toBeNull();
  });

  it("joins a second pick of the same state to its rules and keeps a different state apart", async () => {
    const user = userEvent.setup();
    openPad();
    await addFrom(user, "counter level");
    await addFrom(user, "demo agent alpha");
    expect(runHeads()).toContain("Demo agent alpha");
    await addFrom(user, "counter level"); // the same state again
    const counter = inFeedback().getByRole("heading", { name: "Counter level" }).closest("section")!;
    expect(within(counter).getAllByRole("listitem", { name: /^Rule \d$/ })).toHaveLength(2);
    expect(select(within(counter).getAllByRole("listitem")[1]!, "Rule 2 value").value).toBe("low");
  });
});

describe("rules are numbered, in the order the runtime tries them, and move only when told to", () => {
  async function fourRules(user: ReturnType<typeof userEvent.setup>) {
    await addFrom(user, "counter level");
    await user.selectOptions(select(rule(1), "Rule 1 look"), "Resting look");
    await user.click(inFeedback().getByRole("button", { name: "Add a rule for each of the 3 other values" }));
  }
  const values = () => [1, 2, 3, 4].map((n) => select(rule(n), `Rule ${n} value`).value);

  it("moves a rule up with its button, says where it went and keeps the cursor on the same button", async () => {
    const user = userEvent.setup();
    openPad();
    await fourRules(user);
    expect(values()).toEqual(["zero", "low", "high", "max"]);
    await user.click(within(rule(3)).getByRole("button", { name: "Move rule 3 up" }));
    expect(values()).toEqual(["zero", "high", "low", "max"]);
    expect(inFeedback().getByText("Rule moved up: it is now rule 2 of 4.")).toBeTruthy();
    expect(document.activeElement).toBe(within(rule(2)).getByRole("button", { name: "Move rule 2 up" }));
    // Again: the look of each rule went with it.
    await user.keyboard("{Enter}");
    expect(values()).toEqual(["high", "zero", "low", "max"]);
    expect(select(rule(1), "Rule 1 look").value).toBe("look:high");
    expect(document.activeElement).toBe(within(rule(1)).getByRole("button", { name: "Move rule 1 up" }));
  });

  it("moves a rule down with its button", async () => {
    const user = userEvent.setup();
    openPad();
    await fourRules(user);
    await user.click(within(rule(1)).getByRole("button", { name: "Move rule 1 down" }));
    expect(values()).toEqual(["low", "zero", "high", "max"]);
    expect(document.activeElement).toBe(within(rule(2)).getByRole("button", { name: "Move rule 2 down" }));
  });

  it("moves with Alt+↑ and Alt+↓ from any field of the rule, and the cursor follows the rule", async () => {
    const user = userEvent.setup();
    openPad();
    await fourRules(user);
    const condition = select(rule(4), "Rule 4 condition");
    condition.focus();
    await user.keyboard("{Alt>}{ArrowUp}{/Alt}");
    expect(values()).toEqual(["zero", "low", "max", "high"]);
    expect(document.activeElement).toBe(select(rule(3), "Rule 3 condition"));
    await user.keyboard("{Alt>}{ArrowUp}{ArrowUp}{/Alt}");
    expect(values()).toEqual(["max", "zero", "low", "high"]);
    expect(document.activeElement).toBe(select(rule(1), "Rule 1 condition"));
    await user.keyboard("{Alt>}{ArrowDown}{/Alt}");
    expect(values()).toEqual(["zero", "max", "low", "high"]);
  });

  it("says so, and changes nothing, at either end", async () => {
    const user = userEvent.setup();
    openPad();
    await fourRules(user);
    const up = within(rule(1)).getByRole("button", { name: "Move rule 1 up" });
    expect(up.getAttribute("aria-disabled")).toBe("true");
    await user.click(up);
    expect(inFeedback().getByText("Rule 1 is already the first.")).toBeTruthy();
    await user.click(within(rule(4)).getByRole("button", { name: "Move rule 4 down" }));
    expect(inFeedback().getByText("Rule 4 is already the last.")).toBeTruthy();
    expect(values()).toEqual(["zero", "low", "high", "max"]);
  });

  it("removes a rule, renumbers the rest and puts the cursor on the one that took its place", async () => {
    const user = userEvent.setup();
    openPad();
    await fourRules(user);
    await user.click(within(rule(2)).getByRole("button", { name: "Remove rule 2" }));
    expect(inFeedback().getAllByRole("listitem", { name: /^Rule \d$/ })).toHaveLength(3);
    expect([1, 2, 3].map((n) => select(rule(n), `Rule ${n} value`).value)).toEqual(["zero", "high", "max"]);
    expect(document.activeElement).toBe(within(rule(2)).getByRole("button", { name: "Remove rule 2" }));
    // The last one: the cursor goes to the one before it.
    await user.click(within(rule(3)).getByRole("button", { name: "Remove rule 3" }));
    expect(document.activeElement).toBe(within(rule(2)).getByRole("button", { name: "Remove rule 2" }));
  });

  it("saves in the order shown", async () => {
    const user = userEvent.setup();
    openPad();
    await fourRules(user);
    await user.click(within(rule(4)).getByRole("button", { name: "Move rule 4 up" }));
    await user.click(within(rule(1)).getByRole("button", { name: "Move rule 1 down" }));
    const shown = values();
    await user.click(save());
    await waitFor(() => expect(savedAt(FREE)).toBeTruthy());
    expect(savedAt(FREE)!.feedback.map((r) => ("is" in r.when && "value" in r.when.is ? r.when.is.value : null))).toEqual(shown);
  });

  it("explains how the order works, where the rules are", async () => {
    const user = userEvent.setup();
    openPad();
    await fourRules(user);
    expect(inFeedback().getByText("Rules are tried from 1 down. The first one that is true decides the look.")).toBeTruthy();
    expect(inFeedback().getByText(/If none is true, the pad shows its resting look/)).toBeTruthy();
  });
});

describe("what the runtime says about a draft", () => {
  it("shows what it refuses next to the field it is about, blocks saving and keeps what the person typed", async () => {
    const user = userEvent.setup();
    runtimeAnswers((c) =>
      c.feedback.length === 0
        ? clean
        : review({ issues: [{ rule: 0, field: "test", message: "`Counter level` is not a number, so it cannot be compared with below, above or at" }] }),
    );
    openPad();
    await addFrom(user, "counter level");
    await user.selectOptions(select(rule(1), "Rule 1 look"), "Resting look");
    const message = await within(rule(1)).findByText(/is not a number/);
    expect(message.className).toBe("field-error");
    expect(select(rule(1), "Rule 1 condition").getAttribute("aria-invalid")).toBe("true");
    expect(select(rule(1), "Rule 1 condition").getAttribute("aria-describedby")).toBe(message.parentElement!.id);
    expect(save().disabled).toBe(true);
    expect(screen.getByText("Fix the highlighted feedback rules to save.")).toBeTruthy();
    expect(select(rule(1), "Rule 1 look").value).toBe("look:idle");
  });

  it("says a rule is never reached when Rust has proved another covers it", async () => {
    const user = userEvent.setup();
    runtimeAnswers((c) => (c.feedback.length < 2 ? clean : review({ notes: [{ kind: "shadowed", rule: 1, by: 0 }] })));
    openPad();
    await addFrom(user, "counter level");
    await user.selectOptions(select(rule(1), "Rule 1 condition"), "has a value");
    await user.click(inFeedback().getByRole("button", { name: "+ Add a rule" }));
    expect(await within(rule(2)).findByText(/Never used: rule 1 is tried first and is true whenever this one is/)).toBeTruthy();
    expect(within(rule(1)).queryByText(/Never used/)).toBeNull();
  });

  it("offers a way out of a rule that can never be reached: move it, and the note goes", async () => {
    const user = userEvent.setup();
    runtimeAnswers((c) => {
      const first = c.feedback[0];
      const covers = first && "is" in first.when && first.when.is.op === "hasValue";
      return c.feedback.length >= 2 && covers ? review({ notes: [{ kind: "shadowed", rule: 1, by: 0 }] }) : clean;
    });
    openPad();
    await addFrom(user, "counter level");
    await user.selectOptions(select(rule(1), "Rule 1 condition"), "has a value");
    await user.click(inFeedback().getByRole("button", { name: "+ Add a rule" }));
    await within(rule(2)).findByText(/Never used/);
    await user.click(within(rule(2)).getByRole("button", { name: "Move rule 2 up" }));
    await waitFor(() => expect(inFeedback().queryByText(/Never used/)).toBeNull());
  });
});

describe("saving", () => {
  it("keeps Save for a finished draft, and Revert goes back to what is saved", async () => {
    const user = userEvent.setup();
    openPad();
    expect(save().disabled).toBe(true);
    await addFrom(user, "counter level");
    expect(screen.getByText("Not saved")).toBeTruthy();
    await user.click(screen.getByRole("button", { name: "Revert" }));
    expect(inFeedback().getByText("No feedback yet.")).toBeTruthy();
    expect(screen.queryByText("Not saved")).toBeNull();
  });

  it("writes only the rules and looks: no value, no status, nothing the runtime reads", async () => {
    const user = userEvent.setup();
    openPad();
    await addFrom(user, "counter level");
    await user.selectOptions(select(rule(1), "Rule 1 look"), "Resting look");
    await user.click(save());
    await waitFor(() => expect(savedAt(FREE)).toBeTruthy());
    const saved = savedAt(FREE)!;
    expect(Object.keys(saved).sort()).toEqual(["description", "feedback", "icon", "id", "label", "legacyBindings", "longPress", "pos", "press", "span", "states", "visual"].filter((k) => k in saved).sort());
    const text = JSON.stringify(saved);
    for (const word of ["known", "updatedAt", "instances", "availability", "observed", "providerName"]) expect(text).not.toContain(word);
    expect(saved.feedback).toEqual([{ when: { state: { provider: "example.counter", id: "level" }, is: { op: "equals", value: "zero" } }, show: "idle" }]);
  });

  it("opens a saved pad with its rules as they were saved, numbered, with scope and order intact", async () => {
    const user = userEvent.setup();
    const control: Control = controlSchema.parse({
      ...counterFixture,
      id: "g.7.7",
      pos: FREE,
      label: "Saved",
      press: null,
      feedback: [
        { when: { state: { provider: "example.channels", id: "status", args: { channel: "beta" } }, is: { op: "equals", value: "down" } }, show: "error" },
        { when: { state: { provider: "example.counter", id: "level" }, is: { op: "equals", value: "high" } }, show: "warning" },
        { when: { state: { provider: "example.channels", id: "status", args: { channel: "alpha" } }, is: { op: "notEquals", value: "up" } }, show: "active" },
      ],
    });
    await api.updateControl("development", "home", control);
    openPad();
    expect(save().disabled).toBe(true); // nothing changed
    expect(select(rule(1), "Rule 1 value").value).toBe("down");
    expect(select(rule(2), "Rule 2 value").value).toBe("high");
    expect(select(rule(3), "Rule 3 condition").value).toBe("notEquals");
    // Neighbours that read different states are not merged: the order is the order saved.
    const heads = runHeads();
    expect(heads).toEqual(["Channel status", "Counter level", "Channel status"]);
    expect(inFeedback().getByText(/channel beta/)).toBeTruthy();
    expect(inFeedback().getByText(/channel alpha/)).toBeTruthy();
    await user.click(within(rule(1)).getByRole("button", { name: "Move rule 1 down" }));
    await user.click(save());
    await waitFor(() => expect(savedAt(FREE)!.feedback[0]).toMatchObject({ when: { state: { id: "level" } } }));
    expect(savedAt(FREE)!.feedback[1]).toEqual(control.feedback[0]);
    expect(savedAt(FREE)!.feedback[2]).toEqual(control.feedback[2]);
  });
});

describe("a rule whose state is gone", () => {
  const orphan: FeedbackRule = { when: { state: { provider: "acme.gone", id: "thing" }, is: { op: "equals", value: "running" } }, show: "busy" };
  const saved = async (feedback: FeedbackRule[]) =>
    api.updateControl("development", "home", controlSchema.parse({ ...counterFixture, id: "g.7.7", pos: FREE, label: "Orphan", press: null, states: { busy: { color: "#f59e0b", animation: "none" } }, feedback }));

  it("is kept as it is, said in words, with a way to replace or remove it", async () => {
    await saved([orphan]);
    runtimeAnswers(() => review({ notes: [{ kind: "missingObservable", rule: 0, observableKind: "state", provider: "acme.gone", id: "thing" }] }));
    openPad();
    const run = inFeedback().getByRole("heading", { name: "A state that is no longer offered" }).closest("section")!;
    expect(run.textContent).toMatch(/No installed provider offers “acme\.gone · thing” any more/);
    // The rule is still there, editable where it can be: it is read as a sentence and keeps its look.
    expect(within(run).getByText("is running")).toBeTruthy();
    expect(select(rule(1), "Rule 1 look").value).toBe("look:busy");
    expect(within(run).getByRole("button", { name: "Replace state…" })).toBeTruthy();
    expect(within(run).getByRole("button", { name: "Remove these rules" })).toBeTruthy();
    expect(save().disabled).toBe(true); // nothing was changed, so nothing to save
  });

  it("can be renamed or moved without losing it", async () => {
    const user = userEvent.setup();
    await saved([orphan]);
    openPad();
    await user.type(screen.getByLabelText("Label", { exact: true }), "!");
    await user.click(save());
    await waitFor(() => expect(savedAt(FREE)!.label).toBe("Orphan!"));
    expect(savedAt(FREE)!.feedback).toEqual([orphan]);
  });

  it("is replaced by another state, and a condition that does not fit it is reset and said so", async () => {
    const user = userEvent.setup();
    await saved([orphan]);
    openPad();
    await user.click(inFeedback().getByRole("button", { name: "Replace state…" }));
    await user.type(screen.getByRole("combobox", { name: "Search states" }), "counter level");
    await user.keyboard("{Enter}");
    expect(inFeedback().getByRole("heading", { name: "Counter level" })).toBeTruthy();
    expect(select(rule(1), "Rule 1 value").value).toBe("zero");
    expect(inFeedback().getByText(/Now following “Counter level”\. One condition did not fit it and was reset: check it\./)).toBeTruthy();
    expect(select(rule(1), "Rule 1 look").value).toBe("look:busy");
  });

  it("is removed, and the cursor goes to the way of adding feedback", async () => {
    const user = userEvent.setup();
    await saved([orphan]);
    openPad();
    await user.click(inFeedback().getByRole("button", { name: "Remove these rules" }));
    expect(inFeedback().getByText("No feedback yet.")).toBeTruthy();
    expect(document.activeElement).toBe(addFeedback());
    expect(inFeedback().getByText("Removed the rule for a state that is no longer offered.")).toBeTruthy();
  });

  it("says when a provider is there but stopped, with the reason Rust gave", async () => {
    await saved([{ when: { state: { provider: "example.counter", id: "level" }, is: { op: "equals", value: "low" } }, show: "busy" }]);
    runtimeAnswers(() => review({ notes: [{ kind: "providerUnavailable", rule: 0, provider: "example.counter", providerName: "Counter Plugin", reason: "The plugin is turned off in Plugins." }] }));
    const level = useStore.getState().observables.observables.find((o) => o.id === "example.counter.level")!;
    useStore.setState({
      observables: {
        ...useStore.getState().observables,
        observables: useStore.getState().observables.observables.map((o) => (o === level ? { ...level, instances: [], instanceCount: 0, availability: { status: "unavailable" as const, cause: "provider" as const, reason: "The plugin is turned off in Plugins." } } : o)),
      },
    });
    openPad();
    const run = inFeedback().getByRole("heading", { name: "Counter level" }).closest("section")!;
    expect(within(run).getByText("Unavailable")).toBeTruthy();
    expect(await within(run).findByText("Counter Plugin is not running: The plugin is turned off in Plugins.")).toBeTruthy();
    // The rules still work as rules: nothing was dropped.
    expect(select(rule(1), "Rule 1 value").value).toBe("low");
  });
});

describe("an event rule", () => {
  async function counterChanged(user: ReturnType<typeof userEvent.setup>) {
    await addFrom(user, "counter changed", "event");
  }

  it("is numbered apart from the state rules and says what it does in order", async () => {
    const user = userEvent.setup();
    openPad();
    await addFrom(user, "counter level");
    await counterChanged(user);
    expect(rule(1)).toBeTruthy();
    expect(eventRule(1)).toBeTruthy();
    expect(inFeedback().getByText("Reacts to an event")).toBeTruthy();
    expect(within(eventRule(1)).getByText("Counter changed")).toBeTruthy();
    expect(within(eventRule(1)).queryByRole("combobox", { name: /condition: / })).toBeNull(); // no condition yet
  });

  it("builds the event pad: a scoped state, and an event that shows Error for 0.7 seconds, and saves what the runtime was tested with", async () => {
    const user = userEvent.setup();
    openPad();
    await user.click(addFeedback());
    await user.type(screen.getByRole("combobox", { name: "Search states" }), "channel");
    await user.keyboard("{Enter}");
    await user.type(screen.getByLabelText(/^Channel/), "alpha");
    await user.click(screen.getByRole("button", { name: "Add the first rule" }));
    await createLook(user, rule(1), "Rule 1 look", "Active");

    await counterChanged(user);
    const ev = eventRule(1);
    await user.selectOptions(select(ev, "Event rule 1: add a condition"), "Level");
    await user.selectOptions(select(ev, "Event rule 1 condition: Level"), "High");
    expect(select(ev, "Event rule 1 duration").value).toBe("for");
    // The bounds are the runtime's limits, which arrive with its review once the draft has been still for a moment.
    const seconds = await waitFor(() => {
      const el = within(ev).getByRole("spinbutton", { name: "Event rule 1 seconds" }) as HTMLInputElement;
      expect(el.min).toBe("0.2");
      return el;
    });
    expect(seconds.value).toBe("3");
    expect(seconds.max).toBe("60");
    await user.clear(seconds);
    await user.type(seconds, "0.7");
    expect(within(ev).getByText("Shown for 0.7 seconds, then the pad goes back to what its state rules say.")).toBeTruthy();
    await createLook(user, ev, "Event rule 1 look", "Error", async (form) => {
      await user.selectOptions(within(form).getByLabelText("New look animation"), "Flash");
    });
    await user.clear(screen.getByLabelText("Label", { exact: true }));
    await user.type(screen.getByLabelText("Label", { exact: true }), "Alert");
    await waitFor(() => expect(save().disabled).toBe(false));
    await user.click(save());
    await waitFor(() => expect(savedAt(FREE)).toBeTruthy());
    expect(savedAt(FREE)!.feedback).toEqual(eventFixture.feedback);
    expect(savedAt(FREE)!.states).toEqual(eventFixture.states);
  });

  it("can keep its look until the next event, and says so", async () => {
    const user = userEvent.setup();
    openPad();
    await counterChanged(user);
    await user.selectOptions(select(eventRule(1), "Event rule 1 duration"), "Until another event");
    expect(within(eventRule(1)).queryByRole("spinbutton", { name: "Event rule 1 seconds" })).toBeNull();
    expect(within(eventRule(1)).getByText(/stays until another event replaces it/)).toBeTruthy();
    await user.selectOptions(select(eventRule(1), "Event rule 1 look"), "Resting look");
    await user.click(save());
    await waitFor(() => expect(savedAt(FREE)).toBeTruthy());
    expect(savedAt(FREE)!.feedback[0]).toMatchObject({ hold: "latch", show: "idle" });
  });

  it("takes a condition on a field with listed values from the list, and one on a number from a number", async () => {
    const user = userEvent.setup();
    openPad();
    await counterChanged(user);
    await user.selectOptions(select(eventRule(1), "Event rule 1: add a condition"), "Value");
    const value = within(eventRule(1)).getByRole("spinbutton", { name: "Event rule 1 condition: Value" });
    await user.clear(value);
    await user.type(value, "5");
    await user.selectOptions(select(eventRule(1), "Event rule 1: add a condition"), "Level");
    expect(names(select(eventRule(1), "Event rule 1 condition: Level"))).toEqual(["Empty", "Low", "High", "Full"]);
    // Every field used: nothing more to add.
    expect(within(eventRule(1)).queryByRole("combobox", { name: "Event rule 1: add a condition" })).toBeNull();
    await user.click(within(eventRule(1)).getByRole("button", { name: "Remove the condition on Level" }));
    expect(within(eventRule(1)).queryByRole("combobox", { name: "Event rule 1 condition: Level" })).toBeNull();
    expect(select(eventRule(1), "Event rule 1: add a condition")).toBeTruthy();
  });

  it("is replaced by another event, and conditions it does not have are dropped with a word about it", async () => {
    const user = userEvent.setup();
    openPad();
    await counterChanged(user);
    await user.selectOptions(select(eventRule(1), "Event rule 1: add a condition"), "Level");
    await user.click(within(eventRule(1)).getByRole("button", { name: "Change event…" }));
    await user.type(screen.getByRole("combobox", { name: "Search events" }), "channel changed");
    await user.keyboard("{Enter}");
    expect(within(eventRule(1)).getByText("Channel changed")).toBeTruthy();
    expect(within(eventRule(1)).queryByRole("combobox", { name: /condition: Level/ })).toBeNull();
    expect(inFeedback().getByText(/Now listening to “Channel changed”\. Conditions or a look from fields it does not have were removed: check them\./)).toBeTruthy();
  });

  it("is reordered among events only", async () => {
    const user = userEvent.setup();
    openPad();
    await addFrom(user, "counter level");
    await counterChanged(user);
    await addFrom(user, "channel changed", "event");
    expect(within(eventRule(1)).getByText("Counter changed")).toBeTruthy();
    await user.click(within(eventRule(2)).getByRole("button", { name: "Move event rule 2 up" }));
    expect(within(eventRule(1)).getByText("Channel changed")).toBeTruthy();
    expect(within(eventRule(2)).getByText("Counter changed")).toBeTruthy();
    expect(inFeedback().getByText("Event rule moved up: it is now event rule 1 of 2.")).toBeTruthy();
    // The state rule did not move.
    expect(select(rule(1), "Rule 1 value").value).toBe("zero");
  });

  it("keeps an event whose provider is stopped, in words", async () => {
    const user = userEvent.setup();
    openPad();
    await addFrom(user, "hello", "event");
    const card = eventRule(1);
    expect(within(card).getByText("Unavailable")).toBeTruthy();
    expect(within(card).getByText("hello.completed")).toBeTruthy();
  });

  it("holds for a time the runtime accepts: the limits come from Rust", async () => {
    const user = userEvent.setup();
    runtimeAnswers((c) => (c.feedback.length === 0 ? clean : review({ limits: { ...clean.limits, minHoldMs: 500, maxHoldMs: 10000, defaultHoldMs: 2000 } })));
    openPad();
    await counterChanged(user);
    const seconds = await waitFor(() => {
      const el = within(eventRule(1)).getByRole("spinbutton", { name: "Event rule 1 seconds" }) as HTMLInputElement;
      expect(el.min).toBe("0.5");
      return el;
    });
    expect(seconds.max).toBe("10");
    await user.selectOptions(select(eventRule(1), "Event rule 1 duration"), "Until another event");
    await user.selectOptions(select(eventRule(1), "Event rule 1 duration"), "For a time");
    expect((within(eventRule(1)).getByRole("spinbutton", { name: "Event rule 1 seconds" }) as HTMLInputElement).value).toBe("2");
  });
});

describe("the limits of a pad", () => {
  it("stops adding at the rule limit the runtime gave, and says why", async () => {
    const user = userEvent.setup();
    runtimeAnswers((c) => review({ limits: { ...clean.limits, maxRules: c.feedback.length >= 1 ? 2 : 32 } }));
    openPad();
    await addFrom(user, "counter level");
    await user.click(await waitFor(() => inFeedback().getByRole("button", { name: "+ Add a rule" })));
    await waitFor(() => expect((inFeedback().getByRole("button", { name: "+ Add feedback" }) as HTMLButtonElement).disabled).toBe(true));
    expect(inFeedback().getAllByText(/A pad can have at most 2 rules/).length).toBeGreaterThan(0);
  });
});
