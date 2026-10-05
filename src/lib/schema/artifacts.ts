// Surface files and Packs (PRODUCTIZATION_PLAN.md section 7): what Rust says
// about a file to import (the ImportPlan and its outcomes) and about a file to
// export (the preview), and what the views send back (decisions, a request).
//
// The serde types are in src-tauri/src/artifacts (plan/types.rs,
// plan/decisions.rs, export/{request,preview}.rs, format/manifest.rs,
// review.rs, service.rs, apply.rs, limits.rs). A member Rust leaves out when
// it is empty is optional here, without a default, so that the contract test
// can tell a field Rust added from one it left out. React shows these; it
// never computes one of them.

import { z } from "zod";
import { applicationIdentitySchema } from "./context";

const count = z.number().int().nonnegative();

export const artifactKindSchema = z.enum(["surface", "pack"]);
export type ArtifactKind = z.infer<typeof artifactKindSchema>;

/** A press or long press in a file: the surface by the file's id for it, the page and control by their own. */
export const locationSchema = z.object({
  surfaceSourceId: z.string(),
  pageId: z.string(),
  controlId: z.string(),
  slot: z.enum(["press", "longPress"]),
});
export type Location = z.infer<typeof locationSchema>;

// ---- manifest ---------------------------------------------------------------

export const inputTypeSchema = z.enum(["path", "url", "enum", "string", "application"]);
export type InputType = z.infer<typeof inputTypeSchema>;

/** An answer: text for every input type but `application`, which takes an application. */
export const inputValueSchema = z.discriminatedUnion("kind", [
  z.object({ kind: z.literal("text"), value: z.string() }),
  z.object({ kind: z.literal("application"), namespace: z.string(), id: z.string(), displayName: z.string() }),
]);
export type InputValue = z.infer<typeof inputValueSchema>;

export const inputOptionSchema = z.object({ value: z.string(), label: z.string() });
export type InputOption = z.infer<typeof inputOptionSchema>;

/** What a Pack asks the person importing it. `required` absent means required. */
export const inputDefSchema = z.object({
  id: z.string(),
  type: inputTypeSchema,
  label: z.string(),
  description: z.string().optional(),
  required: z.boolean().optional(),
  default: inputValueSchema.optional(),
  options: z.array(inputOptionSchema).optional(),
  maxLength: count.optional(),
});
export type InputDef = z.infer<typeof inputDefSchema>;

/** Shown as text, never fetched. */
export const authorSchema = z.object({ name: z.string(), url: z.string().optional() });
export type Author = z.infer<typeof authorSchema>;

export const createdWithSchema = z.object({ app: z.string(), version: z.string() });

export const testedWithSchema = z.object({ model: z.string(), kind: z.enum(["hardware", "virtual"]) });
export type TestedWith = z.infer<typeof testedWithSchema>;

/** What a device needs to show every control of a file. */
export const requirementsSchema = z.object({
  grid: z.object({ minWidth: count, minHeight: count }).optional(),
  aux: z.object({ top: count, side: count }),
  rgb: z.boolean(),
});
export type Requirements = z.infer<typeof requirementsSchema>;

/** A plugin the content uses, as the file declares it. */
export const dependencySchema = z.object({
  pluginId: z.string(),
  name: z.string().optional(),
  version: z.string(),
  required: z.boolean(),
});
export type Dependency = z.infer<typeof dependencySchema>;

export const manifestSchema = z.object({
  id: z.string(),
  name: z.string(),
  version: z.string(),
  description: z.string().optional(),
  author: authorSchema.optional(),
  license: z.string().optional(),
  createdWith: createdWithSchema,
  requirements: requirementsSchema,
  testedWith: z.array(testedWithSchema).optional(),
  dependencies: z.array(dependencySchema).optional(),
  inputs: z.array(inputDefSchema).optional(),
  integrity: z.object({ content: z.string() }),
});
export type Manifest = z.infer<typeof manifestSchema>;

