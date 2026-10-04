import { useEffect, useId, useMemo, useRef, useState, type KeyboardEvent } from "react";
import { categoriesOf, groupByCategory, searchActions, tokensOf, unavailableNote } from "../../lib/actions";
import { DRAG_TYPE, SLOT_SHORT, SLOTS, describePos, type Slot } from "../../lib/assign";
import { plural } from "../../lib/format";
import type { ActionDefinition, Page } from "../../lib/schema";
import { controlAt, useStore } from "../../stores/runtime";
import { ActionGlyph, PrivilegedTag, SourceTag, UnavailableTag } from "./parts";

const optionId = (list: string, id: string): string => `${list}-opt-${id.replace(/[^a-zA-Z0-9_-]/g, "_")}`;

/**
 * Everything a pad can do, in one searchable place. The catalog comes from
 * Rust; searching happens here, on what was already sent, so typing never
 * waits for a round trip. An action can be given to the selected pad with the
 * button or Enter, or dragged onto any pad.
 */
export function ActionLibrary({ page }: { page: Page }) {
  const open = useStore((s) => s.libraryOpen);
  const setOpen = useStore((s) => s.setLibraryOpen);
  const index = useStore((s) => s.catalog);
  const query = useStore((s) => s.libraryQuery);
  const setQuery = useStore((s) => s.setLibraryQuery);
  const category = useStore((s) => s.libraryCategory);
  const setCategory = useStore((s) => s.setLibraryCategory);
  const focusSignal = useStore((s) => s.libraryFocus);
  const selected = useStore((s) => s.selected);
  const requestAssign = useStore((s) => s.requestAssign);
  const setDragging = useStore((s) => s.setDragging);
  const setView = useStore((s) => s.setView);

  const listId = useId();
  const inputRef = useRef<HTMLInputElement>(null);
  const [activeId, setActiveId] = useState<string | null>(null);
  const [slot, setSlot] = useState<Slot>("press");
  const [note, setNote] = useState("");

  const results = useMemo(() => searchActions(index, query, { category }), [index, query, category]);
  const categories = useMemo(() => categoriesOf(index), [index]);
  const searching = tokensOf(query).length > 0;
  const groups = useMemo(() => (searching ? [{ category: "", actions: results }] : groupByCategory(results)), [results, searching]);
  const active: ActionDefinition | undefined = results.find((a) => a.id === activeId) ?? results[0];

  // "/" jumps to the search from anywhere on the page, unless a field already has the keys.
  useEffect(() => {
    const onKey = (e: globalThis.KeyboardEvent) => {
      if (e.key !== "/" || e.ctrlKey || e.metaKey || e.altKey) return;
      const t = e.target as HTMLElement | null;
      if (t && (t.tagName === "INPUT" || t.tagName === "TEXTAREA" || t.tagName === "SELECT" || t.isContentEditable)) return;
      e.preventDefault();
      setOpen(true);
      inputRef.current?.focus();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [setOpen]);

  // Other parts of the screen ("Browse the library") ask for the cursor here.
  useEffect(() => {
    if (focusSignal > 0) inputRef.current?.focus();
  }, [focusSignal, open]);

  // Keep the highlighted row in view while moving with the keys.
  useEffect(() => {
    if (!active) return;
    document.getElementById(optionId(listId, active.id))?.scrollIntoView?.({ block: "nearest" });
  }, [active, listId]);

  const targetControl = selected ? controlAt(page, selected) : null;
  const targetName = selected ? (targetControl?.label ?? describePos(selected)) : null;

  const assign = (def: ActionDefinition | undefined) => {
    if (!def) return;
    if (requestAssign({ definitionId: def.id, slot })) setNote(`“${def.name}” is in the draft for ${targetName ?? "the pad"}. Review its settings, then save.`);
  };

  const move = (delta: number) => {
    if (results.length === 0) return;
    const at = Math.max(0, results.findIndex((a) => a.id === active?.id));
    setActiveId(results[Math.min(results.length - 1, Math.max(0, at + delta))]!.id);
  };

  const onKeyDown = (e: KeyboardEvent<HTMLInputElement>) => {
    switch (e.key) {
      case "ArrowDown":
        e.preventDefault();
        return move(1);
      case "ArrowUp":
        e.preventDefault();
        return move(-1);
      case "PageDown":
        e.preventDefault();
        return move(6);
      case "PageUp":
        e.preventDefault();
        return move(-6);
      case "Enter":
        e.preventDefault();
        return assign(active);
      case "Escape":
        if (query) {
          e.preventDefault();
          setQuery("");
        } else {
          e.currentTarget.blur();
        }
    }
  };

  if (!open) {
    return (
      <aside className="library rail" aria-label="Action Library">
        <button type="button" className="rail-btn" aria-expanded="false" aria-label="Open the Action Library" onClick={() => setOpen(true)}>
          <span aria-hidden="true">›</span>
          <span className="rail-text">Actions</span>
          <em className="count">{index.catalog.actions.length}</em>
        </button>
      </aside>
    );
  }

  const stopped = index.catalog.providers.filter((p) => p.status !== "ready");
  const status = results.length === 0 ? (searching ? `Nothing matches “${query.trim()}”.` : "No actions in this category.") : `${plural(results.length, "action")}${searching ? " found" : ""}`;

  return (
    <aside className="library" aria-label="Action Library">
      <header className="library-head">
        <div>
          <h3>Actions</h3>
          <p className="hint">Find one, then give it to a pad.</p>
        </div>
        <button type="button" className="icon-btn" aria-expanded="true" aria-label="Collapse the Action Library" onClick={() => setOpen(false)}>
          ‹
        </button>
      </header>

      <div className="library-search">
        <input
          ref={inputRef}
          role="combobox"
          aria-label="Search actions"
          aria-expanded="true"
          aria-controls={listId}
          aria-activedescendant={active ? optionId(listId, active.id) : undefined}
          aria-autocomplete="list"
          autoComplete="off"
          spellCheck={false}
          placeholder="Search actions"
          value={query}
          onChange={(e) => {
            setQuery(e.target.value);
            setNote("");
          }}
          onKeyDown={onKeyDown}
        />
        <kbd aria-hidden="true" title="Press / to search">
          /
        </kbd>
      </div>

      <label className="library-category">
        <span>Category</span>
        <select value={category ?? ""} onChange={(e) => setCategory(e.target.value || null)}>
          <option value="">All categories ({index.catalog.actions.length})</option>
          {categories.map((c) => (
            <option key={c.name} value={c.name}>
              {c.name} ({c.count})
            </option>
          ))}
        </select>
      </label>

      <p className="library-status hint" role="status" aria-live="polite">
        {status}
      </p>

      <div className="library-list" role="listbox" id={listId} aria-label="Actions">
        {groups.map((g) => (
          <div key={g.category || "results"} role="group" aria-label={g.category || "Results"}>
            {g.category && (
              <div className="library-group" aria-hidden="true">
                {g.category}
              </div>
            )}
            {g.actions.map((a) => (
              <ActionRow
                key={a.id}
                def={a}
                optionDomId={optionId(listId, a.id)}
                active={a.id === active?.id}
                onActivate={() => {
                  setActiveId(a.id);
                  inputRef.current?.focus();
                }}
                onAssign={() => assign(a)}
                onDragStart={() => setDragging(true)}
                onDragEnd={() => setDragging(false)}
              />
            ))}
          </div>
        ))}
        {results.length === 0 && (
          <div className="library-empty">
            <p>{searching ? "Try fewer words, or part of a name. Search covers names, keywords, categories, sources and descriptions." : "Nothing here."}</p>
            {(searching || category) && (
              <button
                type="button"
                className="btn small"
                onClick={() => {
                  setQuery("");
                  setCategory(null);
                }}
              >
                Show everything
              </button>
            )}
          </div>
        )}
      </div>

      {stopped.length > 0 && (
        <details className="library-note">
          <summary>
            <b>{plural(stopped.length, "source")} not running.</b> Their actions stay listed as unavailable.
          </summary>
          <ul>
            {stopped.map((p) => (
              <li key={p.id}>
                <span>{p.name}</span>: {p.reason ?? p.status}
              </li>
            ))}
          </ul>
          <button type="button" className="link" onClick={() => setView("plugins")}>
            Open Plugins
          </button>
        </details>
      )}
      {index.catalog.issues.length > 0 && (
        <details className="library-note">
          <summary>{plural(index.catalog.issues.length, "problem")} with providers</summary>
          <ul>
            {index.catalog.issues.map((m, i) => (
              <li key={i}>{m}</li>
            ))}
          </ul>
        </details>
      )}

      {active && (
        <section className="library-detail" aria-label={`About ${active.name}`}>
          <div className="detail-title">
            <ActionGlyph def={active} />
            <h4>{active.name}</h4>
          </div>
          <div className="tags">
            <SourceTag index={index} def={active} />
            <PrivilegedTag def={active} />
            <UnavailableTag def={active} />
          </div>
          <p className="detail-text">{active.description}</p>
          {unavailableNote(active) && <p className="detail-reason">Unavailable now: {unavailableNote(active)}</p>}
          {active.params.length > 0 && (
            <p className="hint">
              Settings: {active.params.map((p) => (p.required && p.default === undefined ? `${p.label} (needed)` : p.label)).join(", ")}
            </p>
          )}
          <div className="detail-assign">
            <div className="slot-pick" role="group" aria-label="Assign as">
              {SLOTS.map((s) => (
                <button key={s} type="button" className="chip" aria-pressed={slot === s} onClick={() => setSlot(s)}>
                  {SLOT_SHORT[s]}
                </button>
              ))}
            </div>
            <button type="button" className="btn primary" disabled={!selected} onClick={() => assign(active)}>
              {targetName ? `Assign to ${targetName}` : "Assign to a pad"}
            </button>
          </div>
          <p className="hint">{selected ? "Or drag the row onto any pad." : "Select a pad, or drag the row onto one."}</p>
          {note && (
            <p className="hint good" role="status">
              {note}
            </p>
          )}
        </section>
      )}
    </aside>
  );
}

function ActionRow({
  def,
  optionDomId,
  active,
  onActivate,
  onAssign,
  onDragStart,
  onDragEnd,
}: {
  def: ActionDefinition;
  optionDomId: string;
  active: boolean;
  onActivate: () => void;
  onAssign: () => void;
  onDragStart: () => void;
  onDragEnd: () => void;
}) {
  const index = useStore((s) => s.catalog);
  const reason = unavailableNote(def);
  return (
    <div
      id={optionDomId}
      role="option"
      aria-selected={active}
      className={`lib-row${active ? " active" : ""}${reason ? " is-unavailable" : ""}`}
      draggable
      title={reason ? `${def.description}\nUnavailable now: ${reason}` : def.description}
      onClick={onActivate}
      onDoubleClick={onAssign}
      onDragStart={(e) => {
        e.dataTransfer.setData(DRAG_TYPE, def.id);
        e.dataTransfer.setData("text/plain", def.name);
        e.dataTransfer.effectAllowed = "copy";
        onActivate();
        onDragStart();
      }}
      onDragEnd={onDragEnd}
    >
      <ActionGlyph def={def} />
      <span className="lib-main">
        <span className="lib-name">{def.name}</span>
        <span className="lib-sub">
          <SourceTag index={index} def={def} />
          <PrivilegedTag def={def} />
          <UnavailableTag def={def} />
        </span>
      </span>
    </div>
  );
}
