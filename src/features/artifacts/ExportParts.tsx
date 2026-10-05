// The parts of the export view: which surfaces, the file's words, the context
// rules, the values the person importing is asked for, and what Rust says the
// file holds. Each change goes back to Rust as a whole new request.

import { useState } from "react";
import { plural } from "../../lib/format";
import type { ArtifactKind, ArtifactLimits, ExportedRule, ExportPreview, Parameter, Workspace } from "../../lib/schema";
import { localSurface, padLabel, pageName, sameArg, withSurface, type AskedType, type ExportForm, type MetadataForm } from "./request";
import { ASK, ASKED_TYPE, KIND, REVIEW_KIND, size, strippedText, SYSTEMS, systemName } from "./text";

export type Update = (next: ExportForm, typed?: boolean) => void;

/** A label of at most the inputs' label length, when Rust said what that is. */
const fit = (label: string, limits: ArtifactLimits | null): string => (limits ? label.slice(0, limits.inputLabel).trimEnd() : label);

export function SurfaceChoice({ workspace, form, update }: { workspace: Workspace | null; form: ExportForm; update: Update }) {
  const order = workspace?.surfaces.map((s) => s.id) ?? [];
  return (
    <section className="panel">
      <h3>Surfaces</h3>
      <ul className="plain">
        {workspace?.surfaces.map((s) => (
          <li key={s.id}>
            <label className="check">
              <input type="checkbox" checked={form.surfaceIds.includes(s.id)} onChange={(e) => update(withSurface(form, order, s.id, e.currentTarget.checked))} />
              {s.name}
            </label>
            <span className="hint">{plural(s.pages.length, "page")}</span>
          </li>
        ))}
      </ul>
      <p className="hint">One surface and nothing else makes a Surface file. More surfaces, context rules or questions make a Pack.</p>
    </section>
  );
}

interface MetadataProps {
  form: ExportForm;
  preview: ExportPreview | null;
  limits: ArtifactLimits | null;
  update: Update;
}

/** The file's words. Left empty, the name, version and id are Rust's proposals, shown in grey. */
export function MetadataFields({ form, preview, limits, update }: MetadataProps) {
  const m = form.metadata;
  const set = (patch: Partial<MetadataForm>) => update({ ...form, metadata: { ...m, ...patch } }, true);
  const field = (label: string, key: keyof MetadataForm, max: number | undefined, placeholder?: string) => (
    <label className="field">
      <span>{label}</span>
      <input type="text" value={m[key]} maxLength={max} placeholder={placeholder} spellCheck={false} onChange={(e) => set({ [key]: e.currentTarget.value })} />
    </label>
  );
  return (
    <section className="panel">
      <h3>About the file</h3>
      <p className="hint">Left empty, the name, version and id are proposed for you; the proposals show in grey.</p>
      <div className="meta-grid">
        {field("Name", "name", limits?.name, preview?.manifest.name)}
        {field("Version", "version", limits?.version, preview?.manifest.version)}
        {field("Id", "id", limits?.id, preview?.manifest.id)}
        {field("Author", "author", limits?.authorName)}
        {field("Author's web address", "authorUrl", limits?.authorUrl, "https://")}
        {field("License", "license", limits?.license)}
        <label className="field wide">
          <span>Description</span>
          <textarea rows={2} value={m.description} maxLength={limits?.description} onChange={(e) => set({ description: e.currentTarget.value })} />
        </label>
      </div>
    </section>
  );
}

interface RulesProps {
  form: ExportForm;
  preview: ExportPreview | null;
  limits: ArtifactLimits | null;
  update: Update;
}

export function RuleChoices({ form, preview, limits, update }: RulesProps) {
  const rules = preview?.contextRules ?? [];
  return (
    <section className="panel">
      <h3>Context rules</h3>
      <label className="check">
        <input type="checkbox" checked={form.includeContextRules} onChange={(e) => update({ ...form, includeContextRules: e.currentTarget.checked })} />
        Include Auto Context rules
      </label>
      <p className="hint">The rules that show a chosen surface go with it. The person importing selects the ones to add, and their Auto Context stays as they have it.</p>
      {form.includeContextRules && preview && rules.length === 0 && <p className="hint">No rule shows these surfaces.</p>}
      {form.includeContextRules && preview && rules.length > 0 && (
        <ul className="plain">
          {rules.map((r) => (
            <RuleChoice key={r.rule} rule={r} form={form} preview={preview} limits={limits} update={update} />
          ))}
        </ul>
      )}
    </section>
  );
}

interface RuleChoiceProps {
  rule: ExportedRule;
  form: ExportForm;
  preview: ExportPreview;
  limits: ArtifactLimits | null;
  update: Update;
}

