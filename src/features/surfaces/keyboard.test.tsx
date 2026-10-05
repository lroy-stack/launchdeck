// @vitest-environment jsdom
// The Launchpad surface under the keyboard: one tab stop, arrows between pads,
// Enter and Space doing what a click or a finger does.

import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { api } from "../../lib/ipc";
import { posKey } from "../../lib/schema";
import { useStore } from "../../stores/runtime";
import { SurfaceView } from "./SurfaceView";

let off: () => void = () => undefined;

async function open(mode: "live" | "edit") {
  off = await useStore.getState().init();
  await api.activateSurface("development");
  await api.setMode(mode);
  await waitFor(() => expect(useStore.getState().state?.mode).toBe(mode));
  useStore.setState({ view: "surface", selected: null, intent: null, libraryOpen: false });
  render(<SurfaceView />);
}

afterEach(() => {
  cleanup();
  off();
  vi.restoreAllMocks();
});

const deck = () => screen.getByRole("group", { name: "Launchpad surface" });
const pads = () => within(deck()).getAllByRole("button");
const stops = () => pads().filter((p) => p.tabIndex === 0);
// Places as drawn on a Launchpad X: row 1 is the top row, column 9 the side column, the 8x8 grid in between.
const keyOf = (col: number, row: number) => (row === 1 ? `t.${col - 1}` : col === 9 ? `s.${row - 2}` : `g.${col - 1}.${row - 2}`);
const at = (col: number, row: number) => deck().querySelector<HTMLElement>(`[data-cell="${keyOf(col, row)}"]`)!;
const place = (el: Element | null) => {
  const [kind, a, b] = (el as HTMLElement).dataset.cell!.split(".");
  return kind === "t" ? [Number(a) + 1, 1] : kind === "s" ? [9, Number(a) + 2] : [Number(a) + 1, Number(b) + 2];
};

describe("the surface is one tab stop", () => {
  beforeEach(() => open("edit"));

  it("has exactly one pad Tab can land on, however many pads there are", () => {
    expect(pads().length).toBeGreaterThan(70);
    expect(stops()).toHaveLength(1);
    // The first pad in reading order, when nothing is selected or was focused.
    expect(place(stops()[0]!)).toEqual([1, 1]);
  });

  it("leaves the surface with one Tab instead of walking every pad", async () => {
    const user = userEvent.setup();
    stops()[0]!.focus();
    await user.tab();
    expect(deck().contains(document.activeElement)).toBe(false);
    await user.tab({ shift: true });
    expect(document.activeElement).toBe(stops()[0]);
  });

  it("makes the selected pad the tab stop, so Tab comes back to where the work is", async () => {
    const user = userEvent.setup();
    await user.click(at(3, 5));
    expect(stops()).toEqual([at(3, 5)]);
  });
});

describe("arrow keys move between pads", () => {
  beforeEach(() => open("edit"));

  it("moves one pad at a time in the four directions and takes the tab stop along", async () => {
    const user = userEvent.setup();
    at(3, 5).focus();
    await user.keyboard("{ArrowRight}");
    expect(place(document.activeElement)).toEqual([4, 5]);
    await user.keyboard("{ArrowDown}");
    expect(place(document.activeElement)).toEqual([4, 6]);
    await user.keyboard("{ArrowLeft}{ArrowLeft}");
    expect(place(document.activeElement)).toEqual([2, 6]);
    await user.keyboard("{ArrowUp}");
    expect(place(document.activeElement)).toEqual([2, 5]);
    expect(stops()).toEqual([document.activeElement]);
  });

  it("stays put at an edge", async () => {
    const user = userEvent.setup();
    at(1, 1).focus();
    await user.keyboard("{ArrowLeft}{ArrowUp}");
    expect(place(document.activeElement)).toEqual([1, 1]);
  });

  it("reaches the top row and the side column, which are pads like the others", async () => {
    const user = userEvent.setup();
    at(8, 2).focus();
    await user.keyboard("{ArrowUp}");
    expect(place(document.activeElement)).toEqual([8, 1]);
    await user.keyboard("{ArrowDown}{ArrowRight}");
    expect(place(document.activeElement)).toEqual([9, 2]);
    // The corner above the side column is the logo, not a pad: the arrow stays.
    await user.keyboard("{ArrowUp}");
    expect(place(document.activeElement)).toEqual([9, 2]);
  });

  it("goes to the ends of a row with Home and End, and of the surface with Ctrl", async () => {
    const user = userEvent.setup();
    at(4, 5).focus();
    await user.keyboard("{End}");
    expect(place(document.activeElement)).toEqual([9, 5]);
    await user.keyboard("{Home}");
    expect(place(document.activeElement)).toEqual([1, 5]);
    await user.keyboard("{Control>}{End}{/Control}");
    expect(place(document.activeElement)).toEqual([9, 9]);
    await user.keyboard("{Control>}{Home}{/Control}");
    expect(place(document.activeElement)).toEqual([1, 1]);
  });

  it("leaves Alt+arrows alone", async () => {
    const user = userEvent.setup();
    at(3, 5).focus();
    await user.keyboard("{Alt>}{ArrowRight}{/Alt}");
    expect(place(document.activeElement)).toEqual([3, 5]);
  });
});

describe("Enter and Space", () => {
  it("select the pad in Edit mode, as a click does", async () => {
    await open("edit");
    const user = userEvent.setup();
    at(6, 7).focus();
    await user.keyboard("{Enter}");
    expect(posKey(useStore.getState().selected!)).toBe(posKey({ kind: "grid", col: 5, row: 5 }));
    await user.keyboard("{ArrowRight} ");
    expect(posKey(useStore.getState().selected!)).toBe(posKey({ kind: "grid", col: 6, row: 5 }));
    expect(at(7, 7).getAttribute("aria-pressed")).toBe("true");
  });

  it("press the pad in Live mode for as long as the key is held, once, and never select", async () => {
    await open("live");
    const input = vi.spyOn(api, "virtualInput").mockResolvedValue(undefined);
    const pad = at(1, 2);
    pad.focus();
    fireEvent.keyDown(pad, { key: "Enter" });
    fireEvent.keyDown(pad, { key: "Enter", repeat: true });
    expect(input.mock.calls).toEqual([[{ kind: "grid", col: 0, row: 0 }, true]]);
    fireEvent.keyUp(pad, { key: "Enter" });
    expect(input.mock.calls).toEqual([
      [{ kind: "grid", col: 0, row: 0 }, true],
      [{ kind: "grid", col: 0, row: 0 }, false],
    ]);
    fireEvent.keyDown(pad, { key: " " });
    fireEvent.blur(pad);
    expect(input.mock.calls.slice(2)).toEqual([
      [{ kind: "grid", col: 0, row: 0 }, true],
      [{ kind: "grid", col: 0, row: 0 }, false],
    ]);
    expect(useStore.getState().selected).toBeNull();
  });

  it("leaves the pointer as it was: down presses, up releases", async () => {
    await open("live");
    const input = vi.spyOn(api, "virtualInput").mockResolvedValue(undefined);
    const pad = at(1, 2);
    pad.setPointerCapture = () => undefined;
    fireEvent.pointerDown(pad, { pointerId: 1 });
    fireEvent.pointerUp(pad, { pointerId: 1 });
    expect(input.mock.calls.map((c) => c[1])).toEqual([true, false]);
  });
});
