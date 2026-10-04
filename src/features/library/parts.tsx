// Small pieces the Library, the Inspector and the surface share, so an action
// looks the same wherever it appears: its glyph, where it comes from, and
// whether it can run. State is always text, never colour alone.

import { inkFor } from "../../lib/color";
import { sourceOf, type CatalogIndex } from "../../lib/actions";
import type { ActionDefinition } from "../../lib/schema";

const NEUTRAL = "#2a2a36";

/** The action's own glyph and colour when it suggests one, otherwise its category's initial. */
export function ActionGlyph({ def }: { def: ActionDefinition }) {
  const color = def.appearance?.color ?? NEUTRAL;
  const glyph = def.appearance?.icon ?? def.category.slice(0, 1).toUpperCase();
  return (
    <span className="lib-glyph" aria-hidden="true" style={{ background: color, color: inkFor(color) }}>
      {glyph}
    </span>
  );
}

export function SourceTag({ index, def }: { index: CatalogIndex; def: ActionDefinition }) {
  return <span className={`tag source ${index.providers.get(def.provider)?.kind ?? ""}`}>{sourceOf(index, def)}</span>;
}

export function UnavailableTag({ def }: { def: ActionDefinition }) {
  if (def.availability.status === "available") return null;
  return (
    <span className="tag unavailable" title={def.availability.reason}>
      Unavailable
    </span>
  );
}

export function PrivilegedTag({ def }: { def: ActionDefinition }) {
  return def.privileged ? (
    <span className="tag privileged" title="Runs on this computer or sends a request from it">
      Privileged
    </span>
  ) : null;
}
