// @vitest-environment jsdom
import { screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it } from "vitest";
import { emptyObservableCatalog } from "../../lib/schema";
import { useStore } from "../../stores/runtime";
import { FREE, inFeedback, openPad, rule, savedAt, installEditorHarness } from "./testkit";

installEditorHarness();

const addFeedback = () => screen.getByRole("button", { name: /add feedback/i });
const search = (kind: "states" | "events" = "states") => screen.getByRole("combobox", { name: kind === "states" ? "Search states" : "Search events" });
const options = (kind: "States" | "Events" = "States") => within(screen.getByRole("listbox", { name: kind })).queryAllByRole("option");
const option = (name: RegExp, kind: "States" | "Events" = "States") => within(screen.getByRole("listbox", { name: kind })).getByRole("option", { name });

describe("the empty pad", () => {
  it("says it has no feedback and offers one way in", async () => {
    openPad();
    expect(inFeedback().getByText("No feedback yet.")).toBeTruthy();
    expect(addFeedback()).toBeTruthy();
    // Nothing is asked of the runtime for a pad with nothing to explain.
    expect(screen.queryByRole("region", { name: "Live feedback" })).toBeNull();
  });

  it("keeps the action and the feedback apart, under their own names", () => {
    openPad();
    expect(screen.getByRole("region", { name: "Action" })).toBeTruthy();
    expect(screen.getByRole("region", { name: "Feedback" })).toBeTruthy();
    // The pad's own colour is the look it rests in, said so.
    const resting = screen.getByRole("region", { name: "Resting look" });
    expect(within(resting).getByText(/when no feedback rule applies/i)).toBeTruthy();
  });
});

describe("the observable picker", () => {
  it("lists the states of every provider in one list, grouped by provider, in words and not in ids", async () => {
    const user = userEvent.setup();
    openPad();
    await user.click(addFeedback());
    const list = screen.getByRole("listbox", { name: "States" });
    expect(within(list).getAllByRole("group").map((g) => g.getAttribute("aria-label"))).toEqual(["LaunchDeck", "Demo", "Agent Control", "Channels", "Counter Plugin"]);
    // A plugin's state is not set apart from LaunchDeck's own or Agent Control's.
    for (const name of [/Counter level/, /Channel status/, /Agent track status/, /Active surface/, /Demo agent alpha/]) expect(option(name)).toBeTruthy();
    // What it holds, and what it holds now.
    const counter = option(/Counter level/);
    expect(counter.textContent).toContain("Empty, Low, High, Full");
    expect(within(counter).getByText("Known")).toBeTruthy();
    expect(within(counter).getByText("Low")).toBeTruthy();
    // A state with arguments says so; one value cannot be shown for it.
    expect(within(option(/Channel status/)).getByText("Per channel")).toBeTruthy();
    expect(within(option(/Channel status/)).getByText("2 values reported")).toBeTruthy();
    // No technical ids by default.
    expect(list.textContent).not.toMatch(/example\.counter|agent-control\.|slot\.status/);
  });

  it("narrows as you type, by name, provider, value or keyword, and says how many", async () => {
    const user = userEvent.setup();
    openPad();
    await user.click(addFeedback());
    await user.type(search(), "counter");
    expect(options().map((o) => o.textContent)).toEqual([expect.stringContaining("Counter level")]);
    expect(screen.getByText("1 state found")).toBeTruthy();
    await user.clear(search());
    await user.type(search(), "health");
    expect(options()).toHaveLength(1);
    expect(options()[0]!.textContent).toContain("Channel status");
    await user.clear(search());
    await user.type(search(), "levle"); // a slip of the keys
    expect(option(/Counter level/)).toBeTruthy();
  });

  it("says when nothing matches and gives a way back", async () => {
    const user = userEvent.setup();
    openPad();
    await user.click(addFeedback());
    await user.type(search(), "zzzzqq");
    expect(options()).toHaveLength(0);
    expect(screen.getByText(/Nothing matches/)).toBeTruthy();
    await user.click(screen.getByRole("button", { name: "Show everything" }));
    expect(options().length).toBeGreaterThan(5);
  });

  it("lists events apart from states, from plugins, Agent Control and LaunchDeck alike, and keeps a stopped provider's in the list, said in words", async () => {
    const user = userEvent.setup();
    openPad();
    await user.click(addFeedback());
    expect(options("States").every((o) => !/Counter changed|Approval requested/.test(o.textContent ?? ""))).toBe(true);
    await user.click(screen.getByRole("button", { name: "An event" }));
    expect(screen.getByRole("button", { name: "An event" }).getAttribute("aria-pressed")).toBe("true");
    for (const name of [/Counter changed/, /Approval requested/, /Action completed/, /Control pressed/]) expect(option(name, "Events")).toBeTruthy();
    expect(options("Events").every((o) => !/Counter level/.test(o.textContent ?? ""))).toBe(true);
    await user.type(search("events"), "hello");
    const stopped = option(/hello\.completed/, "Events");
    expect(within(stopped).getByText("Unavailable")).toBeTruthy();
    expect(stopped.getAttribute("title")).toMatch(/turned off/);
  });

  it("moves with the arrow keys, takes the highlighted one with Enter and puts the cursor on the new rule", async () => {
    const user = userEvent.setup();
    openPad();
    await user.click(addFeedback());
    await user.type(search(), "counter");
    expect(search().getAttribute("aria-activedescendant")).toBe(options()[0]!.id);
    await user.keyboard("{Enter}");
    const first = rule(1);
    expect(first).toBeTruthy();
    expect(within(first).getByRole("combobox", { name: "Rule 1 condition" })).toBe(document.activeElement);
    // The picker closed and a first rule was added, ready to finish.
    expect(screen.queryByRole("listbox", { name: "States" })).toBeNull();
  });

  it("walks the list with the keys, group after group", async () => {
    const user = userEvent.setup();
    openPad();
    await user.click(addFeedback());
    const rows = options();
    expect(search().getAttribute("aria-activedescendant")).toBe(rows[0]!.id);
    await user.keyboard("{ArrowDown}{ArrowDown}");
    expect(search().getAttribute("aria-activedescendant")).toBe(rows[2]!.id);
    expect(rows[2]!.getAttribute("aria-selected")).toBe("true");
    await user.keyboard("{ArrowUp}");
    expect(search().getAttribute("aria-activedescendant")).toBe(rows[1]!.id);
  });

  it("clears the search with Escape before it closes anything", async () => {
    const user = userEvent.setup();
    openPad();
    await user.click(addFeedback());
    await user.type(search(), "agent");
    await user.keyboard("{Escape}");
    expect((search() as HTMLInputElement).value).toBe("");
  });

  it("can be cancelled without changing the pad", async () => {
    const user = userEvent.setup();
    openPad();
    await user.click(addFeedback());
    await user.click(within(screen.getByRole("region", { name: "Add feedback" })).getByRole("button", { name: "Cancel" }));
    expect(screen.queryByRole("region", { name: "Add feedback" })).toBeNull();
    expect(inFeedback().getByText("No feedback yet.")).toBeTruthy();
  });

  it("explains an empty catalog and points to Plugins", async () => {
    const user = userEvent.setup();
    openPad();
    useStore.setState({ observables: emptyObservableCatalog });
    await user.click(addFeedback());
    expect(screen.getByText(/No plugin or part of LaunchDeck offers a state yet/)).toBeTruthy();
    await user.click(screen.getByRole("button", { name: "Open Plugins" }));
    expect(useStore.getState().view).toBe("plugins");
  });
});

