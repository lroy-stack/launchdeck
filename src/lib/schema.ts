// Zod schemas for everything that crosses the Tauri IPC boundary.
// They mirror the serde types in src-tauri (camelCase). The contract test
// parses examples/default-workspace.json, which Rust also guards against drift.

import { z } from "zod";
import { agentsSnapshotSchema } from "./agents";

export const hexColor = z.string().regex(/^#[0-9a-fA-F]{6}$/);
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

/**
 * What a control does: a provider, one of its actions and the settings for it.
 * Portable and semantic: no display names, no code, nothing derived from the UI.
 */
export const assignmentSchema = z.object({
  provider: z.string(),
  action: z.string(),
  args: z.record(z.string(), z.unknown()).default({}),
});
export type Assignment = z.infer<typeof assignmentSchema>;

/** `provider.action`, the stable id of a definition in the catalog. */
export const definitionId = (a: Pick<Assignment, "provider" | "action">): string => `${a.provider}.${a.action}`;

export const paramKindSchema = z.enum(["string", "number", "boolean", "enum", "reference", "json"]);
export type ParamKind = z.infer<typeof paramKindSchema>;

export const paramDefSchema = z.object({
  name: z.string(),
  label: z.string(),
  description: z.string().default(""),
  type: paramKindSchema,
  required: z.boolean().default(false),
  default: z.unknown().optional(),
  placeholder: z.string().optional(),
  multiline: z.boolean().default(false),
  monospace: z.boolean().default(false),
  maxLength: z.number().int().optional(),
  min: z.number().optional(),
  max: z.number().optional(),
  integer: z.boolean().default(false),
  options: z.array(z.object({ value: z.string(), label: z.string() })).default([]),
  target: z.enum(["surface", "page"]).optional(),
  scope: z.string().optional(),
});
export type ParamDef = z.infer<typeof paramDefSchema>;

export const availabilitySchema = z.discriminatedUnion("status", [
  z.object({ status: z.literal("available") }),
  z.object({ status: z.literal("unavailable"), cause: z.enum(["provider", "state"]), reason: z.string() }),
]);
export type Availability = z.infer<typeof availabilitySchema>;

/** A suggested look for a control that runs an action. A hint, never forced. */
export const appearanceSchema = z.object({
  label: z.string().optional(),
  color: hexColor.optional(),
  icon: z.string().optional(),
});
export type Appearance = z.infer<typeof appearanceSchema>;

export const providerKindSchema = z.enum(["builtin", "module", "plugin"]);
export type ProviderKind = z.infer<typeof providerKindSchema>;

export const actionDefinitionSchema = z.object({
  id: z.string(),
  provider: z.string(),
  action: z.string(),
  name: z.string(),
  description: z.string(),
  category: z.string(),
  keywords: z.array(z.string()).default([]),
  params: z.array(paramDefSchema).default([]),
  availability: availabilitySchema,
  privileged: z.boolean().default(false),
  appearance: appearanceSchema.optional(),
});
export type ActionDefinition = z.infer<typeof actionDefinitionSchema>;

export const providerInfoSchema = z.object({
  id: z.string(),
  name: z.string(),
  kind: providerKindSchema,
  version: z.string(),
  description: z.string(),
  status: z.enum(["ready", "disabled", "error"]),
  reason: z.string().optional(),
});
export type ProviderInfo = z.infer<typeof providerInfoSchema>;

/** A value a state holds: a boolean, a number or a short text. */
export const scalarSchema = z.union([z.boolean(), z.number(), z.string()]);
export type Scalar = z.infer<typeof scalarSchema>;

/** A state a provider offers: something a feedback rule can read. */
export const observableSchema = z.object({
  id: z.string(),
  provider: z.string(),
  local: z.string(),
  name: z.string(),
  description: z.string().default(""),
  category: z.string().optional(),
  keywords: z.array(z.string()).default([]),
  kind: z.enum(["state", "event"]),
  params: z.array(paramDefSchema).default([]),
  value: z
    .object({
      type: z.enum(["enum", "boolean", "number", "string"]),
      values: z.array(z.object({ value: scalarSchema, label: z.string(), role: z.string().optional() })).default([]),
    })
    .optional(),
});
export type Observable = z.infer<typeof observableSchema>;

/** Everything the Action Library offers, as Rust evaluated it. Searched locally. */
export const catalogSchema = z.object({
  providers: z.array(providerInfoSchema),
  actions: z.array(actionDefinitionSchema),
  /** The states providers offer to feedback rules. */
  observables: z.array(observableSchema).default([]),
  issues: z.array(z.string()).default([]),
});
export type Catalog = z.infer<typeof catalogSchema>;
export const emptyCatalog: Catalog = { providers: [], actions: [], observables: [], issues: [] };

export const issueSchema = z.object({ param: z.string().optional(), message: z.string() });
export type Issue = z.infer<typeof issueSchema>;

export const validationSchema = z.object({
  ok: z.boolean(),
  assignment: assignmentSchema.optional(),
  issues: z.array(issueSchema).default([]),
});
export type Validation = z.infer<typeof validationSchema>;

// ---------------------------------------------------------------------------
// Feedback rules (workspace schema 4): what decides which look a control shows.
// State rules are continuous (they read the value a provider reported); event
// rules are momentary (they show a look for a while). See state/feedback.rs.
// ---------------------------------------------------------------------------

export const stateAddressSchema = z.object({
  provider: z.string(),
  id: z.string(),
  args: z.record(z.string(), scalarSchema).optional(),
});
export type StateAddress = z.infer<typeof stateAddressSchema>;

export const stateTestSchema = z.discriminatedUnion("op", [
  z.object({ op: z.literal("equals"), value: scalarSchema }),
  z.object({ op: z.literal("notEquals"), value: scalarSchema }),
  z.object({ op: z.literal("lessThan"), value: z.number() }),
  z.object({ op: z.literal("atMost"), value: z.number() }),
  z.object({ op: z.literal("greaterThan"), value: z.number() }),
  z.object({ op: z.literal("atLeast"), value: z.number() }),
  z.object({ op: z.literal("hasValue") }),
  z.object({ op: z.literal("noValue") }),
  z.object({ op: z.literal("unavailable") }),
]);
export type StateTest = z.infer<typeof stateTestSchema>;

const whichLook = {
  show: z.string().optional(),
  showFromValue: z.boolean().optional(),
  showFromField: z.string().optional(),
};

export const stateRuleSchema = z.object({
  when: z.object({ state: stateAddressSchema, is: stateTestSchema }),
  ...whichLook,
});
export type StateRule = z.infer<typeof stateRuleSchema>;

export const eventRuleSchema = z.object({
  when: z.object({
    event: z.object({ provider: z.string(), id: z.string() }),
    where: z.array(z.object({ field: z.string(), equals: scalarSchema })).default([]),
  }),
  ...whichLook,
  hold: z.union([z.literal("latch"), z.object({ ms: z.number().int() })]).optional(),
});
export type EventRule = z.infer<typeof eventRuleSchema>;

export const feedbackRuleSchema = z.union([stateRuleSchema, eventRuleSchema]);
export type FeedbackRule = z.infer<typeof feedbackRuleSchema>;

export const isStateRule = (rule: FeedbackRule): rule is StateRule => "state" in rule.when;

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
});
export type Workspace = z.infer<typeof workspaceSchema>;

