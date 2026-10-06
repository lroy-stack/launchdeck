// @vitest-environment jsdom
// Device status in the sidebar and in Settings > Devices, for each status
// Rust reports (`DeviceStatus` in devices/manager.rs). The words come from
// src/lib/devices.ts; MIDI and Programmer mode appear only under Technical details.

import { cleanup, render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { Sidebar } from "../../app/Sidebar";
import { api } from "../../lib/ipc";
import type { DeviceInfo, DeviceStatus } from "../../lib/schema";
import { useStore } from "../../stores/runtime";
import { SettingsView } from "./SettingsView";

const caps = (grid: boolean): DeviceInfo["capabilities"] => ({
  grid: grid ? { width: 8, height: 8 } : null,
  aux: grid ? { top: 8, side: 8, logo: true } : { top: 0, side: 0, logo: false },
  rgb: grid,
  velocity: grid,
  pressure: false,
  animations: { pulse: false, flash: false },
});

const PROBLEM = "LaunchDeck could not open the Launchpad. Another program may be using it; close it and press Look again.";

const simulator: DeviceInfo = {
  identity: { id: "virtual-launchpad-x", name: "Virtual Launchpad X", model: "launchpad-x", kind: "virtual" },
  capabilities: caps(true),
  connected: true,
  status: "connected",
};

function hardware(status: DeviceStatus): DeviceInfo {
  const unsupported = status === "unsupported";
  return {
    identity: unsupported
      ? { id: "unsupported:launchpad-pro-mk3", name: "Launchpad Pro MK3", model: "unsupported", kind: "hardware" }
      : { id: "launchpad-x:lpx-midi", name: "Launchpad X", model: "launchpad-x", kind: "hardware" },
    capabilities: caps(!unsupported),
    connected: status === "connected",
    status,
    problem: status === "failed" ? PROBLEM : undefined,
  };
}

let off: () => void = () => undefined;

async function show(devices: DeviceInfo[], ui: "settings" | "sidebar") {
  off = await useStore.getState().init();
  const state = useStore.getState().state!;
  useStore.setState({ state: { ...state, devices }, view: "settings" });
  return render(ui === "settings" ? <SettingsView /> : <Sidebar />);
}

/** Text a person reads without opening Technical details. */
function normalText(container: HTMLElement): string {
  const copy = container.cloneNode(true) as HTMLElement;
  copy.querySelectorAll("details").forEach((d) => d.remove());
  return copy.textContent ?? "";
}

beforeEach(() => vi.restoreAllMocks());
afterEach(() => {
  cleanup();
  off();
});

const WORDS: Record<DeviceStatus, string> = {
  connected: "Connected",
  available: "Found, not connected",
  unplugged: "Unplugged: plug it in again and it reconnects",
  unsupported: "Not supported yet: LaunchDeck works with the Launchpad X and the Launchpad Mini MK3",
  failed: "Could not connect",
};

const devicesList = () => screen.getByRole("list", { name: "Devices" });
const rowOf = (name: string) => within(devicesList()).getAllByRole("listitem").find((li) => li.querySelector("b")?.textContent === name)!;

describe("Settings > Devices", () => {
  for (const status of Object.keys(WORDS) as DeviceStatus[]) {
    it(`says ${status} in words, without MIDI terms`, async () => {
      const { container } = await show([simulator, hardware(status)], "settings");
      const name = status === "unsupported" ? "Launchpad Pro MK3" : "Launchpad X";
      const row = rowOf(name);
      expect(within(row).getByText(WORDS[status])).toBeTruthy();
      expect(within(rowOf("Virtual Launchpad X")).getByText("Simulator")).toBeTruthy();
      if (status === "failed") expect(within(row).getByText(PROBLEM)).toBeTruthy();
      expect(normalText(container)).not.toMatch(/midi|programmer/i);
    });
  }

  it("offers Connect, Disconnect or nothing, as the status allows", async () => {
    await show([simulator, hardware("available"), hardware("unsupported")], "settings");
    expect(within(rowOf("Virtual Launchpad X")).getByRole("button", { name: "Disconnect Virtual Launchpad X" })).toBeTruthy();
    expect(within(rowOf("Launchpad X")).getByRole("button", { name: "Connect Launchpad X" })).toBeTruthy();
    expect(within(rowOf("Launchpad Pro MK3")).queryByRole("button")).toBeNull();
    cleanup();
    off();
    await show([simulator, hardware("unplugged")], "settings");
    expect(within(rowOf("Launchpad X")).queryByRole("button")).toBeNull();
    cleanup();
    off();
    await show([simulator, hardware("failed")], "settings");
    expect(within(rowOf("Launchpad X")).getByRole("button", { name: "Connect Launchpad X" })).toBeTruthy();
  });

  it("looks again through the rescan command", async () => {
    const rescan = vi.spyOn(api, "rescanDevices").mockResolvedValue(undefined);
    await show([simulator], "settings");
    expect(screen.getByText("No Launchpad found. The simulator works the same way.")).toBeTruthy();
    await userEvent.click(screen.getByRole("button", { name: "Look again" }));
    expect(rescan).toHaveBeenCalledOnce();
  });

  it("asks for any Launchpad, and names each model's port only under Technical details", async () => {
    const { container } = await show([simulator], "settings");
    expect(screen.getByText("Plug in a Launchpad and LaunchDeck connects it within a few seconds. If it does not show up, press Look again.")).toBeTruthy();
    const details = container.querySelector("details")!;
    expect(details.textContent).toMatch(/LPX MIDI on a Launchpad X/);
    expect(details.textContent).toMatch(/LPMiniMK3 MIDI on a Launchpad Mini MK3/);
  });

  it("keeps capabilities and port words under Technical details", async () => {
    const { container } = await show([simulator, hardware("connected")], "settings");
    const details = container.querySelector("details")!;
    expect(within(details).getByText("Technical details")).toBeTruthy();
    expect(details.open).toBe(false);
    expect(details.textContent).toMatch(/Programmer Mode/);
    expect(details.textContent).toMatch(/8×8 grid/);
    expect(normalText(container)).not.toMatch(/grid|RGB/);
  });
});

describe("the sidebar's device card", () => {
  const card = () => screen.getByRole("button", { name: /Launchpad|No device/ });

  it("asks for a Launchpad, of either model, when there is no device", async () => {
    await show([], "sidebar");
    expect(within(card()).getByText("No device")).toBeTruthy();
    expect(within(card()).getByText("Plug in a Launchpad")).toBeTruthy();
  });

  it("shows connected hardware by its own name", async () => {
    await show([simulator, { ...hardware("connected"), identity: { ...hardware("connected").identity, name: "Launchpad X (2)" } }], "sidebar");
    expect(within(card()).getByText("Launchpad X (2)")).toBeTruthy();
    expect(within(card()).getByText("Connected")).toBeTruthy();
  });

  it("shows the simulator when no Launchpad X is there, even next to an unsupported model", async () => {
    await show([simulator, hardware("unsupported")], "sidebar");
    expect(within(card()).getByText("Virtual Launchpad X")).toBeTruthy();
    expect(within(card()).getByText("Simulator")).toBeTruthy();
  });

  for (const status of ["available", "unplugged", "failed"] as const) {
    it(`shows a ${status} Launchpad X over the simulator`, async () => {
      const { container } = await show([simulator, hardware(status)], "sidebar");
      expect(within(card()).getByText("Launchpad X")).toBeTruthy();
      expect(within(card()).getByText(WORDS[status])).toBeTruthy();
      if (status === "failed") expect(within(card()).getByText(PROBLEM)).toBeTruthy();
      expect(normalText(container)).not.toMatch(/midi|programmer/i);
    });
  }
});
