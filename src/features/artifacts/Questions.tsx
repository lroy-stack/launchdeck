// The questions a Pack asks the person importing it: one field per input, of
// the input's type, with what Rust says about the answer under it. An answer
// goes back with all the other decisions; typing waits for a pause.

import type { ArtifactLimits, ImportDecisions, ImportPlan, InputValue, PlannedInput } from "../../lib/schema";
import { useStore } from "../../stores/runtime";

type Change = (next: ImportDecisions, typed?: boolean) => void;

export function Questions({ plan, decisions, change, limits }: { plan: ImportPlan; decisions: ImportDecisions; change: Change; limits: ArtifactLimits | null }) {
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
