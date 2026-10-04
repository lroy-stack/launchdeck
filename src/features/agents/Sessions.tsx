import { api } from "../../lib/ipc";
import { formatTime, plural } from "../../lib/format";
import { byUrgency, elapsed, isActive, isTerminal, type AuditEntry, type SessionView } from "../../lib/agents";
import { useStore } from "../../stores/runtime";
import { ApprovalNote, CommandButtons, SessionNotes, StatusBadge, useNow } from "./parts";

export function Sessions() {
  const agents = useStore((s) => s.agents);
  const attempt = useStore((s) => s.attempt);
  const toast = useStore((s) => s.toast);
  const now = useNow(agents.sessions.some((s) => isActive(s.status)));

  // Sessions on a pad first (in pad order), then the rest by urgency.
  const sessions = [...agents.sessions].sort((a, b) => {
    const sa = a.slot ?? 99;
    const sb = b.slot ?? 99;
    return sa - sb || byUrgency(a, b);
  });
  const finished = sessions.filter((s) => isTerminal(s.status)).length;

  const dismissFinished = () =>
    void attempt(async () => {
      const n = await api.agents.dismiss(null);
      toast(n === 0 ? "Nothing to dismiss." : `Dismissed ${plural(n, "finished session")}.`);
    });

  return (
    <>
      <section aria-label="Active sessions">
        <div className="section-head">
          <h2 className="section-title">Sessions</h2>
          <button type="button" className="btn small" disabled={finished === 0} onClick={dismissFinished}>
            Dismiss finished{finished > 0 ? ` (${finished})` : ""}
          </button>
        </div>
        {sessions.length === 0 ? (
          <p className="empty-state">No sessions. Start a task from an agent, or run the demo.</p>
        ) : (
          <div className="table-wrap">
            <table className="table">
              <thead>
                <tr>
                  <th>Pad</th>
                  <th>Agent</th>
                  <th>Task</th>
                  <th>Status</th>
                  <th>Time</th>
                  <th>Commands</th>
                </tr>
              </thead>
              <tbody>
                {sessions.map((s) => (
                  <Row key={s.id} session={s} now={now} focused={agents.focus === s.slot && s.slot != null} />
                ))}
              </tbody>
            </table>
          </div>
        )}
      </section>

      <section aria-label="Audit log">
        <h2 className="section-title">Audit log</h2>
        <p className="hint">
          Session starts and ends, approvals and the interventions you made. Prompts, commands and file contents are never written here.
        </p>
        <AuditList entries={agents.audit} />
      </section>
    </>
  );
}

function Row({ session, now, focused }: { session: SessionView; now: number; focused: boolean }) {
  const attempt = useStore((s) => s.attempt);
  const time = isActive(session.status) ? elapsed(session.startedAt, now) : elapsed(session.startedAt, session.updatedAt);
  const target = { kind: "session", id: session.id } as const;
  return (
    <tr className={focused ? "focused" : ""}>
      <td className="pad-cell">
        {typeof session.slot === "number" ? (
          <button
            type="button"
            className={`slot-btn${focused ? " on" : ""}`}
            title="Select on the Launchpad"
            aria-pressed={focused}
            onClick={() => void attempt(() => api.agents.focus(session.slot ?? null))}
          >
            {session.slot + 1}
          </button>
        ) : (
          <span className="hint" title="More sessions than pads: this one has no pad">
            –
          </span>
        )}
      </td>
      <td>{session.agentName}</td>
      <td className="task-cell">
        <div className="task-stack">
          <span className="task-text" title={session.task ?? undefined}>
            {session.task ?? "–"}
          </span>
          {session.detail && isActive(session.status) && <small>{session.detail}</small>}
          {session.pendingApproval && <ApprovalNote approval={session.pendingApproval} />}
          <SessionNotes session={session} />
        </div>
      </td>
      <td>
        <StatusBadge status={session.status} />
      </td>
      <td className="mono">{time || "–"}</td>
      <td>
        <div className="row wrap">
          <CommandButtons commands={session.commands} target={target} />
          {isTerminal(session.status) && (
            <button type="button" className="btn small" onClick={() => void attempt(() => api.agents.dismiss(session.id))}>
              Dismiss
            </button>
          )}
        </div>
      </td>
    </tr>
  );
}

const KIND_LABEL: Record<string, string> = {
  "agent.registered": "Agent registered",
  "session.started": "Session started",
  "session.completed": "Session completed",
  "session.failed": "Session failed",
  "session.cancelled": "Session cancelled",
  "approval.requested": "Approval requested",
  "approval.approved": "Approved",
  "approval.rejected": "Rejected",
  "approval.withdrawn": "Approval withdrawn",
  "approval.superseded": "Approval replaced",
  "intervention.pause": "Paused",
  "intervention.resume": "Resumed",
  "intervention.cancel": "Cancelled by you",
  "intervention.retry": "Retried",
  "intervention.spawn": "Task started",
  "command.failed": "Command failed",
  "command.refused": "Command refused",
  "access.granted": "Control granted",
  "access.revoked": "Control revoked",
};

function auditText(e: AuditEntry): string {
  const label = KIND_LABEL[e.kind] ?? e.kind;
  const what = [e.agentId, e.tool].filter(Boolean).join(" · ");
  return [label, what, e.detail].filter(Boolean).join(" · ");
}

function AuditList({ entries }: { entries: AuditEntry[] }) {
  if (entries.length === 0) return <p className="empty-state">Nothing recorded yet.</p>;
  const recent = [...entries].slice(-40).reverse();
  return (
    <ul className="feed audit">
      {recent.map((e, i) => (
        <li key={`${e.ts}-${i}`} className={e.actor === "user" ? "tone-accent" : e.kind.endsWith("failed") || e.kind === "approval.rejected" ? "tone-bad" : e.kind.endsWith("completed") ? "tone-good" : ""}>
          <time>{formatTime(e.ts)}</time>
          <span className="feed-text">{auditText(e)}</span>
          <span className="feed-source">{e.actor}</span>
        </li>
      ))}
    </ul>
  );
}
