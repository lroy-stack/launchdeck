// @vitest-environment jsdom
import { act, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it } from "vitest";
import { api } from "../../lib/ipc";
import { applyDelta } from "../../lib/observables";
import { controlSchema, type FeedbackRule, type Visual } from "../../lib/schema";
import { useStore } from "../../stores/runtime";
import { FREE, clean, explained, installEditorHarness, openPad, review, runtimeAnswers } from "./testkit";

installEditorHarness();

const level = (value: string, show: string): FeedbackRule => ({ when: { state: { provider: "example.counter", id: "level" }, is: { op: "equals", value } }, show });
const event = (show: string): FeedbackRule => ({ when: { event: { provider: "example.counter", id: "counter.changed" }, where: [] }, show, hold: { ms: 3000 } });
const look = (color: string): Visual => ({ color, animation: "none" });

const saveWith = (feedback: FeedbackRule[], states: Record<string, Visual> = {}) =>
  api.updateControl("development", "home", controlSchema.parse({ id: "g.7.7", pos: FREE, label: "Pad", visual: { color: "#2f81f7", animation: "none" }, states, feedback }));

const panel = () => screen.findByRole("region", { name: /Live feedback|Feedback if you save now/ });
const row = (scope: HTMLElement, term: string): string => {
  const dt = [...scope.querySelectorAll("dt")].find((d) => d.textContent?.startsWith(term));
  return (dt?.nextElementSibling as HTMLElement | null)?.textContent ?? "";
};

describe("the live panel", () => {
  it("is not there for a pad with no feedback, and not asked for", async () => {
    const asked = runtimeAnswers(() => clean);
    openPad();
    expect(screen.queryByRole("region", { name: /Live feedback/ })).toBeNull();
    await new Promise((r) => setTimeout(r, 300));
    expect(asked.reviewFeedback).not.toHaveBeenCalled();
    expect(asked.explainControl).not.toHaveBeenCalled();
  });

  it("says, from Rust, what the state reads, which rule decided and which look is on the pad", async () => {
    await saveWith([level("low", "nope")], {});
    runtimeAnswers(() => clean, () => explained("missingLook"));
    openPad();
    const live = await panel();
    await within(live).findByText("Low");
    expect(within(live).getByRole("heading", { name: "Live feedback" })).toBeTruthy();
    expect(within(live).getByText("Counter Plugin / Counter level")).toBeTruthy();
    expect(within(live).getByText("Known")).toBeTruthy();
    expect(row(live, "Rule that decided")).toBe("Rule 1: it is Low");
    expect(row(live, "Look from the state rules")).toBe("Nope");
    expect(row(live, "On the pad")).toBe("Nope");
    // Rust found that the look is not there, and says what that means for the pad.
    expect(within(live).getByText(/Rule 1 chose the look “nope”, which this pad does not have, so the pad rests/)).toBeTruthy();
  });

  it("says a state is unknown or unavailable, and why, in the words Rust gave", async () => {
    const rules = [
      { when: { state: { provider: "acme.missing", id: "thing" }, is: { op: "hasValue" as const } }, show: "x" },
      { when: { state: { provider: "example.channels", id: "status", args: { channel: "zeta" } }, is: { op: "hasValue" as const } }, show: "x" },
      { when: { state: { provider: "example.hello", id: "never" }, is: { op: "hasValue" as const } }, show: "x" },
    ];
    await saveWith(rules, { x: look("#ff0000") });
    runtimeAnswers(() => clean, () => explained("unknown"));
    openPad();
    const live = await panel();
    await within(live).findByText("Nothing named `acme.missing` is installed.");
    const states = [...live.querySelectorAll(".live-reading dd")].map((d) => d.textContent);
    expect(states).toEqual(["Unavailable Nothing named `acme.missing` is installed.", "Unknown Channels is running but has not reported `status` for these arguments yet.", "Unavailable The plugin is turned off in Plugins."]);
    expect(within(live).getByText(/channel zeta/)).toBeTruthy();
    // No rule is true, so there is no deciding rule and the pad rests. No value is invented for any of them.
    expect(row(live, "Rule that decided")).toBe("None is true, so the pad rests.");
    expect(row(live, "On the pad")).toBe("Resting look");
    expect(live.textContent).not.toMatch(/Known/);
  });

  it("shows the temporary look an event put on the pad, over what the state rules say, with how long is left", async () => {
    await saveWith([level("low", "low"), event("blink")], { low: look("#22c55e"), blink: look("#ef4444") });
    runtimeAnswers(() => clean, () => explained("overlay"));
    openPad();
    const live = await panel();
    await within(live).findByText(/from event rule 1/);
    expect(row(live, "Temporary look")).toContain("Blink");
    expect(row(live, "Temporary look")).toContain("from event rule 1, 29.7 seconds left");
    expect(row(live, "Look from the state rules")).toBe("Low");
    expect(row(live, "On the pad")).toContain("Blink");
    expect(row(live, "On the pad")).toContain("the temporary look is over the state rules");
  });

  it("keeps technical addresses out of sight until asked", async () => {
    await saveWith([level("low", "nope")], {});
    runtimeAnswers(() => clean, () => explained("missingLook"));
    openPad();
    const live = await panel();
    await within(live).findByText("Low");
    const details = live.querySelector("details") as HTMLDetailsElement;
    expect(details.open).toBe(false);
    expect(within(details).getByText("example.counter.level")).toBeTruthy();
    expect(within(details).getByText("Technical details")).toBeTruthy();
  });

  it("says the explanation comes from the desktop app when the runtime is not there, and does not break", async () => {
    await saveWith([level("low", "nope")], {});
    openPad(); // the preview transport has no runtime: explain_control refuses
    const live = await panel();
    expect(await within(live).findByText(/Explanations come from the desktop app/)).toBeTruthy();
    // The rules themselves are still there to edit.
    expect(screen.getByRole("listitem", { name: "Rule 1" })).toBeTruthy();
  });

  it("explains the draft, as a draft, once its rules differ from what is saved, and does not claim it is live", async () => {
    const user = userEvent.setup();
    await saveWith([level("low", "low")], { low: look("#22c55e") });
    const asked = runtimeAnswers(
      (c) => {
        const changed = JSON.stringify(c.feedback) !== JSON.stringify([level("low", "low")]);
        return changed ? review({ preview: explained("overlay", { overlay: null, layer: "state", shown: "low", diagnostics: [] }) }) : clean;
      },
      () => explained("overlay"),
    );
    openPad();
    const live = await panel();
    await within(live).findByText(/Temporary look/);
    expect(within(live).getByRole("heading", { name: "Live feedback" })).toBeTruthy();
    const before = asked.explainControl.mock.calls.length;
    await user.selectOptions(screen.getByRole("combobox", { name: "Rule 1 value" }), "High");
    const draft = await screen.findByRole("region", { name: "Feedback if you save now" });
    expect(within(draft).getByText("Draft")).toBeTruthy();
    expect(within(draft).getByText(/Temporary looks from events are not part of it/)).toBeTruthy();
    // The temporary look of the saved pad is not shown for a draft.
    expect(within(draft).queryByText("Temporary look")).toBeNull();
    expect(asked.explainControl.mock.calls.length).toBe(before);
    // Back to the saved rules: live again.
    await user.selectOptions(screen.getByRole("combobox", { name: "Rule 1 value" }), "Low");
    await screen.findByRole("region", { name: "Live feedback" });
  });
});

