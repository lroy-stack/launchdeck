// @vitest-environment jsdom
// Where a new person can land with nothing to see: a page without pads, and no
// surface on screen. Each says one sentence and offers one next step.

import { cleanup, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { api } from "../../lib/ipc";
import { activeSurface, useStore } from "../../stores/runtime";
import { SurfaceView } from "./SurfaceView";

let off: () => void = () => undefined;

beforeEach(async () => {
  off = await useStore.getState().init();
  await api.activateSurface("development");
  await api.setMode("live");
  await waitFor(() => expect(useStore.getState().state?.mode).toBe("live"));
  useStore.setState({ view: "surface", selected: null, intent: null, libraryOpen: false });
});

afterEach(() => {
  cleanup();
  off();
});

/** The active surface with its first page emptied of pads. */
function emptyFirstPage() {
  const core = useStore.getState();
  const ws = core.workspace!;
  const surface = activeSurface(core)!;
  useStore.setState({
    workspace: { ...ws, surfaces: ws.surfaces.map((s) => (s.id === surface.id ? { ...s, pages: s.pages.map((p, i) => (i === 0 ? { ...p, controls: [] } : p)) } : s)) },
    state: { ...core.state!, activePageId: surface.pages[0]!.id },
  });
}

describe("a Live page with no pads", () => {
  it("says so and offers to switch to Edit", async () => {
    emptyFirstPage();
    render(<SurfaceView />);
    expect(screen.getByText("This page has no pads yet. Switch to Edit to add some.")).toBeTruthy();
    await userEvent.setup().click(screen.getByRole("button", { name: "Edit" }));
    await waitFor(() => expect(useStore.getState().state?.mode).toBe("edit"));
    expect(screen.queryByText(/This page has no pads yet/)).toBeNull();
  });

  it("says nothing of the kind on a page that has pads", () => {
    render(<SurfaceView />);
    expect(screen.queryByText(/This page has no pads yet/)).toBeNull();
  });
});

describe("no surface on screen", () => {
  it("offers to open the first surface", async () => {
    const core = useStore.getState();
    const first = core.workspace!.surfaces[0]!;
    useStore.setState({ state: { ...core.state!, activeSurfaceId: "gone" } });
    render(<SurfaceView />);
    expect(screen.getByText("No surface selected.")).toBeTruthy();
    await userEvent.setup().click(screen.getByRole("button", { name: `Open ${first.name}` }));
    await screen.findByRole("heading", { name: first.name, level: 1 });
  });
});