const deviceCapabilitiesSchema = z.object({
  grid: z.object({ width: z.number(), height: z.number() }).nullish(),
  aux: z.object({ top: z.number(), side: z.number(), logo: z.boolean() }),
  rgb: z.boolean(),
  velocity: z.boolean(),
  pressure: z.boolean(),
  animations: z.object({ pulse: z.boolean(), flash: z.boolean() }),
});

export const deviceInfoSchema = z.object({
  identity: z.object({
    id: z.string(),
    name: z.string(),
    model: z.string(),
    kind: z.enum(["virtual", "hardware"]),
  }),
  capabilities: deviceCapabilitiesSchema,
  connected: z.boolean(),
});
export type DeviceInfo = z.infer<typeof deviceInfoSchema>;

export const modeSchema = z.enum(["live", "edit"]);
export type Mode = z.infer<typeof modeSchema>;

export const runtimeStateSchema = z.object({
  activeSurfaceId: z.string(),
  activePageId: z.string(),
  mode: modeSchema,
  controlStates: z.record(z.string(), z.string()),
  devices: z.array(deviceInfoSchema),
  external: z.object({
    enabled: z.boolean(),
    port: z.number(),
    listening: z.boolean(),
    error: z.string().nullish(),
  }),
});
export type RuntimeState = z.infer<typeof runtimeStateSchema>;

