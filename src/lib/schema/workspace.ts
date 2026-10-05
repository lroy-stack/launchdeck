// The workspace as saved: surfaces, pages, controls, their looks and the preferences.

import { z } from "zod";
import { assignmentSchema, hexColor } from "./actions";
import { importOriginSchema } from "./artifacts";
import { contextSettingsSchema } from "./context";
import { feedbackRuleSchema } from "./feedback";

export const animationSchema = z.enum(["none", "pulse", "flash"]);
export type Animation = z.infer<typeof animationSchema>;

export const visualSchema = z.object({
  color: hexColor,
  animation: animationSchema.default("none"),
});
export type Visual = z.infer<typeof visualSchema>;

export const controlPosSchema = z.discriminatedUnion("kind", [
  z.object({ kind: z.literal("grid"), col: z.number().int(), row: z.number().int() }),
  z.object({ kind: z.literal("top"), index: z.number().int() }),
  z.object({ kind: z.literal("side"), index: z.number().int() }),
  z.object({ kind: z.literal("logo") }),
]);
export type ControlPos = z.infer<typeof controlPosSchema>;

export const posKey = (pos: ControlPos): string => {
  switch (pos.kind) {
    case "grid":
      return `g.${pos.col}.${pos.row}`;
    case "top":
      return `t.${pos.index}`;
    case "side":
      return `s.${pos.index}`;
    case "logo":
      return "logo";
  }
};

export const controlSchema = z.object({
  id: z.string(),
  pos: controlPosSchema,
  span: z.object({ w: z.number().int(), h: z.number().int() }).default({ w: 1, h: 1 }),
  label: z.string(),
  description: z.string().default(""),
  icon: z.string().nullish(),
  visual: visualSchema,
  states: z.record(z.string(), visualSchema).default({}),
  press: assignmentSchema.nullish(),
  longPress: assignmentSchema.nullish(),
  /** One ordered list: the first state rule that is true decides the look. */
  feedback: z.array(feedbackRuleSchema).default([]),
  /** Schema 3 bindings that could not become rules, kept word for word. Inert. */
  legacyBindings: z.array(z.unknown()).default([]),
});
export type Control = z.infer<typeof controlSchema>;

export const pageSchema = z.object({ id: z.string(), name: z.string(), controls: z.array(controlSchema) });
export type Page = z.infer<typeof pageSchema>;

export const surfaceSchema = z.object({
  id: z.string(),
  name: z.string(),
  description: z.string().default(""),
  icon: z.string().nullish(),
  category: z.string(),
  pages: z.array(pageSchema),
  context: z.object({ foregroundApps: z.array(z.string()).default([]) }).nullish(),
  /** Where an imported surface came from, for display (the "Imported" badge). Absent on any other. */
  importedFrom: importOriginSchema.nullish(),
});
export type Surface = z.infer<typeof surfaceSchema>;

export const preferencesSchema = z.object({
  onboardingDone: z.boolean().default(false),
  externalEventsPort: z.number().int().default(47821),
  externalEventsEnabled: z.boolean().default(true),
});
export type Preferences = z.infer<typeof preferencesSchema>;

export const workspaceSchema = z.object({
  schemaVersion: z.number().int(),
  surfaces: z.array(surfaceSchema),
  lastActiveSurface: z.string().nullish(),
  preferences: preferencesSchema,
  /** Absent in a workspace that never used Auto Context. */
  context: contextSettingsSchema.optional(),
});
export type Workspace = z.infer<typeof workspaceSchema>;
