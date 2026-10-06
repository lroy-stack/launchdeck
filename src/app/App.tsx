import { useEffect } from "react";
import { Sidebar } from "./Sidebar";
import { StartupNotices } from "./StartupNotices";
import { AgentsView } from "../features/agents/AgentsView";
import { ExportView } from "../features/artifacts/ExportView";
import { ImportView } from "../features/artifacts/ImportView";
import { FirstRun } from "../features/onboarding/FirstRun";
import { HomeView } from "../features/home/HomeView";
import { SettingsView } from "../features/settings/SettingsView";
import { PluginsView } from "../features/plugins/PluginsView";
import { SurfaceView } from "../features/surfaces/SurfaceView";
import { ProblemText } from "../components/Problem";
import { useStore } from "../stores/runtime";

export function App() {
  const ready = useStore((s) => s.ready);
  const fatal = useStore((s) => s.fatal);
  const view = useStore((s) => s.view);
  const exportSeq = useStore((s) => s.exportStart.seq);
  const preview = useStore((s) => s.preview);
  const firstRun = useStore((s) => s.workspace?.preferences.onboardingDone === false);
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
        <ProblemText problem={fatal} className="" />
      </div>
    );
  }
  if (!ready) return <div className="splash">Starting runtime…</div>;

  // The first run takes the place of the sidebar and the views; the notices stay where they are, so a
  // banner dismissed there does not come back with the shell.
  return (
    <div className={firstRun ? "shell first-run-shell" : "shell"}>
      {!firstRun && <Sidebar />}
      <div className="content">
        {preview && <div className="preview-bar">Browser preview · simulated runtime. Run the desktop app for the real thing.</div>}
        <StartupNotices />
        {firstRun ? (
          <FirstRun />
        ) : (
          <>
            {view === "overview" && <HomeView />}
            {view === "agents" && <AgentsView />}
            {view === "surface" && <SurfaceView />}
            {view === "plugins" && <PluginsView />}
            {view === "settings" && <SettingsView />}
            {view === "import" && <ImportView />}
            {view === "export" && <ExportView key={exportSeq} />}
          </>
        )}
      </div>
      <div className="toasts" role="status" aria-live="polite" aria-label="Notifications">
        {toasts.map((t) => (
          <div key={t.id} className={`toast ${t.tone}`}>
            <ProblemText problem={t} className="" />
            <button type="button" className="toast-dismiss" aria-label="Dismiss" onClick={() => dismiss(t.id)}>
              ×
            </button>
          </div>
        ))}
      </div>
    </div>
  );
}
