// Small pieces shared by the Agents views. None of them decide anything: they
// print what Rust derived (statuses, which commands exist and in which state)
// and forward a click as a command. Whether the command is allowed is decided
// by the runtime, the same way it is for a pad.

import { useEffect, useMemo, useState } from "react";
import { api, type AgentCommand, type AgentTarget } from "../../lib/ipc";
import {
  COMMAND_LABEL,
  STATUS_LABEL,
  commandOf,
  elapsed,
  isActive,
  visibleCommands,
  type AgentView,
  type Approval,
  type CommandKind,
  type CommandView,
  type SessionView,
  type Status,
} from "../../lib/agents";
import { useStore } from "../../stores/runtime";

/** Re-renders every second while something is running, so elapsed time moves. */
export function useNow(live = true): number {
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    if (!live) return;
    const id = setInterval(() => setNow(Date.now()), 1000);
    return () => clearInterval(id);
  }, [live]);
  return now;
}

export function usePalette(): Map<Status, { color: string; animation: string }> {
  const palette = useStore((s) => s.agents.palette);
  return useMemo(() => new Map(palette.map((p) => [p.status, { color: p.color, animation: p.animation }])), [palette]);
}

/** Status as the pads show it: the same colour and motion, and always the name. */
export function StatusBadge({ status }: { status: Status }) {
  const look = usePalette().get(status);
  const motion = look && look.animation !== "none" ? `anim-${look.animation}` : "";
  return (
    <span className={`status-badge s-${status}`}>
      <i className={`sdot ${motion}`} style={{ background: look?.color ?? "var(--text-3)" }} aria-hidden />
      {STATUS_LABEL[status]}
    </span>
  );
}

/** Sends a command and reports a refusal as a toast. The outcome itself comes back in the next snapshot. */
export function useRunCommand() {
  const attempt = useStore((s) => s.attempt);
  return (target: AgentTarget, command: AgentCommand) => attempt(() => api.agents.command(target, command));
}

const ORDER: CommandKind[] = ["approve", "reject", "pause", "resume", "retry", "send_instruction", "cancel"];
const rank = (k: CommandKind): number => ORDER.indexOf(k);

const TONE: Partial<Record<CommandKind, string>> = { approve: "approve", reject: "danger" };

/** The state of a command, spoken: used for the tooltip and the accessible name. */
function describeCommand(c: CommandView): string {
  switch (c.state) {
    case "pending":
      return `${COMMAND_LABEL[c.kind]}: waiting for the agent`;
    case "succeeded":
      return `${COMMAND_LABEL[c.kind]}: done`;
    case "failed":
      return `${COMMAND_LABEL[c.kind]}: failed`;
    case "attention":
      return `${COMMAND_LABEL[c.kind]}: waiting for you`;
    default:
      return c.reason ?? COMMAND_LABEL[c.kind];
  }
}

/**
 * Only the commands this agent supports and that make sense right now.
 * Anything else is left out instead of greyed out, so a card stays small.
 */
export function CommandButtons({
  commands,
  target,
  only,
  onInstruct,
}: {
  commands: CommandView[];
  target: AgentTarget;
  /** Restrict to some kinds (the compact "needs you" list). */
  only?: CommandKind[];
  onInstruct?: () => void;
}) {
  const run = useRunCommand();
  const shown = visibleCommands(commands)
    .filter((c) => c.kind !== "spawn" && (!only || only.includes(c.kind)))
    .sort((a, b) => rank(a.kind) - rank(b.kind));
  if (shown.length === 0) return null;
  return (
    <div className="cmds" role="group" aria-label="Commands">
      {shown.map((c) => {
        const busy = c.state === "pending";
        const mark = c.state === "succeeded" ? "✓ " : c.state === "failed" ? "! " : "";
        const click = () => {
          if (c.kind === "send_instruction") return onInstruct?.();
          if (c.kind === "spawn") return; // agent-level; started from the agent card
          void run(target, { kind: c.kind });
        };
        return (
          <button
            key={c.kind}
            type="button"
            className={`btn small cmd ${TONE[c.kind] ?? ""} ${c.state}`}
            disabled={busy || (c.kind === "send_instruction" && !onInstruct)}
            aria-busy={busy}
            title={describeCommand(c)}
            onClick={click}
          >
            {mark}
            {COMMAND_LABEL[c.kind]}
            {busy ? "…" : ""}
          </button>
        );
      })}
    </div>
  );
}

