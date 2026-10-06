// UI state only. The runtime in Rust is the source of truth for the workspace,
// the active surface and page, the mode and every control state; this store
// mirrors what Rust pushes and keeps purely visual choices (view, selection).

import { create } from "zustand";
import { api, isPreview, subscribe, toProblem, type Problem } from "../lib/ipc";
import { emptyAgents, newer, type AgentNotification, type AgentsSnapshot } from "../lib/agents";
import { emptyIndex, indexCatalog, type CatalogIndex } from "../lib/actions";
import type { Slot } from "../lib/assign";
import { primaryDevice } from "../lib/devices";
import { applyDelta } from "../lib/observables";
import type { Catalog, ContextStatus, Control, ControlPos, DeviceInfo, Frame, ObservableCatalog, ObservablesDelta, PluginManifest, RuntimeEvent, RuntimeState, Snapshot, StartupNotice, Surface, Workspace, Page } from "../lib/schema";
import { emptyObservableCatalog, posKey } from "../lib/schema";

export type View = "overview" | "agents" | "surface" | "plugins" | "settings" | "import" | "export";
export type AgentsTab = "overview" | "sessions" | "adapters" | "surface";

const MAX_EVENTS = 120;

export interface Toast {
  id: number;
  tone: "info" | "error";
  message: string;
  /** What Rust kept for a bug report; shown under Details. An error toast with one stays until dismissed. */
  detail?: string;
}

/** A request to put an action on a pad: made in the Library or by a drop, carried out by the Inspector's draft. */
export interface AssignIntent {
  seq: number;
  pos: ControlPos;
  slot: Slot;
  definitionId: string;
}

interface Store {
  ready: boolean;
  preview: boolean;
  /** Why the runtime could not start, as a sentence and a detail. */
  fatal: Problem | null;
  workspace: Workspace | null;
  state: RuntimeState | null;
  plugins: PluginManifest[];
  /** Destinations that enabled plugins ask for and nobody has decided about yet. */
  permissionRequests: number;
  /** The Action Library's catalog as Rust evaluated it, indexed for local search. */
  catalog: CatalogIndex;
  /**
   * Every state and event a control can observe, with what each reads now.
   * Kept current by the changes Rust pushes; never computed here.
   */
  observables: ObservableCatalog;
  events: RuntimeEvent[];
  frame: Frame;
  /** Agent Control as Rust derived it. Never edited here. */
  agents: AgentsSnapshot;
  /** Auto Context as Rust reports it: settings, the application in front, what was decided. */
  context: ContextStatus | null;
  /** What Rust had to tell about this start (a saved workspace set aside). Fixed until the next start. */
  notices: StartupNotice[];

  view: View;
  agentsTab: AgentsTab;
  selected: ControlPos | null;
  showLabels: boolean;
  toasts: Toast[];

  /** The surfaces the export view opens with; `seq` changes each time it is opened. */
  exportStart: { seq: number; surfaceIds: string[] };

  libraryOpen: boolean;
  libraryQuery: string;
  libraryCategory: string | null;
  /** Counts up when something asks the Library to take keyboard focus. */
  libraryFocus: number;
  intent: AssignIntent | null;
  /** An action row is being dragged: pads show where it can land. */
  dragging: boolean;

  init: () => Promise<() => void>;
  setView: (v: View) => void;
  setAgentsTab: (t: AgentsTab) => void;
  /** Open the Agents section on a tab (used by notifications and pads). */
  showAgents: (tab?: AgentsTab) => void;
  /** Open the export view, with this surface chosen when one is given. */
  startExport: (surfaceId?: string) => void;
  select: (pos: ControlPos | null) => void;
  toggleLabels: () => void;
  toast: (message: string, tone?: Toast["tone"], detail?: string) => void;
  /** A failure as an error toast: its sentence, and its detail under Details. */
  toastProblem: (e: unknown) => void;
  dismissToast: (id: number) => void;
  /** Runs an IPC call and turns a failure into a toast. Returns true on success. */
  attempt: (fn: () => Promise<unknown>) => Promise<boolean>;
  notify: (n: AgentNotification) => void;
  /** Re-read the plugin list (status changes after enable, disable, reload, rescan). Also refreshes the catalog. */
  refreshPlugins: () => Promise<void>;
  refreshCatalog: () => Promise<void>;
  /** Take in what Rust pushed about the Observable Catalog: patch it, or fetch it again when definitions changed. */
  applyObservables: (delta: ObservablesDelta) => void;
  /** Fetch the Observable Catalog again. Runs once at a time; a request made meanwhile is covered by one more fetch. */
  refreshObservables: () => Promise<void>;
  setCatalog: (c: Catalog) => void;
  setLibraryOpen: (open: boolean) => void;
  setLibraryQuery: (q: string) => void;
  setLibraryCategory: (c: string | null) => void;
  /** Open the Library and put the cursor in its search box. */
  focusLibrary: () => void;
  /** Put an action on a pad (the selected one when `pos` is omitted). The Inspector carries it out in its draft. */
  requestAssign: (req: { definitionId: string; slot: Slot; pos?: ControlPos | null }) => boolean;
  consumeIntent: (seq: number) => void;
  setDragging: (on: boolean) => void;
}