describe("keeping the panel current without asking all the time", () => {
  const settle = () => act(async () => void (await new Promise((r) => setTimeout(r, 400))));
  const level0 = () => useStore.getState().observables.observables.find((o) => o.id === "example.counter.level")!;
  const push = (id: string, value: string, args: Record<string, string> = {}) =>
    act(() => {
      const o = useStore.getState().observables.observables.find((x) => x.id === id)!;
      useStore.setState({
        observables: applyDelta(useStore.getState().observables, {
          catalog: false,
          providers: [],
          observables: [{ id, availability: o.availability, instances: [{ args, status: "known", value, updatedAt: Date.now() }], instanceCount: 1 }],
        }),
      });
    });

  it("asks again when a value the rules read changes, and shows the new answer", async () => {
    await saveWith([level("low", "nope")], {});
    let current = "Low";
    const asked = runtimeAnswers(
      () => clean,
      () => {
        const ex = explained("missingLook");
        ex.rules[0]!.valueLabel = current;
        return ex;
      },
    );
    openPad();
    const live = await panel();
    await within(live).findByText("Low");
    const calls = asked.explainControl.mock.calls.length;
    current = "High";
    push(level0().id, "high");
    await within(live).findByText("High");
    expect(asked.explainControl.mock.calls.length).toBe(calls + 1);
  });

  it("does not ask again for a value that nothing on this pad reads", async () => {
    await saveWith([level("low", "nope")], {});
    const asked = runtimeAnswers(() => clean, () => explained("missingLook"));
    openPad();
    const live = await panel();
    await within(live).findByText("Low");
    await settle();
    const [reviews, explains] = [asked.reviewFeedback.mock.calls.length, asked.explainControl.mock.calls.length];
    for (const v of ["down", "up", "down", "up"]) push("example.channels.status", v, { channel: "alpha" });
    await settle();
    expect(asked.reviewFeedback.mock.calls.length).toBe(reviews);
    expect(asked.explainControl.mock.calls.length).toBe(explains);
  });

  it("does not ask for what typing a label or a description changes", async () => {
    const user = userEvent.setup();
    await saveWith([level("low", "nope")], {});
    const asked = runtimeAnswers(() => clean, () => explained("missingLook"));
    openPad();
    await within(await panel()).findByText("Low");
    await settle();
    const reviews = asked.reviewFeedback.mock.calls.length;
    await user.type(screen.getByLabelText("Label", { exact: true }), "abc");
    await settle();
    expect(asked.reviewFeedback.mock.calls.length).toBe(reviews);
  });

  it("asks once for a burst of edits, not once for each key", async () => {
    const user = userEvent.setup();
    await saveWith([level("low", "nope")], {});
    const asked = runtimeAnswers(() => clean, () => explained("missingLook"));
    openPad();
    await within(await panel()).findByText("Low");
    await settle();
    const reviews = asked.reviewFeedback.mock.calls.length;
    const value = screen.getByRole("combobox", { name: "Rule 1 value" });
    for (const v of ["High", "Full", "Empty", "Low"]) await user.selectOptions(value, v);
    await settle();
    expect(asked.reviewFeedback.mock.calls.length - reviews).toBeLessThanOrEqual(2);
  });
});
