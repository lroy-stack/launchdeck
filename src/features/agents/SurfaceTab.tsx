import { api } from "../../lib/ipc";
import { STATUS_LABEL } from "../../lib/agents";
import { activePage, activeSurface, useStore } from "../../stores/runtime";
import { PadGrid } from "../../components/PadGrid";
import { usePalette } from "./parts";

export const AGENT_SURFACE_ID = "agent-control";

/** Plain-language key for the colours, taken from the same palette the pads use. */
const MEANING: Record<string, string> = {
  idle: "Online, nothing to do",
  starting: "Session is starting",
  running: "Working",
  tool_use: "Using a tool",
  waiting: "Waiting for input",
  approval_required: "Needs your decision",
  paused: "Paused",
  completed: "Finished",
  failed: "Failed",
  cancelled: "Stopped",
  offline: "Not connected",
};

export function SurfaceTab() {
  const workspace = useStore((s) => s.workspace);
  const state = useStore((s) => s.state);
  const frame = useStore((s) => s.frame);
  const agents = useStore((s) => s.agents);
  const attempt = useStore((s) => s.attempt);
  const setView = useStore((s) => s.setView);
  const select = useStore((s) => s.select);
  const palette = usePalette();

  const exists = !!workspace?.surfaces.some((s) => s.id === AGENT_SURFACE_ID);
  const core = { workspace, state };
  const surface = activeSurface(core);
  const page = activePage(core);
  const current = surface?.id === AGENT_SURFACE_ID && page && state ? { page, state } : null;
  const device = state?.devices.find((d) => d.connected) ?? state?.devices[0] ?? null;
  const hasDemo = agents.agents.some((a) => a.adapterId === "demo");

  const show = () => {
    select(null);
    void attempt(async () => {
      await api.activateSurface(AGENT_SURFACE_ID);
      await api.setMode("live");
    });
  };

  return (
    <div className="surface-tab">
      <div className="surface-tab-main">
        {current ? (
          <PadGrid page={current.page} frame={frame} mode={current.state.mode} device={device} />
        ) : (
          <div className="panel">
            <h2>Agent Control surface</h2>
            <p className="hint">
              {exists
                ? "Another surface is showing on the Launchpad. Switch to Agent Control to see your agents on the pads."
                : "The Agent Control surface is not in this workspace. Resetting the workspace brings it back."}
            </p>
            <div className="row">
              <button type="button" className="btn primary" disabled={!exists} onClick={show}>
                Show on Launchpad
              </button>
            </div>
          </div>
        )}
        {current?.state.mode === "edit" && <p className="warning soft">Edit mode is on, so pads select instead of acting. Switch to Live to use them.</p>}
      </div>

      <aside className="surface-tab-side">
        <section className="field-group first">
          <h4>Try it</h4>
          <div className="row wrap">
            <button type="button" className="btn primary" onClick={() => void attempt(() => api.agents.runDemo())}>
              Run Agent Demo
            </button>
            <button type="button" className="btn" disabled={!hasDemo} onClick={() => void attempt(() => api.agents.resetDemo())}>
              Reset demo
            </button>
          </div>
          <p className="hint">
            Four simulated agents run side by side. One asks for approval, one can be paused, one fails so you can retry it. The virtual Launchpad and a connected
            Launchpad show the same thing as this window.
          </p>
        </section>

        <section className="field-group">
          <h4>Pad colours</h4>
          <ul className="legend">
            {agents.palette.map((p) => (
              <li key={p.status}>
                <span className={`dot ${p.animation !== "none" ? `anim-${p.animation}` : ""}`} style={{ background: p.color, ["--glow" as string]: p.color }} />
                <span>{STATUS_LABEL[p.status]}</span>
                <small>
                  {MEANING[p.status]}
                  {p.animation === "flash" ? " · flashes" : p.animation === "pulse" ? " · pulses" : ""}
                </small>
              </li>
            ))}
          </ul>
          {palette.size === 0 && <p className="hint">The palette arrives with the runtime.</p>}
          <p className="hint">Colour is never the only signal: a flashing pad and the status name in this window say the same thing.</p>
        </section>

        <section className="field-group">
          <h4>Layout</h4>
          <ul className="plain bullets hint-list">
            <li>Top row: one pad per session, coloured by status. Press one to select it.</li>
            <li>Second row: a marker under the selected session.</li>
            <li>Approve and Reject (large) flash while the selected session waits for you. Pause, Resume, Retry and Stop light up only when they can work.</li>
            <li>The Continue pad and the cyan pads start a task on the selected agent.</li>
          </ul>
        </section>

        {current && (
          <div className="row">
            <button type="button" className="link" onClick={() => setView("surface")}>
              Edit this surface
            </button>
          </div>
        )}
      </aside>
    </div>
  );
}
