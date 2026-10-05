// Browser-only stand-in for the Rust runtime. It exists so the UI can be
// developed and screenshotted with `npm run dev` without Tauri. It is not
// the product: no devices, no plugins running, no persistence. The real
// behaviour lives in src-tauri and is covered by Rust tests.

import { agentsSnapshotSchema, type AgentsSnapshot } from "./agents";
import { indexCatalog, normalizeArgs } from "./actions";
import { catalogSchema, contextSettingsSchema, definitionId, feedbackReviewSchema, observableCatalogSchema, posKey, snapshotSchema, workspaceSchema } from "./schema";
import type { Assignment, ContextSettings, ContextStatus, Control, ControlPos, Frame, Issue, Mode, PluginManifest, RuntimeEvent, RuntimeState, Snapshot, Workspace } from "./schema";

type Handler = (payload: unknown) => void;

const PLUGINS_IN_PREVIEW = "Browser preview: external plugins run in the desktop app.";
const AGENTS_IN_PREVIEW = "Browser preview: agents are a still picture here. Run the desktop app to command them.";
const OBSERVABLES_IN_PREVIEW = "Browser preview: states and explanations come from the desktop app's runtime.";

/** The captured snapshot has timestamps from when it was recorded; move them to now so elapsed times look right. */
function rebase(a: AgentsSnapshot, now: number): AgentsSnapshot {
  const stamps = [...a.sessions.map((s) => s.updatedAt), ...a.agents.map((x) => x.lastSeen), ...a.audit.map((e) => e.ts)];
  const delta = now - Math.max(0, ...stamps);
  const mv = (t: number): number => t + delta;
  return {
    ...a,
    agents: a.agents.map((x) => ({ ...x, registeredAt: mv(x.registeredAt), lastSeen: mv(x.lastSeen) })),
    sessions: a.sessions.map((s) => ({
      ...s,
      startedAt: s.startedAt == null ? s.startedAt : mv(s.startedAt),
      updatedAt: mv(s.updatedAt),
      pendingApproval: s.pendingApproval ? { ...s.pendingApproval, createdAt: mv(s.pendingApproval.createdAt) } : s.pendingApproval,
      lastCommand: s.lastCommand ? { ...s.lastCommand, at: mv(s.lastCommand.at) } : s.lastCommand,
    })),
    adapters: a.adapters.map((x) => ({ ...x, diagnostics: x.diagnostics.map((d) => ({ ...d, ts: mv(d.ts) })) })),
    audit: a.audit.map((e) => ({ ...e, ts: mv(e.ts) })),
  };
}

