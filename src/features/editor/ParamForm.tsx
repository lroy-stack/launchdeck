import { useEffect, useId, useState } from "react";
import type { Args } from "../../lib/actions";
import type { ParamDef, Surface } from "../../lib/schema";

interface Props {
  params: ParamDef[];
  args: Args;
  onChange: (next: Args) => void;
  /** Messages per setting name, from the parameter rules and from Rust. */
  errors: Map<string, string[]>;
  surfaces: Surface[];
  /** The surface being edited: where an unscoped page reference looks. */
  currentSurface: Surface;
}

/**
 * The form of an action, drawn from its parameter schema. Nothing here knows
 * any action by name: a provider describes its settings, LaunchDeck draws them.
 */
export function ParamForm({ params, args, onChange, errors, surfaces, currentSurface }: Props) {
  // A setting that is still empty is "needed", not "wrong": say so only once the person has been there.
  const [touched, setTouched] = useState<ReadonlySet<string>>(new Set());
  const touch = (name: string) => setTouched((t) => (t.has(name) ? t : new Set(t).add(name)));
  const shown = (name: string): string[] => (errors.get(name) ?? []).filter((m) => touched.has(name) || !m.endsWith(" is required"));
  const set = (def: ParamDef, value: unknown) => {
    touch(def.name);
    const next: Args = { ...args };
    if (value === undefined) delete next[def.name];
    else next[def.name] = value;
    // A page belongs to the surface chosen next to it: changing the surface clears it.
    if (def.type === "reference" && def.target === "surface") {
      for (const other of params) if (other.scope === def.name) delete next[other.name];
    }
    onChange(next);
  };

  if (params.length === 0) return <p className="hint">This action has no settings.</p>;
  return (
    <div className="param-form">
      {params.map((def) => (
        <ParamField key={def.name} def={def} value={args[def.name]} onChange={(v) => set(def, v)} onBlur={() => touch(def.name)} errors={shown(def.name)} args={args} surfaces={surfaces} currentSurface={currentSurface} />
      ))}
    </div>
  );
}

interface FieldProps {
  def: ParamDef;
  value: unknown;
  onChange: (value: unknown) => void;
  onBlur: () => void;
  errors: string[];
  args: Args;
  surfaces: Surface[];
  currentSurface: Surface;
}

function ParamField(props: FieldProps) {
  const { def, errors } = props;
  const id = useId();
  const hintId = `${id}-hint`;
  const errId = `${id}-err`;
  const described = [def.description ? hintId : "", errors.length ? errId : ""].filter(Boolean).join(" ") || undefined;
  const common = { id, "aria-required": def.required || undefined, "aria-invalid": errors.length > 0 || undefined, "aria-describedby": described, onBlur: props.onBlur };

  const label = (
    <>
      {def.label}
      {def.required && def.default === undefined && (props.value === undefined || props.value === "") && <i className="req"> (needed)</i>}
    </>
  );

  if (def.type === "boolean") {
    return (
      <div className="field">
        <label className="check" htmlFor={id}>
          <input {...common} type="checkbox" checked={props.value === undefined ? def.default === true : props.value === true} onChange={(e) => props.onChange(e.target.checked)} />
          {def.label}
        </label>
        <Notes def={def} hintId={hintId} errId={errId} errors={errors} />
      </div>
    );
  }

  return (
    <div className="field">
      <label htmlFor={id}>
        <span>{label}</span>
      </label>
      <Control {...props} common={common} />
      <Notes def={def} hintId={hintId} errId={errId} errors={errors} />
    </div>
  );
}

function Notes({ def, hintId, errId, errors }: { def: ParamDef; hintId: string; errId: string; errors: string[] }) {
  return (
    <>
      {def.description && (
        <small id={hintId} className="param-hint">
          {def.description}
        </small>
      )}
      {errors.length > 0 && (
        <small id={errId} className="field-error">
          {errors.join(" ")}
        </small>
      )}
    </>
  );
}

type Common = { id: string; "aria-required"?: boolean; "aria-invalid"?: boolean; "aria-describedby"?: string; onBlur: () => void };

