import { useState } from "react";
import { api } from "../../lib/ipc";
import { plural } from "../../lib/format";
import type { Mode, Page, Surface } from "../../lib/schema";
import { activePage, activeSurface, useStore } from "../../stores/runtime";
import { Inspector } from "../editor/Inspector";
import { ActionLibrary } from "../library/ActionLibrary";
import { LivePanel } from "./LivePanel";
import { PadGrid } from "../../components/PadGrid";

export function SurfaceView() {
  const workspace = useStore((s) => s.workspace);
  const state = useStore((s) => s.state);
  const libraryOpen = useStore((s) => s.libraryOpen);
  const frame = useStore((s) => s.frame);
  const attempt = useStore((s) => s.attempt);
  const select = useStore((s) => s.select);
  const showLabels = useStore((s) => s.showLabels);
  const toggleLabels = useStore((s) => s.toggleLabels);
  const startExport = useStore((s) => s.startExport);

  const core = { workspace, state };
  const surface = activeSurface(core);
  const page = activePage(core);
  if (!workspace || !state || !surface || !page) return <NoSurface first={workspace?.surfaces[0] ?? null} />;

  const mode = state.mode;
  const device = state.devices.find((d) => d.connected) ?? state.devices[0] ?? null;
  const setMode = (m: Mode) => {
    if (m === mode) return;
    select(null);
    void attempt(() => api.setMode(m));
  };

  return (
    <div className={`surface-view mode-${mode}`}>
      <header className="surface-head">
        <div className="titles">
          <span className="eyebrow">{surface.category}</span>
          <h1>{surface.name}</h1>
          <p>{surface.description}</p>
        </div>
        <div className="head-actions">
          <button type="button" className="btn small" onClick={() => startExport(surface.id)}>
            Export…
          </button>
          <label className="check">
            <input type="checkbox" checked={showLabels} onChange={toggleLabels} /> Labels
          </label>
          <div className="segmented" role="tablist" aria-label="Mode">
            <button type="button" role="tab" aria-selected={mode === "live"} className={mode === "live" ? "on live" : ""} onClick={() => setMode("live")}>
              <i /> Live
            </button>
            <button type="button" role="tab" aria-selected={mode === "edit"} className={mode === "edit" ? "on edit" : ""} onClick={() => setMode("edit")}>
              Edit
            </button>
          </div>
        </div>
      </header>

      <div className="surface-body" data-library={mode === "edit" ? (libraryOpen ? "open" : "rail") : "none"}>
        {mode === "live" ? <PagesNav surface={surface} page={page} layout="column" /> : <ActionLibrary page={page} />}

        <main className="stage">
          {mode === "edit" && <PagesNav surface={surface} page={page} layout="strip" />}
          <div className="stage-banner">
            {mode === "live" ? (
              <span>
                <b>Live.</b> Press pads to run actions. Click and hold on the virtual device, or use the real Launchpad.
              </span>
            ) : (
              <span>
                <b>Editing.</b> Actions are paused. Select a pad, then choose or drop an action from the Library.
              </span>
            )}
            <span className="hint">
              {page.name} · {device ? device.identity.name : "No device"}
            </span>
          </div>
          {mode === "live" && page.controls.length === 0 && (
            <div className="slot-empty">
              <p>This page has no pads yet. Switch to Edit to add some.</p>
              <button type="button" className="btn small" onClick={() => setMode("edit")}>
                Edit
              </button>
            </div>
          )}
          <PadGrid page={page} frame={frame} mode={mode} device={device} />
        </main>

        {mode === "edit" ? <Inspector surface={surface} page={page} surfaces={workspace.surfaces} /> : <LivePanel surface={surface} />}
      </div>
    </div>
  );
}

/** Nothing on screen yet: one sentence, and the first surface one click away. */
function NoSurface({ first }: { first: Surface | null }) {
  const attempt = useStore((s) => s.attempt);
  return (
    <div className="empty-state">
      <p>No surface selected.</p>
      {first && (
        <button type="button" className="btn small" onClick={() => void attempt(() => api.activateSurface(first.id))}>
          Open {first.name}
        </button>
      )}
    </div>
  );
}

/** The pages of a surface: a column beside the deck while playing, a strip above it while editing. */
function PagesNav({ surface, page, layout }: { surface: Surface; page: Page; layout: "column" | "strip" }) {
  const mode = useStore((s) => s.state?.mode);
  const attempt = useStore((s) => s.attempt);
  const select = useStore((s) => s.select);
  const [newPage, setNewPage] = useState<string | null>(null);
  return (
    <nav className={layout === "strip" ? "pages strip" : "pages"} aria-label="Pages">
      {layout === "column" && <h4>Pages</h4>}
      {surface.pages.map((p) => (
        <button
          key={p.id}
          type="button"
          className={`page-item${p.id === page.id ? " on" : ""}`}
          aria-current={p.id === page.id ? "page" : undefined}
          onClick={() => {
            select(null);
            void attempt(() => api.activatePage(p.id));
          }}
        >
          <span>{p.name}</span>
          <small>{plural(p.controls.length, "control")}</small>
        </button>
      ))}
      {mode === "edit" &&
        (newPage === null ? (
          <button type="button" className="page-item add" onClick={() => setNewPage("")}>
            + Add page
          </button>
        ) : (
          <form
            className="page-new"
            onSubmit={(e) => {
              e.preventDefault();
              const name = newPage.trim();
              if (!name) return setNewPage(null);
              void attempt(async () => {
                const id = await api.addPage(surface.id, name);
                await api.activatePage(id);
              });
              setNewPage(null);
            }}
          >
            <input autoFocus aria-label="Page name" placeholder="Page name" value={newPage} onChange={(e) => setNewPage(e.target.value)} onBlur={() => !newPage && setNewPage(null)} />
          </form>
        ))}
    </nav>
  );
}
