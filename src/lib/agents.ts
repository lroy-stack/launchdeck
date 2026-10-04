// Agent Control on the TypeScript side: Zod schemas for what Rust sends, and
// the small pure helpers the Agents views share. Nothing here decides what an
// agent may do. Statuses, availability and slot assignment arrive derived from
// the one runtime state in Rust; the UI only displays them.

import { z } from "zod";

export const statusSchema = z.enum([
  "idle",
  "starting",
  "running",
  "tool_use",
  "waiting",
  "approval_required",
  "paused",
  "completed",
  "failed",
  "cancelled",
  "offline",
]);
export type Status = z.infer<typeof statusSchema>;

export const capabilitySchema = z.enum([
  "spawn",
  "send_instruction",
  "pause",
  "resume",
  "cancel",
  "retry",
  "approve",
  "reject",
  "get_status",
  "get_logs",
  "get_diff",
  "open_session",
]);
export type Capability = z.infer<typeof capabilitySchema>;

export const accessSchema = z.enum(["monitor", "control"]);
export type Access = z.infer<typeof accessSchema>;

export const commandKindSchema = z.enum(["approve", "reject", "pause", "resume", "cancel", "retry", "send_instruction", "spawn"]);
export type CommandKind = z.infer<typeof commandKindSchema>;

export const commandStateSchema = z.enum(["unavailable", "available", "attention", "pending", "succeeded", "failed"]);
export type CommandState = z.infer<typeof commandStateSchema>;

export const unavailableSchema = z.enum(["unsupported", "offline", "monitor_only", "no_session", "wrong_state", "nothing_pending"]);
export type UnavailableReason = z.infer<typeof unavailableSchema>;

const animationSchema = z.enum(["none", "pulse", "flash"]);

export const commandViewSchema = z.object({
  kind: commandKindSchema,
  state: commandStateSchema,
  reason: z.string().nullish(),
  reasonCode: unavailableSchema.nullish(),
});
export type CommandView = z.infer<typeof commandViewSchema>;

export const approvalSchema = z.object({
  id: z.string(),
  sessionId: z.string(),
  message: z.string(),
  tool: z.string().nullish(),
  createdAt: z.number(),
});
export type Approval = z.infer<typeof approvalSchema>;

export const agentViewSchema = z.object({
  id: z.string(),
  name: z.string(),
  provider: z.string().nullish(),
  harness: z.string().nullish(),
  adapterId: z.string(),
  capabilities: z.array(capabilitySchema),
  access: accessSchema,
  online: z.boolean(),
  registeredAt: z.number(),
  lastSeen: z.number(),
  status: statusSchema,
  sessionIds: z.array(z.string()),
  pendingApprovals: z.number().int(),
  commands: z.array(commandViewSchema),
});
export type AgentView = z.infer<typeof agentViewSchema>;

export const sessionViewSchema = z.object({
  id: z.string(),
  agentId: z.string(),
  agentName: z.string(),
  task: z.string().nullish(),
  status: statusSchema,
  startedAt: z.number().nullish(),
  updatedAt: z.number(),
  pendingApproval: approvalSchema.nullish(),
  progress: z.number().nullish(),
  detail: z.string().nullish(),
  error: z.string().nullish(),
  lastCommand: z
    .object({
      command: commandKindSchema,
      state: z.enum(["pending", "succeeded", "failed"]),
      error: z.string().nullish(),
      at: z.number(),
    })
    .nullish(),
  slot: z.number().int().nullish(),
  commands: z.array(commandViewSchema),
});
export type SessionView = z.infer<typeof sessionViewSchema>;

export const slotViewSchema = z.object({
  index: z.number().int(),
  status: statusSchema.nullish(),
  agentId: z.string().nullish(),
  sessionId: z.string().nullish(),
  label: z.string().nullish(),
  focused: z.boolean(),
});
export type SlotView = z.infer<typeof slotViewSchema>;

export const adapterViewSchema = z.object({
  id: z.string(),
  name: z.string(),
  kind: z.enum(["simulated", "hooks", "bridge"]),
  description: z.string(),
  limits: z.array(z.string()),
  state: z.enum(["active", "disabled", "error"]),
  detail: z.string().nullish(),
  agentIds: z.array(z.string()),
  diagnostics: z.array(z.object({ ts: z.number(), level: z.enum(["info", "warn", "error"]), message: z.string() })),
});
export type AdapterView = z.infer<typeof adapterViewSchema>;

export const auditEntrySchema = z.object({
  ts: z.number(),
  kind: z.string(),
  actor: z.enum(["user", "agent", "system"]),
  agentId: z.string().nullish(),
  sessionId: z.string().nullish(),
  tool: z.string().nullish(),
  detail: z.string().nullish(),
});
export type AuditEntry = z.infer<typeof auditEntrySchema>;

export const agentsSettingsSchema = z.object({
  notifications: z.boolean(),
  hooksEnabled: z.boolean(),
  hooksPort: z.number().int(),
  hooksHarnesses: z.record(z.string(), z.boolean()),
  hooksHoldSecs: z.number().int(),
});
export type AgentsSettings = z.infer<typeof agentsSettingsSchema>;