/** What the agent wants to do, in the agent's own words. Shown with Approve / Reject. */
export function ApprovalNote({ approval }: { approval: Approval }) {
  return (
    <div className="approval-note" role="note">
      <b>{approval.tool ? `Wants to use ${approval.tool}` : "Needs your decision"}</b>
      <span>{approval.message}</span>
    </div>
  );
}

/** Failure and last-command feedback, concise. Raw diagnostics live in the adapter's log. */
export function SessionNotes({ session }: { session: SessionView }) {
  const last = session.lastCommand;
  return (
    <>
      {session.status === "failed" && session.error && <p className="field-error">{session.error}</p>}
      {last?.state === "failed" && <p className="field-error">{COMMAND_LABEL[last.command]} failed{last.error ? `: ${last.error}` : "."}</p>}
    </>
  );
}

/** Inline form for the one command that needs text. */
export function InstructForm({ target, onDone }: { target: AgentTarget; onDone: () => void }) {
  const run = useRunCommand();
  const [text, setText] = useState("");
  return (
    <form
      className="inline-form row"
      onSubmit={(e) => {
        e.preventDefault();
        const value = text.trim();
        if (!value) return;
        void run(target, { kind: "send_instruction", text: value }).then((ok) => ok && onDone());
      }}
    >
      <input autoFocus className="grow" placeholder="Instruction for this session" value={text} onChange={(e) => setText(e.target.value)} />
      <button type="submit" className="btn small primary" disabled={!text.trim()}>
        Send
      </button>
      <button type="button" className="btn small" onClick={onDone}>
        Cancel
      </button>
    </form>
  );
}

/** One session on one line, plus whatever needs the person's attention. */
export function SessionRow({ session, now, compact = false }: { session: SessionView; now: number; compact?: boolean }) {
  const [instructing, setInstructing] = useState(false);
  const target: AgentTarget = { kind: "session", id: session.id };
  const time = isActive(session.status) ? elapsed(session.startedAt, now) : elapsed(session.startedAt, session.updatedAt);
  const canInstruct = !!commandOf(session.commands, "send_instruction") && commandOf(session.commands, "send_instruction")!.state !== "unavailable";

  return (
    <div className={`session-row s-${session.status}${compact ? " compact" : ""}`}>
      <div className="session-line">
        <StatusBadge status={session.status} />
        <span className="session-task" title={session.task ?? undefined}>
          {compact && <b>{session.agentName}</b>}
          {compact && session.task ? " · " : ""}
          {session.task ?? (compact ? "" : "No task description")}
        </span>
        {time && <time className="session-time">{time}</time>}
        {typeof session.slot === "number" && <span className="slot-tag" title="Pad on the Launchpad">#{session.slot + 1}</span>}
      </div>
      {!compact && session.detail && isActive(session.status) && <p className="session-detail">{session.detail}</p>}
      {session.pendingApproval && <ApprovalNote approval={session.pendingApproval} />}
      <SessionNotes session={session} />
      <CommandButtons
        commands={session.commands}
        target={target}
        only={compact ? ["approve", "reject", "retry"] : undefined}
        onInstruct={canInstruct && !compact ? () => setInstructing(true) : undefined}
      />
      {instructing && <InstructForm target={target} onDone={() => setInstructing(false)} />}
    </div>
  );
}

/** Monitor-only is the default for anything LaunchDeck did not start itself. Control is a deliberate, two-step grant. */
export function AccessControl({ agent }: { agent: AgentView }) {
  const attempt = useStore((s) => s.attempt);
  const [confirm, setConfirm] = useState(false);
  if (agent.access === "control") {
    return (
      <span className="row access">
        <span className="pill live" title="LaunchDeck may send this agent the commands it supports">
          Control
        </span>
        <button type="button" className="link" onClick={() => void attempt(() => api.agents.setAccess(agent.id, "monitor"))}>
          Make read-only
        </button>
      </span>
    );
  }
  if (!confirm) {
    return (
      <span className="row access">
        <span className="pill ghost" title="LaunchDeck can see this agent but cannot command it">
          Read-only
        </span>
        <button type="button" className="link" onClick={() => setConfirm(true)}>
          Allow control
        </button>
      </span>
    );
  }
  return (
    <span className="row access">
      <span className="hint">Let LaunchDeck approve, reject and steer {agent.name}?</span>
      <button
        type="button"
        className="btn small primary"
        onClick={() => {
          setConfirm(false);
          void attempt(() => api.agents.setAccess(agent.id, "control"));
        }}
      >
        Allow
      </button>
      <button type="button" className="btn small" onClick={() => setConfirm(false)}>
        Cancel
      </button>
    </span>
  );
}