let toastId = 0;
let intentSeq = 0;
let catalogTimer: ReturnType<typeof setTimeout> | undefined;
let observablesFetching = false;
let observablesAskedAgain = false;
/** A change arrived before the first snapshot did: the snapshot may be older than it. */
let observablesHeardEarly = false;

/** Best effort: web notifications may be unavailable or denied. Never blocks. */
function showSystemNotification(n: AgentNotification): void {
  try {
    if (typeof document === "undefined" || !document.hidden || typeof Notification === "undefined") return;
    const show = () => new Notification(n.title, { body: n.body, tag: `${n.kind}:${n.sessionId ?? n.agentId ?? ""}` });
    if (Notification.permission === "granted") show();
    else if (Notification.permission === "default") void Notification.requestPermission().then((p) => p === "granted" && show());
  } catch {
    /* no system notifications here; the in-app toast already told the user */
  }
}

/** How many destinations enabled plugins are waiting for a decision on. Never throws: the count is a hint. */
async function countPermissionRequests(): Promise<number> {
  try {
    const overview = await api.plugins.list();
    return overview.plugins
      .filter((p) => p.enabled)
      .reduce((n, p) => n + (p.network?.hosts.filter((h) => h.status === "needsApproval").length ?? 0), 0);
  } catch {
    return 0;
  }
}

export const useStore = create<Store>((set, get) => ({
  ready: false,
  preview: false,
  fatal: null,
  workspace: null,
  state: null,
  plugins: [],
  permissionRequests: 0,
  catalog: emptyIndex,
  observables: emptyObservableCatalog,
  events: [],
  frame: { leds: [] },
  agents: emptyAgents,
  context: null,
  notices: [],

  view: "overview",
  agentsTab: "overview",
  selected: null,
  showLabels: true,
  toasts: [],
  exportStart: { seq: 0, surfaceIds: [] },

  libraryOpen: true,
  libraryQuery: "",
  libraryCategory: null,
  libraryFocus: 0,
  intent: null,
  dragging: false,

  init: async () => {
    try {
      const preview = await isPreview();
      const off = await subscribe({
        onState: (state) => {
          const wasEditing = get().state?.mode === "edit";
          set({ state });
          // What is available can differ by the time someone starts editing.
          if (state.mode === "edit" && !wasEditing) void get().refreshCatalog();
        },
        onWorkspace: (workspace) => set({ workspace }),
        onAgents: (incoming) => {
          set((s) => ({ agents: newer(s.agents, incoming) }));
          // Agent Control actions are available or not depending on what the agents are doing.
          // Only the Library shows that, and only while editing; a burst of updates is one refresh.
          if (get().state?.mode === "edit") {
            clearTimeout(catalogTimer);
            catalogTimer = setTimeout(() => void get().refreshCatalog(), 400);
          }
        },
        onNotification: (n) => get().notify(n),
        onEvent: (e) => {
          set((s) => ({ events: [...s.events, e].slice(-MAX_EVENTS) }));
          // A pad (or anything else) asked the app to show the Agents section.
          if (e.type === "ui.navigate" && (e.payload as { view?: string } | undefined)?.view === "agents") get().showAgents();
          // A pad pressed on the device selects its control while editing.
          const { state } = get();
          if (e.type === "control.pressed" && state?.mode === "edit") {
            const p = (e.payload ?? {}) as { controlId?: string; surfaceId?: string; pageId?: string };
            const page = activePage(get());
            const c = page?.controls.find((x) => x.id === p.controlId);
            if (c && p.surfaceId === state.activeSurfaceId && p.pageId === state.activePageId) set({ selected: c.pos });
          }
        },
        onFrame: (frame) => set({ frame }),
        onObservables: (delta) => get().applyObservables(delta),
        onContext: (context) => set({ context }),
      });
      const snap: Snapshot = await api.snapshot();
      const frame = await api.virtualFrame();
      set({
        ready: true,
        preview,
        workspace: snap.workspace,
        state: snap.state,
        plugins: snap.plugins,
        catalog: indexCatalog(snap.catalog),
        observables: snap.observables,
        events: snap.recentEvents.slice(-MAX_EVENTS),
        frame,
        agents: snap.agents,
        context: snap.context,
        notices: snap.notices,
        view: snap.workspace.preferences.onboardingDone ? "surface" : "overview",
      });
      if (observablesHeardEarly) {
        observablesHeardEarly = false;
        void get().refreshObservables();
      }
      void countPermissionRequests().then((permissionRequests) => set({ permissionRequests }));
      return off;
    } catch (e) {
      set({ fatal: toProblem(e) });
      return () => undefined;
    }
  },

  setView: (view) => set({ view }),
  setAgentsTab: (agentsTab) => set({ agentsTab }),
  showAgents: (tab) => set((s) => ({ view: "agents", agentsTab: tab ?? s.agentsTab })),
  startExport: (surfaceId) => set((s) => ({ view: "export", exportStart: { seq: s.exportStart.seq + 1, surfaceIds: surfaceId ? [surfaceId] : [] } })),
  select: (selected) => set({ selected }),
  toggleLabels: () => set((s) => ({ showLabels: !s.showLabels })),

  toast: (message, tone = "info", detail) => {
    const id = ++toastId;
    const toast: Toast = detail === undefined ? { id, tone, message } : { id, tone, message, detail };
    set((s) => ({ toasts: [...s.toasts, toast].slice(-4) }));
    // An error with details may be read and copied into a bug report: it waits for the person.
    if (tone === "error" && detail !== undefined) return;
    setTimeout(() => get().dismissToast(id), tone === "error" ? 7000 : 3500);
  },
  toastProblem: (e) => {
    const { message, detail } = toProblem(e);
    get().toast(message, "error", detail);
  },
  dismissToast: (id) => set((s) => ({ toasts: s.toasts.filter((t) => t.id !== id) })),

  notify: (n) => {
    // Only the four situations Rust allows ever arrive here. In-app first; an
    // OS notification is a bonus when the window is in the background.
    get().toast(`${n.title}. ${n.body}`, n.kind === "agent_failed" ? "error" : "info");
    showSystemNotification(n);
  },

  attempt: async (fn) => {
    try {
      await fn();
      return true;
    } catch (e) {
      get().toastProblem(e);
      return false;
    }
  },

  refreshPlugins: async () => {
    try {
      const snap = await api.snapshot();
      set({ plugins: snap.plugins, catalog: indexCatalog(snap.catalog), observables: snap.observables, permissionRequests: await countPermissionRequests() });
    } catch (e) {
      get().toastProblem(e);
    }
  },

  refreshCatalog: async () => {
    try {
      get().setCatalog(await api.actions.catalog());
    } catch (e) {
      get().toastProblem(e);
    }
  },
  setCatalog: (c) => set({ catalog: indexCatalog(c) }),

  applyObservables: (delta) => {
    if (!get().ready) {
      observablesHeardEarly = true;
      return;
    }
    // A definition changed, or a fetch is under way whose answer this change may be newer than.
    if (delta.catalog || observablesFetching) return void get().refreshObservables();
    const next = applyDelta(get().observables, delta);
    if (next !== get().observables) set({ observables: next });
  },

  refreshObservables: async () => {
    if (observablesFetching) {
      observablesAskedAgain = true;
      return;
    }
    observablesFetching = true;
    try {
      do {
        observablesAskedAgain = false;
        set({ observables: await api.observables.catalog() });
      } while (observablesAskedAgain);
    } catch (e) {
      get().toastProblem(e);
    } finally {
      observablesFetching = false;
    }
  },

  setLibraryOpen: (libraryOpen) => set({ libraryOpen }),
  setLibraryQuery: (libraryQuery) => set({ libraryQuery }),
  setLibraryCategory: (libraryCategory) => set({ libraryCategory }),
  focusLibrary: () => set((s) => ({ libraryOpen: true, libraryFocus: s.libraryFocus + 1 })),

  requestAssign: ({ definitionId, slot, pos }) => {
    const target = pos ?? get().selected;
    if (!target) {
      get().toast("Select a pad on the surface first, then choose its action.", "info");
      return false;
    }
    set({ selected: target, intent: { seq: ++intentSeq, pos: target, slot, definitionId } });
    return true;
  },
  setDragging: (dragging) => set({ dragging }),
  consumeIntent: (seq) => set((s) => (s.intent?.seq === seq ? { intent: null } : s)),
}));

