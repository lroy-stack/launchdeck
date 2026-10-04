// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { DRAG_TYPE } from "../../lib/assign";
import { api } from "../../lib/ipc";
import type { Control } from "../../lib/schema";
import { activePage, useStore } from "../../stores/runtime";
import { SurfaceView } from "../surfaces/SurfaceView";

// The whole Surface editor on the preview transport, which serves the catalog and
// workspace Rust emitted. What these tests check is the editor's behaviour: finding
// an action, giving it to a pad, filling its form, saving, replacing.

let off: () => void = () => undefined;

const pad = (col: number, row: number) => ({ kind: "grid", col, row }) as const;
const search = () => screen.getByRole("combobox", { name: "Search actions" });
/** The rows of the Library only: a select's own <option>s have the same role. */
const rows = () => within(screen.getByRole("listbox", { name: "Actions" })).queryAllByRole("option");
const row = (name: RegExp) => within(screen.getByRole("listbox", { name: "Actions" })).getByRole("option", { name });
const savedControl = (label: string): Control | undefined => activePage(useStore.getState())?.controls.find((c) => c.label === label);

beforeEach(async () => {
  off = await useStore.getState().init();
  await api.activateSurface("development");
  await api.setMode("edit");
  await waitFor(() => expect(useStore.getState().state?.mode).toBe("edit"));
  useStore.setState({ view: "surface", selected: null, intent: null, libraryOpen: true, libraryQuery: "", libraryCategory: null, dragging: false });
});

afterEach(() => {
  cleanup();
  off();
});

describe("the Action Library", () => {
  it("lists every action of the catalog, grouped, and says where each comes from", () => {
    render(<SurfaceView />);
    expect(rows()).toHaveLength(useStore.getState().catalog.catalog.actions.length);
    expect(within(row(/Open URL/)).getByText("Built-in")).toBeTruthy();
    expect(within(row(/Say hello/)).getByText("Plugin: Hello Plugin")).toBeTruthy();
    expect(within(row(/Approve.*Agent Control/s)).getByText("Agent Control")).toBeTruthy();
  });

  it("narrows as you type and says how many it found", async () => {
    const user = userEvent.setup();
    render(<SurfaceView />);
    await user.type(search(), "terminal");
    expect(rows()).toHaveLength(1);
    expect(rows()[0]!.textContent).toContain("Run shell command");
    expect(screen.getByText("1 action found")).toBeTruthy();
    await user.clear(search());
    await user.type(search(), "zzzzqq");
    expect(rows()).toHaveLength(0);
    expect(screen.getByText(/Nothing matches/)).toBeTruthy();
    await user.click(screen.getByRole("button", { name: "Show everything" }));
    expect(rows().length).toBeGreaterThan(20);
  });

  it("filters by category and keeps searching inside it", async () => {
    const user = userEvent.setup();
    render(<SurfaceView />);
    await user.selectOptions(screen.getByRole("combobox", { name: "Category" }), "Navigation");
    expect(rows().map((o) => o.textContent)).toEqual([expect.stringContaining("Open page"), expect.stringContaining("Open surface")]);
    await user.type(search(), "shell");
    expect(rows()).toHaveLength(0);
  });

  it("shows unavailable actions, with the reason, instead of hiding them", async () => {
    const user = userEvent.setup();
    render(<SurfaceView />);
    await user.type(search(), "increment");
    expect(within(row(/Increment/)).getByText("Unavailable")).toBeTruthy();
    expect(screen.getByText(/Unavailable now: The plugin is turned off/)).toBeTruthy();
    // And says which sources are not running, with a way to fix it.
    expect(screen.getByText(/2 sources not running/)).toBeTruthy();
  });

  it("moves with the arrow keys, assigns with Enter, clears with Escape", async () => {
    const user = userEvent.setup();
    useStore.getState().select(pad(7, 7));
    render(<SurfaceView />);
    await user.type(search(), "open");
    expect(rows().length).toBeGreaterThan(1);
    expect(search().getAttribute("aria-activedescendant")).toBe(rows()[0]!.id);
    await user.keyboard("{ArrowDown}");
    expect(search().getAttribute("aria-activedescendant")).toBe(rows()[1]!.id);
    expect(rows()[1]!.getAttribute("aria-selected")).toBe("true");
    // Walk down to Open URL and take it.
    const target = rows().findIndex((r) => r.textContent?.includes("Open URL"));
    expect(target).toBeGreaterThan(1);
    await user.keyboard("{ArrowDown}".repeat(target - 1));
    expect(rows()[target]!.getAttribute("aria-selected")).toBe("true");
    await user.keyboard("{Enter}");
    expect(await screen.findByLabelText(/^Link/)).toBeTruthy();
    await user.keyboard("{Escape}");
    expect((search() as HTMLInputElement).value).toBe("");
  });

  it("jumps to the search when / is pressed outside a field", () => {
    render(<SurfaceView />);
    (document.activeElement as HTMLElement | null)?.blur();
    fireEvent.keyDown(document.body, { key: "/" });
    expect(document.activeElement).toBe(search());
  });

  it("collapses to a rail and opens again", async () => {
    const user = userEvent.setup();
    render(<SurfaceView />);
    await user.click(screen.getByRole("button", { name: "Collapse the Action Library" }));
    expect(screen.queryByRole("combobox", { name: "Search actions" })).toBeNull();
    await user.click(screen.getByRole("button", { name: "Open the Action Library" }));
    expect(search()).toBeTruthy();
  });

  it("asks for a pad first when none is selected", async () => {
    const user = userEvent.setup();
    render(<SurfaceView />);
    await user.type(search(), "open url");
    expect((screen.getByRole("button", { name: "Assign to a pad" }) as HTMLButtonElement).disabled).toBe(true);
  });
});

