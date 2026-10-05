import type { ActionDefinition, Control, RuntimeEvent, Workspace } from "./schema";

export function formatTime(ts: number): string {
  if (!ts) return "";
  const d = new Date(ts);
  return d.toLocaleTimeString([], { hour12: false });
}

const str = (v: unknown): string => (typeof v === "string" ? v : "");

/** Names for the ids an event carries. Each returns `undefined` for an id it does not know. */
export interface EventNames {
  surface: (surfaceId: string) => string | undefined;
  page: (surfaceId: string, pageId: string) => string | undefined;
  action: (definitionId: string) => string | undefined;
}

const NO_NAMES: EventNames = { surface: () => undefined, page: () => undefined, action: () => undefined };

/** Names from the workspace (surfaces, pages) and the action catalog (actions). */
export function eventNames(workspace: Workspace | null, actions: readonly ActionDefinition[]): EventNames {
  const surfaces = new Map((workspace?.surfaces ?? []).map((s) => [s.id, s]));
  const actionNames = new Map(actions.map((a) => [a.id, a.name]));
  return {
    surface: (id) => surfaces.get(id)?.name,
    page: (surfaceId, pageId) => surfaces.get(surfaceId)?.pages.find((p) => p.id === pageId)?.name,
    action: (id) => actionNames.get(id),
  };
}

/** One human sentence for a runtime event, for the activity feed: names where the ids are known, the ids otherwise. */
export function describeEvent(e: RuntimeEvent, names: EventNames = NO_NAMES): string {
  const p = (e.payload ?? {}) as Record<string, unknown>;
  const action = (): string => {
    const id = str(p.action);
    return (id && names.action(id)) || id || "action";
  };
  const surfaceId = str(p.surfaceId);
  const pageId = str(p.pageId);
  switch (e.type) {
    case "control.pressed":
      return `Pressed ${str(p.label) || str(p.controlId)}`;
    case "action.started":
      return `Running ${action()}`;
    case "action.completed":
      return `Done ${action()}`;
    case "action.failed":
      return `Failed ${action()}: ${str(p.error)}`;
    case "surface.changed":
      return `Surface → ${names.surface(surfaceId) ?? surfaceId}`;
    case "page.changed":
      return `Page → ${names.page(surfaceId, pageId) ?? pageId}`;
    case "plugin.state.changed":
      return `${e.source}: ${str(p.key)} = ${str(p.value)}`;
    case "device.connected":
      return `Device connected: ${str(p.deviceId)}`;
    case "device.disconnected":
      return `Device disconnected: ${str(p.deviceId)}`;
    default:
      return `Event ${e.type} from ${e.source}`;
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
