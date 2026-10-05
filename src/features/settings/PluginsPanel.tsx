import { useEffect, useState } from "react";
import { api, errorMessage } from "../../lib/ipc";
import { plural } from "../../lib/format";
import type { SecretStoreStatus } from "../../lib/schema";
import { useStore } from "../../stores/runtime";
import { secretStoreSentence } from "./text";

/** Settings > Plugins & permissions: where secrets are kept, the plugins folder, and the way to the Plugins view. */
export function PluginsPanel() {
  const attempt = useStore((s) => s.attempt);
  const setView = useStore((s) => s.setView);
  const requests = useStore((s) => s.permissionRequests);
  const [store, setStore] = useState<SecretStoreStatus | null>(null);
  const [problem, setProblem] = useState<string | null>(null);

  useEffect(() => {
    let live = true;
    api.plugins
      .list()
      .then((overview) => live && setStore(overview.secretStore))
      .catch((e: unknown) => live && setProblem(errorMessage(e)));
    return () => {
      live = false;
    };
  }, []);

  return (
    <section className="panel" aria-labelledby="plugins-title">
      <div className="panel-head">
        <div>
          <h2 id="plugins-title">Plugins &amp; permissions</h2>
          <p className="hint">A plugin reaches the network only at the addresses you allow, one by one, on its card in Plugins. Nothing is allowed by itself.</p>
        </div>
        <button type="button" className="btn" onClick={() => setView("plugins")}>
          Open Plugins
        </button>
      </div>
      {store && <p>{secretStoreSentence(store)}</p>}
      {problem && <p className="field-error">{problem}</p>}
      {requests > 0 && <p className="hint">{plural(requests, "address waits", "addresses wait")} for your decision.</p>}
      <div className="row">
        <button type="button" className="btn" onClick={() => void attempt(() => api.plugins.revealFolder())}>
          Show plugins folder
        </button>
      </div>
    </section>
  );
}
