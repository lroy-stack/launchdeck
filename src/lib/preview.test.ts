import { describe, expect, it } from "vitest";
import { createPreviewTransport } from "./preview";
import { feedbackReviewSchema, frameSchema, observableCatalogSchema, snapshotSchema, type RuntimeState } from "./schema";
import { agentsSnapshotSchema } from "./agents";

// The browser preview mirrors the commands of the Rust runtime closely enough
// to exercise the UI. These tests keep that mirror honest about the contract.
describe("preview transport", () => {
  it("serves a snapshot and frame that satisfy the IPC schemas", async () => {
    const t = await createPreviewTransport();
    expect(snapshotSchema.safeParse(await t.invoke("get_snapshot")).success).toBe(true);
    const frame = frameSchema.parse(await t.invoke("virtual_frame"));
    expect(frame.leds.length).toBeGreaterThan(0);
  });

  it("serves the Observable Catalog captured from Rust, and says plainly what it cannot do without a runtime", async () => {
    const t = await createPreviewTransport();
    const catalog = observableCatalogSchema.parse(await t.invoke("observables_catalog"));
    expect(catalog.observables.some((o) => o.id === "example.counter.level")).toBe(true);
    expect(snapshotSchema.parse(await t.invoke("get_snapshot")).observables.observables).toHaveLength(catalog.observables.length);
    await expect(t.invoke("explain_control", { surfaceId: "a", pageId: "b", controlId: "c" })).rejects.toMatch(/desktop app/);
    await expect(t.invoke("observable_state", { provider: "example.counter", id: "level", args: null })).rejects.toMatch(/desktop app/);
  });

  it("switches pages and pushes state to listeners", async () => {
    const t = await createPreviewTransport();
    const seen: RuntimeState[] = [];
    await t.listen("runtime-state", (p) => seen.push(p as RuntimeState));
    await t.invoke("activate_surface", { surfaceId: "development", pageId: null });
    await t.invoke("activate_page", { pageId: "git" });
    expect(seen.at(-1)).toMatchObject({ activeSurfaceId: "development", activePageId: "git" });
  });

  it("creates a surface the way Rust does: an id from the name, the blank surface's text and icon", async () => {
    const t = await createPreviewTransport();
    expect(await t.invoke("create_surface", { name: "My surface" })).toBe("my-surface");
    expect(await t.invoke("create_surface", { name: "My surface" })).toBe("my-surface-2");
    expect(await t.invoke("create_surface", { name: "Ünï" })).toBe("n");
    const created = snapshotSchema.parse(await t.invoke("get_snapshot")).workspace.surfaces.find((s) => s.id === "my-surface");
    expect(created).toMatchObject({ name: "My surface", description: "A blank surface. Select a pad and give it an action.", icon: "＋", category: "custom" });
    await expect(t.invoke("create_surface", { name: "  " })).rejects.toBe("A surface name needs 1 to 40 characters.");
  });

  it("rejects unknown surfaces", async () => {
    const t = await createPreviewTransport();
    await expect(t.invoke("activate_surface", { surfaceId: "nope", pageId: null })).rejects.toBeTruthy();
  });

  it("does not run actions in edit mode", async () => {
    const t = await createPreviewTransport();
    const events: string[] = [];
    await t.listen("runtime-event", (p) => events.push((p as { type: string }).type));
    await t.invoke("set_mode", { mode: "edit" });
    await t.invoke("virtual_input", { pos: { kind: "grid", col: 0, row: 0 }, pressed: true });
    expect(events).toContain("control.pressed");
    expect(events).not.toContain("action.started");
  });

  it("serves the agents captured from the Rust runtime, with times moved to now", async () => {
    const t = await createPreviewTransport();
    const snap = snapshotSchema.parse(await t.invoke("get_snapshot"));
    expect(snap.agents.agents.length).toBeGreaterThan(1);
    const started = snap.agents.sessions.map((s) => s.startedAt ?? 0);
    expect(Math.max(...started)).toBeGreaterThan(Date.now() - 60_000);
  });

  it("does not pretend to command agents without a runtime", async () => {
    const t = await createPreviewTransport();
    await expect(t.invoke("agents_command", { target: { kind: "focus" }, command: { kind: "approve" } })).rejects.toMatch(/preview/i);
    await expect(t.invoke("agents_set_access", { agentId: "x", access: "control" })).rejects.toBeTruthy();
  });

  it("pushes a newer agents snapshot when the selection changes", async () => {
    const t = await createPreviewTransport();
    const before = agentsSnapshotSchema.parse(((await t.invoke("get_snapshot")) as { agents: unknown }).agents);
    const seen: number[] = [];
    await t.listen("agents-changed", (p) => seen.push(agentsSnapshotSchema.parse(p).rev));
    await t.invoke("agents_focus", { slot: 2 });
    expect(seen).toEqual([before.rev + 1]);
  });
});

describe("the editor's questions in a browser", () => {
  it("answers review_feedback with Rust's limits and nothing to report, so no draft is refused or praised", async () => {
    const t = await createPreviewTransport();
    const review = feedbackReviewSchema.parse(await t.invoke("review_feedback", { surfaceId: "development", pageId: "home", control: {} }));
    expect(review.issues).toEqual([]);
    expect(review.limits.maxRules).toBe(32);
  });

  it("says plainly that explanations need the desktop app", async () => {
    const t = await createPreviewTransport();
    await expect(t.invoke("explain_control", { surfaceId: "development", pageId: "home", controlId: "x" })).rejects.toMatch(/desktop app/);
  });
});
