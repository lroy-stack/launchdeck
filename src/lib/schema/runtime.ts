// The running app: devices, the runtime state, events, the snapshot, LED frames, and
// what the app is (its versions, for Settings > About).

import { z } from "zod";
import { agentsSnapshotSchema } from "../agents";
import { catalogSchema } from "./actions";
import { contextStatusSchema } from "./context";
import { observableCatalogSchema } from "./observables";
import { pluginManifestSchema } from "./plugins";
import { hexColor } from "./actions";
import { controlPosSchema, visualSchema, workspaceSchema } from "./workspace";

const deviceCapabilitiesSchema = z.object({
  grid: z.object({ width: z.number(), height: z.number() }).nullish(),
  aux: z.object({ top: z.number(), side: z.number(), logo: z.boolean() }),
  rgb: z.boolean(),
  velocity: z.boolean(),
  pressure: z.boolean(),
  animations: z.object({ pulse: z.boolean(), flash: z.boolean() }),
});

/** `devices::DeviceFeatures`: what a device can do beyond drawing frames. */
const deviceFeaturesSchema = z.object({
  /** LaunchDeck switches it into a mode it owns (Programmer Mode) and back. */
  hostMode: z.boolean(),
  palette: z.boolean(),
  batchedLighting: z.boolean(),
  flashPeriodMs: z.number().int().nullish(),
  pulsePeriodMs: z.number().int().nullish(),
  brightness: z.boolean(),
  sleep: z.boolean(),
  scrollingText: z.boolean(),
  firmwareVersion: z.boolean(),
});

/** `devices::ModeState`: whether LaunchDeck holds the mode it needs on the device. */
export const modeStateSchema = z.enum(["none", "acquiring", "confirmed", "unconfirmed"]);
export type ModeState = z.infer<typeof modeStateSchema>;

/** `devices::DeviceDetails`: what LaunchDeck observed on a connected device. */
const deviceDetailsSchema = z.object({
  /** As the device shows it, such as "0531". */
  firmware: z.string().nullish(),
  mode: modeStateSchema,
  /** Percent, when LaunchDeck set or read it. */
  brightness: z.number().int().nullish(),
  asleep: z.boolean(),
  scrolling: z.boolean(),
  notice: z.string().nullish(),
});

/** `devices::TextScroll`: text a device scrolls across its pads by itself; Rust checks the limits. */
export const textScrollSchema = z.object({
  text: z.string(),
  color: hexColor,
  /** Pads per second. */
  speed: z.number().int(),
  direction: z.enum(["left", "right"]),
  looping: z.boolean(),
});
export type TextScroll = z.infer<typeof textScrollSchema>;

export const deviceStatusSchema = z.enum(["connected", "available", "unplugged", "unsupported", "failed"]);
export type DeviceStatus = z.infer<typeof deviceStatusSchema>;

export const deviceInfoSchema = z.object({
  identity: z.object({
    id: z.string(),
    name: z.string(),
    model: z.string(),
    kind: z.enum(["virtual", "hardware"]),
  }),
  capabilities: deviceCapabilitiesSchema,
  /** `status === "connected"`, kept for older readers. */
  connected: z.boolean(),
  /** Mirrors `DeviceStatus` in `devices/manager.rs`. */
  status: deviceStatusSchema,
  /** A sentence from Rust when something went wrong (a failed connect). */
  problem: z.string().nullish(),
  /** Connected, with a limitation, in a sentence from Rust. */
  notice: z.string().nullish(),
  /** Always sent by Rust; optional so that a device written without them still reads. */
  details: deviceDetailsSchema.optional(),
  features: deviceFeaturesSchema.optional(),
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

export const controlStateKey = (surfaceId: string, pageId: string, controlId: string): string =>
  `${surfaceId}/${pageId}/${controlId}`;

/** The saved workspace was set aside at start (`persistence::StartupNotice`): why, and where its copy is. */
export const startupNoticeSchema = z.object({
  kind: z.enum(["workspaceDamaged", "workspaceTooNew"]),
  /** The backup's path, to show under Details. */
  backup: z.string(),
});
export type StartupNotice = z.infer<typeof startupNoticeSchema>;

export const snapshotSchema = z.object({
  workspace: workspaceSchema,
  state: runtimeStateSchema,
  plugins: z.array(pluginManifestSchema),
  catalog: catalogSchema,
  /** Every state and event a control can observe, with what each reads now. */
  observables: observableCatalogSchema,
  recentEvents: z.array(runtimeEventSchema),
  agents: agentsSnapshotSchema,
  context: contextStatusSchema,
  /** What the person must be told about this start; empty almost always. */
  notices: z.array(startupNoticeSchema).default([]),
});
export type Snapshot = z.infer<typeof snapshotSchema>;

export const frameSchema = z.object({
  leds: z.array(z.object({ pos: controlPosSchema, led: visualSchema })),
});
export type Frame = z.infer<typeof frameSchema>;

/** `commands::app::AppInfo`: which LaunchDeck this is and every compatibility number (docs/architecture/versions.md). */
export const appInfoSchema = z.object({
  /** Cargo.toml's version, the only source of the app version. */
  appVersion: z.string(),
  /** What this build is offered as, such as "alpha". */
  channel: z.string(),
  sdkVersion: z.string(),
  abiVersion: z.number(),
  /** The plugin manifest schemas this build reads, both ends included. */
  manifestSchemas: z.object({ oldest: z.number(), newest: z.number() }),
  workspaceSchema: z.number(),
  bridgeProtocol: z.number(),
  artifactFormat: z.number(),
  /** As Rust names them (`std::env::consts`): "macos", "windows", "linux"; "aarch64", "x86_64". */
  os: z.string(),
  arch: z.string(),
  /** The app data folder, for people to read. */
  dataFolder: z.string(),
});
export type AppInfo = z.infer<typeof appInfoSchema>;