/** A context rule's application in a file: named once per system, or asked of the person importing. */
export const appRefSchema = z.union([
  z.object({ input: z.string() }),
  z.object({ name: z.string(), identities: z.array(z.object({ namespace: z.string(), id: z.string() })) }),
]);
export type AppRef = z.infer<typeof appRefSchema>;

// ---- what runs --------------------------------------------------------------

export const reviewKindSchema = z.enum(["runsCommand", "sendsRequest", "launchesProgram", "instructsAgent"]);
export type ReviewKind = z.infer<typeof reviewKindSchema>;

/** A pad a person looks at before sharing or importing: what it runs, exactly. */
export const reviewItemSchema = z.object({ at: locationSchema, label: z.string(), kind: reviewKindSchema, text: z.string() });
export type ReviewItem = z.infer<typeof reviewItemSchema>;

// ---- a refused file -----------------------------------------------------------

export const rejectionCodeSchema = z.enum([
  "empty",
  "tooLarge",
  "notUtf8",
  "tooDeep",
  "duplicateKey",
  "stringTooLong",
  "notJson",
  "notArtifact",
  "tooNew",
  "unsupportedFormatVersion",
  "unknownField",
  "unsupportedSignature",
  "unsupportedAsset",
  "invalidManifest",
  "integrity",
  "invalidContent",
  "tooMany",
  "kindMismatch",
]);

/** `message` is for a person; `at` a JSON pointer into the file. */
export const rejectionSchema = z.object({ code: rejectionCodeSchema, message: z.string(), at: z.string().optional() });
export type Rejection = z.infer<typeof rejectionSchema>;

// ---- the import plan ----------------------------------------------------------

export const importDecisionsSchema = z.object({
  /** Only when the file was imported before. */
  resolution: z
    .discriminatedUnion("kind", [z.object({ kind: z.literal("importAsNew") }), z.object({ kind: z.literal("replace"), instance: count })])
    .optional(),
  /** Only when a pad runs a command, sends a request or starts a program. */
  runs: z.enum(["keep", "remove"]).optional(),
  inputs: z.record(z.string(), inputValueSchema).default({}),
  /** The file's context rules to add, by position. */
  contextRules: z.array(count).default([]),
  confirmOverwriteEdits: z.boolean().default(false),
});
export type ImportDecisions = z.infer<typeof importDecisionsSchema>;
export type Resolution = NonNullable<ImportDecisions["resolution"]>;

const surfaceActionSchema = z.discriminatedUnion("kind", [
  z.object({ kind: z.literal("add") }),
  z.object({ kind: z.literal("replace"), previousVersion: z.string(), locallyEdited: z.boolean() }),
]);

export const plannedSurfaceSchema = z.object({
  sourceId: z.string(),
  localId: z.string(),
  name: z.string(),
  category: z.string(),
  pages: count,
  controls: count,
  action: surfaceActionSchema,
  idChanged: z.boolean(),
  /** The local id of a surface that already has this name. */
  sameNameAs: z.string().optional(),
  missingProviders: z.array(z.string()),
});
export type PlannedSurface = z.infer<typeof plannedSurfaceSchema>;

export const ruleOutcomeSchema = z.discriminatedUnion("kind", [
  z.object({ kind: z.literal("add"), position: count }),
  z.object({ kind: z.literal("notSelected") }),
  z.object({ kind: z.literal("skipConflict"), localRule: count }),
  z.object({ kind: z.literal("skipShadowed"), by: count }),
  z.object({ kind: z.literal("skipNoIdentity") }),
  z.object({ kind: z.literal("skipCapacity") }),
  z.object({ kind: z.literal("needsInput"), input: z.string() }),
]);
export type RuleOutcome = z.infer<typeof ruleOutcomeSchema>;

export const plannedRuleSchema = z.object({
  index: count,
  applicationName: z.string(),
  application: applicationIdentitySchema.optional(),
  surfaceLocalId: z.string(),
  pageId: z.string().optional(),
  outcome: ruleOutcomeSchema,
});
export type PlannedRule = z.infer<typeof plannedRuleSchema>;

