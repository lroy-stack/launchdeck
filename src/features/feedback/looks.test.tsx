// @vitest-environment jsdom
import { fireEvent, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it } from "vitest";
import { api } from "../../lib/ipc";
import { controlSchema, type FeedbackRule } from "../../lib/schema";
import { FREE, eventRule, inFeedback, installEditorHarness, openPad, review, rule, runtimeAnswers, savedAt } from "./testkit";

installEditorHarness();

const select = (scope: HTMLElement, name: string) => within(scope).getByRole("combobox", { name }) as HTMLSelectElement;
const save = () => screen.getByRole("button", { name: /^(Add control|Save)$/ }) as HTMLButtonElement;
const looks = () => within(screen.getByRole("region", { name: "Looks" }));
const level = (value: string, show?: string): FeedbackRule => ({ when: { state: { provider: "example.counter", id: "level" }, is: { op: "equals", value } }, ...(show ? { show } : {}) });

async function firstRule(user: ReturnType<typeof userEvent.setup>) {
  await user.click(screen.getByRole("button", { name: /^\+ add feedback$/i }));
  await user.type(screen.getByRole("combobox", { name: "Search states" }), "counter level");
  await user.keyboard("{Enter}");
}

const saveWith = (feedback: FeedbackRule[], states = {}) =>
  api.updateControl("development", "home", controlSchema.parse({ id: "g.7.7", pos: FREE, label: "Pad", visual: { color: "#2f81f7", animation: "none" }, states, feedback }));

