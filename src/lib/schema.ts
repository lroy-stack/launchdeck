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
  /** Ids of observables (`provider.local`) a control for this action usually shows. A hint, never forced. */
  suggests: z.array(z.string()).default([]),
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

// ---------------------------------------------------------------------------
// The Observable Catalog (src-tauri/src/state/catalog.rs): every state and
// event a control can observe, whoever offers it, with what each reads now.
// Rust owns the values and the status; this side only displays and searches.
// ---------------------------------------------------------------------------

export const observableKindSchema = z.enum(["state", "event"]);
export type ObservableKind = z.infer<typeof observableKindSchema>;

export const valueTypeSchema = z.enum(["enum", "boolean", "number", "string"]);
export type ValueType = z.infer<typeof valueTypeSchema>;

export const valueOptionSchema = z.object({ value: scalarSchema, label: z.string(), role: z.string().optional() });
export type ValueOption = z.infer<typeof valueOptionSchema>;

/** What a state's value is: its type and, for an enum, the values it can take. */
export const valueSpecSchema = z.object({ type: valueTypeSchema, values: z.array(valueOptionSchema).default([]) });
export type ValueSpec = z.infer<typeof valueSpecSchema>;

/** One payload field of an event that a rule may test. */
export const eventFieldSchema = z.object({
  name: z.string(),
  label: z.string(),
  type: valueTypeSchema,
  values: z.array(valueOptionSchema).default([]),
});
export type EventField = z.infer<typeof eventFieldSchema>;

/** `unknown` means the provider runs but has not reported yet; `unavailable`, that it cannot. */
export const observedStatusSchema = z.enum(["known", "unknown", "unavailable"]);
export type ObservedStatus = z.infer<typeof observedStatusSchema>;

export const observedSchema = z.object({
  status: observedStatusSchema,
  /** Only with status `known`. */
  value: scalarSchema.nullish(),
  /** Milliseconds since the Unix epoch of the last report. */
  updatedAt: z.number().nullish(),
});
export type Observed = z.infer<typeof observedSchema>;

/** One instance of a state: the arguments that pick it, and what it reads. */
export const instanceSchema = observedSchema.extend({ args: z.record(z.string(), scalarSchema).default({}) });
export type Instance = z.infer<typeof instanceSchema>;

/** The part of an entry that changes while its definition stays the same. */
const liveFields = {
  availability: availabilitySchema,
  /** A state without arguments has exactly one. One with arguments lists those that hold a value (capped). */
  instances: z.array(instanceSchema).default([]),
  /** How many instances hold a value; can exceed `instances.length`. */
  instanceCount: z.number().int().default(0),
};
export const liveSchema = z.object(liveFields);
export type Live = z.infer<typeof liveSchema>;

/** One observable, state or event: its definition, its provider and what it reads now. */
export const observableSchema = z.object({
  id: z.string(),
  provider: z.string(),
  providerName: z.string(),
  local: z.string(),
  name: z.string(),
  description: z.string().default(""),
  category: z.string().optional(),
  keywords: z.array(z.string()).default([]),
  kind: observableKindSchema,
  /** What picks one instance of a state (`slot`, `channel`). */
  params: z.array(paramDefSchema).default([]),
  /** States only. */
  value: valueSpecSchema.optional(),
  /** Events only: the payload fields a rule may test. */
  fields: z.array(eventFieldSchema).default([]),
  ...liveFields,
});
export type Observable = z.infer<typeof observableSchema>;

export const observableCatalogSchema = z.object({
  providers: z.array(providerInfoSchema),
  observables: z.array(observableSchema),
  /** Why an offered observable was left out (a contested id). */
  issues: z.array(z.string()).default([]),
});
export type ObservableCatalog = z.infer<typeof observableCatalogSchema>;
export const emptyObservableCatalog: ObservableCatalog = { providers: [], observables: [], issues: [] };

/** The live part of one observable, as pushed when only values or lifecycle changed. */
export const liveUpdateSchema = liveSchema.extend({ id: z.string() });
export type LiveUpdate = z.infer<typeof liveUpdateSchema>;

/**
 * What the `observables-changed` event carries. `catalog: true` means a
 * definition changed: fetch the whole catalog again. Otherwise the message
 * holds just the providers and observables whose live part changed.
 */
export const observablesDeltaSchema = z.object({
  catalog: z.boolean().default(false),
  providers: z.array(providerInfoSchema).default([]),
  observables: z.array(liveUpdateSchema).default([]),
});
export type ObservablesDelta = z.infer<typeof observablesDeltaSchema>;

