// @vitest-environment jsdom
// Device status in the sidebar and in Settings > Devices, for each status
// Rust reports (`DeviceStatus` in devices/manager.rs). The words come from
// src/lib/devices.ts; MIDI and Programmer mode appear only under Technical details.

import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
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

const NOTICE = "Firmware version 0531 does not support brightness. Novation Components can update it.";
const MINI_ID = "launchpad-mini-mk3:lpminimk3-midi";

function mini(details: Partial<NonNullable<DeviceInfo["details"]>> = {}, notice?: string): DeviceInfo {
  return {
    identity: { id: MINI_ID, name: "Launchpad Mini MK3", model: "launchpad-mini-mk3", kind: "hardware" },
    capabilities: { ...caps(true), velocity: false, animations: { pulse: true, flash: true } },
    connected: true,
    status: "connected",
    notice,
    details: { firmware: "0531", mode: "confirmed", brightness: null, asleep: false, scrolling: false, notice: notice ?? null, ...details },
    features: {
      hostMode: true,
      palette: true,
      batchedLighting: true,
      flashPeriodMs: 500,
      pulsePeriodMs: 1000,
      brightness: true,
      sleep: true,
      scrollingText: true,
      firmwareVersion: true,
    },
  };
}

function withBrightness(percent: number | undefined) {
  const workspace = useStore.getState().workspace!;
  useStore.setState({ workspace: { ...workspace, preferences: { ...workspace.preferences, deviceBrightness: percent } } });
}

const settingsOf = (name: string) => screen.getByRole("region", { name: `${name} settings` });

describe("Settings > Devices, for a Launchpad with settings of its own", () => {
  it("sums the device up in one line: model, status, firmware and Programmer Mode", async () => {
    await show([simulator, mini()], "settings");
    expect(within(settingsOf("Launchpad Mini MK3")).getByText("Launchpad Mini MK3 · Connected · Firmware 0531 · Programmer Mode (confirmed)")).toBeTruthy();
  });

  it("shows the notice Rust gives a connected device", async () => {
    await show([simulator, mini({}, NOTICE)], "settings");
    expect(within(rowOf("Launchpad Mini MK3")).getByText(NOTICE)).toBeTruthy();
  });

  it("lists what the device can do, in words", async () => {
    await show([simulator, mini()], "settings");
    const list = within(settingsOf("Launchpad Mini MK3")).getByRole("list", { name: "What the Launchpad Mini MK3 can do" });
    expect(within(list).getByText("Scrolls text across its pads")).toBeTruthy();
    expect(within(list).getByText("Flashes and pulses by itself (a flash every 0.5 s, a pulse every 1 s)")).toBeTruthy();
  });

  it("starts at the device's own level, and sets a brightness through its own command", async () => {
    const set = vi.spyOn(api, "setDeviceBrightness").mockResolvedValue(undefined);
    const prefs = vi.spyOn(api, "updatePreferences");
    await show([simulator, mini()], "settings");
    const own = screen.getByRole("checkbox", { name: "Device's own level" });
    const slider = screen.getByRole("slider", { name: "Brightness" });
    expect(own).toHaveProperty("checked", true);
    expect(slider).toHaveProperty("disabled", true);

    await userEvent.click(own);
    expect(set).toHaveBeenLastCalledWith(100);
    withBrightness(100);
    fireEvent.change(screen.getByRole("slider", { name: "Brightness" }), { target: { value: "40" } });
    await waitFor(() => expect(set).toHaveBeenLastCalledWith(40));
    expect(prefs).not.toHaveBeenCalled();
  });

  it("gives the device its own level back", async () => {
    const set = vi.spyOn(api, "setDeviceBrightness").mockResolvedValue(undefined);
    await show([simulator, mini({ brightness: 40 })], "settings");
    withBrightness(40);
    const own = await screen.findByRole("checkbox", { name: "Device's own level" });
    expect(own).toHaveProperty("checked", false);
    expect(screen.getByRole("slider", { name: "Brightness" })).toHaveProperty("value", "40");
    await userEvent.click(own);
    expect(set).toHaveBeenLastCalledWith(null);
  });

  it("puts the device to sleep and wakes it", async () => {
    const sleep = vi.spyOn(api, "setDeviceSleep").mockResolvedValue(undefined);
    await show([simulator, mini()], "settings");
    await userEvent.click(within(settingsOf("Launchpad Mini MK3")).getByRole("button", { name: "Sleep" }));
    expect(sleep).toHaveBeenLastCalledWith(MINI_ID, true);
    cleanup();
    off();
    await show([simulator, mini({ asleep: true })], "settings");
    expect(within(settingsOf("Launchpad Mini MK3")).getByText(/· Asleep$/)).toBeTruthy();
    await userEvent.click(within(settingsOf("Launchpad Mini MK3")).getByRole("button", { name: "Wake" }));
    expect(sleep).toHaveBeenLastCalledWith(MINI_ID, false);
  });

  it("scrolls a test text, and stops it", async () => {
    const scroll = vi.spyOn(api, "scrollDeviceText").mockResolvedValue(undefined);
    await show([simulator, mini()], "settings");
    await userEvent.click(within(settingsOf("Launchpad Mini MK3")).getByRole("button", { name: "Scroll a test text" }));
    expect(scroll).toHaveBeenLastCalledWith(MINI_ID, { text: "LaunchDeck", color: "#ffffff", speed: 10, direction: "left", looping: false });
    cleanup();
    off();
    await show([simulator, mini({ scrolling: true })], "settings");
    await userEvent.click(within(settingsOf("Launchpad Mini MK3")).getByRole("button", { name: "Stop the text" }));
    expect(scroll).toHaveBeenLastCalledWith(MINI_ID, null);
  });

  it("keeps notes, CCs, SysEx and port names under Technical details", async () => {
    const { container } = await show([simulator, mini()], "settings");
    expect(normalText(container)).not.toMatch(/midi|sysex|\bcc\b|\bnotes?\b|LPMiniMK3|palette/i);
    expect(container.querySelector("details")!.textContent).toMatch(/LPMiniMK3 MIDI/);
  });

  it("offers no setting a device does not have", async () => {
    await show([simulator, hardware("connected")], "settings");
    expect(screen.queryByRole("slider", { name: "Brightness" })).toBeNull();
    const region = settingsOf("Launchpad X");
    expect(within(region).queryByRole("button", { name: "Sleep" })).toBeNull();
    expect(within(region).queryByRole("button", { name: "Scroll a test text" })).toBeNull();
    expect(within(region).getByText("Feels how hard a pad is pressed")).toBeTruthy();
  });

  it("shows settings only for a connected Launchpad", async () => {
    await show([simulator, { ...mini(), status: "unplugged", connected: false }], "settings");
    expect(screen.queryByRole("region", { name: "Launchpad Mini MK3 settings" })).toBeNull();
    expect(screen.queryByRole("region", { name: "Virtual Launchpad X settings" })).toBeNull();
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
