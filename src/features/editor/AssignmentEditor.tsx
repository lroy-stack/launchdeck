import { useEffect, useState } from "react";
import { errorMessage } from "../../lib/ipc";
import { api } from "../../lib/ipc";
import { issuesByParam, resolveAssignment, unavailableNote, type Args } from "../../lib/actions";
import { DRAG_TYPE, SLOT_LABEL, type Slot } from "../../lib/assign";
import type { ActionDefinition, Assignment, Issue, Surface } from "../../lib/schema";
import { definitionId } from "../../lib/schema";
import { useStore } from "../../stores/runtime";
import { ActionGlyph, PrivilegedTag, SourceTag, UnavailableTag } from "../library/parts";
import { ParamForm } from "../../components/forms/ParamForm";

interface Props {
  slot: Slot;
  value: Assignment | null | undefined;
  onChange: (next: Assignment | null) => void;
  /** An action waiting for the person to confirm that it replaces the current one. */
  pending: ActionDefinition | null;
  replaceWarning: string | null;
  onConfirmReplace: () => void;
  onCancelReplace: () => void;
  /** Something was dropped on this slot from the Library. */
  onDropAction: (definitionId: string) => void;
  issues: Issue[];
  surfaces: Surface[];
  currentSurface: Surface;
}

type Run = { state: "idle" } | { state: "running" } | { state: "done"; text: string } | { state: "failed"; text: string };

const hasAction = (e: React.DragEvent): boolean => Array.from(e.dataTransfer.types).includes(DRAG_TYPE);

/**
 * One slot of a control (On press, On long press): what is assigned, its
 * settings drawn from the action's schema, whether it can run, and a way to
 * try it. It edits a draft; nothing is saved until the Inspector saves.
 */
