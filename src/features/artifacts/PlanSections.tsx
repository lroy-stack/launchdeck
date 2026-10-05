// The sections of an import plan, in the order Rust plans it: the file, how it
// fits, earlier imports, surfaces, context rules, plugins, questions, the pads
// that run things, warnings, and the ids and digests under Details. Each
// renders what the plan says; a choice goes back as the whole set of decisions.

import { useState, type ReactNode } from "react";
import { plural } from "../../lib/format";
import type { ArtifactLimits, ExampleInfo, ImportDecisions, ImportPlan, InputValue, PlannedInput } from "../../lib/schema";
import { useStore } from "../../stores/runtime";
import { AUTO_NEVER, DEPENDENCY_TAG, dependencyText, fitText, instanceText, KIND, PUBLISHER, REVIEW_KIND, ruleOutcomeText, size } from "./text";

type Change = (next: ImportDecisions, typed?: boolean) => void;

interface Props {
  plan: ImportPlan;
  decisions: ImportDecisions;
  change: Change;
  /** The example plugins the app ships, to offer installing a missing one. */
  examples: ExampleInfo[];
  install: (pluginId: string) => void;
  limits: ArtifactLimits | null;
}

export function PlanSections({ plan, decisions, change, examples, install, limits }: Props) {
  return (
    <>
      <Header plan={plan} />
      <Compatibility plan={plan} />
      {plan.existing.length > 0 && <Earlier plan={plan} decisions={decisions} change={change} />}
      <Surfaces plan={plan} />
      {plan.contextRules.length > 0 && <Rules plan={plan} decisions={decisions} change={change} />}
      {plan.dependencies.length > 0 && <Plugins plan={plan} examples={examples} install={install} />}
      {plan.inputs.length > 0 && <Questions plan={plan} decisions={decisions} change={change} limits={limits} />}
      {plan.review.length > 0 && <Runs plan={plan} decisions={decisions} change={change} />}
      {plan.warnings.length > 0 && (
        <section className="panel">
          <h3>Warnings</h3>
          <ul className="plain bullets">
            {plan.warnings.map((w, i) => (
              <li key={`${w.code}/${w.at ?? i}`}>{w.message}</li>
            ))}
          </ul>
        </section>
      )}
      <PlanDetails plan={plan} />
    </>
  );
}

/** A Details disclosure whose content is in the page only while it is open. */
export function Disclosure({ children }: { children: ReactNode }) {
  const [open, setOpen] = useState(false);
  return (
    <details onToggle={(e) => setOpen(e.currentTarget.open)}>
      <summary>Details</summary>
      {open && children}
    </details>
  );
}

function Header({ plan }: { plan: ImportPlan }) {
  const a = plan.artifact;
  return (
    <section className="panel artifact-head">
      <div className="artifact-title">
        <h2>{a.name}</h2>
        <span className="pill">v{a.version}</span>
        <span className="pill ghost">{KIND[a.kind]}</span>
      </div>
      {a.description && <p className="artifact-description">{a.description}</p>}
      {(a.author || a.license) && (
        <p className="hint">
          {a.author && `By ${a.author.name}${a.author.url ? ` (${a.author.url})` : ""}`}
          {a.author && a.license && " · "}
          {a.license && `License: ${a.license}`}
        </p>
      )}
      <p className="row wrap">
        <span className="pill warn">{PUBLISHER}</span>
        <span className="hint">The file is intact: its content matches its digest. Nothing says who made it.</span>
      </p>
      <p className="hint">
        {plan.source.fileName} · {size(plan.source.sizeBytes)}
      </p>
    </section>
  );
}

function Compatibility({ plan }: { plan: ImportPlan }) {
  return (
    <section className="panel">
      <h3>Compatibility</h3>
      <ul className="plain">
        {plan.compatibility.devices.map((d) => (
          <li key={`${d.model}/${d.name}`}>
            <b>{d.name}</b>
            <span className={`pill${d.connected ? " live" : " ghost"}`}>{d.connected ? "connected" : "not connected"}</span>
            <span>{fitText(d)}</span>
          </li>
        ))}
      </ul>
    </section>
  );
}