// ---- selectors (pure, usable from tests) ----------------------------------

type Core = Pick<Store, "workspace" | "state">;

export const activeSurface = (s: Core): Surface | null =>
  s.workspace?.surfaces.find((x) => x.id === s.state?.activeSurfaceId) ?? null;

export const activePage = (s: Core): Page | null => {
  const surface = activeSurface(s);
  return surface?.pages.find((p) => p.id === s.state?.activePageId) ?? surface?.pages[0] ?? null;
};

/** The device the app shows and lays pads out for (see `primaryDevice`). */
export const primaryDeviceOf = (s: Pick<Store, "state">): DeviceInfo | null => primaryDevice(s.state?.devices ?? []);

/** The control covering a position, honouring spans on the main grid. */
export function controlAt(page: Page | null, pos: ControlPos | null): Control | null {
  if (!page || !pos) return null;
  if (pos.kind === "grid") {
    return (
      page.controls.find(
        (c) => c.pos.kind === "grid" && pos.col >= c.pos.col && pos.col < c.pos.col + c.span.w && pos.row >= c.pos.row && pos.row < c.pos.row + c.span.h,
      ) ?? null
    );
  }
  return page.controls.find((c) => posKey(c.pos) === posKey(pos)) ?? null;
}

export const controlState = (s: Core, controlId: string): string | undefined => {
  const st = s.state;
  return st ? st.controlStates[`${st.activeSurfaceId}/${st.activePageId}/${controlId}`] : undefined;
};
