import type { Control, RuntimeEvent } from "./schema";

export function formatTime(ts: number): string {
  if (!ts) return "";
  const d = new Date(ts);
  return d.toLocaleTimeString([], { hour12: false });
}

const str = (v: unknown): string => (typeof v === "string" ? v : "");

/** One human sentence for a runtime event, for the activity feed. */
export function describeEvent(e: RuntimeEvent): string {
  const p = (e.payload ?? {}) as Record<string, unknown>;
  switch (e.type) {
    case "control.pressed":
      return `Pressed ${str(p.label) || str(p.controlId)}`;
    case "action.started":
      return `Running ${str(p.action) || "action"}`;
    case "action.completed":
      return `Done ${str(p.action) || "action"}`;
    case "action.failed":
      return `Failed ${str(p.action) || "action"}: ${str(p.error)}`;
    case "surface.changed":
      return `Surface → ${str(p.surfaceId)}`;
    case "page.changed":
      return `Page → ${str(p.pageId)}`;
    case "plugin.state.changed":
      return `${e.source}: ${str(p.key)} = ${str(p.value)}`;
    case "device.connected":
      return `Device connected: ${str(p.deviceId)}`;
    case "device.disconnected":
      return `Device disconnected: ${str(p.deviceId)}`;
    default:
      return e.type;
  }
}

export type EventTone = "neutral" | "good" | "bad" | "accent";

export function eventTone(e: RuntimeEvent): EventTone {
  if (e.type === "action.failed" || e.type.endsWith(".failed") || e.type === "task.failed") return "bad";
  if (e.type === "action.completed" || e.type === "task.completed" || e.type === "task.succeeded") return "good";
  if (e.source !== "runtime" && e.source !== "device") return "accent";
  return "neutral";
}

export function controlTitle(c: Control): string {
  return c.label || c.id;
}

export function plural(n: number, one: string, many = `${one}s`): string {
  return `${n} ${n === 1 ? one : many}`;
}

/** What people read for a surface category (Rust stores a slug). The smart-home starter is "Home". */
const CATEGORY_LABELS: Record<string, string> = {
  agents: "Agents",
  ai: "Agents",
  development: "Development",
  productivity: "Productivity",
  "smart-home": "Home",
  accessibility: "Accessibility",
  custom: "Custom",
};

/** A surface category in words: a known slug's label, any other slug as a phrase ("music-studio" reads "Music studio"). */
export function categoryLabel(slug: string): string {
  const known = CATEGORY_LABELS[slug];
  if (known) return known;
  const words = slug.replace(/[-_]+/g, " ").trim();
  return words.charAt(0).toUpperCase() + words.slice(1);
}
