// The words of the import and export views. Rust decides what a file holds,
// what an import would do and what is wrong; this only says it.

import { plural } from "../../lib/format";
import type { ArtifactKind, DependencyCheck, DependencyStatus, DeviceFit, ExistingInstance, ExportPreview, ImportReport, ReviewKind, RuleOutcome } from "../../lib/schema";
import type { AskedType } from "./request";

export const KIND: Record<ArtifactKind, string> = { surface: "Surface file", pack: "Pack" };

/** Integrity is checked; who made the file is not. */
export const PUBLISHER = "Publisher: not verified";

export const NOTHING_CHANGED = "Nothing was changed.";
export const STALE = "Something changed since this preview; check it again.";
export const EXPIRED = "This import is no longer waiting: another file was opened, or it was finished or cancelled. Choose the file again.";
export const AUTO_NEVER = "An import never turns Auto Context on.";
export const SHARE_NOTE = "A file holds surfaces and, in a Pack, context rules and questions for the person importing it. It never holds passwords, plugin settings, permissions or anything about this computer.";

/** A size as a person reads it. */
export function size(bytes: number): string {
  if (bytes < 1024) return plural(bytes, "byte");
  if (bytes < 1024 * 1024) return `${Math.round(bytes / 1024)} KB`;
  return `${Number((bytes / (1024 * 1024)).toFixed(1))} MB`;
}

export const tooLarge = (limit: number): string => `This file is larger than ${size(limit)}, the most LaunchDeck reads. ${NOTHING_CHANGED}`;

/** How the file fits a device. Never a reason to refuse it. */
export function fitText(d: DeviceFit): string {
  const parts: string[] = [];
  if (!d.fits) parts.push(`${plural(d.outside, "control")} ${d.outside === 1 ? "is" : "are"} outside it and will not show there.`);
  if (!d.rgbOk) parts.push("It shows no colour, and the file uses colour.");
  return parts.length > 0 ? parts.join(" ") : "Every control fits.";
}

/** An earlier import of the same file, compared with this one. */
export function instanceText(e: ExistingInstance): string {
  const versions = { same: "this file has the same version", newer: "this file is newer", older: "this file is older" }[e.relation];
  return `Copy ${e.instance}, version ${e.version}: ${versions}${e.identicalContent ? ", with the same content" : ""}.`;
}

/** What happens to a context rule of the file. `input` names an input by its label. */
export function ruleOutcomeText(outcome: RuleOutcome, input: (id: string) => string): string {
  switch (outcome.kind) {
    case "add":
      return `Added as rule ${outcome.position + 1}.`;
    case "notSelected":
      return "Not added unless you select it.";
    case "skipConflict":
      return `Not added: your rule ${outcome.localRule + 1} already names this application, and yours stays.`;
    case "skipShadowed":
      return `Not added: rule ${outcome.by + 1} of this file names the same application.`;
    case "skipNoIdentity":
      return "Not added: the file does not name this application for this system.";
    case "skipCapacity":
      return "Not added: you already have as many rules as LaunchDeck keeps.";
    case "needsInput":
      return `Waits for your answer to “${input(outcome.input)}” below.`;
  }
}

export const DEPENDENCY_TAG: Record<DependencyStatus["kind"], string> = {
  compatible: "Compatible",
  disabled: "Disabled",
  incompatible: "Incompatible",
  missing: "Missing",
};

export function dependencyText(d: DependencyCheck): string {
  const s = d.status;
  switch (s.kind) {
    case "compatible":
      return `Version ${s.installed} is installed.`;
    case "disabled":
      return `Version ${s.installed} is installed but off, or failed to start. Its pads do nothing until it runs.`;
    case "incompatible":
      return `Version ${s.installed} is installed; the file asks for ${d.requirement}. Some pads may not work.`;
    case "missing":
      return `Not installed. ${d.required ? "Its pads" : "Its looks"} will not work until it is; the import keeps them as they are.`;
  }
}

export const REVIEW_KIND: Record<ReviewKind, string> = {
  runsCommand: "Runs a command",
  sendsRequest: "Sends a request",
  launchesProgram: "Starts a program",
  instructsAgent: "Gives an agent an instruction",
};

/** What an import did, in a few sentences. */
export function reportText(r: ImportReport): string[] {
  const out = [`Added ${plural(r.surfacesAdded.length, "surface")} and ${plural(r.rulesAdded, "rule")}.`];
  if (r.surfacesReplaced.length > 0) out.push(`Replaced ${plural(r.surfacesReplaced.length, "surface")} in place.`);
  if (r.rulesSkipped > 0) out.push(`${plural(r.rulesSkipped, "selected rule was", "selected rules were")} not added.`);
  if (r.slotsRemoved > 0) out.push(`The actions of ${plural(r.slotsRemoved, "pad")} that ran things were removed.`);
  const pads = r.missingDependencies.reduce((n, d) => n + d.usedBy.length, 0);
  if (r.missingDependencies.length > 0) {
    const names = r.missingDependencies.map((d) => `${d.name} ${d.requirement}`).join(", ");
    out.push(`${plural(pads, "pad needs", "pads need")} a plugin: ${names}.`);
  }
  if (r.rulesAdded > 0 && !r.autoContext) out.push("Auto Context is off: the new rules apply once you turn it on in Settings.");
  return out;
}

/** The systems a context rule can also name its application on, for export. */
export const SYSTEMS: ReadonlyArray<{ namespace: string; label: string; example: string }> = [
  { namespace: "macos", label: "macOS", example: "com.microsoft.VSCode" },
  { namespace: "windows", label: "Windows", example: "Code.exe" },
  { namespace: "x11", label: "Linux (X11)", example: "Code" },
];

export const systemName = (namespace: string): string => SYSTEMS.find((s) => s.namespace === namespace)?.label ?? namespace;

export const ASK = "Ask the person importing this";

export const ASKED_TYPE: Record<AskedType, string> = { path: "A path", url: "A web address", string: "Text" };

/** What a file never carries, counted, as one sentence; empty when nothing was left out. */
export function strippedText(s: ExportPreview["stripped"]): string {
  const parts = [
    s.legacyBindings > 0 ? plural(s.legacyBindings, "binding from an older version", "bindings from an older version") : "",
    s.legacyActions > 0 ? plural(s.legacyActions, "action only an older version knew", "actions only an older version knew") : "",
    s.contextHints > 0 ? plural(s.contextHints, "context hint") : "",
  ].filter(Boolean);
  return parts.length > 0 ? `Left out: ${parts.join(", ")}.` : "";
}
