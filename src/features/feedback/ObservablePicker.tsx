// One picker for every provider: plugins, Agent Control, LaunchDeck itself, the
// demo. It lists what the Observable Catalog lists, in the words of each
// definition, and never shows an id unless asked. Searching happens here, on the
// catalog Rust already sent, so typing never waits for a round trip.

import { useEffect, useId, useMemo, useRef, useState, type KeyboardEvent } from "react";
import type { Args } from "../../lib/actions";
import { issuesByParam } from "../../lib/actions";
import { groupByProvider, nowText, scopeOf, searchObservables, typeSummary } from "../../lib/feedbackEdit";
import { plural } from "../../lib/format";
import type { Observable, ObservableCatalog, ObservableKind, Scalar, Surface } from "../../lib/schema";
import { useStore } from "../../stores/runtime";
import { ParamForm } from "../editor/ParamForm";
import { StatusTag } from "./parts";

const optionId = (list: string, id: string): string => `${list}-opt-${id.replace(/[^a-zA-Z0-9_-]/g, "_")}`;

const KIND_WORD: Record<ObservableKind, { one: string; many: string; search: string }> = {
  state: { one: "state", many: "states", search: "Search states" },
  event: { one: "event", many: "events", search: "Search events" },
};

interface PickerProps {
  catalog: ObservableCatalog;
  kind: ObservableKind;
  onPick: (o: Observable) => void;
  autoFocus?: boolean;
}

/** Search box and grouped list. Arrow keys move, Enter picks, Escape clears. */
export function ObservablePicker({ catalog, kind, onPick, autoFocus }: PickerProps) {
  const listId = useId();
  const setView = useStore((s) => s.setView);
  const inputRef = useRef<HTMLInputElement>(null);
  const [query, setQuery] = useState("");
  const [activeId, setActiveId] = useState<string | null>(null);

  const results = useMemo(() => searchObservables(catalog, query, kind), [catalog, query, kind]);
  const groups = useMemo(() => groupByProvider(results), [results]);
  // Arrow keys walk the list the way it is drawn: group after group.
  const flat = useMemo(() => groups.flatMap((g) => g.items), [groups]);
  const active = flat.find((o) => o.id === activeId) ?? flat[0];
  const word = KIND_WORD[kind];
  const searching = query.trim() !== "";

  useEffect(() => {
    if (autoFocus) inputRef.current?.focus();
  }, [autoFocus, kind]);

  useEffect(() => {
    if (active) document.getElementById(optionId(listId, active.id))?.scrollIntoView?.({ block: "nearest" });
  }, [active, listId]);

  const move = (delta: number) => {
    if (flat.length === 0) return;
    const at = Math.max(0, flat.findIndex((o) => o.id === active?.id));
    setActiveId(flat[Math.min(flat.length - 1, Math.max(0, at + delta))]!.id);
  };

  const onKeyDown = (e: KeyboardEvent<HTMLInputElement>) => {
    switch (e.key) {
      case "ArrowDown":
        e.preventDefault();
        return move(1);
      case "ArrowUp":
        e.preventDefault();
        return move(-1);
      case "Home":
        if (e.ctrlKey) {
          e.preventDefault();
          return setActiveId(flat[0]?.id ?? null);
        }
        return;
      case "Enter":
        e.preventDefault();
        if (active) onPick(active);
        return;
      case "Escape":
        if (query) {
          e.preventDefault();
          e.stopPropagation();
          setQuery("");
        }
    }
  };

  const total = catalog.observables.filter((o) => o.kind === kind).length;
  const status = total === 0 ? `No ${word.many} are offered.` : results.length === 0 ? `Nothing matches “${query.trim()}”.` : `${plural(results.length, word.one)}${searching ? " found" : ""}`;

  return (
    <div className="picker">
      <input
        ref={inputRef}
        role="combobox"
        aria-label={word.search}
        aria-expanded="true"
        aria-controls={listId}
        aria-activedescendant={active ? optionId(listId, active.id) : undefined}
        aria-autocomplete="list"
        autoComplete="off"
        spellCheck={false}
        placeholder={`${word.search}: name, provider or value`}
        value={query}
        onChange={(e) => setQuery(e.target.value)}
        onKeyDown={onKeyDown}
      />
      <p className="hint picker-status" role="status" aria-live="polite">
        {status}
      </p>
      <div className="picker-list" role="listbox" id={listId} aria-label={kind === "state" ? "States" : "Events"}>
        {groups.map((g) => (
          <div key={g.provider} role="group" aria-label={g.name}>
            <div className="picker-group" aria-hidden="true">
              {g.name}
            </div>
            {g.items.map((o) => (
              <Row
                key={o.id}
                o={o}
                domId={optionId(listId, o.id)}
                active={o.id === active?.id}
                onActivate={() => {
                  setActiveId(o.id);
                  inputRef.current?.focus();
                }}
                onPick={() => onPick(o)}
              />
            ))}
          </div>
        ))}
        {results.length === 0 && (
          <div className="library-empty">
            <p>
              {total === 0
                ? kind === "state"
                  ? "No plugin or part of LaunchDeck offers a state yet. Enable a plugin that reports one."
                  : "No plugin or part of LaunchDeck offers an event yet."
                : "Try fewer words, or part of a name. Search covers names, providers, categories, keywords and values."}
            </p>
            {total === 0 ? (
              <button type="button" className="link" onClick={() => setView("plugins")}>
                Open Plugins
              </button>
            ) : (
              <button type="button" className="btn small" onClick={() => setQuery("")}>
                Show everything
              </button>
            )}
          </div>
        )}
      </div>
    </div>
  );
}

