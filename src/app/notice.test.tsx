// @vitest-environment jsdom
// A saved workspace that could not be used: the app says so in a banner that stays, from what the snapshot carries.

import { cleanup, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";
import { api } from "../lib/ipc";
import { snapshotSchema, type StartupNotice } from "../lib/schema";
import { useStore } from "../stores/runtime";
import { App } from "./App";

const BACKUP = "/Users/someone/Library/Application Support/dev.launchdeck.app/workspace.json.bak";

/** The preview's own snapshot, with these notices in it. */
function startWith(notices: StartupNotice[]) {
  const snapshot = api.snapshot;
  vi.spyOn(api, "snapshot").mockImplementation(async () => ({ ...(await snapshot()), notices }));
}

async function renderApp() {
  render(<App />);
  await waitFor(() => expect(useStore.getState().ready).toBe(true));
}

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
  useStore.setState({ ready: false, notices: [] });
});

describe("the notice of a workspace that could not be opened", () => {
  it("reads the shape Rust writes (persistence::tests::a_notice_reaches_the_ui_in_camel_case)", () => {
    const rust = { kind: "workspaceTooNew", backup: "/data/workspace.json.bak" };
    expect(snapshotSchema.shape.notices.parse([rust])).toEqual([rust]);
    expect(snapshotSchema.shape.notices.parse(undefined)).toEqual([]);
    expect(() => snapshotSchema.shape.notices.parse([{ ...rust, kind: "lost" }])).toThrow();
  });

  it("says what happened, shows the folder and keeps the path under Details", async () => {
    const user = userEvent.setup();
    startWith([{ kind: "workspaceDamaged", backup: BACKUP }]);
    const reveal = vi.spyOn(api, "revealDataFolder").mockResolvedValue(undefined);
    await renderApp();

    const banner = await screen.findByRole("alert");
    expect(banner.textContent).toContain("Your surfaces could not be opened, so LaunchDeck started fresh. Your file was kept as workspace.json.bak.");
    expect(within(banner).getByText(BACKUP).closest("details")).not.toBeNull();
    await user.click(within(banner).getByRole("button", { name: "Show folder" }));
    expect(reveal).toHaveBeenCalledOnce();
    // It is not a toast: it is still there.
    expect(screen.getByRole("alert")).toBe(banner);
  });

  it("says when a newer LaunchDeck saved the file, and goes when dismissed", async () => {
    const user = userEvent.setup();
    startWith([{ kind: "workspaceTooNew", backup: BACKUP }]);
    await renderApp();

    const banner = await screen.findByRole("alert");
    expect(banner.textContent).toContain("Your surfaces were saved by a newer LaunchDeck, so this one started fresh.");
    await user.click(within(banner).getByRole("button", { name: "Dismiss" }));
    expect(screen.queryByRole("alert")).toBeNull();
  });

  it("is absent when the workspace opened", async () => {
    await renderApp();
    expect(useStore.getState().notices).toEqual([]);
    expect(screen.queryByText(/started fresh/)).toBeNull();
  });
});