export const runtimeEventSchema = z.object({
  source: z.string(),
  type: z.string(),
  payload: z.unknown().optional(),
  ts: z.number().default(0),
});
export type RuntimeEvent = z.infer<typeof runtimeEventSchema>;

export const pluginStatusSchema = z.enum(["enabled", "disabled", "error"]);
export type PluginStatus = z.infer<typeof pluginStatusSchema>;

/** Present only for plugins installed from disk (WebAssembly). Built-in plugins have none. */
export const externalInfoSchema = z.object({
  status: pluginStatusSchema,
  error: z.string().optional(),
  author: z.string(),
  abi: z.number(),
  sdk: z.string().optional(),
  events: z.array(z.object({ type: z.string(), description: z.string() })),
});

export const pluginManifestSchema = z.object({
  id: z.string(),
  name: z.string(),
  version: z.string(),
  description: z.string(),
  kind: providerKindSchema.default("builtin"),
  actions: z.array(
    z.object({
      id: z.string(),
      label: z.string(),
      description: z.string(),
      category: z.string().optional(),
      keywords: z.array(z.string()).default([]),
      params: z.array(paramDefSchema).default([]),
      privileged: z.boolean().default(false),
      appearance: appearanceSchema.optional(),
    }),
  ),
  states: z.array(z.object({ key: z.string(), description: z.string(), values: z.array(z.string()) })),
  permissions: z.array(z.string()),
  external: externalInfoSchema.optional(),
});
export type PluginManifest = z.infer<typeof pluginManifestSchema>;

export const pluginConfigSchema = z.object({
  key: z.string(),
  type: z.enum(["string", "number", "boolean"]),
  description: z.string(),
  secret: z.boolean(),
  /** Never present for secrets. */
  value: z.unknown().optional(),
  isSet: z.boolean(),
  default: z.unknown().optional(),
});
export type PluginConfig = z.infer<typeof pluginConfigSchema>;

export const pluginInfoSchema = z.object({
  plugin: pluginManifestSchema,
  dir: z.string(),
  /** What the person chose; `plugin.external.status` is what actually happened. */
  enabled: z.boolean(),
  config: z.array(pluginConfigSchema),
});
export type PluginInfo = z.infer<typeof pluginInfoSchema>;

export const pluginsOverviewSchema = z.object({
  plugins: z.array(pluginInfoSchema),
  issues: z.array(z.object({ dir: z.string(), error: z.string() })),
  folders: z.array(z.string()),
});
export type PluginsOverview = z.infer<typeof pluginsOverviewSchema>;

export const scanReportSchema = z.object({
  loaded: z.number(),
  reloaded: z.number(),
  unchanged: z.number(),
  removed: z.number(),
  issues: z.number(),
});
export type ScanReport = z.infer<typeof scanReportSchema>;

export const pluginLogSchema = z.object({
  ts: z.number(),
  level: z.enum(["debug", "info", "warn", "error"]),
  origin: z.enum(["plugin", "host"]),
  message: z.string(),
});
export type PluginLog = z.infer<typeof pluginLogSchema>;

export const snapshotSchema = z.object({
  workspace: workspaceSchema,
  state: runtimeStateSchema,
  plugins: z.array(pluginManifestSchema),
  catalog: catalogSchema,
  recentEvents: z.array(runtimeEventSchema),
  agents: agentsSnapshotSchema,
});
export type Snapshot = z.infer<typeof snapshotSchema>;

export const frameSchema = z.object({
  leds: z.array(z.object({ pos: controlPosSchema, led: visualSchema })),
});
export type Frame = z.infer<typeof frameSchema>;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

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

export const controlStateKey = (surfaceId: string, pageId: string, controlId: string): string =>
  `${surfaceId}/${pageId}/${controlId}`;
