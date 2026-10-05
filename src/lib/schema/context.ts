// Auto Context: the surface follows the application in front.

import { z } from "zod";

/** An application, by what the operating system itself calls it. `displayName` is for people and is never compared. */
export const applicationIdentitySchema = z.object({
  /** Which kind of identity `id` is: `macos` (bundle id), `windows` (executable name), `x11` (WM_CLASS). */
  namespace: z.string(),
  id: z.string(),
  displayName: z.string().default(""),
});
export type ApplicationIdentity = z.infer<typeof applicationIdentitySchema>;

export const contextRuleSchema = z.object({
  application: applicationIdentitySchema,
  surfaceId: z.string(),
  pageId: z.string().optional(),
});
export type ContextRule = z.infer<typeof contextRuleSchema>;

/** What happens for an application no rule names. */
export const contextFallbackSchema = z.discriminatedUnion("kind", [
  z.object({ kind: z.literal("keep") }),
  z.object({ kind: z.literal("surface"), surfaceId: z.string(), pageId: z.string().optional() }),
]);
export type ContextFallback = z.infer<typeof contextFallbackSchema>;

/** Everything about context that is saved. Nothing about which application is or was in front. */
export const contextSettingsSchema = z.object({
  auto: z.boolean().default(false),
  rules: z.array(contextRuleSchema).default([]),
  fallback: contextFallbackSchema.default({ kind: "keep" }),
});
export type ContextSettings = z.infer<typeof contextSettingsSchema>;

/** Whether this system can tell which application is in front, and if not, why. */
export const observerStatusSchema = z.discriminatedUnion("state", [
  z.object({ state: z.literal("available"), mechanism: z.string() }),
  z.object({ state: z.literal("degraded"), mechanism: z.string(), reason: z.string() }),
  z.object({ state: z.literal("unavailable"), reason: z.string() }),
]);
export type ObserverStatus = z.infer<typeof observerStatusSchema>;

export const contextModeSchema = z.enum(["off", "auto", "manual", "paused", "unavailable"]);
export type ContextMode = z.infer<typeof contextModeSchema>;

/** What is in front now. LaunchDeck's own window and "nothing" are never treated as applications. */
export const foregroundSchema = z.discriminatedUnion("kind", [
  z.object({ kind: z.literal("unknown") }),
  z.object({ kind: z.literal("nothing") }),
  z.object({ kind: z.literal("launchDeck") }),
  z.object({ kind: z.literal("application"), application: applicationIdentitySchema }),
]);
export type Foreground = z.infer<typeof foregroundSchema>;

/** What the rules said about the application and what was done about it. */
export const contextDecisionSchema = z.object({
  application: applicationIdentitySchema,
  /** The rule that matched, from 0. Absent when none did (the fallback decided). */
  rule: z.number().int().optional(),
  outcome: z.enum(["activated", "alreadyShown", "kept", "missingSurface"]),
  /** Set when the decision was not acted on, with the reason. */
  held: z.enum(["off", "paused", "manual"]).optional(),
  surfaceId: z.string().optional(),
  pageId: z.string().optional(),
  pageMissing: z.boolean(),
});
export type ContextDecision = z.infer<typeof contextDecisionSchema>;

export const ruleProblemSchema = z.discriminatedUnion("kind", [
  z.object({ kind: z.literal("missingSurface") }),
  z.object({ kind: z.literal("missingPage") }),
  z.object({ kind: z.literal("shadowed"), by: z.number().int() }),
  z.object({ kind: z.literal("otherSystem") }),
]);
export type RuleProblem = z.infer<typeof ruleProblemSchema>;

/** Auto Context as it is now. Only `settings` is ever saved. */
export const contextStatusSchema = z.object({
  settings: contextSettingsSchema,
  observer: observerStatusSchema,
  mode: contextModeSchema,
  foreground: foregroundSchema,
  /** The application rules are asked about: the last one that came to the front and stayed. */
  application: applicationIdentitySchema.optional(),
  decision: contextDecisionSchema.optional(),
  /** Seen in front since LaunchDeck started, newest first. In memory only. */
  recent: z.array(applicationIdentitySchema),
  /** One entry per rule, in order. */
  problems: z.array(ruleProblemSchema.nullable()),
  namespace: z.string().optional(),
});
export type ContextStatus = z.infer<typeof contextStatusSchema>;
