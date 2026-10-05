// @vitest-environment jsdom
// The first run: one screen while the onboarding flag is off. The starter is chosen through the
// normal activation path, the flag is set last, and nothing else is configured on the person's behalf.

import { cleanup, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi, type MockInstance } from "vitest";
import fixtureJson from "../../../examples/default-workspace.json";
import { App } from "../../app/App";
import { api } from "../../lib/ipc";
import { workspaceSchema } from "../../lib/schema";
import { useStore } from "../../stores/runtime";

const fixture = workspaceSchema.parse(fixtureJson);
const describedAs = (id: string) => fixture.surfaces.find((s) => s.id === id)!.description;
const KEYS_NOTE = "Some pads press keys for you; macOS will ask for Accessibility permission the first time.";

async function renderApp() {
  render(<App />);
  await waitFor(() => expect(useStore.getState().ready).toBe(true));
}

const purposes = () => screen.getByRole("radiogroup", { name: "What is it for?" });
const card = (name: string) => screen.getByRole("radio", { name }).closest("label")!;
const shell = () => screen.queryByRole("navigation", { name: "Main" });

/** Calls in the order they were made, by name. */
function order(spies: Record<string, MockInstance>): string[] {
  return Object.entries(spies)
    .flatMap(([name, spy]) => spy.mock.invocationCallOrder.map((n) => [n, name] as const))
    .sort((a, b) => a[0] - b[0])
    .map(([, name]) => name);
}

beforeEach(async () => {
  // The preview runtime lives as long as this file: every test starts with the flag off, in Live.
  await api.setOnboardingDone(false);
  await api.setMode("live");
  useStore.setState({ ready: false, view: "overview", selected: null, libraryFocus: 0 });
});

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
  useStore.setState({ ready: false, notices: [] });
});