export const dependencyStatusSchema = z.discriminatedUnion("kind", [
  z.object({ kind: z.literal("compatible"), installed: z.string() }),
  z.object({ kind: z.literal("disabled"), installed: z.string() }),
  z.object({ kind: z.literal("incompatible"), installed: z.string() }),
  z.object({ kind: z.literal("missing") }),
]);
export type DependencyStatus = z.infer<typeof dependencyStatusSchema>;

export const dependencyCheckSchema = z.object({
  pluginId: z.string(),
  name: z.string(),
  requirement: z.string(),
  required: z.boolean(),
  status: dependencyStatusSchema,
  usedBy: z.array(z.object({ surfaceSourceId: z.string(), pageId: z.string(), controlId: z.string(), label: z.string() })),
});
export type DependencyCheck = z.infer<typeof dependencyCheckSchema>;

export const plannedInputSchema = z.object({
  def: inputDefSchema,
  value: inputValueSchema.optional(),
  /** Sentences for a person; empty when the answer is fine. */
  issues: z.array(z.string()),
  usedBy: z.array(locationSchema),
});
export type PlannedInput = z.infer<typeof plannedInputSchema>;

export const existingInstanceSchema = z.object({
  instance: count,
  version: z.string(),
  relation: z.enum(["same", "newer", "older"]),
  identicalContent: z.boolean(),
  surfaces: z.array(z.object({ localId: z.string(), name: z.string(), sourceId: z.string(), locallyEdited: z.boolean() })),
});
export type ExistingInstance = z.infer<typeof existingInstanceSchema>;

export const deviceFitSchema = z.object({
  name: z.string(),
  model: z.string(),
  kind: z.enum(["virtual", "hardware"]),
  connected: z.boolean(),
  fits: z.boolean(),
  outside: count,
  rgbOk: z.boolean(),
});
export type DeviceFit = z.infer<typeof deviceFitSchema>;

export const compatibilitySchema = z.object({
  namespace: z.string().optional(),
  requirements: requirementsSchema,
  devices: z.array(deviceFitSchema),
});

export const planWarningSchema = z.object({
  code: z.enum([
    "extensionMismatch",
    "unknownCategory",
    "keptFromPreviousVersion",
    "externalSurfaceReference",
    "invalidForInstalledVersion",
    "unknownHostProvider",
    "externalEventSource",
    "unusedDependency",
    "missingDependency",
    "disabledDependency",
    "incompatibleDependency",
  ]),
  message: z.string(),
  at: z.string().optional(),
});
export type PlanWarning = z.infer<typeof planWarningSchema>;

export const blockerSchema = z.object({
  code: z.enum(["resolutionNeeded", "overwriteEditsNotConfirmed", "runsDecisionNeeded", "inputRequired", "inputInvalid", "workspaceFull"]),
  message: z.string(),
  input: z.string().optional(),
});
export type Blocker = z.infer<typeof blockerSchema>;

/** Exactly what applying writes, and the only part the plan's digest covers. */
export const planEffectsSchema = z.object({
  instance: count,
  addSurfaces: z.array(z.object({ sourceId: z.string(), localId: z.string(), name: z.string() })),
  replaceSurfaces: z.array(z.object({ sourceId: z.string(), localId: z.string() })),
  addRules: z.array(z.object({ index: count, application: applicationIdentitySchema, surfaceLocalId: z.string(), pageId: z.string().optional() })),
  removedSlots: z.array(locationSchema),
  inputValues: z.record(z.string(), inputValueSchema),
});

