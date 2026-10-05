// Plugins: manifests, settings, network access, secrets, logs and scans.

import { z } from "zod";
import { appearanceSchema, paramDefSchema, providerKindSchema } from "./actions";

export const pluginStatusSchema = z.enum(["enabled", "disabled", "error"]);
export type PluginStatus = z.infer<typeof pluginStatusSchema>;

/** What kind of problem keeps a plugin from running. Rust decides it; `features/plugins/text.ts` says it in a sentence. */
export const problemKindSchema = z.enum(["incompatible", "invalidManifest", "invalidModule", "duplicate", "initFailed", "stopped"]);
export type ProblemKind = z.infer<typeof problemKindSchema>;

/** Present only for plugins installed from disk (WebAssembly). Built-in plugins have none. */
export const externalInfoSchema = z.object({
  status: pluginStatusSchema,
  /** The detail, in the host's words. */
  error: z.string().optional(),
  /** Present exactly when `error` is. */
  kind: problemKindSchema.optional(),
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

/** `unavailable`: this system has no secure credential store, so secrets cannot be kept at all. */
export const secretStoreSchema = z.enum(["keychain", "memory", "unavailable"]);
export type SecretStoreKind = z.infer<typeof secretStoreSchema>;

/** Where secrets are kept on this computer, and when they cannot be, why. */
export const secretStoreStatusSchema = z.object({
  kind: secretStoreSchema,
  available: z.boolean(),
  /** What this system calls its credential store. */
  name: z.string(),
  reason: z.string().optional(),
});
export type SecretStoreStatus = z.infer<typeof secretStoreStatusSchema>;

/** A secret an earlier version kept in plain text: its name, never its value. */
export const legacySecretSchema = z.object({
  plugin: z.string(),
  pluginName: z.string().optional(),
  key: z.string(),
  /** `file`: the old `secrets.json`. `settings`: a setting an older version of the plugin kept in `plugins.json`. */
  source: z.enum(["file", "settings"]),
});
export type LegacySecret = z.infer<typeof legacySecretSchema>;

export const legacySecretsSchema = z.object({
  entries: z.array(legacySecretSchema),
  file: z.string().optional(),
  unreadable: z.boolean(),
});
export type LegacySecrets = z.infer<typeof legacySecretsSchema>;

export const legacyMigrationSchema = z.object({
  moved: z.number(),
  kept: z.number(),
  failed: z.array(z.object({ plugin: z.string(), key: z.string(), reason: z.string() })),
  fileRemoved: z.boolean(),
});
export type LegacyMigration = z.infer<typeof legacyMigrationSchema>;

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

/** A folder that held something that is not a usable plugin. */
export const pluginIssueSchema = z.object({ dir: z.string(), kind: problemKindSchema, error: z.string() });
export type PluginIssue = z.infer<typeof pluginIssueSchema>;

export const pluginsOverviewSchema = z.object({
  plugins: z.array(pluginInfoSchema),
  issues: z.array(pluginIssueSchema),
  folders: z.array(z.string()),
  secretStore: secretStoreStatusSchema,
  /** Secrets an earlier version left in plain text. They are not used. */
  legacySecrets: legacySecretsSchema,
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
