import { describe, expect, it } from "vitest";
import observablesJson from "../../examples/observables.json";
import workspaceJson from "../../examples/default-workspace.json";
import { describeRule, holdText } from "./feedback";
import { feedbackRuleSchema, isStateRule, observableCatalogSchema, workspaceSchema } from "./schema";

const ws = workspaceSchema.parse(workspaceJson);

describe("feedback rules from Rust", () => {
  it("parses every rule the shipped workspace carries, and none is left as an old binding", () => {
    let rules = 0;
    for (const s of ws.surfaces)
      for (const p of s.pages)
        for (const c of p.controls) {
          rules += c.feedback.length;
          expect(c.legacyBindings, `${s.id}/${c.id}`).toEqual([]);
        }
    expect(rules).toBeGreaterThanOrEqual(24);
  });

  it("describes a track pad, the selected marker and a command pad in words", () => {
    const home = ws.surfaces.find((s) => s.id === "agent-control")!.pages[0]!;
    const track = home.controls.find((c) => c.label === "Agent 3")!.feedback[0]!;
    expect(describeRule(track)).toEqual({
      when: "agent-control · slot.status (slot 2) has a value",
      show: "the look named after the value",
    });
    const marker = home.controls.find((c) => c.label === "Select 3")!.feedback[0]!;
    expect(describeRule(marker)).toEqual({
      when: "agent-control · slot.focused (slot 2) is true",
      show: "look “focused”",
    });
    const approve = home.controls.find((c) => c.label === "Approve")!.feedback[0]!;
    expect(describeRule(approve).when).toBe("agent-control · command.state (command approve) has a value");
  });

  it("tells state rules from event rules and describes an event rule's hold", () => {
    const state = feedbackRuleSchema.parse({
      when: { state: { provider: "demo", id: "alpha" }, is: { op: "atLeast", value: 3 } },
      show: "warn",
    });
    const event = feedbackRuleSchema.parse({
      when: { event: { provider: "ci", id: "build.finished" }, where: [{ field: "result", equals: "failed" }] },
      show: "failed",
      hold: { ms: 4000 },
    });
    expect(isStateRule(state)).toBe(true);
    expect(isStateRule(event)).toBe(false);
    expect(describeRule(state).when).toBe("demo · alpha is at least 3");
    expect(describeRule(event)).toEqual({
      when: "ci sends build.finished where result is “failed”",
      show: "look “failed” for 4 s",
    });
    expect(holdText("latch")).toBe("until the next event");
    expect(holdText(undefined)).toBe("for 3 s");
    expect(holdText({ ms: 1500 })).toBe("for 1.5 s");
  });

  it("refuses a rule that is neither", () => {
    expect(feedbackRuleSchema.safeParse({ when: { nothing: true }, show: "x" }).success).toBe(false);
  });
});

describe("the states providers offer, from Rust", () => {
  const catalog = observableCatalogSchema.parse(observablesJson.catalog);

  it("lists the Demo states and the three Agent Control states with their arguments", () => {
    const ids = catalog.observables.map((o) => o.id);
    expect(ids).toEqual(expect.arrayContaining(["demo.alpha", "agent-control.slot.status", "agent-control.slot.focused", "agent-control.command.state"]));
    const status = catalog.observables.find((o) => o.id === "agent-control.slot.status")!;
    expect(status.kind).toBe("state");
    expect(status.params.map((p) => p.name)).toEqual(["slot"]);
    expect(status.value?.values.map((v) => v.value)).toContain("approval_required");
  });
});
