// The first run: one screen, shown instead of the shell while the onboarding flag is off.
// It says which device is there, offers a starter or a Pack to import (the import view, which the app shows
// in its place and which sets the flag once the import is applied), and sets the flag last. Starting goes through
// the same activation as a click in the sidebar; nothing else is configured on the person's behalf
// (no demo, no Auto Context rule, no surface removed).

import { useId, useRef, useState } from "react";
import { api, errorMessage } from "../../lib/ipc";
import type { Surface } from "../../lib/schema";
import { primaryDeviceOf, useStore } from "../../stores/runtime";
import { DeviceStatusLine } from "../../components/DeviceStatusLine";

interface Purpose {
  label: string;
  /** The starter it opens; null builds a new surface. */
  surfaceId: string | null;
  /** Its pads send keystrokes, which macOS asks permission for. */
  pressesKeys?: boolean;
}

const PURPOSES: Purpose[] = [
  { label: "Productivity", surfaceId: "productivity", pressesKeys: true },
  { label: "Agent Control", surfaceId: "agent-control" },
  { label: "Development", surfaceId: "development" },
  { label: "Accessibility", surfaceId: "accessibility", pressesKeys: true },
  { label: "Build my own", surfaceId: null },
];

const BLANK_NAME = "My surface";
const BLANK_TEXT = "Start from a blank surface and give each pad an action from the Library.";
const KEYS_NOTE = "Some pads press keys for you; macOS will ask for Accessibility permission the first time.";

export function FirstRun() {
  const workspace = useStore((s) => s.workspace);
  const device = useStore(primaryDeviceOf);
  const attempt = useStore((s) => s.attempt);
  const [choice, setChoice] = useState<Purpose | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  // A surface Build my own already made, so trying again after a failure does not make another.
  const created = useRef<string | null>(null);
  const purposeId = useId();
  const surfaces = workspace?.surfaces ?? [];

  const surfaceOf = (p: Purpose): Surface | undefined => surfaces.find((s) => s.id === p.surfaceId);
  const chosen = choice?.surfaceId ? surfaceOf(choice) : undefined;
  const startLabel = !choice ? "Start" : choice.surfaceId ? `Start with ${chosen?.name ?? choice.label}` : "Start with a blank surface";

  /** Runs the steps, then sets the flag; a failure keeps this screen up with the error. */
  const finish = async (steps: () => Promise<void>) => {
    setBusy(true);
    setError(null);
    try {
      await steps();
      await api.setOnboardingDone(true);
    } catch (e) {
      setError(errorMessage(e));
      setBusy(false);
    }
  };

  const start = () => {
    if (!choice) return;
    void finish(async () => {
      const { select, setView, focusLibrary } = useStore.getState();
      select(null);
      if (choice.surfaceId) {
        await api.activateSurface(choice.surfaceId);
        await api.setMode("live");
        setView("surface");
        return;
      }
      created.current ??= await api.createSurface(BLANK_NAME);
      await api.activateSurface(created.current);
      await api.setMode("edit");
      setView("surface");
      focusLibrary();
    });
  };

  return (
    <div className="page-scroll first-run">
      <header>
        <span className="eyebrow">First run</span>
        <h1>Welcome to LaunchDeck</h1>
        <p>Pick what your pads are for. You can change everything later, and the other starters stay in the sidebar.</p>
      </header>

      <section aria-labelledby={`${purposeId}-device`}>
        <h2 id={`${purposeId}-device`}>Your Launchpad</h2>
        <div className="row first-run-device">
          <div className="device-card">
            <DeviceStatusLine device={device} />
          </div>
          <button type="button" className="btn" onClick={() => void attempt(() => api.rescanDevices())}>
            Look again
          </button>
        </div>
        <p className="hint">No Launchpad? The simulator on screen works the same way, and a Launchpad can be connected any time.</p>
      </section>

      <section>
        <h2 id={purposeId}>What is it for?</h2>
        <div role="radiogroup" aria-labelledby={purposeId} className="cards purposes">
          {PURPOSES.map((p, i) => {
            const surface = surfaceOf(p);
            const missing = p.surfaceId !== null && !surface;
            const on = choice === p;
            const id = `${purposeId}-${i}`;
            return (
              <label key={p.label} className={`card purpose${on ? " on" : ""}${missing ? " missing" : ""}`}>
                <header>
                  <input
                    type="radio"
                    name={purposeId}
                    checked={on}
                    disabled={missing || busy}
                    aria-labelledby={`${id}-name`}
                    aria-describedby={`${id}-text`}
                    onChange={() => setChoice(p)}
                  />
                  <span className="card-icon" aria-hidden>
                    {p.surfaceId ? (surface?.icon ?? "▦") : "＋"}
                  </span>
                  <h3 id={`${id}-name`}>{p.label}</h3>
                </header>
                <p id={`${id}-text`}>{p.surfaceId ? (surface?.description ?? "This starter is not in your workspace any more.") : BLANK_TEXT}</p>
                {p.pressesKeys && !missing && <p className="hint">{KEYS_NOTE}</p>}
              </label>
            );
          })}
        </div>
      </section>

      {error && (
        <p className="first-run-error" role="alert">
          {error}
        </p>
      )}

      <div className="row">
        <button type="button" className="btn primary large" disabled={!choice || busy} onClick={start}>
          {startLabel}
        </button>
        <button type="button" className="btn large" disabled={busy} onClick={() => void finish(async () => undefined)}>
          Skip
        </button>
        <button type="button" className="btn large" disabled={busy} onClick={() => useStore.getState().setView("import")}>
          Import a Pack instead
        </button>
      </div>
    </div>
  );
}
