// The values states hold and the Observable Catalog.

import { z } from "zod";
import { availabilitySchema, paramDefSchema, providerInfoSchema } from "./actions";

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
