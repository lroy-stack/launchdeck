// What a person reads about a device. Rust decides the status (and the
// problem sentence); this only words it. No MIDI terms: they belong to
// Technical details.

import type { DeviceInfo, DeviceStatus, ModeState } from "./schema";

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

/** A notice is about a connected device: connected, with a limitation, in Rust's sentence. */
export const noticeOf = (info: DeviceInfo): string | null => (info.status === "connected" ? (info.notice ?? null) : null);

const MODE: Record<ModeState, string | null> = {
  none: null,
  acquiring: "Switching to Programmer Mode",
  confirmed: "Programmer Mode (confirmed)",
  unconfirmed: "Programmer Mode (not confirmed)",
};

/**
 * One line for a device in Settings: its name, status, firmware and the mode LaunchDeck holds, then whether it
 * sleeps or scrolls text. What the device does not report is left out.
 */
export function summaryText(info: DeviceInfo): string {
  const details = info.details;
  return [
    info.identity.name,
    statusText(info),
    details?.firmware ? `Firmware ${details.firmware}` : null,
    details ? MODE[details.mode] : null,
    details?.asleep ? "Asleep" : null,
    details?.scrolling ? "Scrolling text" : null,
  ]
    .filter((part): part is string => part !== null)
    .join(" · ");
}

const seconds = (ms: number): string => `${ms / 1000} s`;

function buttonsText(top: number, side: number): string | null {
  if (top > 0 && side > 0) return `${top} buttons along the top and ${side} down the side`;
  if (top > 0) return `${top} buttons along the top`;
  if (side > 0) return `${side} buttons down the side`;
  return null;
}

/** The device's own flash and pulse, with their pace when the device fixes it. */
function animationText(info: DeviceInfo): string | null {
  const { flash, pulse } = info.capabilities.animations;
  const verbs = [flash ? "Flashes" : null, pulse ? "pulses" : null].filter((v): v is string => v !== null);
  if (verbs.length === 0) return null;
  const f = info.features;
  const pace = [
    flash && f?.flashPeriodMs ? `a flash every ${seconds(f.flashPeriodMs)}` : null,
    pulse && f?.pulsePeriodMs ? `a pulse every ${seconds(f.pulsePeriodMs)}` : null,
  ].filter((p): p is string => p !== null);
  const said = verbs.join(" and ");
  return `${said.charAt(0).toUpperCase()}${said.slice(1)} by itself${pace.length > 0 ? ` (${pace.join(", ")})` : ""}`;
}

/** What a device can do, in words, from its capabilities and features. Only what it has; no MIDI terms. */
export function capabilityList(info: DeviceInfo): string[] {
  const c = info.capabilities;
  const f = info.features;
  return [
    c.grid ? `${c.grid.width * c.grid.height} pads, in ${c.grid.height} rows of ${c.grid.width}` : null,
    buttonsText(c.aux.top, c.aux.side),
    c.aux.logo ? "A logo that lights up" : null,
    c.rgb ? "Lights in any colour" : null,
    c.velocity ? "Feels how hard a pad is pressed" : null,
    c.pressure ? "Feels the pressure on a held pad" : null,
    animationText(info),
    f?.brightness ? "Brightness can be set" : null,
    f?.sleep ? "Sleeps with its lights off on request" : null,
    f?.scrollingText ? "Scrolls text across its pads" : null,
    f?.firmwareVersion ? "Reports its firmware version" : null,
    f?.hostMode ? "LaunchDeck takes it over while connected and hands it back afterwards" : null,
  ].filter((words): words is string => words !== null);
}
