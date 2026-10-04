// UI state only. The runtime in Rust is the source of truth for the workspace,
// the active surface and page, the mode and every control state; this store
// mirrors what Rust pushes and keeps purely visual choices (view, selection).

import { create } from "zustand";
import { api, errorMessage, isPreview, subscribe } from "../lib/ipc";
import { emptyAgents, newer, type AgentNotification, type AgentsSnapshot } from "../lib/agents";
import { emptyIndex, indexCatalog, type CatalogIndex } from "../lib/actions";
import type { Slot } from "../lib/assign";
import type { Catalog, Control, ControlPos, Frame, PluginManifest, RuntimeEvent, RuntimeState, Snapshot, Surface, Workspace, Page } from "../lib/schema";
import { posKey } from "../lib/schema";

export type View = "home" | "agents" | "surface" | "plugins" | "settings";
export type AgentsTab = "overview" | "sessions" | "adapters" | "surface";

const MAX_EVENTS = 120;

interface Toast {
  id: number;
  tone: "info" | "error";
  text: string;
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
  fatal: string | null;
  workspace: Workspace | null;
  state: RuntimeState | null;
  plugins: PluginManifest[];
  /** The Action Library's catalog as Rust evaluated it, indexed for local search. */
  catalog: CatalogIndex;
  events: RuntimeEvent[];
  frame: Frame;
  /** Agent Control as Rust derived it. Never edited here. */
  agents: AgentsSnapshot;

  view: View;
  agentsTab: AgentsTab;
  selected: ControlPos | null;
  showLabels: boolean;
  toasts: Toast[];

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
  select: (pos: ControlPos | null) => void;
  toggleLabels: () => void;
  toast: (text: string, tone?: Toast["tone"]) => void;
  dismissToast: (id: number) => void;
  /** Runs an IPC call and turns a failure into a toast. Returns true on success. */
  attempt: (fn: () => Promise<unknown>) => Promise<boolean>;
  notify: (n: AgentNotification) => void;
  /** Re-read the plugin list (status changes after enable, disable, reload, rescan). Also refreshes the catalog. */
  refreshPlugins: () => Promise<void>;
  refreshCatalog: () => Promise<void>;
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

export const useStore = create<Store>((set, get) => ({
  ready: false,
  preview: false,
  fatal: null,
  workspace: null,
  state: null,
  plugins: [],
  catalog: emptyIndex,
  events: [],
  frame: { leds: [] },
  agents: emptyAgents,

  view: "home",
  agentsTab: "overview",
  selected: null,
  showLabels: true,
  toasts: [],

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
        events: snap.recentEvents.slice(-MAX_EVENTS),
        frame,
        agents: snap.agents,
        view: snap.workspace.preferences.onboardingDone ? "surface" : "home",
      });
      return off;
    } catch (e) {
      set({ fatal: errorMessage(e) });
      return () => undefined;
    }
  },

  setView: (view) => set({ view }),
  setAgentsTab: (agentsTab) => set({ agentsTab }),
  showAgents: (tab) => set((s) => ({ view: "agents", agentsTab: tab ?? s.agentsTab })),
  select: (selected) => set({ selected }),
  toggleLabels: () => set((s) => ({ showLabels: !s.showLabels })),

  toast: (text, tone = "info") => {
    const id = ++toastId;
    set((s) => ({ toasts: [...s.toasts, { id, tone, text }].slice(-4) }));
    setTimeout(() => get().dismissToast(id), tone === "error" ? 7000 : 3500);
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
      get().toast(errorMessage(e), "error");
      return false;
    }
  },

  refreshPlugins: async () => {
    try {
      const snap = await api.snapshot();
      set({ plugins: snap.plugins, catalog: indexCatalog(snap.catalog) });
    } catch (e) {
      get().toast(errorMessage(e), "error");
    }
  },

  refreshCatalog: async () => {
    try {
      get().setCatalog(await api.actions.catalog());
    } catch (e) {
      get().toast(errorMessage(e), "error");
    }
  },
  setCatalog: (c) => set({ catalog: indexCatalog(c) }),

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