/** Earlier imports of this file: import it as a new copy, or replace one copy in place. */
function Earlier({ plan, decisions, change }: { plan: ImportPlan; decisions: ImportDecisions; change: Change }) {
  const resolution = decisions.resolution;
  const overwrites = plan.surfaces.some((s) => s.action.kind === "replace" && s.action.locallyEdited);
  return (
    <section className="panel">
      <h3>Already imported</h3>
      <p className="hint">You imported this file before. Import it as a new copy, with surfaces of its own, or replace the surfaces of one copy in place.</p>
      <ul className="plain">
        {plan.existing.map((e) => (
          <li key={e.instance}>
            <span>{instanceText(e)}</span>
            <span>{e.surfaces.map((s) => s.name).join(", ")}</span>
            {e.surfaces.some((s) => s.locallyEdited) && <span className="pill warn">changed here</span>}
          </li>
        ))}
      </ul>
      <div className="choices" role="radiogroup" aria-label="How to import it">
        <label className="check">
          <input type="radio" name="resolution" checked={resolution?.kind === "importAsNew"} onChange={() => change({ ...decisions, resolution: { kind: "importAsNew" } })} />
          Import as a new copy
        </label>
        {plan.existing.map((e) => (
          <label key={e.instance} className="check">
            <input
              type="radio"
              name="resolution"
              checked={resolution?.kind === "replace" && resolution.instance === e.instance}
              onChange={() => change({ ...decisions, resolution: { kind: "replace", instance: e.instance } })}
            />
            Replace copy {e.instance}
          </label>
        ))}
      </div>
      {(overwrites || decisions.confirmOverwriteEdits) && (
        <label className="check">
          <input type="checkbox" checked={decisions.confirmOverwriteEdits} onChange={(e) => change({ ...decisions, confirmOverwriteEdits: e.currentTarget.checked })} />
          Replace surfaces changed here since they were imported
        </label>
      )}
    </section>
  );
}

function Surfaces({ plan }: { plan: ImportPlan }) {
  const workspace = useStore((s) => s.workspace);
  const surfaceName = (id: string) => workspace?.surfaces.find((s) => s.id === id)?.name ?? id;
  const pluginName = (id: string) => plan.dependencies.find((d) => d.pluginId === id)?.name ?? id;
  return (
    <section className="panel">
      <h3>Surfaces to add</h3>
      <ul className="plain">
        {plan.surfaces.map((s) => (
          <li key={s.sourceId}>
            <b>{s.name}</b>
            <span>
              {plural(s.pages, "page")}, {plural(s.controls, "pad")}
            </span>
            {s.action.kind === "replace" ? (
              <span className="pill">replaces the copy of version {s.action.previousVersion}</span>
            ) : (
              <span className="pill live">new</span>
            )}
            {s.action.kind === "replace" && s.action.locallyEdited && <span className="pill warn">changed here</span>}
            {s.idChanged && <span className="pill ghost">id changed</span>}
            {s.sameNameAs && <span className="hint">Same name as a surface you have: {surfaceName(s.sameNameAs)}.</span>}
            {s.missingProviders.length > 0 && <span className="hint">Needs {s.missingProviders.map(pluginName).join(", ")}.</span>}
          </li>
        ))}
      </ul>
    </section>
  );
}

function Rules({ plan, decisions, change }: { plan: ImportPlan; decisions: ImportDecisions; change: Change }) {
  const auto = useStore((s) => s.context?.settings.auto ?? false);
  const inputLabel = (id: string) => plan.inputs.find((i) => i.def.id === id)?.def.label ?? id;
  const surfaceName = (id: string) => plan.surfaces.find((s) => s.localId === id)?.name ?? id;
  const toggle = (index: number, on: boolean) => {
    const rest = decisions.contextRules.filter((i) => i !== index);
    change({ ...decisions, contextRules: on ? [...rest, index].sort((a, b) => a - b) : rest });
  };
  return (
    <section className="panel">
      <h3>Context rules</h3>
      <p className="hint">
        Each shows a surface when an application comes to the front. None is added unless you select it. {AUTO_NEVER}
        {auto ? "" : " Auto Context is off now."}
      </p>
      <ul className="plain">
        {plan.contextRules.map((r) => (
          <li key={r.index} className="stacked">
            <label className="check">
              <input type="checkbox" checked={decisions.contextRules.includes(r.index)} onChange={(e) => toggle(r.index, e.currentTarget.checked)} />
              {r.applicationName} → {surfaceName(r.surfaceLocalId)}
              {r.pageId ? ` (page ${r.pageId})` : ""}
            </label>
            <small className="hint">{ruleOutcomeText(r.outcome, inputLabel)}</small>
          </li>
        ))}
      </ul>
    </section>
  );
}

