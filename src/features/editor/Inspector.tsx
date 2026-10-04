import { useEffect, useMemo, useState } from "react";
import { api } from "../../lib/ipc";
import { isMissingIssue, isPrivileged, normalizeArgs } from "../../lib/actions";
import { SLOTS, describePos, lookHints, newControl, planAssign, replaceWarning, setSlot, withLook, type Slot } from "../../lib/assign";
import { SWATCHES, normalizeHex } from "../../lib/color";
import type { ActionDefinition, Animation, Assignment, Control, Issue, Page, Surface, Visual } from "../../lib/schema";
import { definitionId } from "../../lib/schema";
import { controlAt, useStore } from "../../stores/runtime";
import { AssignmentEditor } from "./AssignmentEditor";

interface Props {
  surface: Surface;
  page: Page;
  surfaces: Surface[];
}

export function Inspector({ surface, page, surfaces }: Props) {
  const selected = useStore((s) => s.selected);
  const existing = controlAt(page, selected);
  const base = useMemo<Control | null>(() => existing ?? (selected && selected.kind !== "logo" ? newControl(selected) : null), [existing, selected]);

  if (!selected || !base) {
    return (
      <aside className="inspector empty-state">
        <h3>Inspector</h3>
        <p>Select a pad on the surface to edit it, then give it an action from the Library. Empty pads can be given a label, colour and action.</p>
        <p className="hint">Tip: pressing a pad on the real Launchpad also selects it here.</p>
      </aside>
    );
  }
  // A different pad is a fresh editor, so a draft never leaks from one pad to another.
  return <ControlEditor key={`${surface.id}/${page.id}/${JSON.stringify(selected)}`} surface={surface} page={page} surfaces={surfaces} base={base} isNew={!existing} />;
}

const dedupe = (issues: Issue[]): Issue[] => {
  const seen = new Set<string>();
  return issues.filter((i) => {
    const k = `${i.param ?? ""}|${i.message}`;
    return seen.has(k) ? false : (seen.add(k), true);
  });
};

/**
 * What is wrong with one slot's settings. The parameter rules answer at once as
 * a person types; Rust answers a moment later with the same rules plus the
 * provider's own checks (a link needs an allowed scheme). A slot nobody
 * changed is not checked, exactly as saving does not check it.
 */
function useIssues(draft: Assignment | null | undefined, saved: Assignment | null | undefined): Issue[] {
  const index = useStore((s) => s.catalog);
  const key = draft ? JSON.stringify(draft) : "";
  const unchanged = key === (saved ? JSON.stringify(saved) : "");
  const def = draft ? index.byId.get(definitionId(draft)) : undefined;
  const local = useMemo(() => (draft && def && !unchanged ? normalizeArgs(def.params, draft.args).issues : []), [key, def, unchanged]); // eslint-disable-line react-hooks/exhaustive-deps
  const [remote, setRemote] = useState<{ key: string; issues: Issue[] } | null>(null);

  useEffect(() => {
    if (!draft || !def || unchanged || local.length > 0) return;
    const timer = setTimeout(() => {
      api.actions
        .validate(draft)
        .then((r) => setRemote({ key, issues: r.issues }))
        .catch(() => undefined);
    }, 250);
    return () => clearTimeout(timer);
  }, [key, def, unchanged, local.length]); // eslint-disable-line react-hooks/exhaustive-deps

  return dedupe([...local, ...(remote && remote.key === key && !unchanged ? remote.issues : [])]);
}

