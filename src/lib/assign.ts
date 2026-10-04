// Putting an action on a control. Pure functions, unit tested.
//
// Assigning only ever changes the assignment. A control's label, colour, icon,
// state looks and bindings belong to the person; an action's appearance is a
// hint that is applied when a person asks for it, or when the control is new.

import type { CatalogIndex } from "./actions";
import { assignmentFor, resolveAssignment } from "./actions";
import type { ActionDefinition, Assignment, Control, ControlPos } from "./schema";
import { definitionId, posKey } from "./schema";

export type Slot = "press" | "longPress";
export const SLOTS: Slot[] = ["press", "longPress"];
export const SLOT_LABEL: Record<Slot, string> = { press: "On press", longPress: "On long press" };
/** The short names of the two slots, for the Library's "Assign as" choice. */
export const SLOT_SHORT: Record<Slot, string> = { press: "Press", longPress: "Long press" };

/** The drag payload the Library puts on a row and the surface reads on a pad. */
export const DRAG_TYPE = "application/x-launchdeck-action";

/** Where a pad is, in words. */
export function describePos(p: ControlPos): string {
  switch (p.kind) {
    case "grid":
      return `Pad column ${p.col + 1}, row ${p.row + 1}`;
    case "top":
      return `Top button ${p.index + 1}`;
    case "side":
      return `Side button ${p.index + 1}`;
    case "logo":
      return "Logo";
  }
}

export function newControl(pos: ControlPos): Control {
  return {
    id: posKey(pos),
    pos,
    span: { w: 1, h: 1 },
    label: "New control",
    description: "",
    icon: null,
    visual: { color: "#3b82f6", animation: "none" },
    states: {},
    press: null,
    longPress: null,
    bindings: [],
  };
}

/** How a control would change to follow an action's suggestion. Empty when it already does. */
export function lookHints(control: Control, def: ActionDefinition | undefined): { label?: string; color?: string; icon?: string } {
  const a = def?.appearance;
  if (!a) return {};
  const out: { label?: string; color?: string; icon?: string } = {};
  if (a.label && a.label !== control.label) out.label = a.label;
  if (a.color && a.color.toLowerCase() !== control.visual.color.toLowerCase()) out.color = a.color;
  if (a.icon && a.icon !== (control.icon ?? "")) out.icon = a.icon;
  return out;
}

/** The control with the suggested label, colour and icon. Looks for states, bindings and everything else stay. */
export function withLook(control: Control, def: ActionDefinition | undefined): Control {
  const h = lookHints(control, def);
  return {
    ...control,
    label: h.label ?? control.label,
    icon: h.icon ?? control.icon,
    visual: { ...control.visual, color: h.color ?? control.visual.color },
  };
}

export type AssignPlan =
  /** Nothing is there, or the same action is: the assignment can be applied now. */
  | { kind: "apply"; assignment: Assignment }
  /** Another action is there. Ask first; say what would be replaced. */
  | { kind: "confirm"; assignment: Assignment; current: Assignment }
  /** The same action is already there. Its settings stay as they are. */
  | { kind: "same" };

export function planAssign(control: Control, slot: Slot, def: ActionDefinition): AssignPlan {
  const current = control[slot];
  const assignment = assignmentFor(def);
  if (!current) return { kind: "apply", assignment };
  if (definitionId(current) === def.id) return { kind: "same" };
  return { kind: "confirm", assignment, current };
}

export const setSlot = (control: Control, slot: Slot, a: Assignment | null): Control => ({ ...control, [slot]: a });

/** What the person is about to lose by replacing, in words: the action and its settings. */
export function replaceWarning(index: CatalogIndex, current: Assignment, next: ActionDefinition): string {
  const was = index.byId.get(definitionId(current))?.name ?? definitionId(current);
  const settings = Object.keys(current.args).length;
  const lost = settings > 0 ? ` and its ${settings === 1 ? "setting" : `${settings} settings`}` : "";
  return `This replaces “${was}”${lost} with “${next.name}”. The pad’s label, colour, icon, state looks and bindings are not changed.`;
}

/**
 * Why a control cannot do what it is set to do right now, when the cause is a
 * provider that is missing, off or broken. Not for actions that are only waiting
 * for the right moment (approving with nothing to approve): those are normal.
 */
export function problemOf(index: CatalogIndex, control: Control): string | null {
  for (const slot of SLOTS) {
    const a = control[slot];
    if (!a) continue;
    const r = resolveAssignment(index, a);
    if (r.kind === "missing" || (r.kind === "unavailable" && r.cause === "provider")) return `${SLOT_LABEL[slot]}: ${r.reason}`;
  }
  return null;
}
