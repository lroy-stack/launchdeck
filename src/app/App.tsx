import { useEffect } from "react";
import { Sidebar } from "./Sidebar";
import { StartupNotices } from "./StartupNotices";
import { AgentsView } from "../features/agents/AgentsView";
import { HomeView } from "../features/home/HomeView";
import { SettingsView } from "../features/settings/SettingsView";
import { PluginsView } from "../features/plugins/PluginsView";
import { SurfaceView } from "../features/surfaces/SurfaceView";
import { useStore } from "../stores/runtime";

export function App() {
  const ready = useStore((s) => s.ready);
  const fatal = useStore((s) => s.fatal);
  const view = useStore((s) => s.view);
  const preview = useStore((s) => s.preview);
  const toasts = useStore((s) => s.toasts);
  const dismiss = useStore((s) => s.dismissToast);

  useEffect(() => {
    let off: (() => void) | undefined;
    let cancelled = false;
    void useStore
      .getState()
      .init()
      .then((fn) => {
        if (cancelled) fn();
        else off = fn;
      });
    return () => {
      cancelled = true;
      off?.();
    };
  }, []);

  if (fatal) {
    return (
      <div className="splash error">
        <h1>LaunchDeck could not start</h1>
        <pre>{fatal}</pre>
      </div>
    );
  }
  if (!ready) return <div className="splash">Starting runtime…</div>;

  return (
    <div className="shell">
      <Sidebar />
      <div className="content">
        {preview && <div className="preview-bar">Browser preview · simulated runtime. Run the desktop app for the real thing.</div>}
        <StartupNotices />
        {view === "home" && <HomeView />}
        {view === "agents" && <AgentsView />}
        {view === "surface" && <SurfaceView />}
        {view === "plugins" && <PluginsView />}
        {view === "settings" && <SettingsView />}
      </div>
      <div className="toasts" role="status" aria-live="polite">
        {toasts.map((t) => (
          <button key={t.id} type="button" className={`toast ${t.tone}`} onClick={() => dismiss(t.id)}>
            {t.text}
          </button>
        ))}
      </div>
    </div>
  );
}
