// Export surfaces as a Surface file or a Pack. The person chooses what goes
// in; every change is sent whole to Rust (`artifacts_export_preview`), which
// proposes what was left empty, says what the file holds and what is wrong;
// Save writes exactly that request (`artifacts_export_save`) into the exports
// folder. The webview never sends a path.

import { useEffect, useMemo, useRef, useState } from "react";
import { api, errorMessage } from "../../lib/ipc";
import type { ArtifactLimits, ExportPreview, ExportSaved } from "../../lib/schema";
import { useStore } from "../../stores/runtime";
import { ExportSummary, MetadataFields, RuleChoices, SurfaceChoice, ValueChoices } from "./ExportParts";
import { buildRequest, emptyForm, type ExportForm } from "./request";
import { TYPING_PAUSE_MS, useAsk } from "./useAsk";

export function ExportView() {
  const start = useStore((s) => s.exportStart);
  const workspace = useStore((s) => s.workspace);
  const attempt = useStore((s) => s.attempt);
  const [form, setForm] = useState<ExportForm>(() => emptyForm(start.surfaceIds));
  const [limits, setLimits] = useState<ArtifactLimits | null>(null);
  const [preview, setPreview] = useState<ExportPreview | null>(null);
  const [saved, setSaved] = useState<ExportSaved | null>(null);
  const [saveError, setSaveError] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);
  const { send, pending } = useAsk(api.artifacts.exportPreview, setPreview);
  const request = useMemo(() => buildRequest(form), [form]);
  const opened = useRef(request);

  useEffect(() => {
    void attempt(async () => setLimits(await api.artifacts.limits()));
  }, [attempt]);
  useEffect(() => send(opened.current), [send]);

  const update = (next: ExportForm, typed = false) => {
    setForm(next);
    setSaved(null);
    setSaveError(null);
    send(buildRequest(next), typed ? TYPING_PAUSE_MS : 0);
  };

  const save = async () => {
    setSaving(true);
    setSaved(null);
    setSaveError(null);
    try {
      setSaved(await api.artifacts.exportSave(request));
    } catch (e) {
      setSaveError(errorMessage(e));
    } finally {
      setSaving(false);
    }
  };

  const blocked = !preview || preview.errors.length > 0 || pending || saving;
  return (
    <div className="page-scroll artifacts">
      <header className="page-head">
        <h1>Export</h1>
        <p>
          Save surfaces as a file to share: one surface as a Surface file, or several, with context rules and questions, as a Pack. You see what goes into it
          before it is saved.
        </p>
      </header>
      <SurfaceChoice workspace={workspace} form={form} update={update} />
      <MetadataFields form={form} preview={preview} limits={limits} update={update} />
      <RuleChoices form={form} preview={preview} limits={limits} update={update} />
      {preview && preview.parameters.length > 0 && <ValueChoices form={form} preview={preview} workspace={workspace} limits={limits} update={update} />}
      {preview && <ExportSummary preview={preview} kind={request.kind} />}
      <footer className="import-actions">
        {saved && (
          <p className="row wrap" role="status">
            <span>
              Saved {saved.fileName} in {saved.folder}.
            </span>
            <button type="button" className="btn small" onClick={() => void attempt(() => api.artifacts.showExport())}>
              Show
            </button>
          </p>
        )}
        {saveError && (
          <p className="field-error" role="alert">
            {saveError}
          </p>
        )}
        <div className="row">
          <span className="hint grow">It goes in your Downloads folder, under another name if that one is taken. Nothing is overwritten.</span>
          <button type="button" className="btn primary" disabled={blocked} onClick={() => void save()}>
            {saving ? "Saving…" : "Save"}
          </button>
        </div>
      </footer>
    </div>
  );
}