/** One rule: its application on each system, asked of the person importing or named on more systems. */
function RuleChoice({ rule, form, preview, limits, update }: RuleChoiceProps) {
  const surfaceName = preview.surfaces.find((s) => s.artifactId === rule.surface)?.name ?? rule.surface;
  const surface = localSurface(preview, rule.surface) ?? "";
  const asked = form.askedApps.find((a) => a.rule === rule.rule);
  const app = "input" in rule.application ? null : rule.application;
  const ask = (on: boolean) =>
    update({
      ...form,
      askedApps: on
        ? [...form.askedApps, { rule: rule.rule, label: fit(`Application for ${surfaceName}`, limits), surface }]
        : form.askedApps.filter((a) => a.rule !== rule.rule),
    });
  const relabel = (label: string) => update({ ...form, askedApps: form.askedApps.map((a) => (a.rule === rule.rule ? { ...a, label } : a)) }, true);
  return (
    <li className="stacked export-rule">
      <span>
        <b>{app?.name ?? "An application the person importing chooses"}</b> → {surfaceName}
        {rule.page ? ` (page ${rule.page})` : ""}
      </span>
      <label className="check">
        <input type="checkbox" checked={!!asked} onChange={(e) => ask(e.currentTarget.checked)} />
        Ask the person importing for the application
      </label>
      {asked ? (
        <label className="field">
          <span>Question</span>
          <input type="text" value={asked.label} maxLength={limits?.inputLabel} onChange={(e) => relabel(e.currentTarget.value)} />
        </label>
      ) : (
        app && <Identities rule={rule.rule} identities={app.identities} surface={surface} form={form} update={update} />
      )}
    </li>
  );
}

interface IdentitiesProps {
  rule: number;
  identities: Array<{ namespace: string; id: string }>;
  surface: string;
  form: ExportForm;
  update: Update;
}

/** The ids the application has, and one more for a system it has none on. */
function Identities({ rule, identities, surface, form, update }: IdentitiesProps) {
  const others = SYSTEMS.filter((s) => !identities.some((i) => i.namespace === s.namespace));
  const [namespace, setNamespace] = useState(others[0]?.namespace ?? "");
  const [id, setId] = useState("");
  const system = others.some((s) => s.namespace === namespace) ? namespace : (others[0]?.namespace ?? "");
  const added = (i: { namespace: string; id: string }) => form.extras.some((e) => e.rule === rule && e.namespace === i.namespace && e.id === i.id);
  const add = () => {
    update({ ...form, extras: [...form.extras, { rule, namespace: system, id: id.trim(), surface }] });
    setId("");
  };
  const remove = (i: { namespace: string; id: string }) =>
    update({ ...form, extras: form.extras.filter((e) => !(e.rule === rule && e.namespace === i.namespace && e.id === i.id)) });
  return (
    <>
      <ul className="plain identities">
        {identities.map((i) => (
          <li key={`${i.namespace}/${i.id}`}>
            <span>{systemName(i.namespace)}</span>
            <code>{i.id}</code>
            {added(i) && (
              <button type="button" className="link" onClick={() => remove(i)}>
                Remove
              </button>
            )}
          </li>
        ))}
      </ul>
      {others.length > 0 && (
        <div className="row wrap">
          <label className="field">
            <span>Other system</span>
            <select value={system} onChange={(e) => setNamespace(e.currentTarget.value)}>
              {others.map((s) => (
                <option key={s.namespace} value={s.namespace}>
                  {s.label}
                </option>
              ))}
            </select>
          </label>
          <label className="field grow">
            <span>Id on that system</span>
            <input type="text" value={id} spellCheck={false} placeholder={others.find((s) => s.namespace === system)?.example} onChange={(e) => setId(e.currentTarget.value)} />
          </label>
          <button type="button" className="btn small" disabled={!id.trim()} onClick={add}>
            Add
          </button>
        </div>
      )}
    </>
  );
}

interface ValuesProps {
  form: ExportForm;
  preview: ExportPreview;
  workspace: Workspace | null;
  limits: ArtifactLimits | null;
  update: Update;
}

/** Every text argument of the chosen pads, each of which can be asked of the person importing instead. */
export function ValueChoices({ form, preview, workspace, limits, update }: ValuesProps) {
  return (
    <section className="panel">
      <h3>Values on this computer</h3>
      <p className="hint">Pads carry these values into the file. Ask the person importing for any that differ on their computer: the value here then stays out of the file.</p>
      <ul className="plain values">
        {preview.parameters.map((p) => (
          <ValueChoice
            key={`${p.at.surfaceSourceId}/${p.at.pageId}/${p.at.controlId}/${p.at.slot}/${p.arg}`}
            parameter={p}
            form={form}
            preview={preview}
            workspace={workspace}
            limits={limits}
            update={update}
          />
        ))}
      </ul>
    </section>
  );
}

