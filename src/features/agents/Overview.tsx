import { useState } from "react";
import { api } from "../../lib/ipc";
import { plural } from "../../lib/format";
import { actionableSession, byUrgency, commandOf, providerLabel, type AgentView, type SessionView } from "../../lib/agents";
import { useStore } from "../../stores/runtime";
import { AccessControl, SessionRow, StatusBadge, useNow, useRunCommand } from "./parts";

const MAX_ROWS = 3;

export function Overview() {
  const agents = useStore((s) => s.agents);
  const setTab = useStore((s) => s.setAgentsTab);
  const attempt = useStore((s) => s.attempt);
  const now = useNow(agents.sessions.some((s) => s.startedAt));
  const { summary } = agents;
  const urgent = actionableSession(agents.sessions);

  if (agents.agents.length === 0) {
    return (
      <div className="panel empty-agents">
        <h2>No agents yet</h2>
        <p className="hint">
          Agents show up here when an adapter registers them. The fastest way to see how it feels is the built-in demo: four simulated agents with an approval to answer,
          a pause to try and a failure to retry. Nothing real runs.
        </p>
        <div className="row">
          <button type="button" className="btn primary" onClick={() => void attempt(() => api.agents.runDemo())}>
            Run Agent Demo
          </button>
          <button type="button" className="btn" onClick={() => setTab("adapters")}>
            Connect a real agent
          </button>
        </div>
      </div>
    );
  }

  return (
    <>
      <section className="stats" aria-label="Summary">
        <Stat label="Agents online" value={`${summary.agentsOnline}/${summary.agentsTotal}`} />
        <Stat label="Active sessions" value={summary.activeSessions} />
        <Stat label="Pending approvals" value={summary.pendingApprovals} tone={summary.pendingApprovals > 0 ? "attention" : undefined} />
        <Stat label="Failed" value={summary.failedSessions} tone={summary.failedSessions > 0 ? "bad" : undefined} />
        <Stat label="Recently completed" value={summary.recentCompletions} tone={summary.recentCompletions > 0 ? "good" : undefined} />
      </section>

      {urgent.length > 0 && (
        <section aria-label="Needs you">
          <h2 className="section-title">Needs you</h2>
          <div className="needs">
            {urgent.map((s) => (
              <SessionRow key={s.id} session={s} now={now} compact />
            ))}
          </div>
        </section>
      )}

      <section aria-label="Agents">
        <h2 className="section-title">Agents</h2>
        <div className="cards wide">
          {agents.agents.map((a) => (
            <AgentCard key={a.id} agent={a} sessions={agents.sessions.filter((s) => s.agentId === a.id)} now={now} />
          ))}
        </div>
      </section>
    </>
  );
}

function Stat({ label, value, tone }: { label: string; value: string | number; tone?: "attention" | "bad" | "good" }) {
  return (
    <div className={`stat ${tone ?? ""}`}>
      <b>{value}</b>
      <span>{label}</span>
    </div>
  );
}

function AgentCard({ agent, sessions, now }: { agent: AgentView; sessions: SessionView[]; now: number }) {
  const setTab = useStore((s) => s.setAgentsTab);
  const ordered = [...sessions].sort(byUrgency);
  const shown = ordered.slice(0, MAX_ROWS);
  const hidden = ordered.length - shown.length;
  const spawn = commandOf(agent.commands, "spawn");
  const canSpawn = !!spawn && spawn.state !== "unavailable";
  const monitorOnly = agent.access === "monitor";

  return (
    <article className={`card agent-card s-${agent.status}`}>
      <header>
        <div className="agent-title">
          <h3>{agent.name}</h3>
          <small>{providerLabel(agent) || "agent"}</small>
        </div>
        <StatusBadge status={agent.status} />
      </header>

      {!agent.online && <p className="hint">Offline. LaunchDeck shows its last known sessions but cannot command it.</p>}

      {shown.length === 0 ? <p className="hint">No sessions yet.</p> : shown.map((s) => <SessionRow key={s.id} session={s} now={now} />)}
      {hidden > 0 && (
        <button type="button" className="link" onClick={() => setTab("sessions")}>
          + {plural(hidden, "more session")}
        </button>
      )}

      {canSpawn && <NewTask agent={agent} />}

      <footer>
        <AccessControl agent={agent} />
        {monitorOnly && agent.pendingApprovals > 0 && <span className="hint">Answer in the agent's own prompt.</span>}
      </footer>
    </article>
  );
}

/** Orchestrate, kept simple: pick the agent (this card), say what to do, optionally where. */
function NewTask({ agent }: { agent: AgentView }) {
  const run = useRunCommand();
  const [open, setOpen] = useState(false);
  const [task, setTask] = useState("");
  const [cwd, setCwd] = useState("");

  if (!open) {
    return (
      <button type="button" className="btn small start-task" onClick={() => setOpen(true)}>
        + New task
      </button>
    );
  }
  return (
    <form
      className="field-group new-task"
      onSubmit={(e) => {
        e.preventDefault();
        const text = task.trim();
        if (!text) return;
        void run({ kind: "agent", id: agent.id }, { kind: "spawn", task: text, cwd: cwd.trim() || null }).then((ok) => {
          if (ok) {
            setTask("");
            setOpen(false);
          }
        });
      }}
    >
      <label className="field">
        <span>Task for {agent.name}</span>
        <textarea autoFocus rows={2} value={task} onChange={(e) => setTask(e.target.value)} placeholder="What should it do?" />
      </label>
      <label className="field">
        <span>Working directory (optional)</span>
        <input value={cwd} onChange={(e) => setCwd(e.target.value)} placeholder="Leave empty for the agent's default" />
      </label>
      <div className="row">
        <button type="submit" className="btn small primary" disabled={!task.trim()}>
          Start
        </button>
        <button type="button" className="btn small" onClick={() => setOpen(false)}>
          Cancel
        </button>
      </div>
    </form>
  );
}