describe("giving an action to a pad", () => {
  it("goes from the library to a form, to a saved assignment, with the action's look on a new pad", async () => {
    const user = userEvent.setup();
    useStore.getState().select(pad(7, 7));
    render(<SurfaceView />);
    await user.type(search(), "open url");
    await user.click(screen.getByRole("button", { name: /^Assign to Pad column 8, row 8/ }));

    const link = await screen.findByLabelText(/^Link/);
    expect((screen.getByLabelText("Label", { exact: true }) as HTMLInputElement).value).toBe("Open link"); // the suggested look, on a pad nobody had touched
    // The form is the schema's: a needed field, nothing said about it until it is visited.
    expect(screen.queryByText("Link is required")).toBeNull();
    const save = screen.getByRole("button", { name: "Add control" }) as HTMLButtonElement;
    expect(save.disabled).toBe(true);
    expect(screen.getByText("Fill in the settings marked (needed) to save.")).toBeTruthy();

    await user.type(link, "https://example.com/docs");
    await waitFor(() => expect(save.disabled).toBe(false));
    await user.click(save);

    await waitFor(() => expect(savedControl("Open link")).toBeDefined());
    const saved = savedControl("Open link")!;
    expect(saved.press).toEqual({ provider: "system", action: "openUrl", args: { url: "https://example.com/docs" } });
    expect(saved.visual.color).toBe("#2f81f7");
  });

  it("blocks saving a value the rules refuse, and says which setting", async () => {
    const user = userEvent.setup();
    useStore.getState().select(pad(6, 7));
    render(<SurfaceView />);
    await user.type(search(), "keyboard shortcut");
    await user.keyboard("{Enter}");
    // A number outside its range, on an action that has one.
    await user.clear(search());
    await user.type(search(), "focus slot");
    await user.keyboard("{Enter}");
    await screen.findByText(/Replace/); // another action is already in the draft: asks first
    await user.click(screen.getByRole("button", { name: "Replace" }));
    const slot = await screen.findByLabelText(/^Track/);
    await user.clear(slot);
    await user.type(slot, "99");
    expect(await screen.findByText(/Track must be between/)).toBeTruthy();
    expect(screen.getByRole("button", { name: "Add control" }).hasAttribute("disabled")).toBe(true);
    expect(screen.getByText("Fix the highlighted settings to save.")).toBeTruthy();
  });

  it("asks before replacing an action, names what is lost, and keeps the pad's look", async () => {
    const user = userEvent.setup();
    const before = savedControl("Terminal")!;
    expect(before.press?.provider).toBeDefined();
    useStore.getState().select(before.pos);
    render(<SurfaceView />);
    await user.type(search(), "open url");
    await user.click(screen.getByRole("button", { name: /^Assign to Terminal/ }));

    const confirm = await screen.findByRole("group", { name: "Confirm replacing the action" });
    expect(confirm.textContent).toContain("This replaces");
    expect(confirm.textContent).toContain("Open URL");
    expect(confirm.textContent).toContain("state looks and bindings are not changed");
    // Nothing happened yet.
    expect(screen.queryByLabelText(/^Link/)).toBeNull();
    await user.click(within(confirm).getByRole("button", { name: "Keep current" }));
    expect(screen.queryByRole("group", { name: "Confirm replacing the action" })).toBeNull();
    expect(screen.queryByText("Not saved")).toBeNull();

    await user.click(screen.getByRole("button", { name: /^Assign to Terminal/ }));
    await user.click(within(await screen.findByRole("group", { name: "Confirm replacing the action" })).getByRole("button", { name: "Replace" }));
    await screen.findByLabelText(/^Link/);
    // Only the assignment changed in the draft; label, colour and look stay.
    expect((screen.getByLabelText("Label", { exact: true }) as HTMLInputElement).value).toBe(before.label);
    // The offer of the new look is explicit.
    expect(screen.getByRole("button", { name: "Use suggested look" })).toBeTruthy();
  });

  it("does nothing, and says so, when the same action is chosen again", async () => {
    const user = userEvent.setup();
    useStore.getState().select(pad(7, 6));
    render(<SurfaceView />);
    await user.type(search(), "shell");
    await user.click(screen.getByRole("button", { name: /^Assign to / }));
    const command = await screen.findByLabelText(/^Command/);
    await user.type(command, "echo hi");
    await user.click(screen.getByRole("button", { name: /^Assign to / }));
    expect((screen.getByLabelText(/^Command/) as HTMLTextAreaElement).value).toBe("echo hi");
  });

  it("accepts an action dropped on a pad, and on the long press slot", async () => {
    render(<SurfaceView />);
    const source = row(/Open URL/);
    const transfer = { types: [DRAG_TYPE], getData: (t: string) => (t === DRAG_TYPE ? "system.openUrl" : ""), setData: () => undefined, effectAllowed: "", dropEffect: "" };
    fireEvent.dragStart(source, { dataTransfer: transfer });
    expect(useStore.getState().dragging).toBe(true);
    const empty = screen.getAllByRole("button", { name: "Empty pad" })[5]!;
    fireEvent.dragOver(empty, { dataTransfer: transfer });
    fireEvent.drop(empty, { dataTransfer: transfer });
    await screen.findByLabelText(/^Link/);
    expect(useStore.getState().selected).not.toBeNull();

    const longPress = screen.getByRole("region", { name: "On long press" });
    fireEvent.drop(longPress, { dataTransfer: { ...transfer, getData: () => "system.launchApplication" } });
    await within(longPress).findByLabelText(/^Application/);
    fireEvent.dragEnd(source);
    expect(useStore.getState().dragging).toBe(false);
  });

  it("ignores a drop that is not an action", () => {
    render(<SurfaceView />);
    const empty = screen.getAllByRole("button", { name: "Empty pad" })[0]!;
    fireEvent.drop(empty, { dataTransfer: { types: ["text/plain"], getData: (t: string) => (t === "text/plain" ? "hello" : "") } });
    expect(useStore.getState().intent).toBeNull();
    expect(useStore.getState().selected).toBeNull();
  });
});

