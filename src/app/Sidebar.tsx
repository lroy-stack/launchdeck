import { useState } from "react";
import { api } from "../lib/ipc";
import { useStore, type View } from "../stores/runtime";
import { ContextControl } from "../features/context/ContextControl";

const NAV: Array<{ view: View; label: string; glyph: string }> = [
  { view: "home", label: "Home", glyph: "⌂" },
  { view: "agents", label: "Agents", glyph: "◉" },
  { view: "surface", label: "Surfaces", glyph: "▦" },
  { view: "plugins", label: "Plugins", glyph: "⧉" },
  { view: "settings", label: "Settings", glyph: "⚙" },
];

export function Sidebar() {
  const view = useStore((s) => s.view);
  const setView = useStore((s) => s.setView);
  const workspace = useStore((s) => s.workspace);
  const state = useStore((s) => s.state);
  const attempt = useStore((s) => s.attempt);
  const select = useStore((s) => s.select);
  const pending = useStore((s) => s.agents.summary.pendingApprovals);
  const failed = useStore((s) => s.agents.summary.failedSessions);
  const requests = useStore((s) => s.permissionRequests);
  const [creating, setCreating] = useState<string | null>(null);

  const hardware = state?.devices.find((d) => d.identity.kind === "hardware" && d.connected);
  const device = hardware ?? state?.devices.find((d) => d.connected) ?? null;

  const open = (surfaceId: string) => {
    select(null);
    setView("surface");
    void attempt(() => api.activateSurface(surfaceId));
  };

  return (
    <aside className="sidebar">
      <div className="brand">
        <span className="brand-mark" aria-hidden>
          <i /> <i /> <i /> <i />
        </span>
        <b>LaunchDeck</b>
      </div>

      <nav aria-label="Main">
        {NAV.map((n) => (
          <button key={n.view} type="button" className={`nav-item${view === n.view ? " on" : ""}`} onClick={() => setView(n.view)}>
            <span className="glyph">{n.glyph}</span>
            {n.label}
            {n.view === "agents" && pending > 0 && (
              <b className="badge attention" title={`${pending} waiting for your decision`} aria-label={`${pending} approvals pending`}>
                {pending}
              </b>
            )}
            {n.view === "plugins" && requests > 0 && (
              <b className="badge attention" title="A plugin is waiting for your permission" aria-label={`${requests} permission ${requests === 1 ? "request" : "requests"} waiting`}>
                {requests}
              </b>
            )}
            {n.view === "agents" && pending === 0 && failed > 0 && (
              <b className="badge bad" title={`${failed} failed`} aria-label={`${failed} failed sessions`}>
                {failed}
              </b>
            )}
          </button>
        ))}
        <div className="nav-item disabled" aria-disabled title="Preview: not available yet">
          <span className="glyph">◈</span>
          Marketplace
          <em>Preview</em>
        </div>
      </nav>

      <button type="button" className="device-card" onClick={() => setView("settings")} title="Manage devices">
        <span className={`status-dot ${device ? "ok" : ""}`} />
        <span className="device-text">
          <b>{device ? (device.identity.kind === "hardware" ? "Launchpad X" : "Virtual Launchpad X") : "No device"}</b>
          <small>{device ? (device.identity.kind === "hardware" ? "Connected" : "Simulator · connected") : "Disconnected"}</small>
        </span>
      </button>

      <ContextControl />

      <div className="side-section">
        <div className="side-title">
          <span>Surfaces</span>
          <button type="button" className="icon-btn" aria-label="New surface" onClick={() => setCreating("")}>
            +
          </button>
        </div>
        <ul className="surface-list">
          {workspace?.surfaces.map((s) => {
            const on = view === "surface" && state?.activeSurfaceId === s.id;
            return (
              <li key={s.id}>
                <button type="button" className={`surface-item${on ? " on" : ""}`} onClick={() => open(s.id)}>
                  <span className="surface-icon">{s.icon ?? "▦"}</span>
                  <span>{s.name}</span>
                </button>
              </li>
            );
          })}
        </ul>
        {creating !== null && (
          <form
            className="inline-form"
            onSubmit={(e) => {
              e.preventDefault();
              const name = creating.trim();
              setCreating(null);
              if (!name) return;
              void attempt(async () => {
                const id = await api.createSurface(name);
                open(id);
                await api.setMode("edit");
              });
            }}
          >
            <input autoFocus placeholder="Surface name" value={creating} onChange={(e) => setCreating(e.target.value)} onBlur={() => setCreating(null)} />
          </form>
        )}
      </div>
    </aside>
  );
}
