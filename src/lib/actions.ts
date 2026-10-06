// Finding, describing and configuring actions. Pure functions, unit tested.
//
// Rust owns the catalog and the final validation. This module only reads the
// catalog it was given: it searches it without a round trip per keystroke,
// builds the starting settings of a form, and mirrors the parameter rules so a
// form can mark a mistake as it is typed. The mirror is checked against Rust in
// the tests (every action of the default workspace has to pass it).

import type { ActionDefinition, Assignment, Catalog, Issue, ParamDef, ProviderInfo } from "./schema";
import { definitionId } from "./schema";

export type Args = Record<string, unknown>;

// ---------------------------------------------------------------------------
// The catalog, indexed for lookups and search
// ---------------------------------------------------------------------------

interface Entry {
  def: ActionDefinition;
  /** Lower case, without accents. */
  name: string;
  words: string[];
  keywords: string[];
  category: string;
  source: string;
  description: string;
  id: string;
}

export interface CatalogIndex {
  catalog: Catalog;
  byId: Map<string, ActionDefinition>;
  providers: Map<string, ProviderInfo>;
  entries: Entry[];
}

export const fold = (s: string): string =>
  s
    .toLowerCase()
    .normalize("NFD")
    .replace(/\p{M}/gu, "");

/** "Built-in", the module's own name ("Agent Control"), or "Plugin: Hello". */
export function sourceLabel(provider: ProviderInfo | undefined, fallbackId: string): string {
  if (!provider) return fallbackId;
  switch (provider.kind) {
    case "builtin":
      return "Built-in";
    case "module":
      return provider.name;
    case "plugin":
      return `Plugin: ${provider.name}`;
  }
}

export function indexCatalog(catalog: Catalog): CatalogIndex {
  const providers = new Map(catalog.providers.map((p) => [p.id, p]));
  const byId = new Map(catalog.actions.map((a) => [a.id, a]));
  const entries = catalog.actions.map((def): Entry => {
    const name = fold(def.name);
    return {
      def,
      name,
      words: name.split(/[^a-z0-9]+/).filter(Boolean),
      keywords: def.keywords.map(fold),
      category: fold(def.category),
      source: fold(`${sourceLabel(providers.get(def.provider), def.provider)} ${providers.get(def.provider)?.name ?? ""}`),
      description: fold(def.description),
      id: fold(def.id),
    };
  });
  return { catalog, byId, providers, entries };
}

export const emptyIndex: CatalogIndex = indexCatalog({ providers: [], actions: [], issues: [] });

export const sourceOf = (index: CatalogIndex, def: ActionDefinition): string => sourceLabel(index.providers.get(def.provider), def.provider);

// ---------------------------------------------------------------------------
// Search: tokens, every one of which must match; typos and gaps tolerated
// ---------------------------------------------------------------------------

/** True when `a` and `b` differ by at most one insertion, deletion, substitution or swap. */
export function withinOneEdit(a: string, b: string): boolean {
  if (a === b) return true;
  if (Math.abs(a.length - b.length) > 1) return false;
  let i = 0;
  while (i < a.length && i < b.length && a[i] === b[i]) i++;
  if (a.length === b.length) {
    if (a.slice(i + 1) === b.slice(i + 1)) return true; // one substitution
    return a[i] === b[i + 1] && a[i + 1] === b[i] && a.slice(i + 2) === b.slice(i + 2); // one swap
  }
  const [long, short] = a.length > b.length ? [a, b] : [b, a];
  return long.slice(i + 1) === short.slice(i);
}

const isSubsequence = (needle: string, hay: string): boolean => {
  let at = 0;
  for (const ch of hay) if (ch === needle[at] && ++at === needle.length) return true;
  return needle.length === 0;
};

function scoreToken(e: Entry, t: string): number {
  let best = 0;
  const at = e.name.indexOf(t);
  if (at === 0) best = 14;
  else if (at > 0) best = e.words.some((w) => w.startsWith(t)) ? 11 : 6;

  if (e.keywords.includes(t)) best = Math.max(best, 9);
  else if (e.keywords.some((k) => k.startsWith(t))) best = Math.max(best, 7);
  else if (e.keywords.some((k) => k.includes(t))) best = Math.max(best, 3);

  if (e.category.startsWith(t)) best = Math.max(best, 5);
  else if (e.category.includes(t)) best = Math.max(best, 3);
  if (e.source.includes(t)) best = Math.max(best, 4);
  if (e.description.includes(t)) best = Math.max(best, e.description.split(/[^a-z0-9]+/).some((w) => w.startsWith(t)) ? 2 : 1);
  if (e.id.includes(t)) best = Math.max(best, 1);

  // Nothing matched as written: forgive one typo in a word, or skipped letters in a name that
  // starts the way the word does ("kbshrtct" for Keyboard shortcut). Short words get neither,
  // because three letters are a subsequence of almost anything.
  if (best === 0 && t.length >= 4) {
    if ([...e.words, ...e.keywords].some((w) => withinOneEdit(w, t))) best = 3;
    else if (e.words.some((w) => w[0] === t[0]) && isSubsequence(t, e.name.replace(/[^a-z0-9]/g, ""))) best = 2;
  }
  return best;
}

