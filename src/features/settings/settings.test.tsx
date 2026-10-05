// @vitest-environment jsdom
// Settings as a person reads it. About shows what Rust's `app_info` answers
// (examples/app-info.json, captured from Rust) and copies it for a bug report.

import { cleanup, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import appInfoJson from "../../../examples/app-info.json";
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
