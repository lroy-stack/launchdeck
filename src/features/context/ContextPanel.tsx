// Auto Context in Settings: switch it on, see what is in front and why the
// surface is what it is, and keep the short ordered list of rules.
//
// Everything shown is what Rust reported. Every change is sent whole (Auto,
// the rules in order, the fallback); Rust checks it and answers with the new
// status, or with which rule is wrong and why.

import { useState } from "react";
import { api } from "../../lib/ipc";
import type { ContextFallback, ContextRule, ContextSettings, ContextStatus, Workspace } from "../../lib/schema";
import { useStore } from "../../stores/runtime";
import { explain, identityOf, MODE, nameOf, NAMESPACE, problemText } from "./text";

export function ContextPanel() {
  const context = useStore((s) => s.context);
  const workspace = useStore((s) => s.workspace);
  const attempt = useStore((s) => s.attempt);
  if (!context || !workspace) return null;
  const { settings } = context;
  const save = (next: ContextSettings) => attempt(() => api.context.update(next));
  const usable = context.observer.state !== "unavailable";

  return (
    <section className="panel context-panel" aria-label="Auto Context">
      <div className="panel-head">
        <div>
          <h2>Auto Context</h2>
          <p className="hint">The surface follows the application in front. One rule per application, tried in order: the first that matches wins.</p>
        </div>
        <div className="segmented" role="radiogroup" aria-label="Auto Context">
          <button type="button" role="radio" aria-checked={settings.auto} className={settings.auto ? "on live" : ""} onClick={() => void save({ ...settings, auto: true })}>
            <i /> On
          </button>
          <button type="button" role="radio" aria-checked={!settings.auto} className={!settings.auto ? "on edit" : ""} onClick={() => void save({ ...settings, auto: false })}>
            Off
          </button>
        </div>
      </div>

      <Now context={context} workspace={workspace} />

      <h4>Rules</h4>
      {settings.rules.length === 0 ? (
        <p className="hint">No rules yet. Without one, every application gets the fallback below.</p>
      ) : (
        <ol className="context-rules" aria-label="Context rules">
          {settings.rules.map((rule, i) => (
            <RuleRow key={`${identityOf(rule.application)}/${i}`} index={i} rule={rule} context={context} workspace={workspace} save={save} />
          ))}
        </ol>
      )}
      <AddRule context={context} workspace={workspace} save={save} usable={usable} />

      <h4>When no rule matches</h4>
      <FallbackField fallback={settings.fallback} workspace={workspace} save={(fallback) => save({ ...settings, fallback })} />

      <p className="hint foot">
        A surface you choose yourself is never switched back while the same application stays in front. LaunchDeck's own window does not count as an
        application. Nothing about which applications you use is saved: only these rules. See docs/context-engine.md.
      </p>
    </section>
  );
}

/** What is in front, what the rules were asked about, and what came of it. */
function Now({ context, workspace }: { context: ContextStatus; workspace: Workspace }) {
  const attempt = useStore((s) => s.attempt);
  const o = context.observer;
  const front = context.foreground;
  return (
    <div className="context-now" role="group" aria-label="Context now">
      <dl>
        <dt>Observer</dt>
        <dd>
          {o.state === "available" && (
            <>
              <span className="pill live">
                <i />
                Available
              </span>{" "}
              Watching through {o.mechanism}.
            </>
          )}
          {o.state === "degraded" && (
            <>
              <span className="pill warn">Degraded</span> {o.reason}
            </>
          )}
          {o.state === "unavailable" && (
            <>
              <span className="pill warn">Not available</span> {o.reason}
            </>
          )}
        </dd>
        <dt>In front now</dt>
        <dd>
          {front.kind === "application" && (
            <>
              <b>{nameOf(front.application)}</b> <code>{identityOf(front.application)}</code>
            </>
          )}
          {front.kind === "launchDeck" && <>LaunchDeck itself, which never counts as an application.</>}
          {front.kind === "nothing" && <>No application (the desktop, or a window that says nothing about itself).</>}
          {front.kind === "unknown" && <>{o.state === "unavailable" ? "Not known on this system." : "Not seen yet."}</>}
        </dd>
        {context.application && front.kind !== "application" && (
          <>
            <dt>Last application</dt>
            <dd>
              <b>{nameOf(context.application)}</b> <code>{identityOf(context.application)}</code>
              <span className="hint"> the rules are asked about this one</span>
            </dd>
          </>
        )}
        <dt>Mode</dt>
        <dd>
          <b>{MODE[context.mode]}</b>
          {context.mode === "manual" && (
            <>
              {" "}
              <button type="button" className="btn small primary" onClick={() => void attempt(() => api.context.resume())}>
                Resume Auto
              </button>
            </>
          )}
          {context.mode === "unavailable" && <span className="hint"> Auto Context is on and cannot work here. Switching surfaces yourself works as always.</span>}
        </dd>
        <dt>Decision</dt>
        <dd>{explain(context, workspace)}</dd>
      </dl>
    </div>
  );
}