export const agentsSnapshotSchema = z.object({
  rev: z.number().int(),
  agents: z.array(agentViewSchema),
  sessions: z.array(sessionViewSchema),
  slots: z.array(slotViewSchema),
  focus: z.number().int().nullish(),
  adapters: z.array(adapterViewSchema),
  summary: z.object({
    agentsOnline: z.number().int(),
    agentsTotal: z.number().int(),
    activeSessions: z.number().int(),
    pendingApprovals: z.number().int(),
    failedSessions: z.number().int(),
    recentCompletions: z.number().int(),
  }),
  palette: z.array(z.object({ status: statusSchema, color: z.string(), animation: animationSchema })),
  audit: z.array(auditEntrySchema),
  settings: agentsSettingsSchema,
});
export type AgentsSnapshot = z.infer<typeof agentsSnapshotSchema>;

/** Setup text for connecting a harness. Contains the hook token. */
export const hookSetupSchema = z.object({
  enabled: z.boolean(),
  port: z.number().int(),
  envVar: z.string(),
  token: z.string(),
  shellExport: z.string(),
  claudeCodeSettings: z.string(),
  codexConfig: z.string(),
});
export type HookSetup = z.infer<typeof hookSetupSchema>;

export const notificationSchema = z.object({
  kind: z.enum(["approval_required", "agent_failed", "long_running_completed", "adapter_disconnected"]),
  title: z.string(),
  body: z.string(),
  agentId: z.string().nullish(),
  sessionId: z.string().nullish(),
});
export type AgentNotification = z.infer<typeof notificationSchema>;

export const emptyAgents: AgentsSnapshot = {
  rev: 0,
  agents: [],
  sessions: [],
  slots: [],
  focus: null,
  adapters: [],
  summary: { agentsOnline: 0, agentsTotal: 0, activeSessions: 0, pendingApprovals: 0, failedSessions: 0, recentCompletions: 0 },
  palette: [],
  audit: [],
  settings: { notifications: true, hooksEnabled: false, hooksPort: 47822, hooksHarnesses: {}, hooksHoldSecs: 90 },
};

// ---------------------------------------------------------------------------
// Presentation helpers
// ---------------------------------------------------------------------------

/** Status names are always printed; colour is never the only signal. */
export const STATUS_LABEL: Record<Status, string> = {
  idle: "Idle",
  starting: "Starting",
  running: "Running",
  tool_use: "Using a tool",
  waiting: "Waiting",
  approval_required: "Needs approval",
  paused: "Paused",
  completed: "Completed",
  failed: "Failed",
  cancelled: "Cancelled",
  offline: "Offline",
};

export const COMMAND_LABEL: Record<CommandKind, string> = {
  approve: "Approve",
  reject: "Reject",
  pause: "Pause",
  resume: "Resume",
  cancel: "Stop",
  retry: "Retry",
  send_instruction: "Instruct",
  spawn: "New task",
};

/** Same urgency order Rust uses to pick what represents an agent. */
export const STATUS_URGENCY: Record<Status, number> = {
  approval_required: 100,
  failed: 90,
  waiting: 80,
  running: 70,
  tool_use: 65,
  starting: 60,
  paused: 50,
  completed: 40,
  cancelled: 30,
  idle: 20,
  offline: 0,
};

export const isActive = (s: Status): boolean => ["starting", "running", "tool_use", "waiting", "approval_required", "paused"].includes(s);
export const isTerminal = (s: Status): boolean => s === "completed" || s === "failed" || s === "cancelled";

export function byUrgency(a: SessionView, b: SessionView): number {
  return STATUS_URGENCY[b.status] - STATUS_URGENCY[a.status] || b.updatedAt - a.updatedAt;
}

/** `42s`, `3m 05s`, `1h 12m`. Empty for sessions that never started. */
export function elapsed(startedAt: number | null | undefined, now: number): string {
  if (!startedAt || now < startedAt) return "";
  const s = Math.floor((now - startedAt) / 1000);
  if (s < 60) return `${s}s`;
  const m = Math.floor(s / 60);
  if (m < 60) return `${m}m ${String(s % 60).padStart(2, "0")}s`;
  return `${Math.floor(m / 60)}h ${String(m % 60).padStart(2, "0")}m`;
}

/** Commands worth showing on a card: supported by the agent, and not just
 * unavailable because of the current state. */
export function visibleCommands(commands: CommandView[]): CommandView[] {
  return commands.filter((c) => c.state !== "unavailable");
}

/** The commands the agent supports at all (for explaining what is greyed out). */
export function supportedCommands(commands: CommandView[]): CommandView[] {
  return commands.filter((c) => c.reasonCode !== "unsupported");
}

export const commandOf = (commands: CommandView[], kind: CommandKind): CommandView | undefined => commands.find((c) => c.kind === kind);

export function actionableSession(sessions: SessionView[]): SessionView[] {
  return sessions.filter((s) => s.status === "approval_required" || s.status === "failed").sort(byUrgency);
}

/** Merge a snapshot into the current one only if it is not older. */
export function newer(current: AgentsSnapshot, incoming: AgentsSnapshot): AgentsSnapshot {
  return incoming.rev >= current.rev ? incoming : current;
}

export const providerLabel = (a: Pick<AgentView, "provider" | "harness">): string => [a.provider, a.harness].filter(Boolean).join(" · ");