export const tokensOf = (query: string): string[] => fold(query).split(/[\s,;]+/).filter(Boolean);

export interface SearchOptions {
  /** Only this category; `null` or `undefined` is all of them. */
  category?: string | null;
}

/** Everything that matches, best first. Nothing is dropped for being unavailable. */
export function searchActions(index: CatalogIndex, query: string, options: SearchOptions = {}): ActionDefinition[] {
  const tokens = tokensOf(query);
  const inCategory = (e: Entry) => !options.category || e.def.category === options.category;
  const byName = (a: ActionDefinition, b: ActionDefinition) => a.name.localeCompare(b.name) || a.id.localeCompare(b.id);

  if (tokens.length === 0) {
    return index.entries
      .filter(inCategory)
      .map((e) => e.def)
      .sort((a, b) => a.category.localeCompare(b.category) || byName(a, b));
  }
  const phrase = tokens.join(" ");
  const hits: { def: ActionDefinition; score: number }[] = [];
  for (const e of index.entries) {
    if (!inCategory(e)) continue;
    let total = 0;
    for (const t of tokens) {
      const s = scoreToken(e, t);
      if (s === 0) {
        total = 0;
        break;
      }
      total += s;
    }
    if (total === 0) continue;
    if (e.name.startsWith(phrase)) total += 6;
    if (e.def.availability.status === "available") total += 2;
    hits.push({ def: e.def, score: total });
  }
  return hits.sort((a, b) => b.score - a.score || byName(a.def, b.def)).map((h) => h.def);
}

export interface CategoryCount {
  name: string;
  count: number;
}

export function categoriesOf(index: CatalogIndex): CategoryCount[] {
  const counts = new Map<string, number>();
  for (const a of index.catalog.actions) counts.set(a.category, (counts.get(a.category) ?? 0) + 1);
  return [...counts].map(([name, count]) => ({ name, count })).sort((a, b) => a.name.localeCompare(b.name));
}

export function groupByCategory(actions: ActionDefinition[]): { category: string; actions: ActionDefinition[] }[] {
  const groups: { category: string; actions: ActionDefinition[] }[] = [];
  for (const a of actions) {
    const last = groups[groups.length - 1];
    if (last && last.category === a.category) last.actions.push(a);
    else groups.push({ category: a.category, actions: [a] });
  }
  return groups;
}

// ---------------------------------------------------------------------------
// What an assignment is right now
// ---------------------------------------------------------------------------

export type Resolution =
  | { kind: "ready"; definition: ActionDefinition }
  | { kind: "unavailable"; definition: ActionDefinition; cause: "provider" | "state"; reason: string }
  | { kind: "missing"; reason: string };

export function resolveAssignment(index: CatalogIndex, a: Assignment): Resolution {
  const def = index.byId.get(definitionId(a));
  if (def) {
    return def.availability.status === "available"
      ? { kind: "ready", definition: def }
      : { kind: "unavailable", definition: def, cause: def.availability.cause, reason: def.availability.reason };
  }
  const provider = index.providers.get(a.provider);
  if (provider) {
    return { kind: "missing", reason: `${provider.name} does not offer “${a.action}” any more. The assignment is kept and will work again if it returns.` };
  }
  return { kind: "missing", reason: `The provider “${a.provider}” is not installed. The assignment is kept and will work again when it is.` };
}

export const isPrivileged = (index: CatalogIndex, a: Assignment | null | undefined): boolean => !!a && !!index.byId.get(definitionId(a))?.privileged;

const clip = (s: string, n: number): string => (s.length > n ? `${s.slice(0, n - 1)}…` : s);

/** The settings a person filled in, as a short phrase: `https://github.com · GET`. */
export function describeArgs(def: ActionDefinition, args: Args, max = 2): string {
  const parts: string[] = [];
  for (const p of def.params) {
    const v = args[p.name];
    if (v === undefined || v === null || v === "") continue;
    if (p.type === "boolean") {
      if (v === true) parts.push(p.label);
    } else if (p.type === "enum") {
      parts.push(p.options.find((o) => o.value === v)?.label ?? String(v));
    } else if (p.type === "json") {
      parts.push(clip(JSON.stringify(v), 40));
    } else {
      parts.push(clip(String(v).replace(/\s+/g, " "), 40));
    }
  }
  return parts.slice(0, max).join(" · ");
}

/** One line for lists: the action's name and its main settings. */
export function summarize(index: CatalogIndex, a: Assignment | null | undefined): string {
  if (!a) return "No action";
  const def = index.byId.get(definitionId(a));
  if (!def) return `Missing: ${definitionId(a)}`;
  const settings = describeArgs(def, a.args);
  return settings ? `${def.name} · ${settings}` : def.name;
}

