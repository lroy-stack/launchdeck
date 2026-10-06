// The artifact replies Rust sends, as `cargo run --example dump_artifacts`
// writes them to examples/artifacts (guarded by tests/artifacts_contract.rs).
// Every one parses with the Zod schemas the import and export views read, and
// nothing in it is dropped on the way: a field Rust adds and TypeScript does not
// know fails here.

import { describe, expect, it } from "vitest";
import type { z } from "zod";
import exportJson from "../../../examples/artifacts/export-preview.json";
import planJson from "../../../examples/artifacts/import-plan.json";
import missingJson from "../../../examples/artifacts/import-plan-missing.json";
import rejectedJson from "../../../examples/artifacts/inspect-rejected.json";
import { api } from "../../lib/ipc";
import {
  applyOutcomeSchema,
  exportPreviewSchema,
  exportRequestSchema,
  importDecisionsSchema,
  importOriginSchema,
  inspectOutcomeSchema,
  previewOutcomeSchema,
  workspaceSchema,
} from "../../lib/schema";

/** The schema each fixture is a reply of. A new fixture needs an entry. */
const SCHEMAS: Record<string, z.ZodType> = {
  "export-preview.json": exportPreviewSchema,
  "import-plan.json": inspectOutcomeSchema,
  "import-plan-missing.json": inspectOutcomeSchema,
  "inspect-rejected.json": inspectOutcomeSchema,
};

const fixtures = Object.entries(import.meta.glob("../../../examples/artifacts/*.json", { eager: true, import: "default" })).map(
  ([path, json]) => [path.split("/").pop()!, json] as const,
);

describe("the artifact contract with Rust", () => {
  it("has a schema for every fixture Rust writes", () => {
    expect(fixtures.map(([name]) => name).sort()).toEqual(Object.keys(SCHEMAS).sort());
  });

  it.each(fixtures)("parses %s whole, without losing a field", (name, json) => {
    const schema = SCHEMAS[name]!;
    expect(schema.parse(json)).toEqual(json);
  });

  it("reads an import plan in the order the view shows it", () => {
    const outcome = inspectOutcomeSchema.parse(planJson);
    if (outcome.status !== "plan") throw new Error("expected a plan");
    const { plan } = outcome;
    expect(plan.artifact).toMatchObject({ kind: "pack", name: "Development", version: "1.0.0" });
    expect(plan.surfaces[0]).toMatchObject({ sourceId: "development", localId: "development-2", idChanged: true, sameNameAs: "development" });
    expect(plan.contextRules[0]!.outcome).toEqual({ kind: "notSelected" });
    // Every launch is reviewed, by a plain name too, then the commands.
    expect(plan.review.slice(0, 2).map((r) => [r.label, r.kind])).toEqual([
      ["Terminal", "launchesProgram"],
      ["VS Code", "launchesProgram"],
    ]);
    expect(plan.review.slice(2).every((r) => r.kind === "runsCommand")).toBe(true);
    expect(plan.suggested).toEqual({ runs: "remove", inputs: {}, contextRules: [], confirmOverwriteEdits: false });
    expect(plan.ready).toBe(true);
  });

  it("reads a missing plugin as missing, and the plan as ready all the same", () => {
    const outcome = inspectOutcomeSchema.parse(missingJson);
    if (outcome.status !== "plan") throw new Error("expected a plan");
    expect(outcome.plan.dependencies[0]).toMatchObject({ pluginId: "example.counter", status: { kind: "missing" }, required: true });
    expect(outcome.plan.ready).toBe(true);
  });

  it("reads a refused file as its code, sentence and place", () => {
    expect(inspectOutcomeSchema.parse(rejectedJson)).toEqual({
      status: "rejected",
      rejection: { code: "integrity", message: expect.stringMatching(/^The content does not match/), at: "/manifest/integrity/content" },
    });
  });

  it("reads an export preview with the manifest Rust proposes", () => {
    const preview = exportPreviewSchema.parse(exportJson);
    expect(preview.fileName).toBe("development-1.0.0.ldpack");
    expect(preview.manifest.integrity.content).toBe(preview.contentDigest);
    expect(preview.contextRules[0]!.application).toMatchObject({ name: "Visual Studio Code" });
    expect(preview.errors).toEqual([]);
  });
});

