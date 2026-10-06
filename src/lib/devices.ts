// What a person reads about a device. Rust decides the status (and the
// problem sentence); this only words it. No MIDI terms: they belong to
// Technical details.

import type { DeviceInfo, DeviceStatus } from "./schema";

const HARDWARE: Record<DeviceStatus, string> = {
  connected: "Connected",
  available: "Found, not connected",
  unplugged: "Unplugged: plug it in again and it reconnects",
  unsupported: "Not supported yet: LaunchDeck works with the Launchpad X and the Launchpad Mini MK3",
  failed: "Could not connect",
};

/** The status of one device in a few words. */
export function statusText(info: DeviceInfo): string {
  if (info.identity.kind === "virtual") return info.status === "connected" ? "Simulator" : "Simulator, not connected";
  return HARDWARE[info.status];
}

export type StatusTone = "ok" | "idle" | "warn" | "bad";

const TONE: Record<DeviceStatus, StatusTone> = {
  connected: "ok",
  available: "idle",
  unplugged: "warn",
  unsupported: "warn",
  failed: "bad",
};

/** The colour of the status dot. */
export const statusTone = (info: DeviceInfo): StatusTone => TONE[info.status];

/**
 * The device the app shows and lays pads out for: connected hardware, then
 * any Launchpad LaunchDeck drives (unplugged, failed or not connected), then
 * the simulator.
 * A model LaunchDeck does not drive is listed but never primary.
 */
export function primaryDevice(devices: readonly DeviceInfo[]): DeviceInfo | null {
  const hardware = devices.filter((d) => d.identity.kind === "hardware" && d.status !== "unsupported");
  return (
    hardware.find((d) => d.status === "connected") ??
    hardware[0] ??
    devices.find((d) => d.identity.kind === "virtual") ??
    null
  );
}
