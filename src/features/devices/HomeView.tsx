import { api } from "../../lib/ipc";
import { plural } from "../../lib/format";
import { useStore } from "../../stores/runtime";
import { ActivityFeed } from "../surfaces/ActivityFeed";

const BLURBS: Record<string, string> = {
  "agent-control": "Watch every agent session at a glance. Approve, reject, pause, retry and start tasks from the pads.",
  development: "Git, builds and editor shortcuts on safe demo commands.",
  productivity: "Meetings, notes, focus and volume.",
  accessibility: "Fewer, larger, high-contrast targets for easier pressing.",
  "smart-home": "Template for Home Assistant style webhooks. Edit the URLs to make it yours.",
};

export function HomeView() {
  const workspace = useStore((s) => s.workspace);
  const state = useStore((s) => s.state);
  const events = useStore((s) => s.events);
  const preview = useStore((s) => s.preview);
  const setView = useStore((s) => s.setView);
  const attempt = useStore((s) => s.attempt);
  const select = useStore((s) => s.select);
  const showAgents = useStore((s) => s.showAgents);
  const summary = useStore((s) => s.agents.summary);
  if (!workspace || !state) return null;

  const device = state.devices.find((d) => d.connected);
  const first = !workspace.preferences.onboardingDone;

  const open = async (id: string, edit = false) => {
    select(null);
    const ok = await attempt(async () => {
      await api.activateSurface(id);
      await api.setMode(edit ? "edit" : "live");
      if (first) await api.updatePreferences({ ...workspace.preferences, onboardingDone: true });
    });
    if (ok) setView("surface");
  };

  // The flagship path: start the demo and look at all four agents at once.
  const tryAgents = async () => {
    const ok = await attempt(async () => {
      await api.agents.runDemo();
      await api.activateSurface("agent-control");
      await api.setMode("live");
      if (first) await api.updatePreferences({ ...workspace.preferences, onboardingDone: true });
    });
    if (ok) showAgents("overview");
  };

  return (
    <div className="page-scroll home">
      <section className="hero">
        <span className="eyebrow">Programmable physical interface runtime</span>
        <h1>Turn any MIDI grid into a programmable surface for your digital life.</h1>
        <p>
          LaunchDeck turns MIDI grid controllers into programmable physical interfaces for your computer. Control apps. Automate workflows. Build accessible interfaces.
          Monitor and orchestrate AI agents. No hardware needed to try it: the virtual Launchpad X behaves like the real thing.
        </p>
        <div className="row wrap">
          <button type="button" className="btn primary large" onClick={() => void tryAgents()}>
            Run the Agent Demo
          </button>
          <button type="button" className="btn large" onClick={() => void open("agent-control")}>
            Open Agent Control
          </button>
          <button type="button" className="btn large" onClick={() => setView("settings")}>
            {device?.identity.kind === "hardware" ? "Device settings" : "Connect a Launchpad X"}
          </button>
        </div>
        {preview && <p className="hint">Browser preview: the Rust runtime is not running, so actions are simulated.</p>}
      </section>

      <section className="flagship" aria-label="Agent Control">
        <div>
          <span className="eyebrow">Flagship</span>
          <h2>Agent Control</h2>
          <p>
            A physical human control surface for autonomous and semi-autonomous agents. Observe every session at a glance, intervene with one press, orchestrate new
            tasks. Works with any agent through adapters and a local bridge.
          </p>
        </div>
        <div className="flagship-side">
          <span className="hint">
            {summary.agentsTotal === 0
              ? "No agents connected yet."
              : `${summary.agentsOnline}/${summary.agentsTotal} online · ${summary.activeSessions} active · ${summary.pendingApprovals} waiting for you`}
          </span>
          <button type="button" className="btn" onClick={() => showAgents("overview")}>
            Open Agents
          </button>
        </div>
      </section>

      {first && (
        <section className="steps" aria-label="Getting started">
          <div>
            <b>1</b>
            <span>Pick a surface below.</span>
          </div>
          <div>
            <b>2</b>
            <span>Press pads on the virtual Launchpad in Live mode.</span>
          </div>
          <div>
            <b>3</b>
            <span>Switch to Edit to change a pad. Connect a real Launchpad X any time.</span>
          </div>
        </section>
      )}

      <section>
        <h2>Starter surfaces</h2>
        <div className="cards">
          {workspace.surfaces.map((s) => (
            <article key={s.id} className="card">
              <header>
                <span className="card-icon">{s.icon ?? "▦"}</span>
                <h3>{s.name}</h3>
              </header>
              <p>{BLURBS[s.id] ?? (s.description || "Custom surface.")}</p>
              <footer>
                <small>
                  {plural(s.pages.length, "page")} · {plural(s.pages.reduce((n, p) => n + p.controls.length, 0), "control")}
                </small>
                <div className="row">
                  <button type="button" className="btn small" onClick={() => void open(s.id, true)}>
                    Edit
                  </button>
                  <button type="button" className="btn small primary" onClick={() => void open(s.id)}>
                    Open
                  </button>
                </div>
              </footer>
            </article>
          ))}
        </div>
      </section>

      <section className="two-col">
        <div>
          <h2>Devices</h2>
          <ul className="device-list">
            {state.devices.map((d) => (
              <li key={d.identity.id}>
                <span className={`status-dot ${d.connected ? "ok" : ""}`} />
                <div>
                  <b>{d.identity.name}</b>
                  <small>
                    {d.identity.kind === "virtual" ? "Simulator" : "Hardware"} · {d.connected ? "connected" : "available"}
                  </small>
                </div>
              </li>
            ))}
          </ul>
        </div>
        <div>
          <h2>Recent activity</h2>
          <ActivityFeed events={events.filter((e) => e.type !== "control.pressed")} limit={8} empty="Events from pads, plugins and external tools show up here." />
        </div>
      </section>
    </div>
  );
}
