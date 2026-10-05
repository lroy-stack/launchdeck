// Feedback: the rules that decide a control's look, why a control shows the look
// it shows, and the review of a draft.

import { z } from "zod";
import { observableKindSchema, observedSchema, scalarSchema } from "./observables";

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
