// What the person chooses in the export view, and the ExportRequest it makes.
// Rust decides everything else: the proposed id, name and version, what the
// file holds, what is left out, and what is wrong (artifacts_export_preview).

import type { ExportPreview, ExportRequest, InputDef, Location, Workspace } from "../../lib/schema";

/** The types a text argument can be asked as. */
export type AskedType = "path" | "url" | "string";

/** A pad's text argument the person importing is asked for, instead of the value here. */
export interface AskedValue {
  at: Location;
  arg: string;
  type: AskedType;
  label: string;
  /** The local id of its surface, so that it goes when the surface is no longer chosen. */
  surface: string;
}

/** A context rule (by its position in the workspace) whose application the person importing chooses. */
export interface AskedApplication {
  rule: number;
  label: string;
  surface: string;
}

/** The id a rule's application has on another system. */
export interface ExtraIdentity {
  rule: number;
  namespace: string;
  id: string;
  surface: string;
}

export interface MetadataForm {
  id: string;
  name: string;
  version: string;
  description: string;
  author: string;
  authorUrl: string;
  license: string;
}

export interface ExportForm {
  /** Local ids, in workspace order. */
  surfaceIds: string[];
  metadata: MetadataForm;
  includeContextRules: boolean;
  /** In the order they were asked. */
  asked: AskedValue[];
  askedApps: AskedApplication[];
  extras: ExtraIdentity[];
}

export const emptyForm = (surfaceIds: string[]): ExportForm => ({
  surfaceIds,
  metadata: { id: "", name: "", version: "", description: "", author: "", authorUrl: "", license: "" },
  includeContextRules: false,
  asked: [],
  askedApps: [],
  extras: [],
});

const sameLocation = (a: Location, b: Location): boolean =>
  a.surfaceSourceId === b.surfaceSourceId && a.pageId === b.pageId && a.controlId === b.controlId && a.slot === b.slot;

/** The same argument of the same press or long press. */
export const sameArg = (a: { at: Location; arg: string }, b: { at: Location; arg: string }): boolean => a.arg === b.arg && sameLocation(a.at, b.at);

const text = (value: string): string | undefined => value.trim() || undefined;

/**
 * The request for `form`. Inputs are numbered in the order they appear
 * (`input_1`, `input_2`, ...); a rule's choices count only while rules are
 * included, and a rule asked of the person importing gets no other ids. The
 * kind is a Surface file for exactly one surface with no rules and nothing
 * asked, a Pack otherwise.
 */
export function buildRequest(form: ExportForm): ExportRequest {
  const askedApps = form.includeContextRules ? form.askedApps : [];
  const extras = form.includeContextRules ? form.extras.filter((e) => !askedApps.some((a) => a.rule === e.rule)) : [];
  const valueId = (i: number) => `input_${i + 1}`;
  const appId = (j: number) => `input_${form.asked.length + j + 1}`;
  const inputs: InputDef[] = [
    ...form.asked.map((a, i) => ({ id: valueId(i), type: a.type, label: a.label.trim() })),
    ...askedApps.map((a, j) => ({ id: appId(j), type: "application" as const, label: a.label.trim() })),
  ];
  const single = form.surfaceIds.length === 1 && !form.includeContextRules && inputs.length === 0;
  const m = form.metadata;
  const url = text(m.authorUrl);
  const author = text(m.author) || url ? { name: m.author.trim(), ...(url ? { url } : {}) } : undefined;
  return {
    kind: single ? "surface" : "pack",
    surfaceIds: form.surfaceIds,
    metadata: {
      id: m.id.trim(),
      name: m.name.trim(),
      version: m.version.trim(),
      ...(text(m.description) ? { description: m.description.trim() } : {}),
      ...(author ? { author } : {}),
      ...(text(m.license) ? { license: m.license.trim() } : {}),
    },
    includeContextRules: form.includeContextRules,
    inputs,
    bindings: form.asked.map((a, i) => ({ at: a.at, arg: a.arg, input: valueId(i) })),
    ruleInputs: askedApps.map((a, j) => ({ rule: a.rule, input: appId(j) })),
    extraIdentities: extras.map((e) => ({ rule: e.rule, namespace: e.namespace, id: e.id })),
  };
}

/** `form` with surface `id` chosen or not, in workspace order, and nothing asked about a surface no longer chosen. */
export function withSurface(form: ExportForm, order: string[], id: string, on: boolean): ExportForm {
  const surfaceIds = order.filter((s) => (s === id ? on : form.surfaceIds.includes(s)));
  if (on) return { ...form, surfaceIds };
  const kept = <T extends { surface: string }>(list: T[]) => list.filter((x) => x.surface !== id);
  return { ...form, surfaceIds, asked: kept(form.asked), askedApps: kept(form.askedApps), extras: kept(form.extras) };
}

/** The local id of the surface the file calls `artifactId`. */
export const localSurface = (preview: ExportPreview, artifactId: string): string | undefined => preview.surfaces.find((s) => s.artifactId === artifactId)?.localId;

/** The label of the pad at `at`, from the workspace; its id when it has none. */
export function padLabel(workspace: Workspace | null, preview: ExportPreview, at: Location): string {
  const surface = workspace?.surfaces.find((s) => s.id === localSurface(preview, at.surfaceSourceId));
  const control = surface?.pages.find((p) => p.id === at.pageId)?.controls.find((c) => c.id === at.controlId);
  return control?.label || at.controlId;
}

/** The name of the page at `at`, from the workspace; its id when it is not found. */
export function pageName(workspace: Workspace | null, preview: ExportPreview, at: Location): string {
  const surface = workspace?.surfaces.find((s) => s.id === localSurface(preview, at.surfaceSourceId));
  return surface?.pages.find((p) => p.id === at.pageId)?.name ?? at.pageId;
}
