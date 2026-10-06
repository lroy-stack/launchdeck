import { describe, expect, it } from "vitest";
import { primaryDevice, statusText, statusTone } from "./devices";
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