function RuleRow({ index, rule, context, workspace, save }: { index: number; rule: ContextRule; context: ContextStatus; workspace: Workspace; save: (s: ContextSettings) => Promise<boolean> }) {
  const { settings } = context;
  const n = index + 1;
  const problem = context.problems[index] ?? null;
  const surface = workspace.surfaces.find((s) => s.id === rule.surfaceId);
  const replace = (next: ContextRule) => void save({ ...settings, rules: settings.rules.map((r, i) => (i === index ? next : r)) });
  const move = (to: number) => {
    const rules = [...settings.rules];
    const [it] = rules.splice(index, 1);
    if (it) rules.splice(to, 0, it);
    void save({ ...settings, rules });
  };
  const matched = context.decision?.rule === index;

  return (
    <li className={`context-rule${problem ? " flagged" : ""}${matched ? " matched" : ""}`} aria-label={`Rule ${n}: ${nameOf(rule.application)}`}>
      <span className="rule-number" aria-hidden>
        {n}
      </span>
      <div className="rule-app">
        <b>{nameOf(rule.application)}</b>
        <code title={NAMESPACE[rule.application.namespace] ?? rule.application.namespace}>{identityOf(rule.application)}</code>
      </div>
      <span aria-hidden className="rule-arrow">
        →
      </span>
      <select
        aria-label={`Rule ${n} surface`}
        value={surface ? rule.surfaceId : ""}
        onChange={(e) => replace({ application: rule.application, surfaceId: e.target.value })}
      >
        {!surface && <option value="">(no longer exists)</option>}
        {workspace.surfaces.map((s) => (
          <option key={s.id} value={s.id}>
            {s.name}
          </option>
        ))}
      </select>
      <select
        aria-label={`Rule ${n} page`}
        value={rule.pageId ?? ""}
        disabled={!surface}
        onChange={(e) => replace({ application: rule.application, surfaceId: rule.surfaceId, ...(e.target.value ? { pageId: e.target.value } : {}) })}
      >
        <option value="">Any page</option>
        {rule.pageId && surface && !surface.pages.some((p) => p.id === rule.pageId) && <option value={rule.pageId}>(no longer exists)</option>}
        {surface?.pages.map((p) => (
          <option key={p.id} value={p.id}>
            {p.name}
          </option>
        ))}
      </select>
      <span className="rule-actions">
        <button type="button" className="btn small" aria-label={`Move rule ${n} up`} disabled={index === 0} onClick={() => move(index - 1)}>
          ↑
        </button>
        <button type="button" className="btn small" aria-label={`Move rule ${n} down`} disabled={index === settings.rules.length - 1} onClick={() => move(index + 1)}>
          ↓
        </button>
        <button type="button" className="btn small" aria-label={`Remove rule ${n}`} onClick={() => void save({ ...settings, rules: settings.rules.filter((_, i) => i !== index) })}>
          Remove
        </button>
      </span>
      {problem && (
        <p className="hint warn rule-note" role="note">
          {problemText(problem)}
        </p>
      )}
    </li>
  );
}

