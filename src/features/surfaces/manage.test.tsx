// @vitest-environment jsdom
// Managing surfaces and pages in Edit: renaming in place, and nothing deleted before the person confirms.

import { cleanup, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";
import { api } from "../../lib/ipc";
import { useStore } from "../../stores/runtime";
import { SurfaceView } from "./SurfaceView";

let off: () => void = () => undefined;

async function open(surfaceId: string) {
  off = await useStore.getState().init();
  await api.activateSurface(surfaceId);
  await api.setMode("edit");
  await waitFor(() => expect(useStore.getState().state?.mode).toBe("edit"));
  useStore.setState({ view: "surface", selected: null, intent: null, libraryOpen: false });
  render(<SurfaceView />);
}

afterEach(() => {
  cleanup();
  off();
  vi.restoreAllMocks();
});

const surface = (id: string) => useStore.getState().workspace?.surfaces.find((s) => s.id === id);
const pages = () => screen.getByRole("navigation", { name: "Pages" });

describe("renaming in place", () => {
  it("renames the surface from its header", async () => {
    const user = userEvent.setup();
    await open("development");
    const name = screen.getByRole("textbox", { name: "Surface name" });
    await user.clear(name);
    await user.type(name, "Code{Enter}");
    await waitFor(() => expect(surface("development")?.name).toBe("Code"));
    expect(screen.getByRole("textbox", { name: "Surface name" })).toHaveProperty("value", "Code");
  });

  it("saves the icon and description, and Escape undoes an unsaved change", async () => {
    const user = userEvent.setup();
    await open("development");
    const save = vi.spyOn(api, "updateSurface");
    const description = screen.getByRole("textbox", { name: "Surface description" });
    await user.type(description, " and more{Escape}");
    expect(description).toHaveProperty("value", surface("development")?.description);
    await user.tab();
    expect(save).not.toHaveBeenCalled();

    const icon = screen.getByRole("textbox", { name: "Surface icon" });
    await user.clear(icon);
    await user.type(icon, "⌨{Enter}");
    await waitFor(() => expect(surface("development")?.icon).toBe("⌨"));
    expect(save).toHaveBeenCalledWith("development", "Code", surface("development")?.description, "⌨");
  });

  it("renames a page from its menu", async () => {
    const user = userEvent.setup();
    await open("development");
    await user.click(within(pages()).getByRole("button", { name: "Page options for Git" }));
    await user.click(screen.getByRole("menuitem", { name: "Rename" }));
    const field = screen.getByRole("textbox", { name: "Page name" });
    await user.clear(field);
    await user.type(field, "Source{Enter}");
    await waitFor(() => expect(surface("development")?.pages.map((p) => p.name)).toContain("Source"));
    expect(within(pages()).getByRole("button", { name: /^Source/ })).toBeTruthy();
  });
});

describe("deleting asks first", () => {
  it("names the Auto Context rules that go with a surface, and Cancel changes nothing", async () => {
    const user = userEvent.setup();
    await api.context.update({
      auto: true,
      rules: [{ application: { namespace: "macos", id: "com.microsoft.VSCode", displayName: "Visual Studio Code" }, surfaceId: "development" }],
      fallback: { kind: "surface", surfaceId: "development" },
    });
    await open("development");
    const remove = vi.spyOn(api, "removeSurface");
    const before = useStore.getState().workspace;

    await user.click(screen.getByRole("button", { name: "Delete surface" }));
    const dialog = screen.getByRole("alertdialog", { name: "Delete Code" });
    expect(dialog.textContent).toContain("Visual Studio Code");
    expect(dialog.textContent).toContain("no rule applies");
    expect(remove).not.toHaveBeenCalled();

    await user.click(within(dialog).getByRole("button", { name: "Cancel" }));
    expect(screen.queryByRole("alertdialog")).toBeNull();
    expect(remove).not.toHaveBeenCalled();
    expect(useStore.getState().workspace).toEqual(before);
  });

  it("deletes a page only after Delete is confirmed", async () => {
    const user = userEvent.setup();
    await open("accessibility");
    const remove = vi.spyOn(api, "removePage");
    const options = () => within(pages()).getByRole("button", { name: "Page options for Help" });

    await user.click(options());
    await user.click(screen.getByRole("menuitem", { name: "Delete" }));
    await user.click(within(screen.getByRole("alertdialog", { name: "Delete page Help" })).getByRole("button", { name: "Cancel" }));
    expect(remove).not.toHaveBeenCalled();
    expect(surface("accessibility")?.pages).toHaveLength(2);

    await user.click(options());
    await user.click(screen.getByRole("menuitem", { name: "Delete" }));
    await user.click(within(screen.getByRole("alertdialog", { name: "Delete page Help" })).getByRole("button", { name: "Delete" }));
    expect(remove).toHaveBeenCalledWith("accessibility", "help");
    await waitFor(() => expect(surface("accessibility")?.pages).toHaveLength(1));
  });

  it("does not offer to delete a surface's only page", async () => {
    const user = userEvent.setup();
    await open("productivity");
    await user.click(within(pages()).getByRole("button", { name: /^Page options for/ }));
    expect(screen.getByRole("menuitem", { name: "Delete" })).toHaveProperty("disabled", true);
  });

  it("deletes the surface after Delete is confirmed, with its rules", async () => {
    const user = userEvent.setup();
    await open("development");
    await user.click(screen.getByRole("button", { name: "Delete surface" }));
    await user.click(within(screen.getByRole("alertdialog")).getByRole("button", { name: "Delete" }));
    await waitFor(() => expect(surface("development")).toBeUndefined());
    expect(useStore.getState().state?.activeSurfaceId).toBe(useStore.getState().workspace?.surfaces[0]?.id);
    const context = await api.context.status();
    expect(context.settings.rules).toEqual([]);
    expect(context.settings.fallback).toEqual({ kind: "keep" });
  });
});

describe("a question asked about one surface", () => {
  const showing = async (surfaceId: string) => {
    await api.activateSurface(surfaceId);
    await waitFor(() => expect(useStore.getState().state?.activeSurfaceId).toBe(surfaceId));
  };

  it("is not carried over to the next surface shown", async () => {
    const user = userEvent.setup();
    await open("smart-home");
    await user.click(screen.getByRole("button", { name: "Delete surface" }));
    expect(screen.getByRole("alertdialog")).toBeTruthy();

    await showing("productivity");
    expect(screen.queryByRole("alertdialog")).toBeNull();
  });

  it("about a page is not carried over to a page of the same id elsewhere", async () => {
    const user = userEvent.setup();
    await api.addPage("smart-home", "Extra");
    await open("smart-home");
    await user.click(within(pages()).getByRole("button", { name: "Page options for Home" }));
    await user.click(screen.getByRole("menuitem", { name: "Delete" }));
    expect(screen.getByRole("alertdialog", { name: "Delete page Home" })).toBeTruthy();

    await showing("productivity");
    expect(screen.queryByRole("alertdialog")).toBeNull();
  });

  it("deleting the page on screen leaves no pad of it selected", async () => {
    const user = userEvent.setup();
    const spare = await api.addPage("agent-control", "Spare");
    await open("agent-control");
    await api.activatePage(spare);
    await waitFor(() => expect(useStore.getState().state?.activePageId).toBe(spare));
    useStore.getState().select({ kind: "grid", col: 0, row: 0 });

    await user.click(within(pages()).getByRole("button", { name: "Page options for Spare" }));
    await user.click(screen.getByRole("menuitem", { name: "Delete" }));
    await user.click(within(screen.getByRole("alertdialog", { name: "Delete page Spare" })).getByRole("button", { name: "Delete" }));
    await waitFor(() => expect(useStore.getState().state?.activePageId).not.toBe(spare));
    expect(useStore.getState().selected).toBeNull();
  });
});
