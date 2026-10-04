import fixture from "../../examples/default-workspace.json";
import { describe, expect, it } from "vitest";
import { api } from "../lib/ipc";
import { workspaceSchema, type RuntimeState } from "../lib/schema";
import { activePage, activeSurface, controlAt, controlState, useStore } from "./runtime";

const workspace = workspaceSchema.parse(fixture);

const state = (surface: string, page: string, controlStates: Record<string, string> = {}): RuntimeState => ({
  activeSurfaceId: surface,
  activePageId: page,
  mode: "live",
  controlStates,
  devices: [],
  external: { enabled: true, port: 47821, listening: true },
});

describe("store selectors", () => {
  it("resolves the active surface and page from runtime state", () => {
    const core = { workspace, state: state("development", "git") };
    expect(activeSurface(core)?.name).toBe("Development");
    expect(activePage(core)?.id).toBe("git");
  });

  it("falls back to the first page when the page id is stale", () => {
    const core = { workspace, state: state("development", "gone") };
    expect(activePage(core)?.id).toBe(workspace.surfaces.find((s) => s.id === "development")!.pages[0]!.id);
  });

  it("finds spanning controls from any covered cell, and nothing outside", () => {
    const page = activePage({ workspace, state: state("accessibility", "home") })!;
    const big = page.controls.find((c) => c.pos.kind === "grid" && c.span.w > 1)!;
    expect(big.pos.kind).toBe("grid");
    if (big.pos.kind !== "grid") return;
    const inside = { kind: "grid", col: big.pos.col + big.span.w - 1, row: big.pos.row + big.span.h - 1 } as const;
    expect(controlAt(page, inside)?.id).toBe(big.id);
    expect(controlAt(page, { kind: "grid", col: 7, row: 7 })?.id).not.toBe(big.id);
    expect(controlAt(page, null)).toBeNull();
  });

  it("reads control state by surface/page/control", () => {
    const core = { workspace, state: state("agent-control", "home", { "agent-control/home/g.0.0": "running" }) };
    expect(controlState(core, "g.0.0")).toBe("running");
    expect(controlState(core, "g.1.0")).toBeUndefined();
  });
});

// These run against the in-memory preview transport, which pushes the same
// events the Tauri side does. They check that the store only mirrors them.
describe("store and agents", () => {
  it("starts from the runtime snapshot and keeps agents separate from the workspace", async () => {
    const off = await useStore.getState().init();
    const { agents, ready } = useStore.getState();
    expect(ready).toBe(true);
    expect(agents.agents.length).toBeGreaterThan(0);
    off();
  });

  it("mirrors the snapshot Rust pushes without editing it", async () => {
    const off = await useStore.getState().init();
    const before = useStore.getState().agents;
    await api.agents.focus(3);
    const after = useStore.getState().agents;
    expect(after.rev).toBeGreaterThan(before.rev);
    expect(after.focus).toBe(3);
    // Everything else is exactly what was pushed: the store derives nothing.
    expect(after.sessions).toEqual(before.sessions);
    off();
  });

  it("opens the Agents section when a pad asks for it", async () => {
    const off = await useStore.getState().init();
    useStore.setState({ view: "surface", agentsTab: "overview" });
    await api.sendEvent("agent-control", "ui.navigate", { view: "agents" });
    expect(useStore.getState().view).toBe("agents");
    off();
  });

  it("keeps the selected agents tab when the person switches views", () => {
    useStore.getState().showAgents("adapters");
    useStore.getState().setView("surface");
    useStore.getState().showAgents();
    expect(useStore.getState().agentsTab).toBe("adapters");
    expect(useStore.getState().view).toBe("agents");
  });
});