/** A new rule: an application LaunchDeck has seen in front, and where it leads. Nothing is typed. */
function AddRule({ context, workspace, save, usable }: { context: ContextStatus; workspace: Workspace; save: (s: ContextSettings) => Promise<boolean>; usable: boolean }) {
  const { settings } = context;
  const state = useStore((s) => s.state);
  const candidates = context.recent;
  const [picked, setPicked] = useState("");
  const [surfaceId, setSurfaceId] = useState("");
  const [pageId, setPageId] = useState("");
  const application = candidates.find((a) => identityOf(a) === picked) ?? candidates[0];
  const surface = workspace.surfaces.find((s) => s.id === (surfaceId || state?.activeSurfaceId)) ?? workspace.surfaces[0];

  if (candidates.length === 0) {
    return (
      <p className="hint" role="note">
        {usable
          ? "To add a rule, bring the application you want to the front once, then come back here: it will be offered in a list. Nothing has to be typed."
          : "Rules are written from applications LaunchDeck has seen in front, which it cannot do on this system."}
      </p>
    );
  }
  const add = () => {
    if (!application || !surface) return;
    const rule: ContextRule = { application, surfaceId: surface.id, ...(pageId && surface.pages.some((p) => p.id === pageId) ? { pageId } : {}) };
    void save({ ...settings, rules: [...settings.rules, rule] }).then((ok) => ok && setPageId(""));
  };

  return (
    <div className="context-add" role="group" aria-label="Add a rule">
      <label className="field">
        <span>When this application is in front</span>
        <select aria-label="Application" value={application ? identityOf(application) : ""} onChange={(e) => setPicked(e.target.value)}>
          {candidates.map((a) => (
            <option key={identityOf(a)} value={identityOf(a)}>
              {nameOf(a)} ({identityOf(a)})
            </option>
          ))}
        </select>
      </label>
      <label className="field">
        <span>show</span>
        <select
          aria-label="Surface"
          value={surface?.id ?? ""}
          onChange={(e) => {
            setSurfaceId(e.target.value);
            setPageId("");
          }}
        >
          {workspace.surfaces.map((s) => (
            <option key={s.id} value={s.id}>
              {s.name}
            </option>
          ))}
        </select>
      </label>
      <label className="field">
        <span>on</span>
        <select aria-label="Page" value={pageId} onChange={(e) => setPageId(e.target.value)}>
          <option value="">Any page</option>
          {surface?.pages.map((p) => (
            <option key={p.id} value={p.id}>
              {p.name}
            </option>
          ))}
        </select>
      </label>
      <button type="button" className="btn primary" onClick={add}>
        Add rule
      </button>
      <small className="hint">Applications seen in front since LaunchDeck started, newest first. The list is not saved.</small>
    </div>
  );
}

function FallbackField({ fallback, workspace, save }: { fallback: ContextFallback; workspace: Workspace; save: (f: ContextFallback) => Promise<boolean> }) {
  const surface = fallback.kind === "surface" ? workspace.surfaces.find((s) => s.id === fallback.surfaceId) : undefined;
  return (
    <div className="row wrap context-fallback">
      <select
        aria-label="When no rule matches"
        value={fallback.kind === "keep" ? "" : fallback.surfaceId}
        onChange={(e) => void save(e.target.value === "" ? { kind: "keep" } : { kind: "surface", surfaceId: e.target.value })}
      >
        <option value="">Keep the current surface</option>
        {fallback.kind === "surface" && !surface && <option value={fallback.surfaceId}>(a surface that no longer exists)</option>}
        {workspace.surfaces.map((s) => (
          <option key={s.id} value={s.id}>
            Open {s.name}
          </option>
        ))}
      </select>
      {fallback.kind === "surface" && surface && (
        <select
          aria-label="Fallback page"
          value={fallback.pageId ?? ""}
          onChange={(e) => void save({ kind: "surface", surfaceId: fallback.surfaceId, ...(e.target.value ? { pageId: e.target.value } : {}) })}
        >
          <option value="">Any page</option>
          {surface.pages.map((p) => (
            <option key={p.id} value={p.id}>
              {p.name}
            </option>
          ))}
        </select>
      )}
      <span className="hint">{fallback.kind === "keep" ? "An application without a rule changes nothing." : "Every application without a rule shows this surface."}</span>
    </div>
  );
}