describe("what the views send back", () => {
  it("sends a plan's decisions exactly as Rust wrote them", () => {
    const outcome = inspectOutcomeSchema.parse(planJson);
    if (outcome.status !== "plan") throw new Error("expected a plan");
    expect(importDecisionsSchema.parse(outcome.plan.decisions)).toEqual((planJson as { plan: { decisions: unknown } }).plan.decisions);
  });

  it("reads every kind of decision Rust takes", () => {
    const all = {
      resolution: { kind: "replace", instance: 2 },
      runs: "keep",
      inputs: { folder: { kind: "text", value: "/tmp" }, editor: { kind: "application", namespace: "macos", id: "com.microsoft.VSCode", displayName: "Code" } },
      contextRules: [0, 2],
      confirmOverwriteEdits: true,
    };
    expect(importDecisionsSchema.parse(all)).toEqual(all);
    expect(importDecisionsSchema.parse({ resolution: { kind: "importAsNew" } })).toEqual({
      resolution: { kind: "importAsNew" },
      inputs: {},
      contextRules: [],
      confirmOverwriteEdits: false,
    });
  });

  it("builds an export request Rust reads", () => {
    const request = {
      kind: "pack",
      surfaceIds: ["development"],
      metadata: { id: "", name: "", version: "", author: { name: "Ada" } },
      includeContextRules: true,
      inputs: [{ id: "input_1", type: "path", label: "Docker: command" }],
      bindings: [{ at: { surfaceSourceId: "development", pageId: "home", controlId: "g.3.0", slot: "press" }, arg: "command", input: "input_1" }],
      ruleInputs: [],
      extraIdentities: [{ rule: 0, namespace: "macos", id: "com.microsoft.VSCode" }],
    };
    expect(exportRequestSchema.parse(request)).toEqual(request);
  });

  it("reads every outcome of applying", () => {
    const outcome = inspectOutcomeSchema.parse(planJson);
    if (outcome.status !== "plan") throw new Error("expected a plan");
    const report = { surfacesAdded: ["development-2"], surfacesReplaced: [], rulesAdded: 1, rulesSkipped: 0, slotsRemoved: 10, missingDependencies: [], autoContext: false };
    for (const reply of [
      { status: "applied", report },
      { status: "stale", plan: outcome.plan },
      { status: "blocked", plan: outcome.plan },
      { status: "failed", message: "The workspace could not be saved.", detail: "disk full" },
      { status: "expired" },
    ]) {
      expect(applyOutcomeSchema.parse(reply)).toEqual(reply);
    }
    expect(previewOutcomeSchema.parse({ status: "expired" })).toEqual({ status: "expired" });
  });
});

describe("a surface's origin", () => {
  const origin = {
    format: "launchdeck.pack",
    artifactId: "launchdeck-examples.development",
    artifactName: "Development",
    artifactVersion: "1.0.0",
    instance: 1,
    sourceSurfaceId: "development",
    contentDigest: "sha256:c659b2bb18a2f7d1ace559fb6166ec14534caa0688ad9fc1948104fc6ef8430c",
    surfaceDigest: "sha256:0000000000000000000000000000000000000000000000000000000000000000",
  };

  it("is read for display, and absent on a surface that was not imported", () => {
    expect(importOriginSchema.parse(origin)).toEqual(origin);
    const workspace = workspaceSchema.parse({
      schemaVersion: 4,
      surfaces: [
        { id: "a", name: "A", category: "custom", pages: [], importedFrom: origin },
        { id: "b", name: "B", category: "custom", pages: [] },
      ],
      preferences: {},
    });
    expect(workspace.surfaces[0]!.importedFrom).toEqual(origin);
    expect(workspace.surfaces[1]!.importedFrom).toBeUndefined();
  });
});

describe("in a browser preview", () => {
  it("says that files are imported and exported in the desktop app", async () => {
    await expect(api.artifacts.limits()).rejects.toMatch(/desktop app/);
    await expect(api.artifacts.inspect(new Uint8Array([123, 125]), "a.ldpack")).rejects.toMatch(/desktop app/);
  });
});
