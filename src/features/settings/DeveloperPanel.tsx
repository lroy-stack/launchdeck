import { useState } from "react";
import { api } from "../../lib/ipc";
import type { AppInfo } from "../../lib/schema";
import { useStore } from "../../stores/runtime";

/**
 * Settings > Developer, closed until opened: the external events WebSocket and its
 * test sender, the data folder, and the welcome screen again. Its tools are only
 * drawn while it is open.
 */
export function DeveloperPanel({ info }: { info: AppInfo | null }) {
  const [open, setOpen] = useState(false);
  return (
    <section className="panel" aria-labelledby="developer-title">
      <details className="developer" onToggle={(e) => setOpen(e.currentTarget.open)}>
        <summary>
          <h2 id="developer-title">Developer</h2>
          <span className="hint">The external events socket, the data folder and the welcome screen.</span>
        </summary>
        {open && <DeveloperTools info={info} />}
      </details>
    </section>
  );
}

function DeveloperTools({ info }: { info: AppInfo | null }) {
  const attempt = useStore((s) => s.attempt);
  return (
    <div className="developer-tools">
      <ExternalEvents />
      <div className="field-group">
        <h4>Data folder</h4>
        <p className="hint">The workspace, its backups, plugins and settings.</p>
        <div className="row wrap">
          {info && <code className="grow">{info.dataFolder}</code>}
          <button type="button" className="btn small" onClick={() => void attempt(() => api.revealDataFolder())}>
            Show data folder
          </button>
        </div>
      </div>
      <div className="field-group">
        <h4>Welcome</h4>
        <p className="hint">Shows the first screen again, as on the first start. Your surfaces stay as they are.</p>
        <div>
          <button type="button" className="btn small" onClick={() => void attempt(() => api.setOnboardingDone(false))}>
            Show the welcome again
          </button>
        </div>
      </div>
    </div>
  );
}

function ExternalEvents() {
  const ext = useStore((s) => s.state?.external ?? null);
  const prefs = useStore((s) => s.workspace?.preferences ?? null);
  const attempt = useStore((s) => s.attempt);
  const toast = useStore((s) => s.toast);
  const [source, setSource] = useState("codex");
  const [event, setEvent] = useState("task.started");
  if (!ext || !prefs) return null;

  return (
    <div className="field-group">
      <div className="panel-head">
        <div>
          <h4>External events</h4>
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
          onClick={() => void attempt(() => api.sendEvent(source.trim(), event.trim())).then((ok) => ok && toast(`Sent ${source}: ${event}`))}
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
    </div>
  );
}
