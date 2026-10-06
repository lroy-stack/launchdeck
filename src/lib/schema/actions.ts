// Actions: what a control does (an assignment), the parameters an action takes,
// and the Action Library's catalog and validation, as Rust evaluates them.

import { z } from "zod";

export const hexColor = z.string().regex(/^#[0-9a-fA-F]{6}$/);

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

/** Everything the Action Library offers, as Rust evaluated it. Searched locally. */
export const catalogSchema = z.object({
  providers: z.array(providerInfoSchema),
  actions: z.array(actionDefinitionSchema),
  issues: z.array(z.string()).default([]),
});
export type Catalog = z.infer<typeof catalogSchema>;
export const emptyCatalog: Catalog = { providers: [], actions: [], issues: [] };

/** `missing`: a required value that is absent or empty; `invalid`: anything else (Rust's `IssueKind`). */
export const issueKindSchema = z.enum(["missing", "invalid"]);
export type IssueKind = z.infer<typeof issueKindSchema>;

/** `kind` is optional only for issues from a build older than 0.3, which did not send it. */
export const issueSchema = z.object({ param: z.string().optional(), message: z.string(), kind: issueKindSchema.optional() });
export type Issue = z.infer<typeof issueSchema>;

export const validationSchema = z.object({
  ok: z.boolean(),
  assignment: assignmentSchema.optional(),
  issues: z.array(issueSchema).default([]),
});
export type Validation = z.infer<typeof validationSchema>;
