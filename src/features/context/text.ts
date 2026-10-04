// The words Auto Context is explained with. Rust decides and reports; this
// only says it, the same way in the sidebar and in Settings.

import type { ApplicationIdentity, ContextMode, ContextStatus, RuleProblem, Workspace } from "../../lib/schema";

export const MODE: Record<ContextMode, string> = {
  off: "Off",
  auto: "On",
  manual: "Manual override",
  paused: "Paused while editing",
  unavailable: "Not available here",
};

/** What kind of identity an application has, for the detail under its name. */
export const NAMESPACE: Record<string, string> = {
  macos: "macOS bundle",
  windows: "Windows executable",
  x11: "X11 window class",
};

/** `x11:Code`: the stable identity, as it is saved. */
export const identityOf = (a: ApplicationIdentity): string => `${a.namespace}:${a.id}`;

export const nameOf = (a: ApplicationIdentity): string => a.displayName || a.id;

/** "Development", or "Development / Git" when a page is named. Says so when the surface is gone. */
export function targetName(workspace: Workspace | null, surfaceId?: string, pageId?: string): string {
  if (!surfaceId) return "";
  const surface = workspace?.surfaces.find((s) => s.id === surfaceId);
  if (!surface) return `a surface that no longer exists (${surfaceId})`;
  const page = pageId ? surface.pages.find((p) => p.id === pageId) : undefined;
  return page ? `${surface.name} / ${page.name}` : surface.name;
}

/** Why the surface is what it is, in a sentence or two. */
export function explain(status: ContextStatus, workspace: Workspace | null): string {
  const d = status.decision;
  if (!d) {
    if (status.observer.state === "unavailable") return "Nothing is decided automatically on this system.";
    return "No application has come to the front yet, so nothing has been decided.";
  }
  const who = nameOf(d.application);
  const matched = d.rule === undefined ? `No rule names ${who}` : `Rule ${d.rule + 1} matches ${who}`;
  const target = targetName(workspace, d.surfaceId, d.pageId);
  let said: string;
  switch (d.outcome) {
    case "kept":
      said = `${matched}, and the fallback is to keep the current surface.`;
      break;
    case "missingSurface":
      said = `${matched}, but ${d.rule === undefined ? "the fallback names" : "it names"} ${target}. Nothing was changed.`;
      break;
    case "alreadyShown":
      said = `${matched}${d.rule === undefined ? ", so the fallback applies" : ""}: ${target} was already shown.`;
      break;
    case "activated":
      said = d.held ? `${matched}${d.rule === undefined ? ", so the fallback applies" : ""}: it calls for ${target}.` : `${matched}${d.rule === undefined ? ", so the fallback applied" : ""}: ${target} was activated.`;
      break;
  }
  if (d.pageMissing) said += " The page the rule names no longer exists, so the surface's first page stands in.";
  if (d.held === "off") said += " Not applied: Auto Context is off.";
  if (d.held === "paused") said += " Not applied while editing. It will be when you go back to Live.";
  if (d.held === "manual") said += " Not applied: you chose a surface yourself. That holds until another application comes to the front, or you resume.";
  return said;
}

export function problemText(problem: RuleProblem): string {
  switch (problem.kind) {
    case "missingSurface":
      return "Its surface no longer exists. The rule still wins for this application and shows nothing. Choose another surface or remove it.";
    case "missingPage":
      return "Its page no longer exists. The surface opens on its first page.";
    case "shadowed":
      return `Never reached: rule ${problem.by + 1} names the same application and comes first.`;
    case "otherSystem":
      return "Written on another kind of system. It never matches on this one.";
  }
}