export async function createPreviewTransport() {
  const fixture = (await import("../../examples/default-workspace.json")).default;
  const workspace: Workspace = workspaceSchema.parse(fixture);
  // Captured from the Rust runtime (examples/agents-snapshot.json), so the preview shows real shapes.
  let agents: AgentsSnapshot = rebase(agentsSnapshotSchema.parse((await import("../../examples/agents-snapshot.json")).default), Date.now());
  // Captured from the Rust runtime too (examples/action-catalog.json): the built-in, Agent Control
  // and example plugin actions, with one plugin off and one broken so the unavailable states show.
  const catalog = catalogSchema.parse((await import("../../examples/action-catalog.json")).default);
  const index = indexCatalog(catalog);
  // And the Observable Catalog (examples/observables.json): every state and event with what it read when captured.
  // A still picture: nothing here changes, because no provider runs in the browser.
  const observables = observableCatalogSchema.parse((await import("../../examples/observables.json")).default.catalog);
  const review = feedbackReviewSchema.parse((await import("../../examples/observables.json")).default.review.clean);
  const handlers = new Map<string, Set<Handler>>();
  const emit = (event: string, payload: unknown) => handlers.get(event)?.forEach((h) => h(payload));

  const first = workspace.surfaces[0]!;
  const state: RuntimeState = {
    activeSurfaceId: first.id,
    activePageId: first.pages[0]!.id,
    mode: "live",
    controlStates: {},
    pending: [],
    devices: [
      {
        identity: { id: "virtual-launchpad-x", name: "Virtual Launchpad X", model: "Launchpad X (virtual)", kind: "virtual" },
        capabilities: {
          grid: { width: 8, height: 8 },
          aux: { top: 8, side: 8, logo: true },
          rgb: true,
          velocity: true,
          pressure: false,
          animations: { pulse: true, flash: true },
        },
        connected: true,
        status: "connected",
      },
    ],
    external: { enabled: true, port: 47821, listening: false, error: "Browser preview: no runtime" },
  };
  const events: RuntimeEvent[] = [];

  const page = () => workspace.surfaces.find((s) => s.id === state.activeSurfaceId)!.pages.find((p) => p.id === state.activePageId)!;
  const publish = (type: string, payload: unknown = {}, source = "runtime") => {
    const e: RuntimeEvent = { source, type, payload, ts: Date.now() };
    events.push(e);
    if (events.length > 100) events.shift();
    emit("runtime-event", e);
  };
  // The real runtime drives these pads from the states Agent Control keeps. Here the captured snapshot
  // stands in so the preview shows the same picture (still, not live).
  const COMMAND_PADS: Record<string, string> = { approve: "g.0.2", reject: "g.2.2", pause: "g.4.2", resume: "g.5.2", retry: "g.6.2", cancel: "g.7.2", send_instruction: "g.4.3" };
  const agentPadStates = (): Record<string, string> => {
    const out: Record<string, string> = {};
    const key = (pad: string) => `agent-control/home/${pad}`;
    for (const slot of agents.slots) {
      out[key(`g.${slot.index}.0`)] = slot.status ?? "empty";
      out[key(`g.${slot.index}.1`)] = slot.focused ? "focused" : "idle";
    }
    const focused = agents.sessions.find((s) => s.slot === agents.focus);
    for (const [kind, pad] of Object.entries(COMMAND_PADS)) {
      out[key(pad)] = focused?.commands.find((c) => c.kind === kind)?.state ?? "unavailable";
    }
    return out;
  };
  const frame = (): Frame => ({
    leds: page().controls.map((c) => {
      const key = `${state.activeSurfaceId}/${state.activePageId}/${c.id}`;
      const v = c.states[agentPadStates()[key] ?? state.controlStates[key] ?? ""] ?? c.visual;
      return { pos: c.pos, led: { color: v.color, animation: v.animation } };
    }),
  });
  const push = () => {
    emit("runtime-state", state);
    emit("virtual-frame", frame());
  };
  const pushWorkspace = () => {
    emit("workspace-changed", workspace);
    push();
  };

  const setState = (key: string, value: string | null, resetMs?: number) => {
    if (value === null) delete state.controlStates[key];
    else state.controlStates[key] = value;
    push();
    if (resetMs) setTimeout(() => setState(key, null), resetMs);
  };

  const press = (pos: ControlPos, down: boolean) => {
    const control = page().controls.find((c) => posKey(c.pos) === posKey(pos));
    if (!control || !down) return;
    publish("control.pressed", { controlId: control.id, label: control.label, surfaceId: state.activeSurfaceId, pageId: state.activePageId }, "device");
    if (state.mode === "edit" || !control.press) return;
    const key = `${state.activeSurfaceId}/${state.activePageId}/${control.id}`;
    const a = control.press;
    const id = definitionId(a);
    if (a.provider === "navigation" && a.action === "openSurface") return void activate(String(a.args.surfaceId), (a.args.pageId as string | undefined) ?? undefined);
    if (a.provider === "navigation" && a.action === "openPage") return void activatePage(String(a.args.pageId));
    publish("action.started", { action: id, controlId: control.id });
    if (control.states.running) setState(key, "running");
    setTimeout(() => {
      publish("action.completed", { action: id, controlId: control.id });
      if (control.states.success) setState(key, "success", 1500);
      else setState(key, null);
    }, 700);
  };

  const activate = (surfaceId: string, pageId?: string) => {
    const s = workspace.surfaces.find((x) => x.id === surfaceId);
    if (!s) throw `unknown surface \`${surfaceId}\``;
    state.activeSurfaceId = s.id;
    state.activePageId = pageId ?? s.pages[0]!.id;
    publish("surface.changed", { surfaceId, pageId: state.activePageId });
    push();
  };
  const activatePage = (pageId: string) => {
    state.activePageId = pageId;
    publish("page.changed", { surfaceId: state.activeSurfaceId, pageId });
    push();
  };

  // The same refusals as the runtime, in the same words.
  const surfaceOf = (id: unknown) => {
    const s = workspace.surfaces.find((x) => x.id === id);
    if (!s) throw "That surface no longer exists.";
    return s;
  };
  const pageOf = (s: Workspace["surfaces"][number], id: unknown) => {
    const p = s.pages.find((x) => x.id === id);
    if (!p) throw "That page no longer exists.";
    return p;
  };
  const checkedName = (raw: unknown, what: string): string => {
    const name = String(raw).trim();
    if (name.length === 0 || [...name].length > 40) throw `A ${what} name needs 1 to 40 characters.`;
    return name;
  };

  /** The Plugins screen lists built-in providers from their catalog entries. */
  const bundledManifests = (): PluginManifest[] =>
    catalog.providers
      .filter((p) => p.kind !== "plugin")
      .map((p) => ({
        id: p.id,
        name: p.name,
        version: p.version,
        description: p.description,
        kind: p.kind,
        actions: catalog.actions
          .filter((a) => a.provider === p.id)
          .map((a) => ({ id: a.action, label: a.name, description: a.description, category: a.category, keywords: a.keywords, params: a.params, privileged: a.privileged, appearance: a.appearance, suggests: a.suggests })),
        states: [],
        permissions: [],
      }));

  /** What Rust says about an assignment before saving it, from the same parameter rules. */
  const check = (a: Assignment): { assignment?: Assignment; issues: Issue[] } => {
    const def = index.byId.get(definitionId(a));
    if (!def) return { issues: [{ message: `\`${definitionId(a)}\` is not an action LaunchDeck knows` }] };
    const { args, issues } = normalizeArgs(def.params, a.args);
    return issues.length > 0 ? { issues } : { assignment: { ...a, args }, issues: [] };
  };

  // Nothing observes the foreground in a browser tab: the settings can be
  // edited and are forgotten, and the status says why nothing follows them.
  let contextSettings: ContextSettings = { auto: false, rules: [], fallback: { kind: "keep" } };
  const context = (): ContextStatus => ({
    settings: contextSettings,
    observer: { state: "unavailable", reason: "Browser preview: the desktop app is what sees which application is in front." },
    mode: contextSettings.auto ? "unavailable" : "off",
    foreground: { kind: "unknown" },
    recent: [],
    problems: contextSettings.rules.map(() => null),
  });

  const snapshot = (): Snapshot =>
    snapshotSchema.parse({
      workspace,
      state,
      plugins: bundledManifests(),
      catalog,
      observables,
      recentEvents: events,
      agents,
      context: context(),
    });

  const invoke = async (cmd: string, args: Record<string, unknown> = {}): Promise<unknown> => {
    switch (cmd) {
      case "get_snapshot":
        return snapshot();
      case "context_status":
      case "context_resume":
        return context();
      case "context_update":
        contextSettings = contextSettingsSchema.parse(args.settings);
        emit("context-changed", context());
        return context();
      case "virtual_frame":
        return frame();
      case "set_mode":
        state.mode = args.mode as Mode;
        return push();
      case "activate_surface":
        return activate(args.surfaceId as string, (args.pageId as string | null) ?? undefined);
      case "activate_page":
        return activatePage(args.pageId as string);
      case "virtual_input":
        return press(args.pos as ControlPos, args.pressed as boolean);
      case "update_control": {
        const s = workspace.surfaces.find((x) => x.id === args.surfaceId);
        const p = s?.pages.find((x) => x.id === args.pageId);
        if (!p) throw "unknown page";
        const c = args.control as Control;
        const i = p.controls.findIndex((x) => x.id === c.id);
        for (const slot of ["press", "longPress"] as const) {
          const next = c[slot];
          if (!next || JSON.stringify(next) === JSON.stringify(i >= 0 ? p.controls[i]![slot] : null)) continue;
          const { issues } = check(next);
          if (issues.length > 0) throw `${slot === "press" ? "Press" : "Long press"}: ${issues.map((x) => x.message).join("; ")}`;
        }
        if (i >= 0) p.controls[i] = c;
        else p.controls.push(c);
        return pushWorkspace();
      }
      case "remove_control": {
        const p = workspace.surfaces.find((x) => x.id === args.surfaceId)?.pages.find((x) => x.id === args.pageId);
        if (p) p.controls = p.controls.filter((c) => c.id !== args.controlId);
        return pushWorkspace();
      }
      case "create_surface": {
        const id = `surface-${workspace.surfaces.length + 1}`;
        workspace.surfaces.push({ id, name: String(args.name), description: "", category: "custom", pages: [{ id: "home", name: "Home", controls: [] }] });
        pushWorkspace();
        return id;
      }
      case "add_page": {
        const s = workspace.surfaces.find((x) => x.id === args.surfaceId);
        const id = `page-${(s?.pages.length ?? 0) + 1}`;
        s?.pages.push({ id, name: String(args.name), controls: [] });
        pushWorkspace();
        return id;
      }
      case "remove_surface": {
        const s = surfaceOf(args.surfaceId);
        if (workspace.surfaces.length === 1) throw `${s.name} is the only surface, so it cannot be deleted. Create another one first.`;
        workspace.surfaces = workspace.surfaces.filter((x) => x !== s);
        const fallback = contextSettings.fallback;
        contextSettings = {
          ...contextSettings,
          rules: contextSettings.rules.filter((r) => r.surfaceId !== s.id),
          fallback: fallback.kind === "surface" && fallback.surfaceId === s.id ? { kind: "keep" } : fallback,
        };
        if (state.activeSurfaceId === s.id) activate(workspace.surfaces[0]!.id);
        emit("context-changed", context());
        return pushWorkspace();
      }
      case "update_surface": {
        const s = surfaceOf(args.surfaceId);
        s.name = checkedName(args.name, "surface");
        s.description = String(args.description).trim();
        s.icon = String(args.icon ?? "").trim() || null;
        return pushWorkspace();
      }
      case "rename_page": {
        pageOf(surfaceOf(args.surfaceId), args.pageId).name = checkedName(args.name, "page");
        return pushWorkspace();
      }
      case "remove_page": {
        const s = surfaceOf(args.surfaceId);
        const p = pageOf(s, args.pageId);
        if (s.pages.length === 1) throw `${p.name} is the only page of ${s.name}, so it cannot be deleted.`;
        s.pages = s.pages.filter((x) => x !== p);
        if (state.activeSurfaceId === s.id && state.activePageId === p.id) state.activePageId = s.pages[0]!.id;
        return pushWorkspace();
      }
      case "update_preferences":
        workspace.preferences = args.preferences as Workspace["preferences"];
        return pushWorkspace();
      case "set_onboarding_done":
        workspace.preferences = { ...workspace.preferences, onboardingDone: args.done === true };
        return pushWorkspace();
      case "actions_catalog":
        return catalog;
      case "observables_catalog":
        return observables;
      case "observable_state":
      case "explain_control":
        throw OBSERVABLES_IN_PREVIEW;
      case "review_feedback":
        // Rust's own reply for a control with nothing to report: the limits are real, the rest is empty.
        // Nothing is judged in a browser, so a draft is never refused here either.
        return review;
      case "actions_validate": {
        const { assignment, issues } = check(args.assignment as Assignment);
        return { ok: issues.length === 0, assignment, issues };
      }
      case "run_action": {
        const a = args.action as Assignment;
        const { issues } = check(a);
        if (issues.length > 0) throw issues.map((x) => x.message).join("; ");
        const def = index.byId.get(definitionId(a))!;
        if (def.availability.status === "unavailable") throw def.availability.reason;
        publish("action.started", { action: def.id });
        await new Promise((r) => setTimeout(r, 450));
        publish("action.completed", { action: def.id });
        return null;
      }
      case "connect_device":
      case "disconnect_device":
      case "rescan_devices":
      case "reset_workspace":
        return undefined;
      case "send_event":
        return publish(String(args.event), args.payload ?? {}, String(args.source));
      case "agents_focus": {
        // Selection is the one thing that is harmless to show without a runtime.
        const slot = (args.slot as number | null) ?? null;
        agents = {
          ...agents,
          rev: agents.rev + 1,
          focus: slot,
          slots: agents.slots.map((x) => ({ ...x, focused: x.index === slot })),
        };
        return void emit("agents-changed", agents);
      }
      case "agents_dismiss":
        return 0;
      case "agents_demo":
        return undefined;
      case "plugins_list":
        return {
          plugins: [],
          issues: [],
          folders: [],
          secretStore: { kind: "memory", available: true, name: "memory" },
          legacySecrets: { entries: [], unreadable: false },
        };
      case "plugins_logs":
      case "plugins_examples":
        return [];
      case "plugins_set_enabled":
      case "plugins_set_config":
      case "plugins_reload":
      case "plugins_rescan":
      case "plugins_set_permission":
      case "plugins_set_secret":
      case "plugins_clear_secret":
      case "plugins_migrate_legacy_secrets":
      case "plugins_discard_legacy_secrets":
      case "plugins_migrate_plugin_legacy_secrets":
      case "plugins_remove_plugin_legacy_secrets":
      case "plugins_install_example":
      case "plugins_reveal_folder":
        throw PLUGINS_IN_PREVIEW;
      case "agents_command":
      case "agents_set_access":
      case "agents_update_settings":
      case "agents_hook_setup":
      case "agents_rotate_hook_token":
        throw AGENTS_IN_PREVIEW;
      default:
        throw `preview: unsupported command ${cmd}`;
    }
  };

  const listen = async (event: string, handler: Handler) => {
    const set = handlers.get(event) ?? new Set<Handler>();
    set.add(handler);
    handlers.set(event, set);
    return () => void set.delete(handler);
  };

  return { invoke, listen };
}
