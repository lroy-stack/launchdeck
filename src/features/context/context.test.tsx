// @vitest-environment jsdom
// Auto Context in the interface: the sidebar control and the Settings panel.
// The statuses are what Rust emits (`examples/context-status.json`, kept
// current by a Rust contract test); the runtime's answers to the person's
// changes are stood in for, since a browser has no runtime.

import { cleanup, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import statusesJson from "../../../examples/context-status.json";
import { Sidebar } from "../../components/Sidebar";
import { api } from "../../lib/ipc";
import { contextStatusSchema, type ContextSettings, type ContextStatus } from "../../lib/schema";
import { useStore } from "../../stores/runtime";
import { ContextPanel } from "./ContextPanel";
import { explain, identityOf } from "./text";

const fixture = {
  auto: contextStatusSchema.parse(statusesJson.auto),
  manual: contextStatusSchema.parse(statusesJson.manual),
  paused: contextStatusSchema.parse(statusesJson.paused),
  unavailable: contextStatusSchema.parse(statusesJson.unavailable),
};

let off: () => void = () => undefined;

/** The interface with the runtime saying `status`. `update` answers as the runtime would: the new settings, accepted. */
async function show(status: ContextStatus, ui: "panel" | "sidebar" = "panel") {
  off = await useStore.getState().init();
  useStore.setState({ context: status, view: "settings" });
  const update = vi.spyOn(api.context, "update").mockImplementation(async (settings: ContextSettings) => {
    const next = { ...useStore.getState().context!, settings, problems: settings.rules.map(() => null) };
    useStore.setState({ context: next });
    return next;
  });
  const resume = vi.spyOn(api.context, "resume").mockImplementation(async () => {
    const next: ContextStatus = { ...useStore.getState().context!, mode: "auto" };
    useStore.setState({ context: next });
    return next;
  });
  render(ui === "panel" ? <ContextPanel /> : <Sidebar />);
  return { update, resume };
}

beforeEach(() => {
  vi.restoreAllMocks();
});

afterEach(() => {
  cleanup();
  off();
});

const now = () => screen.getByRole("group", { name: "Context now" });
const rules = () => within(screen.getByRole("list", { name: "Context rules" })).getAllByRole("listitem");

describe("what Rust sends about context", () => {
  it("parses every status the runtime emits, and holds nothing about past applications", () => {
    expect(fixture.auto.mode).toBe("auto");
    expect(fixture.auto.observer).toEqual({ state: "available", mechanism: "the X11 window manager" });
    expect(fixture.auto.decision).toMatchObject({ rule: 0, outcome: "activated", surfaceId: "development", pageId: "git", pageMissing: false });
    expect(fixture.auto.settings.rules[0]).toEqual({ application: { namespace: "x11", id: "Code", displayName: "Visual Studio Code" }, surfaceId: "development", pageId: "git" });
    expect(fixture.manual.foreground).toEqual({ kind: "launchDeck" });
    expect(fixture.manual.decision?.held).toBe("manual");
    expect(fixture.paused.problems).toEqual([null, null, { kind: "shadowed", by: 0 }, { kind: "otherSystem" }]);
    expect(fixture.paused.observer.state).toBe("degraded");
    expect(fixture.unavailable.observer.state).toBe("unavailable");
    expect(fixture.unavailable.mode).toBe("unavailable");
    // Recently seen applications are a list of identities: no times, no counts, no titles.
    expect(Object.keys(fixture.auto.recent[0]!).sort()).toEqual(["displayName", "id", "namespace"]);
    expect(JSON.stringify(statusesJson)).not.toMatch(/title|since|duration|count/i);
  });

  it("a workspace without context settings is one that never used them", async () => {
    off = await useStore.getState().init();
    expect(useStore.getState().workspace?.context).toBeUndefined();
    const context = useStore.getState().context!;
    expect(context.settings).toEqual({ auto: false, rules: [], fallback: { kind: "keep" } });
    expect(context.mode).toBe("off");
  });
});

describe("the sidebar control", () => {
  it("shows whether Auto Context is on and switches it in one click", async () => {
    const user = userEvent.setup();
    const { update } = await show({ ...fixture.auto, mode: "off", settings: { ...fixture.auto.settings, auto: false } }, "sidebar");
    const control = screen.getByRole("group", { name: "Auto Context" });
    expect(control.textContent).toContain("Off");
    const toggle = within(control).getByRole("switch", { name: "Auto Context" });
    expect(toggle.getAttribute("aria-checked")).toBe("false");
    await user.click(toggle);
    expect(update).toHaveBeenCalledWith({ ...fixture.auto.settings, auto: true });
    await waitFor(() => expect(within(control).getByRole("switch").getAttribute("aria-checked")).toBe("true"));
    // No way back is offered when nothing was overridden.
    expect(within(control).queryByRole("button", { name: "Resume Auto" })).toBeNull();
  });

  it("says a surface was chosen by hand and offers the way back", async () => {
    const user = userEvent.setup();
    const { resume } = await show(fixture.manual, "sidebar");
    const control = screen.getByRole("group", { name: "Auto Context" });
    expect(control.textContent).toContain("Manual override");
    await user.click(within(control).getByRole("button", { name: "Resume Auto" }));
    expect(resume).toHaveBeenCalledTimes(1);
    await waitFor(() => expect(within(control).queryByRole("button", { name: "Resume Auto" })).toBeNull());
    expect(control.textContent).toContain("On");
  });

  it("says when it cannot work here, and while editing", async () => {
    await show(fixture.unavailable, "sidebar");
    expect(screen.getByRole("group", { name: "Auto Context" }).textContent).toContain("Not available here");
    cleanup();
    off();
    await show(fixture.paused, "sidebar");
    expect(screen.getByRole("group", { name: "Auto Context" }).textContent).toContain("Paused while editing");
  });
});

describe("seeing why a surface is on screen", () => {
  it("names the observer, the application by its stable identity, the rule that matched and what it did", async () => {
    await show(fixture.auto);
    const text = now().textContent ?? "";
    expect(text).toContain("Available");
    expect(text).toContain("Watching through the X11 window manager.");
    expect(within(now()).getByText("Visual Studio Code")).toBeTruthy();
    expect(within(now()).getByText("x11:Code")).toBeTruthy();
    expect(text).toContain("Rule 1 matches Visual Studio Code: Development / Git was activated.");
    // The rule that decided is marked in the list.
    expect(rules()[0]!.className).toContain("matched");
    expect(rules()[1]!.className).not.toContain("matched");
  });

  it("says LaunchDeck itself is not an application, which one the rules are asked about, and why nothing was applied", async () => {
    const user = userEvent.setup();
    const { resume } = await show(fixture.manual);
    const text = now().textContent ?? "";
    expect(text).toContain("LaunchDeck itself, which never counts as an application.");
    expect(text).toContain("Last application");
    expect(text).toContain("the rules are asked about this one");
    expect(text).toContain("Manual override");
    expect(text).toContain("Rule 1 matches Visual Studio Code: it calls for Development / Git.");
    expect(text).toContain("Not applied: you chose a surface yourself. That holds until another application comes to the front, or you resume.");
    await user.click(within(now()).getByRole("button", { name: "Resume Auto" }));
    expect(resume).toHaveBeenCalledTimes(1);
  });

  it("explains a system where the application in front cannot be seen, and that the rest works", async () => {
    await show(fixture.unavailable);
    const text = now().textContent ?? "";
    expect(text).toContain("Not available");
    expect(text).toContain("This is a Wayland session (GNOME).");
    expect(text).toContain("switching surfaces by hand works as always");
    expect(text).toContain("Auto Context is on and cannot work here.");
    expect(text).toContain("Nothing is decided automatically on this system.");
    // No list of applications can be offered, and it says why instead of an empty form.
    expect(screen.queryByRole("group", { name: "Add a rule" })).toBeNull();
    expect(screen.getByText(/which it cannot do on this system/)).toBeTruthy();
  });

  it("says what the engine would do while editing, and that the observer is in trouble", async () => {
    await show(fixture.paused);
    const text = now().textContent ?? "";
    expect(text).toContain("Degraded");
    expect(text).toContain("The connection to the X display was lost.");
    expect(text).toContain("Paused while editing");
    expect(text).toContain("Not applied while editing. It will be when you go back to Live.");
  });

  it("puts every outcome in words", () => {
    const ws = useStore.getState().workspace;
    const base = fixture.auto;
    const say = (decision: Partial<NonNullable<ContextStatus["decision"]>>) => explain({ ...base, decision: { ...base.decision!, ...decision } }, ws);
    expect(say({ outcome: "alreadyShown" })).toBe("Rule 1 matches Visual Studio Code: Development / Git was already shown.");
    expect(say({ rule: undefined, outcome: "kept", surfaceId: undefined, pageId: undefined })).toBe("No rule names Visual Studio Code, and the fallback is to keep the current surface.");
    expect(say({ rule: undefined, outcome: "activated", surfaceId: "productivity", pageId: undefined })).toBe("No rule names Visual Studio Code, so the fallback applied: Productivity was activated.");
    expect(say({ outcome: "missingSurface", surfaceId: "gone", pageId: undefined })).toBe("Rule 1 matches Visual Studio Code, but it names a surface that no longer exists (gone). Nothing was changed.");
    expect(say({ pageMissing: true, pageId: "home" })).toContain("The page the rule names no longer exists, so the surface's first page stands in.");
    expect(say({ held: "off" })).toBe("Rule 1 matches Visual Studio Code: it calls for Development / Git. Not applied: Auto Context is off.");
    expect(explain({ ...base, decision: undefined }, ws)).toBe("No application has come to the front yet, so nothing has been decided.");
  });
});

describe("the rules", () => {
  it("are numbered in the order they are tried, by application name and stable identity", async () => {
    await show(fixture.auto);
    const [first, second] = rules();
    expect(first!.textContent).toContain("1");
    expect(within(first!).getByText("Visual Studio Code")).toBeTruthy();
    expect(within(first!).getByText("x11:Code")).toBeTruthy();
    expect((within(first!).getByRole("combobox", { name: "Rule 1 surface" }) as HTMLSelectElement).value).toBe("development");
    expect((within(first!).getByRole("combobox", { name: "Rule 1 page" }) as HTMLSelectElement).value).toBe("git");
    expect((within(second!).getByRole("combobox", { name: "Rule 2 page" }) as HTMLSelectElement).value).toBe("");
    // The first cannot go up, the last cannot go down.
    expect((within(first!).getByRole("button", { name: "Move rule 1 up" }) as HTMLButtonElement).disabled).toBe(true);
    expect((within(second!).getByRole("button", { name: "Move rule 2 down" }) as HTMLButtonElement).disabled).toBe(true);
  });

  it("are reordered, retargeted and removed, each as one change sent whole", async () => {
    const user = userEvent.setup();
    const { update } = await show(fixture.auto);
    const [code, terminal] = fixture.auto.settings.rules;

    await user.click(screen.getByRole("button", { name: "Move rule 2 up" }));
    expect(update).toHaveBeenLastCalledWith({ ...fixture.auto.settings, rules: [terminal, code] });
    await waitFor(() => expect(within(rules()[0]!).getByText("Terminal")).toBeTruthy());

    await user.selectOptions(screen.getByRole("combobox", { name: "Rule 1 surface" }), "smart-home");
    expect(update).toHaveBeenLastCalledWith({ ...fixture.auto.settings, rules: [{ application: terminal!.application, surfaceId: "smart-home" }, code] });

    // Choosing a page, then "Any page" again, leaves no page behind.
    await user.selectOptions(screen.getByRole("combobox", { name: "Rule 2 page" }), "");
    expect(update.mock.lastCall![0].rules[1]).toEqual({ application: code!.application, surfaceId: "development" });

    await user.click(screen.getByRole("button", { name: "Remove rule 1" }));
    expect(update.mock.lastCall![0].rules).toHaveLength(1);
    expect(update.mock.lastCall![0].rules[0]!.application).toEqual(code!.application);
  });

  it("are flagged when one can never be reached or was written on another system", async () => {
    await show(fixture.paused);
    const items = rules();
    expect(items).toHaveLength(4);
    expect(within(items[2]!).getByRole("note").textContent).toBe("Never reached: rule 1 names the same application and comes first.");
    expect(within(items[3]!).getByRole("note").textContent).toBe("Written on another kind of system. It never matches on this one.");
    expect(within(items[3]!).getByText("macos:com.apple.Terminal")).toBeTruthy();
    expect(within(items[0]!).queryByRole("note")).toBeNull();
  });

  it("keep a rule whose surface is gone visible and fixable", async () => {
    await show({
      ...fixture.auto,
      settings: { ...fixture.auto.settings, rules: [{ application: fixture.auto.settings.rules[0]!.application, surfaceId: "gone" }] },
      problems: [{ kind: "missingSurface" }],
    });
    const item = rules()[0]!;
    expect(within(item).getByRole("note").textContent).toContain("Its surface no longer exists. The rule still wins for this application and shows nothing.");
    const surface = within(item).getByRole("combobox", { name: "Rule 1 surface" }) as HTMLSelectElement;
    expect(surface.selectedOptions[0]!.textContent).toBe("(no longer exists)");
    expect((within(item).getByRole("combobox", { name: "Rule 1 page" }) as HTMLSelectElement).disabled).toBe(true);
  });

  it("show what the runtime refused, and change nothing", async () => {
    const user = userEvent.setup();
    const { update } = await show(fixture.auto);
    update.mockRejectedValueOnce("rule 1: it names a surface that does not exist (`gone`)");
    await user.selectOptions(screen.getByRole("combobox", { name: "Rule 1 surface" }), "productivity");
    await waitFor(() => expect(useStore.getState().toasts.at(-1)?.tone).toBe("error"));
    expect(useStore.getState().toasts.at(-1)?.text).toContain("rule 1: it names a surface that does not exist");
    expect((screen.getByRole("combobox", { name: "Rule 1 surface" }) as HTMLSelectElement).value).toBe("development");
  });
});

describe("adding a rule", () => {
  it("offers the applications seen in front, newest first, and saves the stable identity", async () => {
    const user = userEvent.setup();
    const { update } = await show(fixture.auto);
    const form = screen.getByRole("group", { name: "Add a rule" });
    const application = within(form).getByRole("combobox", { name: "Application" }) as HTMLSelectElement;
    expect([...application.options].map((o) => o.textContent)).toEqual(["Visual Studio Code (x11:Code)", "Terminal (x11:Gnome-terminal)"]);
    expect(form.textContent).toContain("The list is not saved.");
    // Nothing is typed: there is no text field for an identifier.
    expect(within(form).queryByRole("textbox")).toBeNull();

    await user.selectOptions(application, identityOf(fixture.auto.recent[1]!));
    await user.selectOptions(within(form).getByRole("combobox", { name: "Surface" }), "development");
    await user.selectOptions(within(form).getByRole("combobox", { name: "Page" }), "git");
    await user.click(within(form).getByRole("button", { name: "Add rule" }));
    expect(update.mock.lastCall![0].rules).toHaveLength(3);
    expect(update.mock.lastCall![0].rules[2]).toEqual({
      application: { namespace: "x11", id: "Gnome-terminal", displayName: "Terminal" },
      surfaceId: "development",
      pageId: "git",
    });
    await waitFor(() => expect(rules()).toHaveLength(3));
  });

  it("says how to get an application into the list when none has been seen", async () => {
    await show({ ...fixture.auto, recent: [] });
    expect(screen.queryByRole("group", { name: "Add a rule" })).toBeNull();
    expect(screen.getByText(/bring the application you want to the front once/)).toBeTruthy();
  });
});

describe("the fallback", () => {
  it("is always one of two things, said in words, and changed in one select", async () => {
    const user = userEvent.setup();
    const { update } = await show(fixture.auto);
    const select = screen.getByRole("combobox", { name: "When no rule matches" }) as HTMLSelectElement;
    expect(select.value).toBe("agent-control");
    expect(screen.getByText("Every application without a rule shows this surface.")).toBeTruthy();
    expect([...select.options].map((o) => o.textContent).slice(0, 2)).toEqual(["Keep the current surface", "Open Agent Control"]);

    await user.selectOptions(select, "");
    expect(update).toHaveBeenLastCalledWith({ ...fixture.auto.settings, fallback: { kind: "keep" } });
    await waitFor(() => expect(screen.getByText("An application without a rule changes nothing.")).toBeTruthy());
    expect(screen.queryByRole("combobox", { name: "Fallback page" })).toBeNull();

    await user.selectOptions(screen.getByRole("combobox", { name: "When no rule matches" }), "development");
    expect(update).toHaveBeenLastCalledWith({ ...fixture.auto.settings, fallback: { kind: "surface", surfaceId: "development" } });
    await user.selectOptions(await screen.findByRole("combobox", { name: "Fallback page" }), "git");
    expect(update).toHaveBeenLastCalledWith({ ...fixture.auto.settings, fallback: { kind: "surface", surfaceId: "development", pageId: "git" } });
  });
});

describe("switching Auto Context on and off in Settings", () => {
  it("is a choice of two, and sends the whole settings", async () => {
    const user = userEvent.setup();
    const { update } = await show(fixture.auto);
    const group = screen.getByRole("radiogroup", { name: "Auto Context" });
    expect(within(group).getByRole("radio", { name: "On" }).getAttribute("aria-checked")).toBe("true");
    await user.click(within(group).getByRole("radio", { name: "Off" }));
    expect(update).toHaveBeenLastCalledWith({ ...fixture.auto.settings, auto: false });
    await waitFor(() => expect(within(group).getByRole("radio", { name: "Off" }).getAttribute("aria-checked")).toBe("true"));
  });
});
