import { useState } from "react";
import { api } from "../../lib/ipc";
import { useStore } from "../../stores/runtime";
import { ContextPanel } from "../context/ContextPanel";
import { DevicesPanel } from "./DevicesPanel";

export function SettingsView() {
  const state = useStore((s) => s.state);
  const workspace = useStore((s) => s.workspace);
  const attempt = useStore((s) => s.attempt);
  const toast = useStore((s) => s.toast);
  const [source, setSource] = useState("codex");
  const [event, setEvent] = useState("task.started");
  const [confirmReset, setConfirmReset] = useState(false);
  if (!state || !workspace) return null;
  const ext = state.external;
  const prefs = workspace.preferences;

  return (
    <div className="page-scroll settings">
      <header className="page-head">
        <h1>Settings</h1>
      </header>

      <DevicesPanel />

      <ContextPanel />

      <section className="panel">
        <div className="panel-head">
          <div>
            <h2>External events</h2>
            <p className="hint">
              Local tools can send events to LaunchDeck over WebSocket. It only listens on <code>127.0.0.1</code> and can publish state, never run commands.
            </p>
          </div>
          <span className={`pill ${ext.listening ? "live" : "warn"}`}>{ext.listening ? "Listening" : ext.enabled ? "Not listening" : "Disabled"}</span>
        </div>
        <pre className="snippet">{`ws://127.0.0.1:${ext.port}

{"type":"event","source":"codex","event":"task.started"}
{"type":"event","source":"codex","event":"task.completed"}`}</pre>
        {ext.error && <p className="field-error">{ext.error}</p>}
        <div className="row wrap">
          <label className="field">
            <span>Source</span>
            <input value={source} onChange={(e) => setSource(e.target.value)} />
          </label>
          <label className="field">
            <span>Event</span>
            <input value={event} onChange={(e) => setEvent(e.target.value)} />
          </label>
          <button
            type="button"
            className="btn"
            onClick={() =>
              void attempt(() => api.sendEvent(source.trim(), event.trim())).then((ok) => ok && toast(`Sent ${source}: ${event}`))
            }
          >
            Send test event
          </button>
        </div>
        <label className="check">
          <input
            type="checkbox"
            checked={prefs.externalEventsEnabled}
            onChange={(e) => void attempt(() => api.updatePreferences({ ...prefs, externalEventsEnabled: e.target.checked }))}
          />
          Accept external events (applies on next launch)
        </label>
      </section>

      <section className="panel">
        <h2>Security</h2>
        <ul className="plain bullets">
          <li>Shell commands and HTTP requests only run from pads you configured, and are marked <span className="pill warn">Privileged</span> in the editor.</li>
          <li>Events received over the WebSocket can change state. They cannot start actions or send commands.</li>
          <li>The event socket accepts only local connections from this machine.</li>
        </ul>
      </section>

      <section className="panel">
        <h2>Workspace</h2>
        <p className="hint">Surfaces are stored as local JSON in the app data folder, with a backup if a file is damaged.</p>
        {confirmReset ? (
          <div className="row">
            <span>Replace everything with the starter surfaces?</span>
            <button
              type="button"
              className="btn danger"
              onClick={() => {
                setConfirmReset(false);
                void attempt(() => api.resetWorkspace()).then((ok) => ok && toast("Workspace reset"));
              }}
            >
              Reset
            </button>
            <button type="button" className="btn" onClick={() => setConfirmReset(false)}>
              Cancel
            </button>
          </div>
        ) : (
          <button type="button" className="btn" onClick={() => setConfirmReset(true)}>
            Reset to starter surfaces
          </button>
        )}
      </section>
    </div>
  );
}
