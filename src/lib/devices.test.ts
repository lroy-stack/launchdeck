import { describe, expect, it } from "vitest";
import { capabilityList, noticeOf, primaryDevice, statusText, statusTone, summaryText } from "./devices";
import type { DeviceInfo, DeviceStatus } from "./schema";

const caps: DeviceInfo["capabilities"] = {
  grid: { width: 8, height: 8 },
  aux: { top: 8, side: 8, logo: true },
  rgb: true,
  velocity: true,
  pressure: false,
  animations: { pulse: false, flash: false },
};

function device(id: string, status: DeviceStatus, kind: "virtual" | "hardware" = "hardware", problem?: string): DeviceInfo {
  return {
    identity: { id, name: kind === "virtual" ? "Virtual Launchpad X" : id, model: "launchpad-x", kind },
    capabilities: caps,
    connected: status === "connected",
    status,
    problem,
  };
}

const simulator = device("virtual-launchpad-x", "connected", "virtual");

describe("statusText", () => {
  it("says each hardware status in words", () => {
    expect(statusText(device("a", "connected"))).toBe("Connected");
    expect(statusText(device("a", "available"))).toBe("Found, not connected");
    expect(statusText(device("a", "unplugged"))).toBe("Unplugged: plug it in again and it reconnects");
    expect(statusText(device("a", "unsupported"))).toBe("Not supported yet: LaunchDeck works with the Launchpad X and the Launchpad Mini MK3");
    expect(statusText(device("a", "failed", "hardware", "Another program may be using it."))).toBe("Could not connect");
  });

  it("calls the virtual device the simulator", () => {
    expect(statusText(simulator)).toBe("Simulator");
    expect(statusText(device("virtual-launchpad-x", "available", "virtual"))).toBe("Simulator, not connected");
  });

  it("never uses MIDI or Programmer words", () => {
    const statuses: DeviceStatus[] = ["connected", "available", "unplugged", "unsupported", "failed"];
    for (const s of statuses) {
      for (const kind of ["virtual", "hardware"] as const) {
        expect(statusText(device("a", s, kind))).not.toMatch(/midi|programmer/i);
      }
    }
  });

  it("gives a tone for the dot", () => {
    expect(statusTone(device("a", "connected"))).toBe("ok");
    expect(statusTone(device("a", "available"))).toBe("idle");
    expect(statusTone(device("a", "unplugged"))).toBe("warn");
    expect(statusTone(device("a", "unsupported"))).toBe("warn");
    expect(statusTone(device("a", "failed"))).toBe("bad");
  });
});

describe("primaryDevice", () => {
  it("prefers connected hardware over the simulator", () => {
    const lpx = device("launchpad-x:1", "connected");
    expect(primaryDevice([simulator, device("launchpad-x:0", "available"), lpx])).toBe(lpx);
  });

  it("then any Launchpad it drives, connected or not", () => {
    const unplugged = device("launchpad-x:1", "unplugged");
    expect(primaryDevice([simulator, unplugged])).toBe(unplugged);
    const failed = device("launchpad-x:2", "failed");
    expect(primaryDevice([simulator, failed])).toBe(failed);
    const mini = { ...device("launchpad-mini-mk3:1", "available"), identity: { id: "launchpad-mini-mk3:1", name: "Launchpad Mini MK3", model: "launchpad-mini-mk3", kind: "hardware" as const } };
    expect(primaryDevice([simulator, mini])).toBe(mini);
  });

  it("then the simulator; a model LaunchDeck does not drive is never primary", () => {
    expect(primaryDevice([device("unsupported:launchpad-pro-mk3", "unsupported"), simulator])).toBe(simulator);
    expect(primaryDevice([simulator])).toBe(simulator);
  });

  it("is null without devices", () => {
    expect(primaryDevice([])).toBeNull();
  });
});

const MINI_FEATURES: NonNullable<DeviceInfo["features"]> = {
  hostMode: true,
  palette: true,
  batchedLighting: true,
  flashPeriodMs: 500,
  pulsePeriodMs: 1000,
  brightness: true,
  sleep: true,
  scrollingText: true,
  firmwareVersion: true,
};

function mini(details: Partial<NonNullable<DeviceInfo["details"]>> = {}, status: DeviceStatus = "connected"): DeviceInfo {
  return {
    identity: { id: "launchpad-mini-mk3:1", name: "Launchpad Mini MK3", model: "launchpad-mini-mk3", kind: "hardware" },
    capabilities: { ...caps, velocity: false, animations: { pulse: true, flash: true } },
    connected: status === "connected",
    status,
    details: { firmware: "0531", mode: "confirmed", brightness: null, asleep: false, scrolling: false, notice: null, ...details },
    features: MINI_FEATURES,
  };
}

describe("summaryText", () => {
  it("names the model, the status, the firmware and the mode LaunchDeck holds", () => {
    expect(summaryText(mini())).toBe("Launchpad Mini MK3 · Connected · Firmware 0531 · Programmer Mode (confirmed)");
    expect(summaryText(mini({ mode: "unconfirmed" }))).toBe("Launchpad Mini MK3 · Connected · Firmware 0531 · Programmer Mode (not confirmed)");
    expect(summaryText(mini({ mode: "acquiring", firmware: null }))).toBe("Launchpad Mini MK3 · Connected · Switching to Programmer Mode");
  });

  it("says when the device sleeps or scrolls text", () => {
    expect(summaryText(mini({ asleep: true }))).toBe("Launchpad Mini MK3 · Connected · Firmware 0531 · Programmer Mode (confirmed) · Asleep");
    expect(summaryText(mini({ scrolling: true }))).toBe("Launchpad Mini MK3 · Connected · Firmware 0531 · Programmer Mode (confirmed) · Scrolling text");
  });

  it("leaves out what a device does not report", () => {
    expect(summaryText(device("launchpad-x:1", "connected"))).toBe("launchpad-x:1 · Connected");
    expect(summaryText(simulator)).toBe("Virtual Launchpad X · Simulator");
  });
});

describe("noticeOf", () => {
  const NOTICE = "Firmware version 0531 does not support brightness. Novation Components can update it.";

  it("gives a connected device's notice", () => {
    expect(noticeOf({ ...mini(), notice: NOTICE })).toBe(NOTICE);
  });

  it("gives none for a device that is not connected, or has nothing to say", () => {
    expect(noticeOf({ ...mini({}, "unplugged"), notice: NOTICE })).toBeNull();
    expect(noticeOf(mini())).toBeNull();
  });
});

describe("capabilityList", () => {
  it("says what a Launchpad Mini MK3 can do, in words", () => {
    expect(capabilityList(mini())).toEqual([
      "64 pads, in 8 rows of 8",
      "8 buttons along the top and 8 down the side",
      "A logo that lights up",
      "Lights in any colour",
      "Flashes and pulses by itself (a flash every 0.5 s, a pulse every 1 s)",
      "Brightness can be set",
      "Sleeps with its lights off on request",
      "Scrolls text across its pads",
      "Reports its firmware version",
      "LaunchDeck takes it over while connected and hands it back afterwards",
    ]);
  });

  it("says when pads feel how hard they are pressed, and claims nothing a device lacks", () => {
    const words = capabilityList(device("launchpad-x:1", "connected"));
    expect(words).toContain("Feels how hard a pad is pressed");
    expect(words.join(" ")).not.toMatch(/flash|bright|sleep|text|firmware/i);
  });

  it("uses no MIDI words", () => {
    expect(capabilityList(mini()).join(" ")).not.toMatch(/midi|note|cc|sysex|palette|grid|rgb/i);
  });
});
