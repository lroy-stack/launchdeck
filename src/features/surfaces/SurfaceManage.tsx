// Managing a surface while editing: its name, description and icon where they are shown, deleting it, and the
// menu of each page (rename, delete). Rust checks every change and says what is wrong; nothing is deleted
// before the person confirms.

import { useState, type KeyboardEvent } from "react";
import { api } from "../../lib/ipc";
import { plural } from "../../lib/format";
import type { ContextSettings, Page, Surface } from "../../lib/schema";
import { useStore } from "../../stores/runtime";

interface Details {
  name: string;
  description: string;
  icon: string;
}

const detailsOf = (s: Surface): Details => ({ name: s.name, description: s.description, icon: s.icon ?? "" });
const same = (a: Details, b: Details) => a.name === b.name && a.description === b.description && a.icon === b.icon;

/** The surface's icon, name and description as fields in the header. Enter or leaving a field saves; Escape undoes. */
export function SurfaceDetails({ surface }: { surface: Surface }) {
  const attempt = useStore((s) => s.attempt);
  const saved = detailsOf(surface);
  const [draft, setDraft] = useState<Details>(saved);
  // What Rust sends back is what the fields show, after a save or a change made elsewhere.
  const [shown, setShown] = useState<Details>(saved);
  if (!same(shown, saved)) {
    setShown(saved);
    setDraft(saved);
  }

  const save = () => {
    if (same(draft, saved)) return;
    void attempt(() => api.updateSurface(surface.id, draft.name, draft.description, draft.icon.trim() || null)).then((ok) => {
      if (!ok) setDraft(saved);
    });
  };
  const field = (key: keyof Details) => ({
    value: draft[key],
    onChange: (e: { target: { value: string } }) => setDraft({ ...draft, [key]: e.target.value }),
    onBlur: save,
    onKeyDown: (e: KeyboardEvent) => {
      if (e.key === "Escape") setDraft(saved);
    },
  });

  return (
    <form
      className="surface-details"
      onSubmit={(e) => {
        e.preventDefault();
        save();
      }}
    >
      <span className="eyebrow">{surface.category}</span>
      <div className="row">
        <input className="surface-icon" aria-label="Surface icon" placeholder="Icon" {...field("icon")} />
        <input className="surface-name" aria-label="Surface name" {...field("name")} />
      </div>
      <input className="surface-description" aria-label="Surface description" placeholder="What this surface is for" {...field("description")} />
      {/* Enter in any field saves. */}
      <button type="submit" hidden />
    </form>
  );
}

/** The Auto Context rules and fallback that show this surface: Rust removes or resets them with it. */
function contextOf(settings: ContextSettings | undefined, surfaceId: string) {
  const rules = (settings?.rules ?? []).filter((r) => r.surfaceId === surfaceId);
  const fallback = settings?.fallback.kind === "surface" && settings.fallback.surfaceId === surfaceId;
  return { rules, fallback };
}

/** Delete surface, with a confirmation that names what goes with it. */
export function DeleteSurface({ surface }: { surface: Surface }) {
  const attempt = useStore((s) => s.attempt);
  const select = useStore((s) => s.select);
  const workspace = useStore((s) => s.workspace);
  // Auto Context as Rust last reported it: the same settings the workspace saves.
  const context = useStore((s) => s.context?.settings);
  const [asking, setAsking] = useState(false);
  const only = (workspace?.surfaces.length ?? 0) <= 1;

  if (!asking) {
    return (
      <button type="button" className="btn small danger" disabled={only} title={only ? "The only surface cannot be deleted." : undefined} onClick={() => setAsking(true)}>
        Delete surface
      </button>
    );
  }
  const { rules, fallback } = contextOf(context, surface.id);
  return (
    <div className="confirm" role="alertdialog" aria-label={`Delete ${surface.name}`}>
      <p>
        Delete {surface.name} and its {plural(surface.pages.length, "page")}? This cannot be undone.
      </p>
      {rules.length > 0 && (
        <>
          <p>These Auto Context rules show it and are removed with it:</p>
          <ul className="plain bullets">
            {rules.map((r) => (
              <li key={`${r.application.namespace}:${r.application.id}`}>{r.application.displayName || r.application.id}</li>
            ))}
          </ul>
        </>
      )}
      {fallback && <p>Auto Context shows it when no rule applies; that goes back to keeping the surface on screen.</p>}
      <div className="row">
        <button
          type="button"
          className="btn danger"
          onClick={() => {
            setAsking(false);
            select(null);
            void attempt(() => api.removeSurface(surface.id));
          }}
        >
          Delete
        </button>
        <button type="button" className="btn" onClick={() => setAsking(false)}>
          Cancel
        </button>
      </div>
    </div>
  );
}

type PageStep = "closed" | "menu" | "rename" | "delete";

/** One page in the Edit strip, with its menu: Rename in place, Delete after a confirmation. */
export function PageItem({ surface, page, current, onOpen }: { surface: Surface; page: Page; current: boolean; onOpen: () => void }) {
  const attempt = useStore((s) => s.attempt);
  const select = useStore((s) => s.select);
  const [step, setStep] = useState<PageStep>("closed");
  const [name, setName] = useState(page.name);
  const only = surface.pages.length <= 1;

  if (step === "rename") {
    const done = () => setStep("closed");
    return (
      <form
        className="page-new"
        onSubmit={(e) => {
          e.preventDefault();
          done();
          if (name.trim() !== page.name) void attempt(() => api.renamePage(surface.id, page.id, name));
        }}
      >
        <input autoFocus aria-label="Page name" value={name} onChange={(e) => setName(e.target.value)} onBlur={done} onKeyDown={(e) => e.key === "Escape" && done()} />
      </form>
    );
  }

  return (
    <span className="page-slot">
      <button type="button" className={`page-item${current ? " on" : ""}`} aria-current={current ? "page" : undefined} onClick={onOpen}>
        <span>{page.name}</span>
        <small>{plural(page.controls.length, "control")}</small>
      </button>
      <button type="button" className="icon-btn" aria-label={`Page options for ${page.name}`} aria-expanded={step === "menu"} onClick={() => setStep(step === "menu" ? "closed" : "menu")}>
        ⋯
      </button>
      {step === "menu" && (
        <span className="page-menu" role="menu" aria-label={`Page ${page.name}`}>
          <button
            type="button"
            role="menuitem"
            onClick={() => {
              setName(page.name);
              setStep("rename");
            }}
          >
            Rename
          </button>
          <button type="button" role="menuitem" disabled={only} title={only ? "A surface keeps at least one page." : undefined} onClick={() => setStep("delete")}>
            Delete
          </button>
        </span>
      )}
      {step === "delete" && (
        <span className="confirm" role="alertdialog" aria-label={`Delete page ${page.name}`}>
          <span>
            Delete the page {page.name} and its {plural(page.controls.length, "control")}?
          </span>
          <button
            type="button"
            className="btn small danger"
            onClick={() => {
              setStep("closed");
              // Rust shows the surface's first page in its place, as opening a page would: nothing stays selected.
              if (current) select(null);
              void attempt(() => api.removePage(surface.id, page.id));
            }}
          >
            Delete
          </button>
          <button type="button" className="btn small" onClick={() => setStep("closed")}>
            Cancel
          </button>
        </span>
      )}
    </span>
  );
}
