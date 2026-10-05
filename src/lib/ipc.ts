// The only module that talks to Tauri. Everything coming back is validated
// with Zod; the React tree never sees unchecked data.

import { z } from "zod";
import {
  agentsSnapshotSchema,
  hookSetupSchema,
  notificationSchema,
  type Access,
  type AgentNotification,
  type AgentsSnapshot,
  type HookSetup,
} from "./agents";
import {
  catalogSchema,
  controlExplanationSchema,
  exampleInfoSchema,
  feedbackReviewSchema,
  frameSchema,
  observableCatalogSchema,
  observablesDeltaSchema,
  observedSchema,
  pluginInfoSchema,
  pluginLogSchema,
  pluginsOverviewSchema,
  legacyMigrationSchema,
  contextStatusSchema,
  runtimeEventSchema,
  runtimeStateSchema,
  scanReportSchema,
  snapshotSchema,
  validationSchema,
  workspaceSchema,
  type Assignment,
  type Catalog,
  type Control,
  type ControlExplanation,
  type FeedbackReview,
  type ControlPos,
  type ExampleInfo,
  type Frame,
  type Mode,
  type ObservableCatalog,
  type ObservablesDelta,
  type Observed,
  type LegacyMigration,
  type ContextSettings,
  type ContextStatus,
  type PluginInfo,
  type PluginLog,
  type PluginsOverview,
  type Preferences,
  type RuntimeEvent,
  type RuntimeState,
  type ScanReport,
  type Snapshot,
  type Validation,
  type Workspace,
} from "./schema";

export const inTauri = (): boolean => typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

type Invoke = (cmd: string, args?: Record<string, unknown>) => Promise<unknown>;
type Listen = (event: string, handler: (payload: unknown) => void) => Promise<() => void>;

interface Transport {
  invoke: Invoke;
  listen: Listen;
  /** True when no Rust runtime is behind this transport. */
  preview: boolean;
}

let transport: Promise<Transport> | null = null;

function getTransport(): Promise<Transport> {
  transport ??= (async (): Promise<Transport> => {
    if (inTauri()) {
      const [{ invoke }, { listen }] = await Promise.all([import("@tauri-apps/api/core"), import("@tauri-apps/api/event")]);
      return {
        invoke: (cmd, args) => invoke(cmd, args),
        listen: (event, handler) => listen(event, (e) => handler(e.payload)),
        preview: false,
      };
    }
    // Plain browser (npm run dev, screenshots): a tiny in-memory stand-in.
    const { createPreviewTransport } = await import("./preview");
    return { ...(await createPreviewTransport()), preview: true };
  })();
  return transport;
}

export const isPreview = async (): Promise<boolean> => (await getTransport()).preview;

async function call<T>(cmd: string, schema: z.ZodType<T> | null, args?: Record<string, unknown>): Promise<T> {
  const t = await getTransport();
  const raw = await t.invoke(cmd, args);
  if (!schema) return raw as T;
  const parsed = schema.safeParse(raw);
  if (!parsed.success) throw new Error(`Unexpected reply from ${cmd}: ${parsed.error.message}`);
  return parsed.data;
}

const msg = (e: unknown): string => (typeof e === "string" ? e : e instanceof Error ? e.message : JSON.stringify(e));
export const errorMessage = msg;

/** Which track an Agent Control command is aimed at. */
export type AgentTarget = { kind: "focus" } | { kind: "session"; id: string } | { kind: "agent"; id: string };

/** A command with its payload, as Rust expects it. */
export type AgentCommand =
  | { kind: "approve" | "reject" | "pause" | "resume" | "cancel" | "retry" }
  | { kind: "send_instruction"; text: string }
  | { kind: "spawn"; task: string; cwd?: string | null };

export interface AgentsSettingsPatch {
  notifications?: boolean;
  hooksEnabled?: boolean;
  hooksPort?: number;
  hooksHarnesses?: Record<string, boolean>;
  hooksHoldSecs?: number;
}