describe("the resting look", () => {
  it("is the pad's own look and a choice on every rule, not a rule that is always true", async () => {
    const user = userEvent.setup();
    openPad();
    await firstRule(user);
    // Listed first among the looks, with what it is.
    const list = looks().getAllByRole("listitem");
    expect(list[0]!.textContent).toContain("Resting look");
    expect(list[0]!.textContent).toContain("The pad’s own colour");
    // And offered by name on a rule.
    expect(within(select(rule(1), "Rule 1 look")).getByRole("option", { name: "Resting look" })).toBeTruthy();
    await user.selectOptions(select(rule(1), "Rule 1 look"), "Resting look");
    await user.click(save());
    await waitFor(() => expect(savedAt(FREE)).toBeTruthy());
    const saved = savedAt(FREE)!;
    // One rule, showing `idle`; no catch-all rule, and no look called idle was made.
    expect(saved.feedback).toHaveLength(1);
    expect(saved.feedback[0]!.show).toBe("idle");
    expect(saved.states).toEqual({});
  });

  it("is edited under Resting look, and the pad's colour there is what the rule shows", async () => {
    const user = userEvent.setup();
    openPad();
    await firstRule(user);
    await user.selectOptions(select(rule(1), "Rule 1 look"), "Resting look");
    fireEvent.change(screen.getByLabelText("Colour picker"), { target: { value: "#123456" } });
    // The swatch beside the rule's look is the pad's own colour.
    const swatch = rule(1).querySelector(".look-swatch") as HTMLElement;
    expect(swatch.style.background).toMatch(/rgb\(18, 52, 86\)|#123456/);
    expect(looks().getAllByRole("listitem")[0]!.querySelector(".look-swatch")).toBeTruthy();
  });
});

describe("making a look without leaving the rule", () => {
  it("opens a small form, starts from the colour the name says, makes the look and shows it from the rule in one step", async () => {
    const user = userEvent.setup();
    openPad();
    await firstRule(user);
    await user.selectOptions(select(rule(1), "Rule 1 look"), "+ Create a new look…");
    const form = within(rule(1)).getByRole("group", { name: "New look" });
    expect(document.activeElement).toBe(within(form).getByLabelText("Name of the new look"));
    await user.type(within(form).getByLabelText("Name of the new look"), "Warning");
    expect((within(form).getByLabelText("New look colour picker") as HTMLInputElement).value).toBe("#f59e0b");
    await user.selectOptions(within(form).getByLabelText("New look animation"), "Pulse");
    await user.click(within(form).getByRole("button", { name: "Create look" }));
    expect(select(rule(1), "Rule 1 look").value).toBe("look:warning");
    expect(within(rule(1)).queryByRole("group", { name: "New look" })).toBeNull();
    expect(looks().getByText("Warning")).toBeTruthy();
    expect(looks().getByText(/Pulse · Shown by Rule 1/)).toBeTruthy();
    expect(inFeedback().getByText("Created the look “Warning” and showed it from rule 1.")).toBeTruthy();
    await user.click(save());
    await waitFor(() => expect(savedAt(FREE)).toBeTruthy());
    expect(savedAt(FREE)!.states).toEqual({ warning: { color: "#f59e0b", animation: "pulse" } });
    expect(savedAt(FREE)!.feedback[0]!.show).toBe("warning");
  });

  it("takes the name with Enter, a colour from the swatches and gives up with Escape", async () => {
    const user = userEvent.setup();
    openPad();
    await firstRule(user);
    await user.selectOptions(select(rule(1), "Rule 1 look"), "+ Create a new look…");
    await user.keyboard("{Escape}");
    expect(within(rule(1)).queryByRole("group", { name: "New look" })).toBeNull();
    await user.selectOptions(select(rule(1), "Rule 1 look"), "+ Create a new look…");
    await user.type(within(rule(1)).getByLabelText("Name of the new look"), "Calm");
    await user.click(within(rule(1)).getByRole("button", { name: "#a855f7" }));
    await user.keyboard("{Enter}"); // focus is on the swatch: Enter presses it, nothing is created yet
    await user.click(within(rule(1)).getByLabelText("Name of the new look"));
    await user.keyboard("{Enter}");
    expect(select(rule(1), "Rule 1 look").value).toBe("look:calm");
    await user.click(save());
    await waitFor(() => expect(savedAt(FREE)!.states.calm!.color).toBe("#a855f7"));
  });

  it("keeps the cursor on the look field after a look is chosen or made, so the next key still acts on it", async () => {
    // Found in the real desktop window with the keyboard: choosing a look sent the cursor to the rule's card.
    const user = userEvent.setup();
    openPad();
    await firstRule(user);
    await user.selectOptions(select(rule(1), "Rule 1 look"), "Resting look");
    expect(document.activeElement).toBe(select(rule(1), "Rule 1 look"));
    await user.selectOptions(select(rule(1), "Rule 1 look"), "+ Create a new look…");
    await user.type(within(rule(1)).getByLabelText("Name of the new look"), "Calm");
    await user.keyboard("{Enter}");
    expect(select(rule(1), "Rule 1 look").value).toBe("look:calm");
    expect(document.activeElement).toBe(select(rule(1), "Rule 1 look"));
  });

  it("keeps a colour typed in the hex field as typed, even though a shorter hex such as #ef4 is a colour on the way", async () => {
    // Found in the real desktop window: the field rewrote "#ef4" to "#eeff44" while the rest was still being typed.
    const user = userEvent.setup();
    openPad();
    await firstRule(user);
    await user.selectOptions(select(rule(1), "Rule 1 look"), "+ Create a new look…");
    const form = within(rule(1)).getByRole("group", { name: "New look" });
    await user.type(within(form).getByLabelText("Name of the new look"), "Broken");
    const hex = form.querySelector<HTMLInputElement>("input.mono")!;
    await user.clear(hex);
    await user.type(hex, "#ef4444");
    expect(hex.value).toBe("#ef4444");
    expect((within(form).getByLabelText("New look colour picker") as HTMLInputElement).value).toBe("#ef4444");
    await user.click(within(form).getByRole("button", { name: "Create look" }));
    await user.click(save());
    await waitFor(() => expect(savedAt(FREE)!.states.broken!.color).toBe("#ef4444"));
  });

  it("will not make a look without a name, with a name that is taken, or that means the resting look", async () => {
    const user = userEvent.setup();
    openPad();
    await firstRule(user);
    await user.selectOptions(select(rule(1), "Rule 1 look"), "+ Create a new look…");
    const form = within(rule(1)).getByRole("group", { name: "New look" });
    const create = within(form).getByRole("button", { name: "Create look" }) as HTMLButtonElement;
    expect(create.disabled).toBe(true);
    await user.type(within(form).getByLabelText("Name of the new look"), "Idle");
    expect(within(form).getByText(/“idle” is the resting look/)).toBeTruthy();
    expect(create.disabled).toBe(true);
    await user.clear(within(form).getByLabelText("Name of the new look"));
    await user.type(within(form).getByLabelText("Name of the new look"), "Calm");
    expect(create.disabled).toBe(false);
    await user.click(create);
    // The same name again, on a second rule.
    await user.click(inFeedback().getByRole("button", { name: "+ Add a rule" }));
    await user.selectOptions(select(rule(2), "Rule 2 look"), "+ Create a new look…");
    await user.type(within(rule(2)).getByLabelText("Name of the new look"), "calm");
    expect(within(rule(2)).getByText(/There is already a look called “Calm”/)).toBeTruthy();
    expect((within(rule(2)).getByRole("button", { name: "Create look" }) as HTMLButtonElement).disabled).toBe(true);
  });

  it("makes a look for an event rule as well", async () => {
    const user = userEvent.setup();
    openPad();
    await user.click(screen.getByRole("button", { name: /^\+ add feedback$/i }));
    await user.click(screen.getByRole("button", { name: "An event" }));
    await user.type(screen.getByRole("combobox", { name: "Search events" }), "counter changed");
    await user.keyboard("{Enter}");
    await user.selectOptions(select(eventRule(1), "Event rule 1 look"), "+ Create a new look…");
    await user.type(within(eventRule(1)).getByLabelText("Name of the new look"), "Error");
    await user.keyboard("{Enter}");
    expect(select(eventRule(1), "Event rule 1 look").value).toBe("look:error");
    expect(looks().getByText(/Shown by Event rule 1/)).toBeTruthy();
  });
});

describe("the looks of a pad", () => {
  const withWarning = async () => {
    await saveWith([level("high", "warning"), level("max", "warning"), level("low", "calm")], { warning: { color: "#f59e0b", animation: "none" }, calm: { color: "#22c55e", animation: "none" } });
    openPad();
  };

  it("lists each look with its animation and the rules that show it", async () => {
    await withWarning();
    const rows = looks().getAllByRole("listitem");
    expect(rows).toHaveLength(3);
    expect(rows[0]!.textContent).toContain("Resting look");
    expect(rows[1]!.textContent).toContain("Warning");
    expect(rows[2]!.textContent).toContain("Calm");
    expect(looks().getByText("Solid · Shown by Rule 1, rule 2")).toBeTruthy();
    expect(looks().getByText("Solid · Shown by Rule 3")).toBeTruthy();
  });

  it("changes a look's colour and animation, and the draft says it is not saved", async () => {
    const user = userEvent.setup();
    await withWarning();
    await user.click(looks().getByRole("button", { name: "Edit the look Warning" }));
    fireEvent.change(looks().getByLabelText("Warning colour picker"), { target: { value: "#ff0000" } });
    await user.selectOptions(looks().getByLabelText("Warning animation"), "Flash");
    expect(screen.getByText("Not saved")).toBeTruthy();
    await user.click(save());
    await waitFor(() => expect(savedAt(FREE)!.states.warning).toEqual({ color: "#ff0000", animation: "flash" }));
  });

  it("renames a look and the rules that show it, so nothing is left pointing at the old name", async () => {
    const user = userEvent.setup();
    await withWarning();
    await user.click(looks().getByRole("button", { name: "Edit the look Warning" }));
    const name = looks().getByLabelText("Name of the look Warning");
    await user.clear(name);
    await user.type(name, "Too full{Enter}");
    expect(select(rule(1), "Rule 1 look").value).toBe("look:too-full");
    expect(select(rule(2), "Rule 2 look").value).toBe("look:too-full");
    expect(select(rule(3), "Rule 3 look").value).toBe("look:calm");
    expect(inFeedback().getByText("Renamed to “Too full”. The rules that showed it show the new name.")).toBeTruthy();
    await user.click(save());
    await waitFor(() => expect(Object.keys(savedAt(FREE)!.states)).toEqual(["too-full", "calm"]));
    expect(savedAt(FREE)!.feedback.map((r) => r.show)).toEqual(["too-full", "too-full", "calm"]);
  });

  it("refuses a name that is taken and leaves the look as it was", async () => {
    const user = userEvent.setup();
    await withWarning();
    await user.click(looks().getByRole("button", { name: "Edit the look Warning" }));
    const name = looks().getByLabelText("Name of the look Warning");
    await user.clear(name);
    await user.type(name, "Calm");
    expect(looks().getByText(/There is already a look called “Calm”/)).toBeTruthy();
    expect(name.getAttribute("aria-invalid")).toBe("true");
    await user.tab();
    expect((looks().getByLabelText("Name of the look Warning") as HTMLInputElement).value).toBe("Calm"); // still being edited
    await user.keyboard("{Escape}");
  });

  it("removes a look nothing uses at once", async () => {
    const user = userEvent.setup();
    await saveWith([level("low", "calm")], { calm: { color: "#22c55e", animation: "none" }, spare: { color: "#111111", animation: "none" } });
    openPad();
    await user.click(looks().getByRole("button", { name: "Edit the look Spare" }));
    await user.click(looks().getByRole("button", { name: "Remove the look Spare" }));
    expect(looks().queryByText("Spare")).toBeNull();
    expect(inFeedback().getByText("Removed the look “Spare”.")).toBeTruthy();
  });

  it("asks before removing a look that rules show, and what happens to them", async () => {
    const user = userEvent.setup();
    await withWarning();
    await user.click(looks().getByRole("button", { name: "Edit the look Warning" }));
    await user.click(looks().getByRole("button", { name: "Remove the look Warning" }));
    const ask = looks().getByRole("alert");
    expect(ask.textContent).toContain("2 rules show this look");
    await user.click(within(ask).getByRole("button", { name: "Keep it" }));
    expect(looks().getByText(/Shown by Rule 1, rule 2/)).toBeTruthy();
    await user.click(looks().getByRole("button", { name: "Remove the look Warning" }));
    await user.click(looks().getByRole("button", { name: "Remove it anyway" }));
    expect(looks().queryByText(/Warning/)).toBeNull();
    // The rules kept their choice and say the pad has no such look; they were not silently changed.
    const choice = select(rule(1), "Rule 1 look");
    expect(choice.value).toBe("look:warning");
    expect(within(choice).getByRole("option", { name: "Warning (this pad has no such look)" })).toBeTruthy();
  });

  it("shows Rust's finding that a rule names a look the pad does not have, and offers to create it", async () => {
    const user = userEvent.setup();
    await saveWith([level("low", "ghost")], {});
    runtimeAnswers(() => review({ notes: [{ kind: "missingLook", rule: 0, look: "ghost" }] }));
    openPad();
    expect(await within(rule(1)).findByText(/This pad has no look called “ghost”, so it rests instead/)).toBeTruthy();
    expect(select(rule(1), "Rule 1 look").value).toBe("look:ghost");
    await user.selectOptions(select(rule(1), "Rule 1 look"), "+ Create a new look…");
    await user.type(within(rule(1)).getByLabelText("Name of the new look"), "ghost{Enter}");
    expect(looks().getByText("Ghost")).toBeTruthy();
  });
});

describe("looks for the pad's own action", () => {
  it("says that Pending and Failed are shown by the runtime, not by a rule", async () => {
    const user = userEvent.setup();
    openPad();
    await firstRule(user);
    expect(screen.getByRole("region", { name: "Looks" }).textContent).toContain("a look called Pending shows while this pad’s action is running");
    for (const name of ["Pending", "Failed"]) {
      await user.click(looks().getByRole("button", { name: "+ New look" }));
      await user.type(looks().getByLabelText("Name of the new look"), `${name}{Enter}`);
    }
    expect(looks().getByText(/Shown while this pad’s action is running/)).toBeTruthy();
    expect(looks().getByText(/Shown for a moment when this pad’s action fails/)).toBeTruthy();
  });
});

describe("naming the look after the value", () => {
  it("is offered for a state whose value has names, and not for a test without a value", async () => {
    const user = userEvent.setup();
    openPad();
    await firstRule(user);
    const choice = select(rule(1), "Rule 1 look");
    expect(within(choice).getByRole("option", { name: "The look named after the value" })).toBeTruthy();
    await user.selectOptions(choice, "The look named after the value");
    await user.selectOptions(select(rule(1), "Rule 1 condition"), "has no value yet");
    // No value to name a look after: the choice goes with it, and the rule asks again.
    expect(within(select(rule(1), "Rule 1 look")).queryByRole("option", { name: "The look named after the value" })).toBeNull();
    expect(within(rule(1)).getByText("Choose the look to show.")).toBeTruthy();
    await user.selectOptions(select(rule(1), "Rule 1 condition"), "has a value");
    await user.selectOptions(select(rule(1), "Rule 1 look"), "The look named after the value");
    await user.click(save());
    await waitFor(() => expect(savedAt(FREE)).toBeTruthy());
    expect(savedAt(FREE)!.feedback[0]).toEqual({ when: { state: { provider: "example.counter", id: "level" }, is: { op: "hasValue" } }, showFromValue: true });
  });

  it("is offered for an event as the look named in a field that has names, and not for a number", async () => {
    const user = userEvent.setup();
    openPad();
    await user.click(screen.getByRole("button", { name: /^\+ add feedback$/i }));
    await user.click(screen.getByRole("button", { name: "An event" }));
    await user.type(screen.getByRole("combobox", { name: "Search events" }), "counter changed");
    await user.keyboard("{Enter}");
    const choice = select(eventRule(1), "Event rule 1 look");
    const options = within(choice).getAllByRole("option").map((o) => o.textContent);
    expect(options).toContain("The look named in “Level”");
    expect(options.some((o) => o?.includes("“Value”"))).toBe(false);
    await user.selectOptions(choice, "The look named in “Level”");
    await user.click(save());
    await waitFor(() => expect(savedAt(FREE)).toBeTruthy());
    expect(savedAt(FREE)!.feedback[0]).toMatchObject({ showFromField: "level" });
    expect(savedAt(FREE)!.feedback[0]).not.toHaveProperty("show");
  });
});

describe("nothing happens to looks by itself", () => {
  it("leaves the looks alone when a rule is removed", async () => {
    const user = userEvent.setup();
    await saveWith([level("low", "calm")], { calm: { color: "#22c55e", animation: "none" } });
    openPad();
    await user.click(within(rule(1)).getByRole("button", { name: "Remove rule 1" }));
    expect(looks().getByText("Calm")).toBeTruthy();
    expect(looks().getByText(/Not used by any rule/)).toBeTruthy();
  });
});