function ValueChoice({ parameter: p, form, preview, workspace, limits, update }: ValuesProps & { parameter: Parameter }) {
  const asked = form.asked.find((a) => sameArg(a, p));
  const suggestion = preview.suggestions.find((s) => sameArg(s, p));
  const label = padLabel(workspace, preview, p.at);
  const ask = (on: boolean) =>
    update({
      ...form,
      asked: on
        ? [
            ...form.asked,
            { at: p.at, arg: p.arg, type: suggestion?.suggestedType ?? "string", label: fit(`${label}: ${p.arg}`, limits), surface: localSurface(preview, p.at.surfaceSourceId) ?? "" },
          ]
        : form.asked.filter((a) => !sameArg(a, p)),
    });
  const change = (patch: { type?: AskedType; label?: string }, typed = false) =>
    update({ ...form, asked: form.asked.map((a) => (sameArg(a, p) ? { ...a, ...patch } : a)) }, typed);
  return (
    <li className="stacked">
      <span>
        <b>{label}</b>{" "}
        <span className="hint">
          {pageName(workspace, preview, p.at)} · {p.arg}
          {p.at.slot === "longPress" ? " (long press)" : ""}
        </span>
      </span>
      {p.value !== undefined ? <code className="value">{p.value}</code> : <span className="hint">Asked of the person importing.</span>}
      {suggestion && <span className="pill warn">looks like a path on this computer</span>}
      <label className="check">
        <input type="checkbox" checked={!!asked} onChange={(e) => ask(e.currentTarget.checked)} />
        {ASK}
      </label>
      {asked && (
        <div className="row wrap">
          <label className="field">
            <span>Asked as</span>
            <select value={asked.type} onChange={(e) => change({ type: e.currentTarget.value as AskedType })}>
              {(Object.keys(ASKED_TYPE) as AskedType[]).map((t) => (
                <option key={t} value={t}>
                  {ASKED_TYPE[t]}
                </option>
              ))}
            </select>
          </label>
          <label className="field grow">
            <span>Question</span>
            <input type="text" value={asked.label} maxLength={limits?.inputLabel} onChange={(e) => change({ label: e.currentTarget.value }, true)} />
          </label>
        </div>
      )}
    </li>
  );
}

/** What Rust says the file holds, what it leaves out, and what stops it from being saved. */
export function ExportSummary({ preview, kind }: { preview: ExportPreview; kind: ArtifactKind }) {
  const stripped = strippedText(preview.stripped);
  const questions = preview.manifest.inputs?.length ?? 0;
  // Each pad that runs something is listed below with the same advice.
  const warnings = preview.warnings.filter((w) => w.code !== "reviewBeforeSharing");
  return (
    <section className="panel">
      <h3>What the file holds</h3>
      <p className="row wrap">
        <b>{preview.fileName}</b>
        <span className="pill ghost">{KIND[kind]}</span>
        <span className="hint">{size(preview.sizeBytes)}</span>
      </p>
      <ul className="plain">
        {preview.surfaces.map((s) => (
          <li key={s.localId}>
            <b>{s.name}</b>
            <span>
              {plural(s.pages, "page")}, {plural(s.controls, "pad")}
            </span>
          </li>
        ))}
      </ul>
      {(preview.contextRules.length > 0 || questions > 0) && (
        <p>
          {[preview.contextRules.length > 0 ? plural(preview.contextRules.length, "context rule") : "", questions > 0 ? plural(questions, "question") : ""]
            .filter(Boolean)
            .join(", ")}
          .
        </p>
      )}
      {preview.dependencies.length > 0 && (
        <>
          <h4>Plugins it needs</h4>
          <ul className="plain">
            {preview.dependencies.map((d) => (
              <li key={d.pluginId}>
                <b>{d.name ?? d.pluginId}</b>
                <span className="pill ghost">{d.version}</span>
                <span className="hint">{d.required ? "for its pads" : "for its looks only"}</span>
              </li>
            ))}
          </ul>
        </>
      )}
      {preview.review.length > 0 && (
        <>
          <h4>Pads that run things</h4>
          <p className="hint">Check that none of these holds a password or a token before you share the file.</p>
          <ul className="plain review">
            {preview.review.map((r) => (
              <li key={`${r.at.surfaceSourceId}/${r.at.pageId}/${r.at.controlId}/${r.at.slot}`}>
                <b>{r.label}</b>
                <span className="pill warn">{REVIEW_KIND[r.kind]}</span>
                <pre className="command-preview">{r.text}</pre>
              </li>
            ))}
          </ul>
        </>
      )}
      {stripped && <p className="hint">{stripped}</p>}
      {warnings.length > 0 && (
        <>
          <h4>Warnings</h4>
          <ul className="plain bullets">
            {warnings.map((w, i) => (
              <li key={`${w.code}/${w.at ?? i}`}>{w.message}</li>
            ))}
          </ul>
        </>
      )}
      {preview.errors.length > 0 && (
        <div role="alert">
          <h4>To fix before saving</h4>
          <ul className="plain bullets">
            {preview.errors.map((e, i) => (
              <li key={`${e.code}/${e.at ?? i}`} className="field-error">
                {e.message}
              </li>
            ))}
          </ul>
        </div>
      )}
    </section>
  );
}