export const api = {
  snapshot: (): Promise<Snapshot> => call("get_snapshot", snapshotSchema),
  virtualFrame: (): Promise<Frame> => call("virtual_frame", frameSchema),
  setMode: (mode: Mode): Promise<void> => call("set_mode", null, { mode }),
  activateSurface: (surfaceId: string, pageId?: string): Promise<void> =>
    call("activate_surface", null, { surfaceId, pageId: pageId ?? null }),
  activatePage: (pageId: string): Promise<void> => call("activate_page", null, { pageId }),
  virtualInput: (pos: ControlPos, pressed: boolean): Promise<void> => call("virtual_input", null, { pos, pressed }),
  updateControl: (surfaceId: string, pageId: string, control: Control): Promise<void> =>
    call("update_control", null, { surfaceId, pageId, control }),
  removeControl: (surfaceId: string, pageId: string, controlId: string): Promise<void> =>
    call("remove_control", null, { surfaceId, pageId, controlId }),
  createSurface: (name: string): Promise<string> => call("create_surface", z.string(), { name }),
  addPage: (surfaceId: string, name: string): Promise<string> => call("add_page", z.string(), { surfaceId, name }),
  /**
   * Rust refuses the last surface. Auto Context rules that show it are removed and a fallback that shows it goes back
   * to keeping what is on screen. The new workspace arrives as an event, like every edit below.
   */
  removeSurface: (surfaceId: string): Promise<void> => call("remove_surface", null, { surfaceId }),
  /** Name 1-40 characters, description up to 400, icon up to 8 (empty or `null`: no icon); Rust says what is wrong. */
  updateSurface: (surfaceId: string, name: string, description: string, icon: string | null): Promise<void> =>
    call("update_surface", null, { surfaceId, name, description, icon }),
  renamePage: (surfaceId: string, pageId: string, name: string): Promise<void> => call("rename_page", null, { surfaceId, pageId, name }),
  /** Rust refuses a surface's last page. */
  removePage: (surfaceId: string, pageId: string): Promise<void> => call("remove_page", null, { surfaceId, pageId }),
  resetWorkspace: (): Promise<void> => call("reset_workspace", null),
  updatePreferences: (preferences: Preferences): Promise<void> => call("update_preferences", null, { preferences }),
  /** Only the onboarding preference changes, saved before it is made; a reset keeps it. */
  setOnboardingDone: (done: boolean): Promise<void> => call("set_onboarding_done", null, { done }),
  /** Runs an assignment now, as if a pad had triggered it, and resolves with what it returned or rejects with why it failed. */
  runAction: (action: Assignment): Promise<unknown> => call("run_action", null, { action }),
  /** The Action Library: everything on offer, with availability evaluated now. */
  actions: {
    catalog: (): Promise<Catalog> => call("actions_catalog", catalogSchema),
    /** Checks an assignment the way saving does, without saving. */
    validate: (assignment: Assignment): Promise<Validation> => call("actions_validate", validationSchema, { assignment }),
  },
  /**
   * The Observable Catalog: every state and event a control can observe, from
   * every provider, with what each reads now. Rust assembles it; updates arrive
   * through `subscribe` as `onObservables`.
   */
  observables: {
    catalog: (): Promise<ObservableCatalog> => call("observables_catalog", observableCatalogSchema),
    /** What one state reads at one address, checked by Rust against the state's definition. */
    read: (provider: string, id: string, args?: Record<string, string | number | boolean>): Promise<Observed> =>
      call("observable_state", observedSchema, { provider, id, args: args ?? null }),
  },
  /** Why a control shows the look it shows, as the runtime's own evaluator reports it. */
  explainControl: (surfaceId: string, pageId: string, controlId: string): Promise<ControlExplanation> =>
    call("explain_control", controlExplanationSchema, { surfaceId, pageId, controlId }),
  /**
   * A draft of a control's feedback, judged by Rust without saving it: what saving would
   * refuse, what Rust can prove about each rule, and what the draft would show right now.
   */
  reviewFeedback: (surfaceId: string, pageId: string, control: Control): Promise<FeedbackReview> =>
    call("review_feedback", feedbackReviewSchema, { surfaceId, pageId, control }),
  connectDevice: (id: string): Promise<void> => call("connect_device", null, { id }),
  disconnectDevice: (id: string): Promise<void> => call("disconnect_device", null, { id }),
  rescanDevices: (): Promise<void> => call("rescan_devices", null),
  sendEvent: (source: string, event: string, payload?: unknown): Promise<void> =>
    call("send_event", null, { source, event, payload: payload ?? null }),
  /** External plugins (WebAssembly) installed from disk. */
  plugins: {
    list: (): Promise<PluginsOverview> => call("plugins_list", pluginsOverviewSchema),
    setEnabled: (id: string, enabled: boolean): Promise<PluginInfo> => call("plugins_set_enabled", pluginInfoSchema, { id, enabled }),
    /** `null` clears the value. Restarts the plugin so it reads the new setting. */
    setConfig: (id: string, key: string, value: string | number | boolean | null): Promise<PluginInfo> =>
      call("plugins_set_config", pluginInfoSchema, { id, key, value }),
    reload: (id: string): Promise<PluginInfo> => call("plugins_reload", pluginInfoSchema, { id }),
    rescan: (): Promise<ScanReport> => call("plugins_rescan", scanReportSchema),
    logs: (id: string): Promise<PluginLog[]> => call("plugins_logs", z.array(pluginLogSchema), { id }),
    /** Allow or deny one destination a plugin asks for; `null` takes the decision back. */
    setPermission: (id: string, origin: string, decision: "granted" | "denied" | null): Promise<PluginInfo> =>
      call("plugins_set_permission", pluginInfoSchema, { id, origin, decision }),
    /** The value goes to the credential store and is never sent back. */
    setSecret: (id: string, key: string, value: string): Promise<PluginInfo> => call("plugins_set_secret", pluginInfoSchema, { id, key, value }),
    clearSecret: (id: string, key: string): Promise<PluginInfo> => call("plugins_clear_secret", pluginInfoSchema, { id, key }),
    /** Move the secrets an earlier version kept in plain text into the credential store. */
    migrateLegacySecrets: (): Promise<LegacyMigration> => call("plugins_migrate_legacy_secrets", legacyMigrationSchema),
    /** Delete them without moving them. Resolves with how many there were. */
    discardLegacySecrets: (): Promise<number> => call("plugins_discard_legacy_secrets", z.number()),
    /** Move one plugin's plain-text secrets into the credential store. Rejects, with the reason, leaving them untouched, if any step fails. */
    migratePluginLegacySecrets: (id: string): Promise<PluginInfo> => call("plugins_migrate_plugin_legacy_secrets", pluginInfoSchema, { id }),
    /** Delete one plugin's plain-text secrets without moving them. */
    removePluginLegacySecrets: (id: string): Promise<PluginInfo> => call("plugins_remove_plugin_legacy_secrets", pluginInfoSchema, { id }),
    /** The example plugins the app ships (Hello, Counter, Home Assistant), and which are installed. */
    examples: (): Promise<ExampleInfo[]> => call("plugins_examples", z.array(exampleInfoSchema)),
    /** Install one into the plugins folder and turn it on; nothing is granted. Resolves with the new overview. */
    installExample: (id: string): Promise<PluginsOverview> => call("plugins_install_example", pluginsOverviewSchema, { id }),
    /** Show the plugins folder in the system's file manager, creating it first. */
    revealFolder: (): Promise<void> => call("plugins_reveal_folder", null),
  },
  /** Show the app data folder (the workspace, its backups, plugins, settings) in the system's file manager. */
  revealDataFolder: (): Promise<void> => call("app_reveal_data_folder", null),
  /** Auto Context: the surface follows the application in front. */
  context: {
    status: (): Promise<ContextStatus> => call("context_status", contextStatusSchema),
    /** Auto on or off, the rules in order, the fallback. Rust checks them and says which rule is wrong. */
    update: (settings: ContextSettings): Promise<ContextStatus> => call("context_update", contextStatusSchema, { settings }),
    /** End a manual override: the current application's rule applies at once. */
    resume: (): Promise<ContextStatus> => call("context_resume", contextStatusSchema),
  },
  agents: {
    /** Resolves with the request id; the outcome arrives in the next snapshot. */
    command: (target: AgentTarget, command: AgentCommand): Promise<string> => call("agents_command", z.string(), { target, command }),
    focus: (slot: number | null): Promise<void> => call("agents_focus", null, { slot }),
    dismiss: (sessionId: string | null): Promise<number> => call("agents_dismiss", z.number(), { sessionId }),
    setAccess: (agentId: string, access: Access): Promise<void> => call("agents_set_access", null, { agentId, access }),
    runDemo: (): Promise<void> => call("agents_demo", null, { run: true }),
    resetDemo: (): Promise<void> => call("agents_demo", null, { run: false }),
    updateSettings: (patch: AgentsSettingsPatch): Promise<void> => call("agents_update_settings", null, { patch }),
    hookSetup: (): Promise<HookSetup> => call("agents_hook_setup", hookSetupSchema),
    rotateHookToken: (): Promise<void> => call("agents_rotate_hook_token", null),
  },
};

