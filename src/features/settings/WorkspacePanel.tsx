import { useState } from "react";
import { api } from "../../lib/ipc";
import { useStore } from "../../stores/runtime";

/**
 * Settings > Workspace: where surfaces are kept, the import and export views (as the sidebar opens them), and the
 * reset to the starter surfaces after a confirmation.
 */
export function WorkspacePanel() {
  const attempt = useStore((s) => s.attempt);
  const toast = useStore((s) => s.toast);
  const setView = useStore((s) => s.setView);
  const startExport = useStore((s) => s.startExport);
  const [confirmReset, setConfirmReset] = useState(false);

  return (
    <section className="panel" aria-labelledby="workspace-title">
      <h2 id="workspace-title">Workspace</h2>
      <p className="hint">Surfaces are stored as local JSON in the app data folder, with a backup if a file is damaged.</p>
      <div className="row">
        <button type="button" className="btn" onClick={() => setView("import")}>
          Import a Surface or Pack…
        </button>
        <button type="button" className="btn" onClick={() => startExport()}>
          Export…
        </button>
      </div>
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
  );
}
