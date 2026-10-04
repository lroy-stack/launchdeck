import { api } from "../../lib/ipc";
import { useStore, type AgentsTab } from "../../stores/runtime";
import { Adapters } from "./Adapters";
import { Overview } from "./Overview";
import { Sessions } from "./Sessions";
import { SurfaceTab } from "./SurfaceTab";

const TABS: Array<{ id: AgentsTab; label: string }> = [
  { id: "overview", label: "Overview" },
  { id: "sessions", label: "Active sessions" },
  { id: "adapters", label: "Adapters" },
  { id: "surface", label: "Agent Control surface" },
];

/**
 * Observe, intervene, orchestrate. Everything here is a view of one runtime
 * state; the same state lights the virtual and the physical Launchpad.
 */
export function AgentsView() {
  const tab = useStore((s) => s.agentsTab);
  const setTab = useStore((s) => s.setAgentsTab);
  const agents = useStore((s) => s.agents);
  const attempt = useStore((s) => s.attempt);
  const hasDemo = agents.agents.some((a) => a.adapterId === "demo");
  const { summary } = agents;

  return (
    <div className="agents-view">
      <header className="agents-head">
        <div>
          <span className="eyebrow">Agent Control</span>
          <h1>Agents</h1>
          <p className="hint">Observe what your agents are doing, step in when they need you, and start work, from here or from the pads.</p>
        </div>
        <div className="row">
          {hasDemo && (
            <button type="button" className="btn" onClick={() => void attempt(() => api.agents.resetDemo())}>
              Reset demo
            </button>
          )}
          <button type="button" className="btn primary" onClick={() => void attempt(() => api.agents.runDemo())}>
            Run Agent Demo
          </button>
        </div>
      </header>

      <div className="tabs" role="tablist" aria-label="Agents">
        {TABS.map((t) => (
          <button key={t.id} type="button" role="tab" aria-selected={tab === t.id} className={`tab${tab === t.id ? " on" : ""}`} onClick={() => setTab(t.id)}>
            {t.label}
            {t.id === "overview" && summary.pendingApprovals > 0 && <em className="count attention">{summary.pendingApprovals}</em>}
            {t.id === "sessions" && summary.activeSessions > 0 && <em className="count">{summary.activeSessions}</em>}
          </button>
        ))}
      </div>

      <div className="agents-body" role="tabpanel">
        {tab === "overview" && <Overview />}
        {tab === "sessions" && <Sessions />}
        {tab === "adapters" && <Adapters />}
        {tab === "surface" && <SurfaceTab />}
      </div>
    </div>
  );
}
