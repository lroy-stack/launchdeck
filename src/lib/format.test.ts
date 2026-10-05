import { describe, expect, it } from "vitest";
import workspaceJson from "../../examples/default-workspace.json";
import catalogJson from "../../examples/action-catalog.json";
import { categoryLabel, describeEvent, eventNames } from "./format";
import { catalogSchema, workspaceSchema, type RuntimeEvent } from "./schema";

const workspace = workspaceSchema.parse(workspaceJson);
const catalog = catalogSchema.parse(catalogJson);
const names = eventNames(workspace, catalog.actions);

const surface = workspace.surfaces[0]!;
const page = surface.pages[0]!;
const action = catalog.actions.find((a) => a.provider === "shell")!;
const event = (type: string, payload: Record<string, unknown>, source = "runtime"): RuntimeEvent => ({ source, type, payload, ts: 1 });

describe("describeEvent", () => {
  it("names the action instead of its id", () => {
    const at = { surfaceId: surface.id, pageId: page.id };
    expect(describeEvent(event("action.started", { ...at, action: action.id }), names)).toBe(`Running ${action.name}`);
    expect(describeEvent(event("action.completed", { ...at, action: action.id }), names)).toBe(`Done ${action.name}`);
    expect(describeEvent(event("action.failed", { ...at, action: action.id, error: "exit 7" }), names)).toBe(`Failed ${action.name}: exit 7`);
  });

  it("names the surface and the page it moved to", () => {
    expect(describeEvent(event("surface.changed", { surfaceId: surface.id, pageId: page.id }), names)).toBe(`Surface → ${surface.name}`);
    expect(describeEvent(event("page.changed", { surfaceId: surface.id, pageId: page.id }), names)).toBe(`Page → ${page.name}`);
  });

  it("keeps the id when nothing by that id is known", () => {
    expect(describeEvent(event("action.started", { action: "acme.gone" }), names)).toBe("Running acme.gone");
    expect(describeEvent(event("page.changed", { surfaceId: surface.id, pageId: "nowhere" }), names)).toBe("Page → nowhere");
    expect(describeEvent(event("surface.changed", { surfaceId: "elsewhere" }))).toBe("Surface → elsewhere");
  });

  it("says the type and the source of an event it does not know", () => {
    expect(describeEvent(event("build.finished", {}, "ci"), names)).toBe("Event build.finished from ci");
  });
});

describe("category labels", () => {
  it("names every starter's category in words, never by its slug", () => {
    const labels = workspace.surfaces.map((s) => categoryLabel(s.category));
    expect(labels).toEqual(["Agents", "Development", "Productivity", "Home", "Accessibility"]);
  });

  it("names the other known categories", () => {
    expect(categoryLabel("ai")).toBe("Agents");
    expect(categoryLabel("custom")).toBe("Custom");
  });

  it("turns a category it does not know into words", () => {
    expect(categoryLabel("music-studio")).toBe("Music studio");
    expect(categoryLabel("")).toBe("");
  });
});