export interface Listeners {
  onState: (s: RuntimeState) => void;
  onWorkspace: (w: Workspace) => void;
  onEvent: (e: RuntimeEvent) => void;
  onFrame: (f: Frame) => void;
  onAgents: (a: AgentsSnapshot) => void;
  onNotification: (n: AgentNotification) => void;
  /** Only the providers and observables whose value or availability changed; `catalog: true` asks for a full fetch. */
  onObservables: (d: ObservablesDelta) => void;
  onContext: (c: ContextStatus) => void;
}

/** Subscribe to everything Rust pushes. Invalid payloads are dropped, not trusted. */
export async function subscribe(l: Listeners): Promise<() => void> {
  const t = await getTransport();
  const wrap =
    <T>(schema: z.ZodType<T>, handler: (v: T) => void) =>
    (raw: unknown) => {
      const parsed = schema.safeParse(raw);
      if (parsed.success) handler(parsed.data);
      else console.warn("dropped malformed runtime payload", parsed.error.message);
    };
  const offs = await Promise.all([
    t.listen("runtime-state", wrap(runtimeStateSchema, l.onState)),
    t.listen("workspace-changed", wrap(workspaceSchema, l.onWorkspace)),
    t.listen("runtime-event", wrap(runtimeEventSchema, l.onEvent)),
    t.listen("virtual-frame", wrap(frameSchema, l.onFrame)),
    t.listen("agents-changed", wrap(agentsSnapshotSchema, l.onAgents)),
    t.listen("agent-notification", wrap(notificationSchema, l.onNotification)),
    t.listen("observables-changed", wrap(observablesDeltaSchema, l.onObservables)),
    t.listen("context-changed", wrap(contextStatusSchema, l.onContext)),
  ]);
  return () => offs.forEach((off) => off());
}
