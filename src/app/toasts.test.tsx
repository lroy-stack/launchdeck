// @vitest-environment jsdom
// A failure a command reports: Rust's sentence in the toast, what it kept for Details behind a disclosure.

import { act, cleanup, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";
import { api } from "../lib/ipc";
import { useStore } from "../stores/runtime";
import { App } from "./App";

async function renderApp() {
  render(<App />);
  await waitFor(() => expect(useStore.getState().ready).toBe(true));
}

const toasts = () => screen.getByRole("status", { name: "Notifications" });

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
  vi.useRealTimers();
  useStore.setState({ ready: false, fatal: null, toasts: [] });
});

describe("error toasts", () => {
  it("show the sentence, and the detail only after Details is opened", async () => {
    const user = userEvent.setup();
    await renderApp();
    const fail = () => Promise.reject({ message: "The Launchpad could not be reached.", detail: "port busy: Launchpad Mini MK3 LPMiniMK3 MIDI" });
    await act(async () => {
      expect(await useStore.getState().attempt(fail)).toBe(false);
    });

    const toast = within(toasts()).getByText("The Launchpad could not be reached.").closest(".toast") as HTMLElement;
    expect(toast.textContent).not.toContain("port busy");
    await user.click(within(toast).getByText("Details"));
    expect(within(toast).getByText("port busy: Launchpad Mini MK3 LPMiniMK3 MIDI")).toBeTruthy();
    await user.click(within(toast).getByRole("button", { name: "Dismiss" }));
    expect(within(toasts()).queryByText("The Launchpad could not be reached.")).toBeNull();
  });

  it("keep an error with details until it is dismissed, and let a plain one go by itself", async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    await renderApp();
    await act(async () => {
      await useStore.getState().attempt(() => Promise.reject({ message: "Kept.", detail: "why" }));
      await useStore.getState().attempt(() => Promise.reject("Gone soon."));
    });
    expect(within(toasts()).getByText("Gone soon.")).toBeTruthy();
    act(() => void vi.advanceTimersByTime(60_000));
    expect(within(toasts()).queryByText("Gone soon.")).toBeNull();
    expect(within(toasts()).getByText("Kept.")).toBeTruthy();
    expect(useStore.getState().toasts).toEqual([expect.objectContaining({ message: "Kept.", detail: "why", tone: "error" })]);
  });
});

describe("the fatal screen", () => {
  it("shows the sentence and keeps the detail behind Details, with no raw dump", async () => {
    const user = userEvent.setup();
    vi.spyOn(api, "snapshot").mockRejectedValue({ message: "LaunchDeck could not read its settings.", detail: "EACCES /data/workspace.json" });
    render(<App />);
    expect(await screen.findByText("LaunchDeck could not read its settings.")).toBeTruthy();
    expect(document.querySelector("pre")).toBeNull();
    expect(screen.queryByText("EACCES /data/workspace.json")).toBeNull();
    await user.click(screen.getByText("Details"));
    expect(screen.getByText("EACCES /data/workspace.json")).toBeTruthy();
  });
});