function Row({ o, domId, active, onActivate, onPick }: { o: Observable; domId: string; active: boolean; onActivate: () => void; onPick: () => void }) {
  const now = nowText(o);
  const reason = o.availability.status === "unavailable" ? o.availability.reason : undefined;
  return (
    <div
      id={domId}
      role="option"
      aria-selected={active}
      className={`pick-row${active ? " active" : ""}${reason ? " is-unavailable" : ""}`}
      title={reason ? `${o.description}\nUnavailable now: ${reason}` : o.description}
      onClick={onActivate}
      onDoubleClick={onPick}
    >
      <span className="pick-main">
        <span className="pick-name">{o.name}</span>
        <span className="pick-sub">{typeSummary(o)}</span>
        {(now || reason || o.params.length > 0) && (
          <span className="pick-now">
            {/* An event has no value, but whether it can happen is still said in words. */}
            {(now || reason) && <StatusTag status={now?.status ?? "unavailable"} />}
            {now && now.status === "known" && <span className="pick-value">{now.text}</span>}
            {now && now.status !== "known" && o.availability.status === "unavailable" && reason && <span className="pick-value">{reason}</span>}
            {o.params.length > 0 && <span className="tag">Per {o.params.map((p) => p.label.toLowerCase()).join(" and ")}</span>}
          </span>
        )}
      </span>
      {active && (
        <button type="button" className="btn small" onClick={onPick}>
          Use
        </button>
      )}
    </div>
  );
}

// ---------------------------------------------------------------------------
// The flow around the picker: which kind, which observable, which scope
// ---------------------------------------------------------------------------

interface FlowProps {
  catalog: ObservableCatalog;
  /** What the person is doing, in a few words: `Add feedback`, `Change the state`. */
  title: string;
  /** Start on events or states. */
  kind: ObservableKind;
  /** The kind cannot change (replacing a state with a state). */
  locked?: boolean;
  /** The label of the button that finishes a scoped choice. */
  doneLabel: string;
  /** Start on the scope step of this observable, with these arguments (changing only the scope). */
  from?: { observable: Observable; args: Record<string, Scalar> };
  onDone: (o: Observable, args: Record<string, Scalar>) => void;
  onCancel: () => void;
  surfaces: Surface[];
  currentSurface: Surface;
}

/** State or Event, then the observable, then (if it needs one) the scope. */
export function ObservableFlow({ catalog, title, kind: first, locked, doneLabel, from, onDone, onCancel, surfaces, currentSurface }: FlowProps) {
  const [kind, setKind] = useState<ObservableKind>(first);
  const [chosen, setChosen] = useState<Observable | null>(from?.observable ?? null);
  const [args, setArgs] = useState<Args>(from?.args ?? {});

  // The catalog moves on (a value changed): keep showing the observable the person chose, current.
  const current = chosen ? (catalog.observables.find((o) => o.id === chosen.id) ?? chosen) : null;

  const pick = (o: Observable) => {
    if (o.params.length === 0) return onDone(o, {});
    setChosen(o);
    setArgs({});
  };

  return (
    <section className="flow" aria-label={title}>
      <header className="flow-head">
        <h5>{title}</h5>
        <button type="button" className="link" onClick={onCancel}>
          Cancel
        </button>
      </header>

      {!current ? (
        <>
          {!locked && (
            <div className="slot-pick" role="group" aria-label="What should the pad react to?">
              <button type="button" className="chip" aria-pressed={kind === "state"} onClick={() => setKind("state")}>
                A state
              </button>
              <button type="button" className="chip" aria-pressed={kind === "event"} onClick={() => setKind("event")}>
                An event
              </button>
            </div>
          )}
          <p className="hint">{kind === "state" ? "A state is something that has a value for as long as it lasts: a level, a status. The pad follows it." : "An event happens once and passes: a build finished, an approval asked. The pad shows a look for a while."}</p>
          <ObservablePicker catalog={catalog} kind={kind} onPick={pick} autoFocus />
        </>
      ) : (
        <Scope observable={current} args={args} onChange={setArgs} doneLabel={doneLabel} onBack={from ? onCancel : () => setChosen(null)} onDone={(a) => onDone(current, a)} surfaces={surfaces} currentSurface={currentSurface} />
      )}
    </section>
  );
}

function Scope({ observable, args, onChange, doneLabel, onBack, onDone, surfaces, currentSurface }: { observable: Observable; args: Args; onChange: (a: Args) => void; doneLabel: string; onBack: () => void; onDone: (args: Record<string, Scalar>) => void; surfaces: Surface[]; currentSurface: Surface }) {
  const scope = scopeOf(observable, args);
  return (
    <div className="flow-scope">
      <p>
        <b>{observable.name}</b> <span className="hint">· {observable.providerName}</span>
      </p>
      <p className="hint">This state exists once for each {observable.params.map((p) => p.label.toLowerCase()).join(" and ")}. Say which one this pad follows.</p>
      <ParamForm params={observable.params} args={args} onChange={onChange} errors={issuesByParam(scope.issues)} surfaces={surfaces} currentSurface={currentSurface} />
      <div className="foot-buttons">
        <button type="button" className="btn primary" disabled={scope.issues.length > 0} onClick={() => onDone(scope.args)}>
          {doneLabel}
        </button>
        <button type="button" className="btn" onClick={onBack}>
          Back
        </button>
      </div>
    </div>
  );
}