/** Everything the Action Library offers, as Rust evaluated it. Searched locally. */
export const catalogSchema = z.object({
  providers: z.array(providerInfoSchema),
  actions: z.array(actionDefinitionSchema),
  issues: z.array(z.string()).default([]),
});
export type Catalog = z.infer<typeof catalogSchema>;
export const emptyCatalog: Catalog = { providers: [], actions: [], issues: [] };

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

// ---------------------------------------------------------------------------
// explain_control: why a control shows the look it shows. Assembled in Rust
// from the evaluator, the hub and the catalog; the UI displays it as it is.
// ---------------------------------------------------------------------------

export const overlayExplanationSchema = z.object({
  look: z.string(),
  source: z.enum(["event", "action"]),
  /** Position in the control's rules of the event rule that put it there. */
  rule: z.number().int().optional(),
  remainingMs: z.number().optional(),
  latched: z.boolean().default(false),
});
export type OverlayExplanation = z.infer<typeof overlayExplanationSchema>;

export const observableRefSchema = z.object({
  id: z.string(),
  /** The id with its arguments: `agent-control.slot.status(slot=2)`. */
  address: z.string(),
  provider: z.string(),
  /** Absent when no installed provider offers it any more. */
  providerName: z.string().optional(),
  name: z.string().optional(),
  kind: observableKindSchema,
});
export type ObservableRef = z.infer<typeof observableRefSchema>;

export const ruleExplanationSchema = z.object({
  index: z.number().int(),
  kind: z.enum(["state", "event"]),
  /** State rules: the test holds right now. */
  holds: z.boolean(),
  /** The rule decided the look below the overlay (state), or put the overlay there (event). */
  decided: z.boolean(),
  observed: observedSchema.optional(),
  /** The value in the words of its definition (`High`). */
  valueLabel: z.string().optional(),
  look: z.string().optional(),
  /** `false` is why a pad shows its resting look although a rule decided. */
  lookExists: z.boolean().optional(),
  observable: observableRefSchema.optional(),
  /** Why the state reads `unknown` or `unavailable`, in a sentence. */
  reason: z.string().optional(),
});
export type RuleExplanation = z.infer<typeof ruleExplanationSchema>;

export const explanationDiagnosticSchema = z.object({ kind: z.literal("missingLook"), rule: z.number().int(), look: z.string() });
export type ExplanationDiagnostic = z.infer<typeof explanationDiagnosticSchema>;

export const controlExplanationSchema = z.object({
  surfaceId: z.string(),
  pageId: z.string(),
  controlId: z.string(),
  /** The look on screen; `null` is the resting look. */
  shown: z.string().nullable(),
  layer: z.enum(["overlay", "state", "rest"]),
  overlay: overlayExplanationSchema.nullable(),
  /** The look the state rules give, whether or not an overlay covers it. */
  derived: z.string().nullable(),
  winner: z.number().int().nullable(),
  diagnostics: z.array(explanationDiagnosticSchema).default([]),
  rules: z.array(ruleExplanationSchema).default([]),
});
export type ControlExplanation = z.infer<typeof controlExplanationSchema>;

// ---------------------------------------------------------------------------
// review_feedback: a draft of a control's feedback, judged by Rust without
// saving it (src-tauri/src/core/review.rs). Issues are what saving would
// refuse; notes are what Rust can prove about a rule; the preview is the
// explanation of the draft against the values the hub holds now.
// ---------------------------------------------------------------------------

/** The part of a rule an issue is about, so the message can sit next to it. */
export const issueFieldSchema = z.enum(["look", "state", "args", "test", "value", "event", "where", "hold", "rules"]);
export type IssueField = z.infer<typeof issueFieldSchema>;

export const ruleIssueSchema = z.object({
  /** Position in the control's list (0-based). For `rules` it is the list's limit. */
  rule: z.number().int(),
  message: z.string(),
  field: issueFieldSchema.optional(),
  /** The setting an `args` issue is about. */
  param: z.string().optional(),
});
export type RuleIssue = z.infer<typeof ruleIssueSchema>;

export const ruleNoteSchema = z.discriminatedUnion("kind", [
  z.object({ kind: z.literal("missingLook"), rule: z.number().int(), look: z.string() }),
  z.object({ kind: z.literal("shadowed"), rule: z.number().int(), by: z.number().int() }),
  z.object({ kind: z.literal("missingObservable"), rule: z.number().int(), observableKind: observableKindSchema, provider: z.string(), id: z.string() }),
  z.object({ kind: z.literal("providerUnavailable"), rule: z.number().int(), provider: z.string(), providerName: z.string(), reason: z.string() }),
]);
export type RuleNote = z.infer<typeof ruleNoteSchema>;