function ControlEditor({ surface, page, surfaces, base, isNew }: { surface: Surface; page: Page; surfaces: Surface[]; base: Control; isNew: boolean }) {
  const select = useStore((s) => s.select);
  const attempt = useStore((s) => s.attempt);
  const toast = useStore((s) => s.toast);
  const index = useStore((s) => s.catalog);
  const intent = useStore((s) => s.intent);
  const consumeIntent = useStore((s) => s.consumeIntent);

  const [draft, setDraft] = useState<Control>(base);
  const [pending, setPending] = useState<{ slot: Slot; def: ActionDefinition } | null>(null);
  // The stored control changed (saved, or edited elsewhere): start again from it.
  const baseKey = JSON.stringify(base);
  useEffect(() => setDraft(base), [baseKey]); // eslint-disable-line react-hooks/exhaustive-deps

  const apply = (slot: Slot, def: ActionDefinition) => {
    const plan = planAssign(draft, slot, def);
    if (plan.kind === "same") return toast(`“${def.name}” is already there. Its settings were not changed.`);
    if (plan.kind === "confirm") return setPending({ slot, def });
    setPending(null);
    // A control nobody has touched yet takes the action's suggested look; any other keeps its own.
    const untouched = isNew && JSON.stringify({ ...draft, press: null, longPress: null }) === JSON.stringify(newControl(draft.pos));
    const next = setSlot(draft, slot, plan.assignment);
    setDraft(untouched ? withLook(next, def) : next);
  };

  // A pick from the Library, or a drop on this pad, arrives as an intent for this pad.
  useEffect(() => {
    if (!intent || JSON.stringify(intent.pos) !== JSON.stringify(draft.pos)) return;
    consumeIntent(intent.seq);
    const def = index.byId.get(intent.definitionId);
    if (def) apply(intent.slot, def);
  }, [intent]); // eslint-disable-line react-hooks/exhaustive-deps

  const confirmReplace = () => {
    if (!pending) return;
    const { slot, def } = pending;
    setPending(null);
    setDraft((d) => setSlot(d, slot, { provider: def.provider, action: def.action, args: Object.fromEntries(def.params.filter((p) => p.default !== undefined && p.default !== null).map((p) => [p.name, p.default])) }));
  };

  const pressIssues = useIssues(draft.press, base.press);
  const longIssues = useIssues(draft.longPress, base.longPress);
  const issues: Record<Slot, Issue[]> = { press: pressIssues, longPress: longIssues };
  const blocked = pressIssues.length > 0 || longIssues.length > 0;

  const dirty = JSON.stringify(draft) !== JSON.stringify(base);
  const patch = (p: Partial<Control>) => setDraft({ ...draft, ...p });
  const setVisual = (v: Partial<Visual>) => patch({ visual: { ...draft.visual, ...v } });

  const save = async () => {
    const ok = await attempt(() => api.updateControl(surface.id, page.id, draft));
    if (ok) toast(isNew ? "Control added" : "Saved");
  };
  const remove = async () => {
    const ok = await attempt(() => api.removeControl(surface.id, page.id, draft.id));
    if (ok) {
      select(null);
      toast("Control removed");
    }
  };

  const privileged = isPrivileged(index, draft.press) || isPrivileged(index, draft.longPress);
  const suggester = [draft.press, draft.longPress].map((a) => (a ? index.byId.get(definitionId(a)) : undefined)).find((d) => d?.appearance);
  const hints = lookHints(draft, suggester);
  const hasHints = Object.keys(hints).length > 0;

  return (
    <aside className="inspector" aria-label="Inspector">
      <header className="inspector-head">
        <div>
          <h3>{isNew ? "New control" : draft.label || "Control"}</h3>
          <p className="hint">{describePos(draft.pos)}</p>
        </div>
        <div className="tags">
          {dirty && <span className="tag unsaved">Not saved</span>}
          {privileged && <span className="pill warn">Privileged</span>}
        </div>
      </header>

      <div className="inspector-body">
        <label className="field">
          <span>Label</span>
          <input value={draft.label} maxLength={40} onChange={(e) => patch({ label: e.target.value })} />
        </label>

        {SLOTS.map((slot) => (
          <AssignmentEditor
            key={slot}
            slot={slot}
            value={draft[slot]}
            onChange={(next) => {
              setDraft((d) => setSlot(d, slot, next));
              if (pending?.slot === slot && next === null) setPending(null);
            }}
            pending={pending?.slot === slot ? pending.def : null}
            replaceWarning={pending?.slot === slot && draft[slot] ? replaceWarning(index, draft[slot]!, pending.def) : null}
            onConfirmReplace={confirmReplace}
            onCancelReplace={() => setPending(null)}
            onDropAction={(id) => {
              const def = index.byId.get(id);
              if (def) apply(slot, def);
            }}
            issues={issues[slot]}
            surfaces={surfaces}
            currentSurface={surface}
          />
        ))}

        {hasHints && suggester && (
          <div className="notice suggest" role="note">
            <p>
              <b>{suggester.name}</b> suggests a look:{" "}
              {[hints.label && `label “${hints.label}”`, hints.color && `colour ${hints.color}`, hints.icon && `icon ${hints.icon}`].filter(Boolean).join(", ")}. Your label, colour and icon stay as they are unless you use it. State looks and bindings are never touched.
            </p>
            <button type="button" className="btn small" onClick={() => setDraft(withLook(draft, suggester))}>
              Use suggested look
            </button>
          </div>
        )}

        <section className="field-group" aria-label="Look">
          <h4>Look</h4>
        <label className="field">
          <span>Description</span>
          <textarea rows={2} value={draft.description} onChange={(e) => patch({ description: e.target.value })} />
        </label>

        <div className="row">
          <label className="field narrow">
            <span>Icon</span>
            <input value={draft.icon ?? ""} maxLength={4} placeholder="▶" onChange={(e) => patch({ icon: e.target.value || null })} />
          </label>
          <label className="field grow">
            <span>Animation</span>
            <select value={draft.visual.animation} onChange={(e) => setVisual({ animation: e.target.value as Animation })}>
              <option value="none">Solid</option>
              <option value="pulse">Pulse</option>
              <option value="flash">Flash</option>
            </select>
          </label>
        </div>

        <ColorField label="Colour" value={draft.visual.color} onChange={(color) => setVisual({ color })} />

        {draft.pos.kind === "grid" && (
          <div className="row">
            <label className="field grow">
              <span>Width</span>
              <input type="number" min={1} max={8} value={draft.span.w} onChange={(e) => patch({ span: { ...draft.span, w: clamp(+e.target.value) } })} />
            </label>
            <label className="field grow">
              <span>Height</span>
              <input type="number" min={1} max={8} value={draft.span.h} onChange={(e) => patch({ span: { ...draft.span, h: clamp(+e.target.value) } })} />
            </label>
          </div>
        )}
        </section>

        <StatesEditor states={draft.states} onChange={(states) => patch({ states })} />

        {draft.bindings.length > 0 && (
          <section className="field-group">
            <h4>State bindings</h4>
            <p className="hint">This control follows events from other sources. Defined in the surface file; shown read-only here.</p>
            <ul className="bindings">
              {draft.bindings.map((b, i) => (
                <li key={i}>
                  <code>{b.source}</code>
                  {b.rules.map((r, j) => (
                    <div key={j} className="binding-rule">
                      <code>{r.event}</code>
                      {r.when.map((w) => (
                        <code key={w.field}>
                          {w.field}={String(w.equals)}
                        </code>
                      ))}
                      <span>→</span>
                      <code>{r.setState}</code>
                    </div>
                  ))}
                </li>
              ))}
            </ul>
          </section>
        )}
      </div>

      <footer className="inspector-foot">
        {blocked && (
          <p className={`foot-note ${[...pressIssues, ...longIssues].every(isMissingIssue) ? "hint" : "field-error"}`} id="save-blocked">
            {[...pressIssues, ...longIssues].every(isMissingIssue) ? "Fill in the settings marked (needed) to save." : "Fix the highlighted settings to save."}
          </p>
        )}
        <div className="foot-buttons">
          <button type="button" className="btn primary" disabled={!dirty || blocked} aria-describedby={blocked ? "save-blocked" : undefined} onClick={() => void save()}>
            {isNew ? "Add control" : "Save"}
          </button>
          <button type="button" className="btn" disabled={!dirty} onClick={() => (setPending(null), setDraft(base))}>
            Revert
          </button>
          {!isNew && (
            <button type="button" className="btn danger" onClick={() => void remove()}>
              Delete
            </button>
          )}
        </div>
      </footer>
    </aside>
  );
}