export const importPlanSchema = z.object({
  planId: z.string(),
  planDigest: z.string(),
  source: z.object({ fileName: z.string(), sizeBytes: count, contentDigest: z.string(), formatVersion: count }),
  artifact: z.object({
    kind: artifactKindSchema,
    id: z.string(),
    name: z.string(),
    version: z.string(),
    description: z.string(),
    author: authorSchema.optional(),
    license: z.string(),
    createdWith: createdWithSchema,
    testedWith: z.array(testedWithSchema),
  }),
  surfaces: z.array(plannedSurfaceSchema),
  contextRules: z.array(plannedRuleSchema),
  dependencies: z.array(dependencyCheckSchema),
  inputs: z.array(plannedInputSchema),
  review: z.array(reviewItemSchema),
  existing: z.array(existingInstanceSchema),
  compatibility: compatibilitySchema,
  warnings: z.array(planWarningSchema),
  /** The decisions this plan was made with. */
  decisions: importDecisionsSchema,
  /** What the view proposes. Names a resolution or `runs` only when that choice exists. */
  suggested: importDecisionsSchema,
  blockers: z.array(blockerSchema),
  ready: z.boolean(),
  effects: planEffectsSchema,
});
export type ImportPlan = z.infer<typeof importPlanSchema>;

export const inspectOutcomeSchema = z.discriminatedUnion("status", [
  z.object({ status: z.literal("plan"), plan: importPlanSchema }),
  z.object({ status: z.literal("rejected"), rejection: rejectionSchema }),
]);
export type InspectOutcome = z.infer<typeof inspectOutcomeSchema>;

export const previewOutcomeSchema = z.discriminatedUnion("status", [
  z.object({ status: z.literal("plan"), plan: importPlanSchema }),
  /** No pending import has this id any more. */
  z.object({ status: z.literal("expired") }),
]);
export type PreviewOutcome = z.infer<typeof previewOutcomeSchema>;

export const importReportSchema = z.object({
  /** Local ids, in the file's order. */
  surfacesAdded: z.array(z.string()),
  surfacesReplaced: z.array(z.string()),
  rulesAdded: count,
  rulesSkipped: count,
  slotsRemoved: count,
  missingDependencies: z.array(dependencyCheckSchema),
  /** Whether Auto Context is on; an import never changes it. */
  autoContext: z.boolean(),
});
export type ImportReport = z.infer<typeof importReportSchema>;

/** Anything but `applied` changed nothing. */
export const applyOutcomeSchema = z.discriminatedUnion("status", [
  z.object({ status: z.literal("applied"), report: importReportSchema }),
  z.object({ status: z.literal("stale"), plan: importPlanSchema }),
  z.object({ status: z.literal("blocked"), plan: importPlanSchema }),
  z.object({ status: z.literal("failed"), message: z.string(), detail: z.string().optional() }),
  z.object({ status: z.literal("expired") }),
]);
export type ApplyOutcome = z.infer<typeof applyOutcomeSchema>;

/** Where an imported surface came from. Local only, for display: never exported. */
export const importOriginSchema = z.object({
  format: z.string(),
  artifactId: z.string(),
  artifactName: z.string(),
  artifactVersion: z.string(),
  instance: count,
  sourceSurfaceId: z.string(),
  contentDigest: z.string(),
  surfaceDigest: z.string(),
});
export type ImportOrigin = z.infer<typeof importOriginSchema>;

// ---- export -------------------------------------------------------------------

/** An empty id, name or version is proposed by Rust; the preview's manifest shows what it chose. */
export const exportMetadataSchema = z.object({
  id: z.string(),
  name: z.string(),
  version: z.string(),
  description: z.string().optional(),
  author: authorSchema.optional(),
  license: z.string().optional(),
});
export type ExportMetadata = z.infer<typeof exportMetadataSchema>;

export const exportRequestSchema = z.object({
  kind: artifactKindSchema,
  /** Local ids. */
  surfaceIds: z.array(z.string()),
  metadata: exportMetadataSchema,
  includeContextRules: z.boolean(),
  inputs: z.array(inputDefSchema),
  /** The answer to `input` replaces the text argument `arg` of the pad at `at`. */
  bindings: z.array(z.object({ at: locationSchema, arg: z.string(), input: z.string() })),
  /** The person importing chooses the application of workspace rule `rule`. */
  ruleInputs: z.array(z.object({ rule: count, input: z.string() })),
  /** The id rule `rule`'s application has on another system. */
  extraIdentities: z.array(z.object({ rule: count, namespace: z.string(), id: z.string() })),
});
export type ExportRequest = z.infer<typeof exportRequestSchema>;

