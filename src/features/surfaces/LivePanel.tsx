import { api } from "../../lib/ipc";
import { STATUS_LABEL } from "../../lib/agents";
import { useStore } from "../../stores/runtime";
import type { Surface } from "../../lib/schema";
import { ActivityFeed } from "../../components/ActivityFeed";

export function LivePanel({ surface }: { surface: Surface }) {
  const events = useStore((s) => s.events);
  const attempt = useStore((s) => s.attempt);
  const palette = useStore((s) => s.agents.palette);
  const showAgents = useStore((s) => s.showAgents);
  const isAgents = surface.category === "agents";
  const visible = events.filter((e) => e.type !== "control.pressed");

  return (
    <aside className="inspector live-panel" aria-label="Live activity">
      <header className="inspector-head">
        <div>
          <h3>Live</h3>
          <p className="hint">Pads do what they will do on the real device.</p>
        </div>
        <span className="pill live">
          <i /> Live
        </span>
      </header>
      <div className="inspector-body">
        {isAgents && (
          <section className="field-group">
            <h4>Agent states</h4>
            <ul className="legend">
              {palette.map((p) => (
                <li key={p.status}>
                  <span className={`dot ${p.animation !== "none" ? `anim-${p.animation}` : ""}`} style={{ background: p.color, ["--glow" as string]: p.color }} />
                  <span>{STATUS_LABEL[p.status]}</span>
                  <small>{p.animation === "flash" ? "flashes" : p.animation === "pulse" ? "pulses" : ""}</small>
                </li>
              ))}
            </ul>
            <div className="row wrap">
              <button type="button" className="btn primary" onClick={() => void attempt(() => api.agents.runDemo())}>
                Run Agent Demo
              </button>
              <button type="button" className="btn" onClick={() => void attempt(() => api.agents.resetDemo())}>
                Reset
              </button>
              <button type="button" className="btn" onClick={() => showAgents("overview")}>
                Open Agents
              </button>
            </div>
            <p className="hint">Four simulated agents. Real agents connect through adapters or the local bridge, and appear on the same pads.</p>
          </section>
        )}
        <section className="field-group">
          <h4>Activity</h4>
          <ActivityFeed events={visible} />
        </section>
      </div>
    </aside>
  );
}
