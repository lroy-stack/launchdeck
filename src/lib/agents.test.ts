import { describe, expect, it } from "vitest";
import fixtureJson from "../../examples/agents-snapshot.json";
import {
  STATUS_URGENCY,
  actionableSession,
  agentsSnapshotSchema,
  byUrgency,
  capabilitySchema,
  commandOf,
  elapsed,
  emptyAgents,
  isActive,
  isTerminal,
  newer,
  notificationSchema,
  providerLabel,
  statusSchema,
  supportedCommands,
  visibleCommands,
  type CommandView,
} from "./agents";

// Captured from the Rust runtime (`cargo run --example dump_agents`) and guarded
// against drift by tests/agents_contract.rs, so this is the real IPC contract.
const fixture: unknown = fixtureJson;
const snap = agentsSnapshotSchema.parse(fixture);

describe("agents contract with Rust", () => {
  it("parses the snapshot emitted by the runtime", () => {
    expect(snap.agents.length).toBe(4);
    expect(snap.slots).toHaveLength(8);
    expect(snap.palette.map((p) => p.status).sort()).toEqual([...statusSchema.options].sort());
  });

  it("keeps several sessions side by side, each with its own status and pad", () => {
    expect(new Set(snap.sessions.map((s) => s.id)).size).toBe(snap.sessions.length);
    expect(new Set(snap.sessions.map((s) => s.status)).size).toBeGreaterThan(2);
    const slots = snap.sessions.map((s) => s.slot);
    expect(new Set(slots).size).toBe(slots.length);
  });

  it("agrees with itself: the summary counts what the lists show", () => {
    expect(snap.summary.agentsTotal).toBe(snap.agents.length);
    expect(snap.summary.agentsOnline).toBe(snap.agents.filter((a) => a.online).length);
    expect(snap.summary.pendingApprovals).toBe(snap.sessions.filter((s) => s.pendingApproval).length);
    expect(snap.summary.failedSessions).toBe(snap.sessions.filter((s) => s.status === "failed").length);
    expect(snap.summary.activeSessions).toBe(snap.sessions.filter((s) => isActive(s.status)).length);
  });

  it("round-trips without losing data", () => {
    expect(agentsSnapshotSchema.parse(JSON.parse(JSON.stringify(snap)))).toEqual(snap);
  });

  it("rejects a status or capability the runtime does not define", () => {
    expect(statusSchema.safeParse("thinking").success).toBe(false);
    expect(capabilitySchema.safeParse("delete_everything").success).toBe(false);
  });

  it("names no provider in the shared vocabulary", () => {
    const vocabulary = JSON.stringify([statusSchema.options, capabilitySchema.options]).toLowerCase();
    expect(vocabulary).not.toMatch(/claude|codex|openai|anthropic/);
  });

  it("only allows the four notification kinds", () => {
    const ok = notificationSchema.safeParse({ kind: "approval_required", title: "t", body: "b" });
    expect(ok.success).toBe(true);
    expect(notificationSchema.safeParse({ kind: "marketing", title: "t", body: "b" }).success).toBe(false);
  });
});

describe("what a card shows", () => {
  const approving = snap.sessions.find((s) => s.status === "approval_required")!;
  const failed = snap.sessions.find((s) => s.status === "failed")!;
  const running = snap.sessions.find((s) => s.status === "running")!;

  it("offers approve and reject only while a request is pending", () => {
    const kinds = visibleCommands(approving.commands).map((c) => c.kind);
    expect(kinds).toEqual(expect.arrayContaining(["approve", "reject"]));
    expect(visibleCommands(running.commands).map((c) => c.kind)).not.toContain("approve");
  });

  it("offers retry for a failed session and pause for a running one", () => {
    expect(commandOf(failed.commands, "retry")?.state).toBe("available");
    expect(commandOf(running.commands, "pause")?.state).toBe("available");
    expect(commandOf(running.commands, "resume")?.state).toBe("unavailable");
  });

  it("separates 'not possible now' from 'not supported by this agent'", () => {
    const c = (kind: CommandView["kind"], state: CommandView["state"], reasonCode: CommandView["reasonCode"]): CommandView => ({ kind, state, reasonCode });
    const commands = [c("pause", "available", null), c("resume", "unavailable", "wrong_state"), c("retry", "unavailable", "unsupported")];
    expect(visibleCommands(commands).map((x) => x.kind)).toEqual(["pause"]);
    expect(supportedCommands(commands).map((x) => x.kind)).toEqual(["pause", "resume"]);
  });

  it("puts what needs a person first", () => {
    const order = [...snap.sessions].sort(byUrgency).map((s) => s.status);
    expect(order[0]).toBe("approval_required");
    expect(actionableSession(snap.sessions).map((s) => s.status)).toEqual(["approval_required", "failed"]);
    expect(STATUS_URGENCY.approval_required).toBeGreaterThan(STATUS_URGENCY.failed);
  });

  it("describes the provider and harness without inventing either", () => {
    expect(providerLabel({ provider: "demo", harness: "simulated" })).toBe("demo · simulated");
    expect(providerLabel({ provider: null, harness: "cli" })).toBe("cli");
    expect(providerLabel({})).toBe("");
  });
});

describe("helpers", () => {
  it("formats elapsed time and stays empty for sessions that never started", () => {
    expect(elapsed(1_000, 43_000)).toBe("42s");
    expect(elapsed(0, 5_000)).toBe("");
    expect(elapsed(null, 5_000)).toBe("");
    expect(elapsed(1_000, 3_901_000)).toBe("1h 05m");
    expect(elapsed(1_000, 186_000)).toBe("3m 05s");
    expect(elapsed(10_000, 1_000)).toBe("");
  });

  it("tells active from finished", () => {
    for (const s of statusSchema.options) expect(isActive(s) && isTerminal(s), s).toBe(false);
    expect(isActive("approval_required")).toBe(true);
    expect(isTerminal("failed")).toBe(true);
    expect(isActive("offline")).toBe(false);
  });

  it("ignores snapshots older than the one on screen", () => {
    const a = { ...emptyAgents, rev: 5 };
    const stale = { ...emptyAgents, rev: 4 };
    const fresh = { ...emptyAgents, rev: 6 };
    expect(newer(a, stale)).toBe(a);
    expect(newer(a, fresh)).toBe(fresh);
    expect(newer(a, { ...emptyAgents, rev: 5 }).rev).toBe(5);
  });
});