function Control({ def, value, onChange, args, surfaces, currentSurface, common }: FieldProps & { common: Common }) {
  switch (def.type) {
    case "string": {
      const text = typeof value === "string" ? value : "";
      // An emptied field falls back to the default on save; say so instead of showing a blank.
      const placeholder = def.placeholder ?? (typeof def.default === "string" ? `Default: ${def.default}` : undefined);
      const shared = { ...common, value: text, placeholder, maxLength: def.maxLength, spellCheck: false, className: def.monospace ? "mono" : undefined };
      return def.multiline ? (
        <textarea {...shared} rows={3} onChange={(e) => onChange(e.target.value === "" ? undefined : e.target.value)} />
      ) : (
        <input {...shared} onChange={(e) => onChange(e.target.value === "" ? undefined : e.target.value)} />
      );
    }
    case "number":
      return <NumberInput def={def} value={value} onChange={onChange} common={common} />;
    case "enum": {
      const current = typeof value === "string" ? value : "";
      const unset = current === "";
      return (
        <select {...common} value={current} onChange={(e) => onChange(e.target.value === "" ? undefined : e.target.value)}>
          {unset && def.required && def.default === undefined && (
            <option value="" disabled>
              Choose…
            </option>
          )}
          {!def.required && def.default === undefined && <option value="">Not set</option>}
          {current !== "" && !def.options.some((o) => o.value === current) && <option value={current}>{current} (not a choice any more)</option>}
          {def.options.map((o) => (
            <option key={o.value} value={o.value}>
              {o.label}
            </option>
          ))}
        </select>
      );
    }
    case "reference": {
      const isSurface = def.target === "surface";
      const scoped = def.scope ? surfaces.find((s) => s.id === args[def.scope!]) : currentSurface;
      const choices = isSurface ? surfaces.map((s) => ({ value: s.id, label: s.name })) : (scoped?.pages ?? []).map((p) => ({ value: p.id, label: p.name }));
      const current = typeof value === "string" ? value : "";
      const waiting = !isSurface && def.scope !== undefined && !scoped;
      return (
        <select {...common} value={current} disabled={waiting} onChange={(e) => onChange(e.target.value === "" ? undefined : e.target.value)}>
          {waiting ? (
            <option value="">Choose a surface first</option>
          ) : def.required ? (
            current === "" && (
              <option value="" disabled>
                Choose…
              </option>
            )
          ) : (
            <option value="">{def.scope ? "First page" : "Not set"}</option>
          )}
          {current !== "" && !choices.some((c) => c.value === current) && <option value={current}>{current} (not found)</option>}
          {choices.map((c) => (
            <option key={c.value} value={c.value}>
              {c.label}
            </option>
          ))}
        </select>
      );
    }
    case "json":
      return <JsonInput def={def} value={value} onChange={onChange} common={common} />;
    case "boolean":
      return null;
  }
}

function NumberInput({ def, value, onChange, common }: { def: ParamDef; value: unknown; onChange: (v: unknown) => void; common: Common }) {
  const external = typeof value === "number" ? String(value) : "";
  const [text, setText] = useState(external);
  // Follow the value when something else changes it (a different pad, a revert).
  useEffect(() => {
    setText((t) => (t.trim() !== "" && Number(t) === value ? t : external));
  }, [value, external]);
  return (
    <input
      {...common}
      type="number"
      inputMode={def.integer ? "numeric" : "decimal"}
      min={def.min}
      max={def.max}
      step={def.integer ? 1 : "any"}
      placeholder={def.placeholder}
      value={text}
      onChange={(e) => {
        setText(e.target.value);
        const t = e.target.value.trim();
        onChange(t === "" ? undefined : Number.isFinite(Number(t)) ? Number(t) : t);
      }}
    />
  );
}

/** Kept for plugins written against manifest schema 1; new actions should not need it. */
function JsonInput({ def, value, onChange, common }: { def: ParamDef; value: unknown; onChange: (v: unknown) => void; common: Common }) {
  const [text, setText] = useState(value === undefined ? "" : JSON.stringify(value));
  const [broken, setBroken] = useState(false);
  return (
    <>
      <textarea
        {...common}
        className="mono"
        rows={2}
        spellCheck={false}
        placeholder={def.placeholder ?? "JSON"}
        value={text}
        onChange={(e) => {
          setText(e.target.value);
          if (e.target.value.trim() === "") {
            setBroken(false);
            return onChange(undefined);
          }
          try {
            onChange(JSON.parse(e.target.value));
            setBroken(false);
          } catch {
            setBroken(true);
          }
        }}
      />
      {broken && <small className="field-error">Not valid JSON.</small>}
    </>
  );
}