describe("a pad whose action cannot run", () => {
  it("is flagged on the surface and explained in the inspector, and nothing is deleted", async () => {
    const user = userEvent.setup();
    const ws = useStore.getState().workspace!;
    const surface = ws.surfaces.find((s) => s.id === "development")!;
    const page = surface.pages[0]!;
    const gone: Control = { ...page.controls[0]!, id: "gone", label: "Gone pad", pos: pad(4, 6), press: { provider: "acme.vanished", action: "go", args: { speed: 3 } } };
    useStore.setState({ workspace: { ...ws, surfaces: ws.surfaces.map((s) => (s.id === surface.id ? { ...s, pages: s.pages.map((p, i) => (i === 0 ? { ...p, controls: [...p.controls, gone] } : p)) } : s)) } });

    render(<SurfaceView />);
    const padButton = screen.getByRole("button", { name: /Gone pad\. Action unavailable/ });
    expect(padButton.getAttribute("title")).toMatch(/not installed/);
    await user.click(padButton);

    expect(await screen.findByText("acme.vanished.go")).toBeTruthy();
    expect(screen.getByText(/The provider “acme.vanished” is not installed/)).toBeTruthy();
    // The saved settings stay visible, untouched, and the assignment can still be removed on purpose.
    expect(screen.getByText("speed")).toBeTruthy();
    expect((screen.getByRole("button", { name: "Test run" }) as HTMLButtonElement).disabled).toBe(true);
    expect((screen.getByLabelText("Label", { exact: true }) as HTMLInputElement).value).toBe("Gone pad");
    // Editing the label does not need the missing provider.
    await user.type(screen.getByLabelText("Label", { exact: true }), "!");
    expect((screen.getByRole("button", { name: "Save" }) as HTMLButtonElement).disabled).toBe(false);
  });
});