function Plugins({ plan, examples, install }: { plan: ImportPlan; examples: ExampleInfo[]; install: (pluginId: string) => void }) {
  const shipped = (id: string) => examples.some((e) => e.id === id && !e.installed);
  return (
    <section className="panel">
      <h3>Plugins needed</h3>
      <ul className="plain">
        {plan.dependencies.map((d) => (
          <li key={d.pluginId}>
            <b>{d.name}</b>
            <span className="pill ghost">{d.requirement}</span>
            <span className={`pill ${d.status.kind === "compatible" ? "live" : "warn"}`}>{DEPENDENCY_TAG[d.status.kind]}</span>
            <span>{dependencyText(d)}</span>
            {d.usedBy.length > 0 && <span className="hint">Used by {d.usedBy.map((u) => u.label).join(", ")}.</span>}
            {d.status.kind === "missing" && shipped(d.pluginId) && (
              <button type="button" className="btn small primary" aria-label={`Install example ${d.name}`} onClick={() => install(d.pluginId)}>
                Install example
              </button>
            )}
          </li>
        ))}
      </ul>
    </section>
  );
}

function Questions({ plan, decisions, change, limits }: { plan: ImportPlan; decisions: ImportDecisions; change: Change; limits: ArtifactLimits | null }) {
  const set = (id: string, value: InputValue | null, typed: boolean) => {
    const rest = Object.fromEntries(Object.entries(decisions.inputs).filter(([k]) => k !== id));
    change({ ...decisions, inputs: value ? { ...rest, [id]: value } : rest }, typed);
  };
  return (
    <section className="panel">
      <h3>Questions</h3>
      <p className="hint">The file asks for these. Your answers go into the pads and rules that use them.</p>
      {plan.inputs.map((input) => (
        <Question
          key={input.def.id}
          input={input}
          value={decisions.inputs[input.def.id]}
          namespace={plan.compatibility.namespace}
          limits={limits}
          set={(value, typed) => set(input.def.id, value, typed)}
        />
      ))}
    </section>
  );
}

interface QuestionProps {
  input: PlannedInput;
  value: InputValue | undefined;
  namespace: string | undefined;
  limits: ArtifactLimits | null;
  set: (value: InputValue | null, typed: boolean) => void;
}

/** One input: a field of its type, and what Rust says about the answer under it. */
function Question({ input, value, namespace, limits, set }: QuestionProps) {
  const { def, issues } = input;
  const id = `artifact-input-${def.id}`;
  const label = def.required === false ? `${def.label} (optional)` : def.label;
  const text = value?.kind === "text" ? value.value : "";
  const maxLength = { path: limits?.path, url: limits?.url, string: def.maxLength ?? limits?.defaultStringInput, enum: undefined, application: undefined }[def.type];
  return (
    <div className="field question">
      <label htmlFor={id}>{label}</label>
      {def.type === "application" ? (
        <ApplicationField id={id} value={value} namespace={namespace} invalid={issues.length > 0} set={(v) => set(v, false)} />
      ) : def.type === "enum" ? (
        <select id={id} value={text} aria-invalid={issues.length > 0} onChange={(e) => set(e.currentTarget.value ? { kind: "text", value: e.currentTarget.value } : null, false)}>
          <option value="">Choose…</option>
          {(def.options ?? []).map((o) => (
            <option key={o.value} value={o.value}>
              {o.label}
            </option>
          ))}
        </select>
      ) : (
        <input
          id={id}
          type="text"
          value={text}
          maxLength={maxLength}
          spellCheck={false}
          aria-invalid={issues.length > 0}
          onChange={(e) => set(e.currentTarget.value ? { kind: "text", value: e.currentTarget.value } : null, true)}
        />
      )}
      {def.description && <small>{def.description}</small>}
      {issues.map((issue) => (
        <small key={issue} className="field-error">
          {issue}
        </small>
      ))}
    </div>
  );
}

interface ApplicationFieldProps {
  id: string;
  value: InputValue | undefined;
  namespace: string | undefined;
  invalid: boolean;
  set: (value: InputValue | null) => void;
}