describe("the first run", () => {
  it("is one screen instead of the shell while the flag is off", async () => {
    await renderApp();
    expect(screen.getByRole("heading", { level: 1 }).textContent).toBe("Welcome to LaunchDeck");
    expect(shell()).toBeNull();
    expect(screen.getByText("Simulator")).toBeTruthy();
  });

  it("offers five choices, with the text and icon each starter carries", async () => {
    await renderApp();
    const radios = within(purposes()).getAllByRole("radio");
    expect(radios.map((r) => r.getAttribute("aria-labelledby") && document.getElementById(r.getAttribute("aria-labelledby")!)?.textContent)).toEqual([
      "Productivity",
      "Agent Control",
      "Development",
      "Accessibility",
      "Build my own",
    ]);
    for (const [name, id] of [
      ["Productivity", "productivity"],
      ["Agent Control", "agent-control"],
      ["Development", "development"],
      ["Accessibility", "accessibility"],
    ] as const) {
      const text = card(name).textContent ?? "";
      expect(text).toContain(describedAs(id));
      expect(text).toContain(fixture.surfaces.find((s) => s.id === id)!.icon!);
    }
    expect(card("Build my own").textContent).toContain("blank surface");
  });

  it("says that macOS asks for Accessibility permission only on the cards whose pads press keys", async () => {
    await renderApp();
    expect(card("Productivity").textContent).toContain(KEYS_NOTE);
    expect(card("Accessibility").textContent).toContain(KEYS_NOTE);
    for (const name of ["Agent Control", "Development", "Build my own"]) expect(card(name).textContent).not.toContain(KEYS_NOTE);
  });

  it("disables the card of a starter the workspace no longer has", async () => {
    const snapshot = api.snapshot;
    vi.spyOn(api, "snapshot").mockImplementation(async () => {
      const snap = await snapshot();
      return { ...snap, workspace: { ...snap.workspace, surfaces: snap.workspace.surfaces.filter((s) => s.id !== "accessibility") } };
    });
    await renderApp();
    expect(screen.getByRole("radio", { name: "Accessibility" })).toHaveProperty("disabled", true);
    expect(screen.getByRole("radio", { name: "Development" })).toHaveProperty("disabled", false);
  });

  it("looks for the Launchpad again on request", async () => {
    const user = userEvent.setup();
    const rescan = vi.spyOn(api, "rescanDevices");
    await renderApp();
    await user.click(screen.getByRole("button", { name: "Look again" }));
    expect(rescan).toHaveBeenCalledOnce();
  });

  it("starts with a starter in Live through the normal activation, then sets the flag last", async () => {
    const user = userEvent.setup();
    const spies = {
      activateSurface: vi.spyOn(api, "activateSurface"),
      setMode: vi.spyOn(api, "setMode"),
      setOnboardingDone: vi.spyOn(api, "setOnboardingDone"),
    };
    const demo = vi.spyOn(api.agents, "runDemo");
    await renderApp();
    expect(screen.getByRole("button", { name: /^Start/ })).toHaveProperty("disabled", true);

    await user.click(screen.getByRole("radio", { name: "Development" }));
    await user.click(screen.getByRole("button", { name: "Start with Development" }));

    await waitFor(() => expect(shell()).not.toBeNull());
    expect(spies.activateSurface).toHaveBeenCalledWith("development");
    expect(spies.setMode).toHaveBeenCalledWith("live");
    expect(spies.setOnboardingDone).toHaveBeenCalledWith(true);
    expect(order(spies)).toEqual(["activateSurface", "setMode", "setOnboardingDone"]);
    expect(demo).not.toHaveBeenCalled();
    expect(useStore.getState().view).toBe("surface");
    expect(useStore.getState().state?.activeSurfaceId).toBe("development");
    expect(screen.getByRole("heading", { level: 1, name: "Development" })).toBeTruthy();
    // Its category in words, not the slug.
    expect(screen.getByText("Development", { selector: ".eyebrow" })).toBeTruthy();
  });

  it("starts Agent Control without running the demo", async () => {
    const user = userEvent.setup();
    const demo = vi.spyOn(api.agents, "runDemo");
    await renderApp();
    await user.click(screen.getByRole("radio", { name: "Agent Control" }));
    await user.click(screen.getByRole("button", { name: "Start with Agent Control" }));
    await waitFor(() => expect(shell()).not.toBeNull());
    expect(useStore.getState().state?.activeSurfaceId).toBe("agent-control");
    expect(demo).not.toHaveBeenCalled();
  });

  it("builds your own: a new surface, in Edit, with the cursor in the Action Library", async () => {
    const user = userEvent.setup();
    const spies = {
      createSurface: vi.spyOn(api, "createSurface"),
      activateSurface: vi.spyOn(api, "activateSurface"),
      setMode: vi.spyOn(api, "setMode"),
      setOnboardingDone: vi.spyOn(api, "setOnboardingDone"),
    };
    await renderApp();
    const before = useStore.getState().workspace!.surfaces.length;

    await user.click(screen.getByRole("radio", { name: "Build my own" }));
    await user.click(screen.getByRole("button", { name: "Start with a blank surface" }));

    await waitFor(() => expect(shell()).not.toBeNull());
    expect(spies.createSurface).toHaveBeenCalledWith("My surface");
    const id = await spies.createSurface.mock.results[0]!.value;
    expect(spies.activateSurface).toHaveBeenCalledWith(id);
    expect(spies.setMode).toHaveBeenCalledWith("edit");
    expect(order(spies)).toEqual(["createSurface", "activateSurface", "setMode", "setOnboardingDone"]);
    expect(useStore.getState().workspace!.surfaces).toHaveLength(before + 1);
    expect(useStore.getState().state?.mode).toBe("edit");
    await waitFor(() => expect(document.activeElement).toBe(screen.getByRole("combobox", { name: "Search actions" })));
  });

  it("skips: only the flag is set, nothing is activated, created or configured", async () => {
    const user = userEvent.setup();
    const others = [
      vi.spyOn(api, "activateSurface"),
      vi.spyOn(api, "setMode"),
      vi.spyOn(api, "createSurface"),
      vi.spyOn(api, "removeSurface"),
      vi.spyOn(api, "updatePreferences"),
      vi.spyOn(api.agents, "runDemo"),
      vi.spyOn(api.context, "update"),
    ];
    const flag = vi.spyOn(api, "setOnboardingDone");
    await renderApp();
    const surfaces = useStore.getState().workspace!.surfaces;

    await user.click(screen.getByRole("button", { name: "Skip" }));

    await waitFor(() => expect(shell()).not.toBeNull());
    expect(flag).toHaveBeenCalledExactlyOnceWith(true);
    for (const spy of others) expect(spy).not.toHaveBeenCalled();
    expect(useStore.getState().workspace!.surfaces).toEqual(surfaces);
    expect(useStore.getState().view).toBe("overview");
  });

  it("stays on screen with the error when the flag cannot be saved", async () => {
    const user = userEvent.setup();
    vi.spyOn(api, "setOnboardingDone").mockRejectedValue("Your change could not be saved: the disk is full.");
    await renderApp();

    await user.click(screen.getByRole("radio", { name: "Productivity" }));
    await user.click(screen.getByRole("button", { name: "Start with Productivity" }));

    expect((await screen.findByRole("alert")).textContent).toContain("Your change could not be saved: the disk is full.");
    expect(purposes()).toBeTruthy();
    expect(shell()).toBeNull();
  });

  it("does not create a second surface when Build my own is tried again after a failure", async () => {
    const user = userEvent.setup();
    const flag = vi.spyOn(api, "setOnboardingDone").mockRejectedValueOnce("Your change could not be saved.");
    const create = vi.spyOn(api, "createSurface");
    await renderApp();

    await user.click(screen.getByRole("radio", { name: "Build my own" }));
    await user.click(screen.getByRole("button", { name: "Start with a blank surface" }));
    await screen.findByRole("alert");
    await user.click(screen.getByRole("button", { name: "Start with a blank surface" }));

    await waitFor(() => expect(shell()).not.toBeNull());
    expect(create).toHaveBeenCalledOnce();
    expect(flag).toHaveBeenCalledTimes(2);
  });
});

describe("after the first run", () => {
  it("shows the shell directly when the flag is set", async () => {
    await api.setOnboardingDone(true);
    await renderApp();
    expect(shell()).not.toBeNull();
    expect(screen.queryByRole("radiogroup", { name: "What is it for?" })).toBeNull();
  });

  it("calls the overview Overview and has no Marketplace placeholder", async () => {
    await api.setOnboardingDone(true);
    await renderApp();
    const items = within(shell()!).getAllByRole("button").map((b) => b.textContent?.replace(/\d+$/, "")); // without a badge's count
    expect(items).toEqual(["◫Overview", "◉Agents", "▦Surfaces", "⧉Plugins", "⚙Settings"]);
    expect(screen.queryByText(/Marketplace/)).toBeNull();
  });
});