const clamp = (n: number): number => Math.max(1, Math.min(8, Number.isFinite(n) ? Math.round(n) : 1));

function ColorField({ label, value, onChange }: { label: string; value: string; onChange: (hex: string) => void }) {
  const [text, setText] = useState(value);
  useEffect(() => setText(value), [value]);
  return (
    <div className="field">
      <span>{label}</span>
      <div className="color-row">
        <input type="color" aria-label={`${label} picker`} value={value} onChange={(e) => onChange(e.target.value)} />
        <input
          className="mono"
          value={text}
          spellCheck={false}
          onChange={(e) => {
            setText(e.target.value);
            const hex = normalizeHex(e.target.value);
            if (hex) onChange(hex);
          }}
        />
        <div className="swatches">
          {SWATCHES.map((c) => (
            <button key={c} type="button" className={`swatch${c === value ? " on" : ""}`} style={{ background: c }} aria-label={c} onClick={() => onChange(c)} />
          ))}
        </div>
      </div>
    </div>
  );
}

function StatesEditor({ states, onChange }: { states: Record<string, Visual>; onChange: (s: Record<string, Visual>) => void }) {
  const [name, setName] = useState("");
  const entries = Object.entries(states);
  return (
    <section className="field-group">
      <h4>State appearances</h4>
      <p className="hint">How this control looks while it is in a named state. The base colour above is the idle look.</p>
      {entries.map(([key, v]) => (
        <div key={key} className="state-row">
          <code>{key}</code>
          <input type="color" aria-label={`${key} colour`} value={v.color} onChange={(e) => onChange({ ...states, [key]: { ...v, color: e.target.value } })} />
          <select aria-label={`${key} animation`} value={v.animation} onChange={(e) => onChange({ ...states, [key]: { ...v, animation: e.target.value as Animation } })}>
            <option value="none">Solid</option>
            <option value="pulse">Pulse</option>
            <option value="flash">Flash</option>
          </select>
          <button
            type="button"
            className="icon-btn"
            aria-label={`Remove ${key}`}
            onClick={() => {
              const next = { ...states };
              delete next[key];
              onChange(next);
            }}
          >
            ×
          </button>
        </div>
      ))}
      <form
        className="row"
        onSubmit={(e) => {
          e.preventDefault();
          const k = name.trim().toLowerCase().replace(/[^a-z0-9_-]+/g, "-");
          if (!k || k in states) return;
          onChange({ ...states, [k]: { color: "#22c55e", animation: "none" } });
          setName("");
        }}
      >
        <input className="grow" placeholder="Add a state, e.g. running" value={name} onChange={(e) => setName(e.target.value)} />
        <button type="submit" className="btn small" disabled={!name.trim()}>
          Add
        </button>
      </form>
    </section>
  );
}
