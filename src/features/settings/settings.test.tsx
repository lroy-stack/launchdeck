// @vitest-environment jsdom
// Settings as a person reads it: the sections in order, each control reaching the
// IPC call it stands for, the Developer section closed until opened, and no
// repository path in rendered text. About shows what Rust's `app_info` answers
// (examples/app-info.json, captured from Rust) and copies it for a bug report.

import { cleanup, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import appInfoJson from "../../../examples/app-info.json";
import { api } from "../../lib/ipc";
import { appInfoSchema } from "../../lib/schema";
import { useStore } from "../../stores/runtime";
import { SettingsView } from "./SettingsView";

const info = appInfoSchema.parse(appInfoJson);
const SYSTEM: Record<string, string> = { macos: "macOS", windows: "Windows", linux: "Linux" };
const system = `${SYSTEM[info.os] ?? info.os}, ${info.arch}`;

let off: () => void = () => undefined;

async function showSettings() {
  off = await useStore.getState().init();
  useStore.setState({ view: "settings" });
  return render(<SettingsView />);
}

const about = () => screen.getByRole("region", { name: "About" });
const section = (name: string) => screen.getByRole("region", { name });

const ORDER = ["Devices", "Auto Context", "Notifications", "Plugins & permissions", "Security", "Workspace", "Developer", "About"];

beforeEach(() => vi.restoreAllMocks());
afterEach(() => {
  cleanup();
  off();
});

describe("Settings > About", () => {
  it("parses the app-info fixture Rust wrote", () => {
    expect(info.channel).toBe("alpha");
    expect(info.manifestSchemas.oldest).toBeLessThanOrEqual(info.manifestSchemas.newest);
  });

  it("shows every version app_info reports, the channel and the license", async () => {
    await showSettings();
    const region = about();
    await waitFor(() => expect(within(region).getByText(`${info.appVersion} alpha`)).toBeTruthy());
    const row = (term: string) => within(region).getByText(term).nextElementSibling?.textContent;
    expect(row("Plugin SDK")).toBe(`${info.sdkVersion} (ABI ${info.abiVersion})`);
    expect(row("Plugin manifests")).toBe(`schemas ${info.manifestSchemas.oldest} to ${info.manifestSchemas.newest}`);
    expect(row("Workspace")).toBe(`schema ${info.workspaceSchema}`);
    expect(row("Agent Bridge")).toBe(`protocol ${info.bridgeProtocol}`);
    expect(row("Surface and Pack files")).toBe(`format ${info.artifactFormat}`);
    expect(row("System")).toBe(system);
    expect(row("License")).toBe("MIT");
  });

  it("copies a plain-text block for a bug report, without the data folder", async () => {
    // user-event puts a clipboard of its own on navigator.
    const user = userEvent.setup();
    await showSettings();
    const button = await within(about()).findByRole("button", { name: "Copy for a bug report" });
    await waitFor(() => expect((button as HTMLButtonElement).disabled).toBe(false));
    await user.click(button);
    const text = await navigator.clipboard.readText();
    expect(text.split("\n")).toEqual([
      `LaunchDeck ${info.appVersion} (alpha)`,
      `System: ${system}`,
      `Plugin SDK: ${info.sdkVersion} (ABI ${info.abiVersion})`,
      `Plugin manifests: schemas ${info.manifestSchemas.oldest} to ${info.manifestSchemas.newest}`,
      `Workspace: schema ${info.workspaceSchema}`,
      `Agent Bridge: protocol ${info.bridgeProtocol}`,
      `Surface and Pack files: format ${info.artifactFormat}`,
      "License: MIT",
    ]);
    expect(text).not.toContain(info.dataFolder);
    expect(await within(about()).findByRole("button", { name: "Copied" })).toBeTruthy();
  });
});

describe("Settings", () => {
  it("has its sections in order, with a list that leads to each", async () => {
    await showSettings();
    expect(screen.getAllByRole("heading", { level: 2 }).map((h) => h.textContent)).toEqual(ORDER);
    const list = screen.getByRole("navigation", { name: "Settings sections" });
    expect(within(list).getAllByRole("button").map((b) => b.textContent)).toEqual(ORDER);
    for (const name of ORDER) expect(section(name)).toBeTruthy();
    await userEvent.click(within(list).getByRole("button", { name: "About" }));
  });

  it("keeps Developer closed, the WebSocket tester out of sight until it is opened", async () => {
    const send = vi.spyOn(api, "sendEvent").mockResolvedValue(undefined);
    await showSettings();
    const developer = section("Developer");
    expect(developer.querySelector("details")!.open).toBe(false);
    expect(within(developer).queryByText(/ws:\/\/127\.0\.0\.1/)).toBeNull();
    expect(within(developer).queryByRole("button", { name: "Send test event" })).toBeNull();

    await userEvent.click(within(developer).getByText("Developer"));
    expect(developer.querySelector("details")!.open).toBe(true);
    expect(await within(developer).findByText(/ws:\/\/127\.0\.0\.1/)).toBeTruthy();
    await userEvent.click(within(developer).getByRole("button", { name: "Send test event" }));
    expect(send).toHaveBeenCalledWith("codex", "task.started");
    expect(await within(developer).findByText(info.dataFolder)).toBeTruthy();
  });

  it("shows the data folder and the welcome again from Developer", async () => {
    const reveal = vi.spyOn(api, "revealDataFolder").mockResolvedValue(undefined);
    const welcome = vi.spyOn(api, "setOnboardingDone").mockResolvedValue(undefined);
    await showSettings();
    const developer = section("Developer");
    await userEvent.click(within(developer).getByText("Developer"));
    await userEvent.click(await within(developer).findByRole("button", { name: "Show data folder" }));
    expect(reveal).toHaveBeenCalledOnce();
    await userEvent.click(within(developer).getByRole("button", { name: "Show the welcome again" }));
    expect(welcome).toHaveBeenCalledExactlyOnceWith(false);
  });

  it("resets the workspace only after the confirmation", async () => {
    const reset = vi.spyOn(api, "resetWorkspace").mockResolvedValue(undefined);
    await showSettings();
    const workspace = section("Workspace");
    await userEvent.click(within(workspace).getByRole("button", { name: "Reset to starter surfaces" }));
    expect(within(workspace).getByText("Replace everything with the starter surfaces?")).toBeTruthy();
    await userEvent.click(within(workspace).getByRole("button", { name: "Cancel" }));
    expect(reset).not.toHaveBeenCalled();

    await userEvent.click(within(workspace).getByRole("button", { name: "Reset to starter surfaces" }));
    await userEvent.click(within(workspace).getByRole("button", { name: "Reset" }));
    expect(reset).toHaveBeenCalledOnce();
    expect(within(workspace).getByRole("button", { name: "Reset to starter surfaces" })).toBeTruthy();
  });

  it("turns notifications off through Agent Control's settings", async () => {
    const update = vi.spyOn(api.agents, "updateSettings").mockResolvedValue(undefined);
    await showSettings();
    const box = within(section("Notifications")).getByRole("checkbox", { name: "Enabled" });
    expect((box as HTMLInputElement).checked).toBe(true);
    await userEvent.click(box);
    expect(update).toHaveBeenCalledExactlyOnceWith({ notifications: false });
  });

  it("says where plugin secrets are kept, shows the plugins folder and opens Plugins", async () => {
    const reveal = vi.spyOn(api.plugins, "revealFolder").mockResolvedValue(undefined);
    await showSettings();
    const plugins = section("Plugins & permissions");
    // The browser preview's store is the in-memory one.
    expect(await within(plugins).findByText("Plugin secrets are kept in memory only, and forgotten when LaunchDeck closes.")).toBeTruthy();
    await userEvent.click(within(plugins).getByRole("button", { name: "Show plugins folder" }));
    expect(reveal).toHaveBeenCalledOnce();
    await userEvent.click(within(plugins).getByRole("button", { name: "Open Plugins" }));
    expect(useStore.getState().view).toBe("plugins");
  });

  it("names the unauthenticated Agent Bridge under Security", async () => {
    await showSettings();
    expect(within(section("Security")).getByText(/The Agent Bridge is not authenticated/)).toBeTruthy();
  });

  // The Plugins screen has the same test in plugins/plugins.test.tsx.
  it("renders no repository path in Settings or its Auto Context panel", async () => {
    const { container } = await showSettings();
    await userEvent.click(within(section("Developer")).getByText("Developer"));
    await waitFor(() => expect(within(about()).getByText(`${info.appVersion} alpha`)).toBeTruthy());
    expect(within(section("Auto Context")).getByText(/Nothing about which applications you use is saved/)).toBeTruthy();
    expect(container.textContent).not.toMatch(/docs\/|\.md\b/);
  });
});