// ---------------------------------------------------------------------------
// Settings: starting values and the parameter rules
// ---------------------------------------------------------------------------

/** What a new assignment of this action starts with: the defaults its schema names. */
export function defaultArgs(def: ActionDefinition | undefined): Args {
  const out: Args = {};
  for (const p of def?.params ?? []) if (p.default !== undefined && p.default !== null) out[p.name] = p.default;
  return out;
}

export const assignmentFor = (def: ActionDefinition, args?: Args): Assignment => ({ provider: def.provider, action: def.action, args: args ?? defaultArgs(def) });

/** Only what Rust's `char::is_whitespace` calls white space; `String.trim` also drops U+FEFF and keeps U+0085. */
const RUST_WHITE_SPACE = /^[\t\n\v\f\r \u0085\u00a0\u1680\u2000-\u200a\u2028\u2029\u202f\u205f\u3000]*$/;

const isBlank = (p: ParamDef, v: unknown): boolean => {
  if (v === undefined || v === null) return true;
  return typeof v === "string" && (p.type === "string" || p.type === "reference") && RUST_WHITE_SPACE.test(v);
};

/** Rust measures a reference id in UTF-8 bytes. */
const utf8Length = (text: string): number => new TextEncoder().encode(text).length;

const bad = (p: ParamDef, message: string): Issue => ({ param: p.name, message, kind: "invalid" });

function checkValue(p: ParamDef, v: unknown): Issue | null {
  switch (p.type) {
    case "string": {
      if (typeof v !== "string") return bad(p, `${p.label} must be text`);
      const limit = Math.min(p.maxLength ?? 16384, 16384);
      return [...v].length > limit ? bad(p, `${p.label} is longer than ${limit} characters`) : null;
    }
    case "number": {
      if (typeof v !== "number" || !Number.isFinite(v)) return bad(p, `${p.label} must be a number`);
      if (p.integer && !Number.isInteger(v)) return bad(p, `${p.label} must be a whole number`);
      const low = p.min !== undefined && v < p.min;
      const high = p.max !== undefined && v > p.max;
      if (low || high) {
        if (p.min !== undefined && p.max !== undefined) return bad(p, `${p.label} must be between ${p.min} and ${p.max}`);
        return bad(p, p.min !== undefined ? `${p.label} must be at least ${p.min}` : `${p.label} must be at most ${p.max}`);
      }
      return null;
    }
    case "boolean":
      return typeof v === "boolean" ? null : bad(p, `${p.label} must be on or off`);
    case "enum":
      return typeof v === "string" && p.options.some((o) => o.value === v) ? null : bad(p, `${p.label} must be one of the listed choices`);
    case "reference":
      return typeof v === "string" && v.length > 0 && utf8Length(v) <= 128 ? null : bad(p, `${p.label} must be chosen from the list`);
    case "json":
      return v === null || v === undefined ? bad(p, `${p.label} must be a value`) : null;
  }
}

export interface Normalized {
  /** What would be stored: declared settings only, defaults in, blanks out. */
  args: Args;
  issues: Issue[];
}

/**
 * The same rules Rust applies on save (`actions::params::normalize`), so a form can show a mistake as it
 * is made. `examples/param-cases.json`, written by Rust, pins it on accepted and rejected inputs.
 */
export function normalizeArgs(params: ParamDef[], args: Args): Normalized {
  const issues: Issue[] = [];
  for (const key of Object.keys(args)) {
    if (!params.some((p) => p.name === key)) issues.push({ param: key, message: `\`${key}\` is not a setting of this action`, kind: "invalid" });
  }
  const out: Args = {};
  for (const p of params) {
    const given = args[p.name];
    let value: unknown;
    if (isBlank(p, given)) {
      if (p.default !== undefined && p.default !== null) value = p.default;
      else if (p.required) issues.push({ param: p.name, message: `${p.label} is required`, kind: "missing" });
    } else {
      value = given;
    }
    if (value === undefined) continue;
    const issue = checkValue(p, value);
    if (issue) issues.push(issue);
    else out[p.name] = value;
  }
  return { args: out, issues };
}

/**
 * A setting still to fill in, as Rust's `IssueKind::Missing` says. The wording check is only for an
 * issue without a kind, which a build older than 0.3 sent: remove it after 0.3.
 */
export const isMissingIssue = (i: Issue): boolean => (i.kind === undefined ? i.message.endsWith(" is required") : i.kind === "missing");

/** Issues keyed by setting; the ones that belong to no setting go under `""`. */
export function issuesByParam(issues: Issue[]): Map<string, string[]> {
  const map = new Map<string, string[]>();
  for (const i of issues) map.set(i.param ?? "", [...(map.get(i.param ?? "") ?? []), i.message]);
  return map;
}

// ---------------------------------------------------------------------------
// Text helpers for the places that still show a key combination or a list
// ---------------------------------------------------------------------------

export const unavailableNote = (def: ActionDefinition): string | null => (def.availability.status === "unavailable" ? def.availability.reason : null);