/** The numbers the runtime enforces, so the editor does not copy them. */
export const feedbackLimitsSchema = z.object({
  maxRules: z.number().int(),
  maxLookName: z.number().int(),
  maxWhere: z.number().int(),
  defaultHoldMs: z.number().int(),
  minHoldMs: z.number().int(),
  maxHoldMs: z.number().int(),
});
export type FeedbackLimits = z.infer<typeof feedbackLimitsSchema>;

export const feedbackReviewSchema = z.object({
  issues: z.array(ruleIssueSchema).default([]),
  notes: z.array(ruleNoteSchema).default([]),
  limits: feedbackLimitsSchema,
  /** Only when the draft's rules or looks differ from the saved control. */
  preview: controlExplanationSchema.nullish(),
});
export type FeedbackReview = z.infer<typeof feedbackReviewSchema>;

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
  /** Actions that were invoked and have not finished, as the runtime holds them. */
  pending: z
    .array(z.object({ control: z.string().optional(), action: z.string(), since: z.number() }))
    .default([]),
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
      /** Local ids of the provider's states or events this action usually shows. */
      suggests: z.array(z.string()).default([]),
    }),
  ),
  states: z.array(z.object({ key: z.string(), description: z.string(), values: z.array(z.string()) })),
  permissions: z.array(z.string()),
  external: externalInfoSchema.optional(),
});
export type PluginManifest = z.infer<typeof pluginManifestSchema>;

export const pluginConfigSchema = z.object({
  key: z.string(),
  /** The name a person sees. The key when the manifest gives none. */
  label: z.string(),
  type: z.enum(["string", "number", "boolean"]),
  description: z.string(),
  secret: z.boolean(),
  /** Never present for secrets. */
  value: z.unknown().optional(),
  isSet: z.boolean(),
  default: z.unknown().optional(),
});
export type PluginConfig = z.infer<typeof pluginConfigSchema>;

/** How far a destination is from this computer. Rust decides it; the interface says it in words. */
export const reachSchema = z.enum(["loopback", "local", "internet"]);
export type Reach = z.infer<typeof reachSchema>;

export const grantStatusSchema = z.enum(["granted", "denied", "needsApproval"]);
export type GrantStatus = z.infer<typeof grantStatusSchema>;

/** One destination a plugin asks for now. */
export const networkHostSchema = z.object({
  /** Scheme, host and port: exactly what a grant covers. */
  origin: z.string(),
  reach: reachSchema,
  status: grantStatusSchema,
  /** Written in the manifest, or the origin of the URL in one of the plugin's settings. */
  source: z.discriminatedUnion("kind", [z.object({ kind: z.literal("declared") }), z.object({ kind: z.literal("setting"), key: z.string() })]),
  decidedAt: z.number().optional(),
});
export type NetworkHost = z.infer<typeof networkHostSchema>;

/** What a plugin asks of the network and what was decided about it. */
export const networkViewSchema = z.object({
  /** Why the plugin needs it, in its author's words. */
  purpose: z.string(),
  status: grantStatusSchema,
  hosts: z.array(networkHostSchema),
  /** Hosts named through a setting that gives no destination yet. */
  unresolved: z.array(z.object({ setting: z.string(), reason: z.string() })),
  /** Decisions on file for destinations the plugin no longer asks for. They cover nothing. */
  stale: z.array(z.object({ origin: z.string(), decision: z.enum(["granted", "denied"]) })),
});
export type NetworkView = z.infer<typeof networkViewSchema>;

export const networkFailureSchema = z.object({
  code: z.string(),
  message: z.string(),
  origin: z.string().optional(),
  at: z.number(),
});
export type NetworkFailure = z.infer<typeof networkFailureSchema>;

/** A secret of a plugin: everything but its value, which never leaves the host. */
export const pluginSecretSchema = z.object({
  key: z.string(),
  label: z.string(),
  description: z.string(),
  isSet: z.boolean(),
  /** The only destination it is sent to. */
  origin: z.string().optional(),
});
export type PluginSecret = z.infer<typeof pluginSecretSchema>;

export const secretStoreSchema = z.enum(["keychain", "file", "memory"]);
export type SecretStoreKind = z.infer<typeof secretStoreSchema>;

export const pluginInfoSchema = z.object({
  plugin: pluginManifestSchema,
  dir: z.string(),
  /** What the person chose; `plugin.external.status` is what actually happened. */
  enabled: z.boolean(),
  config: z.array(pluginConfigSchema),
  /** Absent for a plugin that asks for no network access. */
  network: networkViewSchema.optional(),
  /** The last request that was refused or failed, unless one succeeded since. */
  networkFailure: networkFailureSchema.optional(),
  secrets: z.array(pluginSecretSchema),
  secretStore: secretStoreSchema,
  refreshSeconds: z.number().optional(),
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
  /** Every state and event a control can observe, with what each reads now. */
  observables: observableCatalogSchema,
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