/** The applications seen in front since LaunchDeck started, on this kind of system. */
function ApplicationField({ id, value, namespace, invalid, set }: ApplicationFieldProps) {
  const recent = useStore((s) => s.context?.recent ?? []);
  const seen = recent.filter((a) => a.namespace === namespace);
  const chosen = value?.kind === "application" ? value : null;
  const options = chosen && !seen.some((a) => a.id === chosen.id) ? [chosen, ...seen] : seen;
  return (
    <>
      <select
        id={id}
        value={chosen?.id ?? ""}
        aria-invalid={invalid}
        onChange={(e) => {
          const app = options.find((a) => a.id === e.currentTarget.value);
          set(app ? { kind: "application", namespace: app.namespace, id: app.id, displayName: app.displayName } : null);
        }}
      >
        <option value="">Choose an application…</option>
        {options.map((a) => (
          <option key={a.id} value={a.id}>
            {a.displayName || a.id}
          </option>
        ))}
      </select>
      {seen.length === 0 && (
        <small>{namespace ? "Bring the application to the front once, then choose it here: LaunchDeck lists the applications it has seen since it started." : "LaunchDeck cannot tell applications apart on this system."}</small>
      )}
    </>
  );
}

/** What runs when these pads are pressed, word for word, and whether to keep it. */
function Runs({ plan, decisions, change }: { plan: ImportPlan; decisions: ImportDecisions; change: Change }) {
  return (
    <section className="panel">
      <h3>Pads that run things</h3>
      <p className="hint">These pads run something on this computer or over the network when pressed. Read what each one runs.</p>
      <ul className="plain review">
        {plan.review.map((r) => (
          <li key={`${r.at.surfaceSourceId}/${r.at.pageId}/${r.at.controlId}/${r.at.slot}`}>
            <b>{r.label}</b>
            <span className="pill warn">{REVIEW_KIND[r.kind]}</span>
            <pre className="command-preview">{r.text}</pre>
          </li>
        ))}
      </ul>
      {plan.suggested.runs ? (
        <div className="choices" role="radiogroup" aria-label="Pads that run things">
          <label className="check">
            <input type="radio" name="runs" checked={decisions.runs === "remove"} onChange={() => change({ ...decisions, runs: "remove" })} />
            Remove what they run: the pads stay, with their labels and looks, and do nothing when pressed
          </label>
          <label className="check">
            <input type="radio" name="runs" checked={decisions.runs === "keep"} onChange={() => change({ ...decisions, runs: "keep" })} />
            Keep them: they run when pressed
          </label>
        </div>
      ) : (
        <p className="hint">Instructions to an agent are always kept. Read them before you import.</p>
      )}
    </section>
  );
}

/** Ids, digests and how the file's surface ids become ids here. */
function PlanDetails({ plan }: { plan: ImportPlan }) {
  const a = plan.artifact;
  return (
    <section className="panel">
      <Disclosure>
        <dl className="artifact-details">
          <dt>File</dt>
          <dd>
            {plan.source.fileName}, {plural(plan.source.sizeBytes, "byte")}, format version {plan.source.formatVersion}
          </dd>
          <dt>Id</dt>
          <dd>
            <code>{a.id}</code>
          </dd>
          <dt>Made with</dt>
          <dd>LaunchDeck {a.createdWith.version}</dd>
          {a.testedWith.length > 0 && (
            <>
              <dt>Tested with</dt>
              <dd>{a.testedWith.map((t) => `${t.model} (${t.kind})`).join(", ")}</dd>
            </>
          )}
          <dt>Content digest</dt>
          <dd>
            <code>{plan.source.contentDigest}</code>
          </dd>
          <dt>Plan</dt>
          <dd>
            <code>{plan.planId}</code>
          </dd>
          <dt>Plan digest</dt>
          <dd>
            <code>{plan.planDigest}</code>
          </dd>
        </dl>
        <table className="remap">
          <thead>
            <tr>
              <th>Surface id in the file</th>
              <th>Id here</th>
            </tr>
          </thead>
          <tbody>
            {plan.surfaces.map((s) => (
              <tr key={s.sourceId}>
                <td>
                  <code>{s.sourceId}</code>
                </td>
                <td>
                  <code>{s.localId}</code>
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      </Disclosure>
    </section>
  );
}