export const exportIssueSchema = z.object({
  code: z.enum([
    "noSurfaces",
    "unknownSurface",
    "kindMismatch",
    "invalidMetadata",
    "invalidInput",
    "invalidBinding",
    "invalidIdentity",
    "credential",
    "tooMany",
    "tooLarge",
    "invalidContent",
    "reviewBeforeSharing",
    "externalSurfaceReference",
    "notInstalled",
    "localPath",
    "ruleTargetMissing",
  ]),
  message: z.string(),
  at: z.string().optional(),
});
export type ExportIssue = z.infer<typeof exportIssueSchema>;

/** A context rule as the file holds it; `rule` is its position in the workspace. */
export const exportedRuleSchema = z.object({ rule: count, application: appRefSchema, surface: z.string(), page: z.string().optional() });
export type ExportedRule = z.infer<typeof exportedRuleSchema>;

/** A text argument of a pad: its value here, or the input that asks for it instead. */
export const parameterSchema = z.object({ at: locationSchema, arg: z.string(), value: z.string().optional(), input: z.string().optional() });
export type Parameter = z.infer<typeof parameterSchema>;

export const suggestionSchema = z.object({ at: locationSchema, arg: z.string(), value: z.string(), suggestedType: z.enum(["path"]) });
export type Suggestion = z.infer<typeof suggestionSchema>;

export const exportPreviewSchema = z.object({
  fileName: z.string(),
  sizeBytes: count,
  contentDigest: z.string(),
  manifest: manifestSchema,
  surfaces: z.array(z.object({ localId: z.string(), artifactId: z.string(), name: z.string(), pages: count, controls: count })),
  contextRules: z.array(exportedRuleSchema),
  dependencies: z.array(dependencySchema),
  requirements: requirementsSchema,
  testedWith: z.array(testedWithSchema),
  review: z.array(reviewItemSchema),
  /** What a file never carries, counted. */
  stripped: z.object({ legacyBindings: count, legacyActions: count, contextHints: count }),
  suggestions: z.array(suggestionSchema),
  parameters: z.array(parameterSchema),
  warnings: z.array(exportIssueSchema),
  /** Anything here stops the file from being saved. */
  errors: z.array(exportIssueSchema),
});
export type ExportPreview = z.infer<typeof exportPreviewSchema>;

export const exportSavedSchema = z.object({ fileName: z.string(), folder: z.string(), sizeBytes: count, contentDigest: z.string() });
export type ExportSaved = z.infer<typeof exportSavedSchema>;

// ---- limits ---------------------------------------------------------------------

/** Every limit of the format, as Rust holds them (artifacts/limits.rs), so the views never copy one. */
export const artifactLimitsSchema = z.object({
  fileBytes: count,
  jsonDepth: count,
  stringBytes: count,
  keyBytes: count,
  assets: count,
  surfaceFileSurfaces: count,
  packSurfaces: count,
  pages: count,
  controlsPerPage: count,
  controls: count,
  looks: count,
  lookName: count,
  feedbackRules: count,
  whereConditions: count,
  minHoldMs: count,
  maxHoldMs: count,
  grid: count,
  span: count,
  aux: count,
  contextRules: count,
  identities: count,
  applicationName: count,
  identityId: count,
  namespace: count,
  argKeys: count,
  argDepth: count,
  argBytes: count,
  argText: count,
  bindings: count,
  name: count,
  description: count,
  authorName: count,
  authorUrl: count,
  license: count,
  version: count,
  requirement: count,
  comparators: count,
  dependencies: count,
  dependencyName: count,
  testedWith: count,
  model: count,
  inputs: count,
  inputId: count,
  inputLabel: count,
  inputDescription: count,
  enumOptions: count,
  optionValue: count,
  stringInput: count,
  defaultStringInput: count,
  path: count,
  url: count,
  id: count,
  label: count,
  surfaceDescription: count,
  controlDescription: count,
  icon: count,
  category: count,
  workspaceSurfaces: count,
});
export type ArtifactLimits = z.infer<typeof artifactLimitsSchema>;