export function AssignmentEditor({ slot, value, onChange, pending, replaceWarning, onConfirmReplace, onCancelReplace, onDropAction, issues, surfaces, currentSurface }: Props) {
  const index = useStore((s) => s.catalog);
  const focusLibrary = useStore((s) => s.focusLibrary);
  const setView = useStore((s) => s.setView);
  const dragging = useStore((s) => s.dragging);
  const [over, setOver] = useState(false);
  const [run, setRun] = useState<Run>({ state: "idle" });

  // What a test said belongs to the settings it ran with.
  const key = value ? JSON.stringify(value) : "";
  useEffect(() => setRun({ state: "idle" }), [key]);

  const resolution = value ? resolveAssignment(index, value) : null;
  const def = resolution && resolution.kind !== "missing" ? resolution.definition : null;
  const errors = issuesByParam(issues);
  const general = errors.get("") ?? [];
  const blocked = issues.length > 0 || resolution?.kind === "missing" || (resolution?.kind === "unavailable" && resolution.cause === "provider");

  const test = async () => {
    if (!value) return;
    setRun({ state: "running" });
    try {
      const result = await api.runAction(value);
      const shown = result === null || result === undefined ? "" : JSON.stringify(result);
      setRun({ state: "done", text: shown && shown !== "{}" ? `Done. It returned ${shown.length > 90 ? `${shown.slice(0, 89)}…` : shown}` : "Done." });
    } catch (e) {
      setRun({ state: "failed", text: errorMessage(e) });
    }
  };

  return (
    <section
      className={`field-group slot${dragging ? " drop-ready" : ""}${over ? " over" : ""}`}
      aria-label={SLOT_LABEL[slot]}
      onDragOver={(e) => {
        if (!hasAction(e)) return;
        e.preventDefault();
        e.dataTransfer.dropEffect = "copy";
        setOver(true);
      }}
      onDragLeave={() => setOver(false)}
      onDrop={(e) => {
        setOver(false);
        if (!hasAction(e)) return;
        e.preventDefault();
        onDropAction(e.dataTransfer.getData(DRAG_TYPE));
      }}
    >
      <div className="field-head">
        <h4>{SLOT_LABEL[slot]}</h4>
        {value && (
          <button type="button" className="link" onClick={() => onChange(null)}>
            Remove
          </button>
        )}
      </div>

      {pending && (
        <div
          className="notice replace"
          role="group"
          aria-label="Confirm replacing the action"
          onKeyDown={(e) => {
            if (e.key === "Escape") onCancelReplace();
          }}
        >
          <p>{replaceWarning}</p>
          <div className="row">
            <button type="button" className="btn small primary" autoFocus onClick={onConfirmReplace}>
              Replace
            </button>
            <button type="button" className="btn small" onClick={onCancelReplace}>
              Keep current
            </button>
          </div>
        </div>
      )}

      {!value && (
        <div className="slot-empty">
          <p>No action. Choose one in the Action Library, or drop it here.</p>
          <button type="button" className="btn small" onClick={focusLibrary}>
            Browse the library
          </button>
        </div>
      )}

      {value && def && (
        <>
          <div className="slot-card">
            <ActionGlyph def={def} />
            <div className="slot-card-text">
              <b>{def.name}</b>
              <div className="tags">
                <SourceTag index={index} def={def} />
                <PrivilegedTag def={def} />
                <UnavailableTag def={def} />
              </div>
            </div>
            <button type="button" className="link" onClick={focusLibrary}>
              Change
            </button>
          </div>
          <p className="hint">{def.description}</p>
          {def.privileged && (
            <div className="warning" role="note">
              <b>Privileged.</b> This runs on your computer with your permissions, from this pad only, never from a remote event. Review the settings before saving.
            </div>
          )}
          {resolution?.kind === "unavailable" && (
            <div className="notice unavailable" role="status">
              <b>Unavailable now.</b> {unavailableNote(def)}{" "}
              {resolution.cause === "provider" ? (
                <>
                  The assignment is kept and works again when this is fixed in{" "}
                  <button type="button" className="link" onClick={() => setView("plugins")}>
                    Plugins
                  </button>
                  .
                </>
              ) : (
                "You can still assign it; it runs when the situation allows."
              )}
            </div>
          )}
          {general.map((m, i) => (
            <p key={i} className="field-error">
              {m}
            </p>
          ))}
          <ParamForm params={def.params} args={value.args as Args} onChange={(args) => onChange({ ...value, args })} errors={errors} surfaces={surfaces} currentSurface={currentSurface} />
        </>
      )}

      {value && resolution?.kind === "missing" && (
        <>
          <div className="slot-card">
            <span className="lib-glyph" aria-hidden="true">
              ?
            </span>
            <div className="slot-card-text">
              <b>{definitionId(value)}</b>
              <div className="tags">
                <span className="tag unavailable">Unavailable</span>
              </div>
            </div>
          </div>
          <div className="notice unavailable" role="status">
            <b>Not available.</b> {resolution.reason}
          </div>
          {Object.keys(value.args).length > 0 && (
            <dl className="saved-args" aria-label="Saved settings">
              {Object.entries(value.args).map(([k, v]) => (
                <div key={k}>
                  <dt>{k}</dt>
                  <dd>{typeof v === "string" ? v : JSON.stringify(v)}</dd>
                </div>
              ))}
            </dl>
          )}
        </>
      )}

      {value && (
        <div className="row test-row">
          <button type="button" className="btn small" disabled={blocked || run.state === "running"} onClick={() => void test()}>
            {run.state === "running" ? "Running…" : "Test run"}
          </button>
          <span className={`run-note ${run.state}`} role="status">
            {run.state === "idle" && (blocked ? "Fix the settings above to test." : "Runs it now, as if the pad were pressed. Nothing is saved.")}
            {run.state === "running" && "Running…"}
            {run.state === "done" && run.text}
            {run.state === "failed" && `Failed: ${run.text}`}
          </span>
        </div>
      )}
    </section>
  );
}
