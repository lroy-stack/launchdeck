// Feedback rules, in words. Pure functions for the read-only view of a control's
// rules; the editor that creates them is a later milestone.

import type { FeedbackRule, Scalar, StateAddress, StateTest } from "./schema";
import { isStateRule } from "./schema";

const DEFAULT_HOLD_MS = 3000;

const quote = (v: Scalar): string => (typeof v === "string" ? `“${v}”` : String(v));

/** `agent-control · slot.status (slot 2)`. */
export function addressText(a: StateAddress): string {
  const args = Object.entries(a.args ?? {});
  const suffix = args.length > 0 ? ` (${args.map(([k, v]) => `${k} ${String(v)}`).join(", ")})` : "";
  return `${a.provider} · ${a.id}${suffix}`;
}

export function testText(t: StateTest): string {
  switch (t.op) {
    case "equals":
      return `is ${quote(t.value)}`;
    case "notEquals":
      return `is not ${quote(t.value)}`;
    case "lessThan":
      return `is below ${t.value}`;
    case "atMost":
      return `is at most ${t.value}`;
    case "greaterThan":
      return `is above ${t.value}`;
    case "atLeast":
      return `is at least ${t.value}`;
    case "hasValue":
      return "has a value";
    case "noValue":
      return "has no value yet";
    case "unavailable":
      return "is not available";
  }
}

function lookText(rule: FeedbackRule): string {
  if (rule.show) return `look “${rule.show}”`;
  if (rule.showFromValue) return "the look named after the value";
  if (rule.showFromField) return `the look named in “${rule.showFromField}”`;
  return "no look";
}

/** How long an event's look lasts, in words. */
export function holdText(hold: "latch" | { ms: number } | undefined): string {
  if (hold === "latch") return "until the next event";
  const ms = hold?.ms ?? DEFAULT_HOLD_MS;
  return `for ${ms % 1000 === 0 ? ms / 1000 : (ms / 1000).toFixed(1)} s`;
}

/** One rule as a condition and what it shows. */
export function describeRule(rule: FeedbackRule): { when: string; show: string } {
  if (isStateRule(rule)) {
    return { when: `${addressText(rule.when.state)} ${testText(rule.when.is)}`, show: lookText(rule) };
  }
  const { event, where } = rule.when;
  const tests = where.map((w) => `${w.field} is ${quote(w.equals)}`).join(" and ");
  return {
    when: `${event.provider} sends ${event.id}${tests ? ` where ${tests}` : ""}`,
    show: `${lookText(rule)} ${holdText(rule.hold)}`,
  };
}
