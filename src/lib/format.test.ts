import { describe, expect, it } from "vitest";
import workspaceJson from "../../examples/default-workspace.json";
import { categoryLabel } from "./format";
import { workspaceSchema } from "./schema";

describe("category labels", () => {
  it("names every starter's category in words, never by its slug", () => {
    const labels = workspaceSchema.parse(workspaceJson).surfaces.map((s) => categoryLabel(s.category));
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