describe("a state with a scope", () => {
  it("asks which channel before any rule exists, and will not go on without it", async () => {
    const user = userEvent.setup();
    openPad();
    await user.click(addFeedback());
    await user.type(search(), "channel");
    await user.click(option(/Channel status/));
    await user.click(within(option(/Channel status/)).getByRole("button", { name: "Use" }));
    const form = screen.getByRole("region", { name: "Add feedback" });
    expect(within(form).getByText(/exists once for each channel/)).toBeTruthy();
    const go = within(form).getByRole("button", { name: "Add the first rule" }) as HTMLButtonElement;
    expect(go.disabled).toBe(true);
    await user.type(within(form).getByLabelText(/^Channel/), "alpha");
    expect(go.disabled).toBe(false);
    await user.click(go);
    expect(rule(1)).toBeTruthy();
    expect(inFeedback().getByRole("heading", { name: "Channel status" })).toBeTruthy();
    expect(inFeedback().getByText(/channel alpha/)).toBeTruthy();
  });

  it("refuses an argument the definition refuses, next to the field", async () => {
    const user = userEvent.setup();
    openPad();
    await user.click(addFeedback());
    await user.type(search(), "track status");
    await user.keyboard("{Enter}");
    const form = screen.getByRole("region", { name: "Add feedback" });
    const track = within(form).getByLabelText(/^Track/);
    await user.type(track, "9");
    expect(within(form).getByText("Track must be between 0 and 7")).toBeTruthy();
    expect((within(form).getByRole("button", { name: "Add the first rule" }) as HTMLButtonElement).disabled).toBe(true);
    await user.clear(track);
    await user.type(track, "3");
    expect((within(form).getByRole("button", { name: "Add the first rule" }) as HTMLButtonElement).disabled).toBe(false);
  });

  it("goes back from the scope to the list without losing the place", async () => {
    const user = userEvent.setup();
    openPad();
    await user.click(addFeedback());
    await user.type(search(), "channel");
    await user.keyboard("{Enter}");
    await user.click(screen.getByRole("button", { name: "Back" }));
    expect(search()).toBeTruthy();
    expect(options().length).toBeGreaterThan(0);
  });

  it("leaves the pad as it was until a rule is saved", () => {
    openPad();
    expect(savedAt(FREE)).toBeUndefined();
  });
});
