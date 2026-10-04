// Auto Context where it can always be seen: in the sidebar. On or off in one
// click, and when a surface was chosen by hand, the way back.

import { api } from "../../lib/ipc";
import { useStore } from "../../stores/runtime";
import { MODE } from "./text";

export function ContextControl() {
  const context = useStore((s) => s.context);
  const attempt = useStore((s) => s.attempt);
  const setView = useStore((s) => s.setView);
  if (!context) return null;
  const { settings, mode } = context;

  return (
    <div className={`context-control ${mode}`} role="group" aria-label="Auto Context">
      <button type="button" className="context-open" onClick={() => setView("settings")} title="Rules and what is in front now">
        <b>Auto Context</b>
        <small>{MODE[mode]}</small>
      </button>
      <button
        type="button"
        role="switch"
        aria-checked={settings.auto}
        aria-label="Auto Context"
        className={`switch${settings.auto ? " on" : ""}`}
        onClick={() => void attempt(() => api.context.update({ ...settings, auto: !settings.auto }))}
      >
        <i />
      </button>
      {mode === "manual" && (
        <button type="button" className="btn small primary context-resume" onClick={() => void attempt(() => api.context.resume())}>
          Resume Auto
        </button>
      )}
    </div>
  );
}
