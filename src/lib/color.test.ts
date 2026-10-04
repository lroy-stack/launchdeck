import { describe, expect, it } from "vitest";
import { hexToRgb, inkFor, isOff, normalizeHex, withAlpha } from "./color";
import { describeEvent, eventTone } from "./format";

describe("colour helpers", () => {
  it("normalises hex input", () => {
    expect(normalizeHex("#ABC")).toBe("#aabbcc");
    expect(normalizeHex("22c55e")).toBe("#22c55e");
    expect(normalizeHex("nope")).toBeNull();
  });
  it("converts and tints", () => {
    expect(hexToRgb("#ff8000")).toEqual([255, 128, 0]);
    expect(withAlpha("#000000", 0.5)).toBe("rgba(0, 0, 0, 0.5)");
    expect(isOff("#000000")).toBe(true);
    expect(isOff("#010101")).toBe(true);
    expect(isOff("#3b82f6")).toBe(false);
  });
  it("picks readable ink", () => {
    expect(inkFor("#ffffff")).toContain("8,8,12");
    expect(inkFor("#101010")).toContain("255,255,255");
  });
});

describe("event formatting", () => {
  it("describes known events and tones", () => {
    const failed = { source: "runtime", type: "action.failed", payload: { action: "shellCommand", error: "exit 7" }, ts: 1 };
    expect(describeEvent(failed)).toBe("Failed shellCommand: exit 7");
    expect(eventTone(failed)).toBe("bad");
    const state = { source: "demo", type: "plugin.state.changed", payload: { key: "claude", value: "running" }, ts: 1 };
    expect(describeEvent(state)).toBe("demo: claude = running");
    expect(eventTone(state)).toBe("accent");
  });
});
