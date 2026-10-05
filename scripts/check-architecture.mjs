#!/usr/bin/env node
// The architecture check: the dependency, platform and ownership rules of
// docs/architecture/ENGINEERING_RULES.md, checked mechanically. Node built-ins only.
//
//   node scripts/check-architecture.mjs              scan src-tauri/src and src; exit 1 on a violation
//   node scripts/check-architecture.mjs --staged     only the files staged in git
//   node scripts/check-architecture.mjs --hook       Claude Code PostToolUse hook: check the edited
//                                                    file; exit 2 with one paragraph on a violation
//   node scripts/check-architecture.mjs --report     the large-file table; never fails
//   --only=rust | --only=web                         limit any mode to one side
//
// The rules are data in scripts/architecture/rust.mjs and web.mjs.

import { execFileSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

import * as rustRules from "./architecture/rust.mjs";
import * as webRules from "./architecture/web.mjs";

const WATCH_LINES = 400;
const ALLOWLIST_LINES = 700;
const GROWTH_PERCENT = 10;
const DUMPING_GROUNDS = ["utils", "helpers", "common", "misc"];
const RULES_DOC = "docs/architecture/ENGINEERING_RULES.md";

// Each rule's number in ENGINEERING_RULES and how to fix a violation.
const RULES = {
  "import-matrix": ["R2", "take what you need as a parameter, or move the code to a module allowed to import it; if the dependency is intended, change CODEBASE_MAP section 5 and scripts/architecture/rust.mjs in one commit"],
  "crate-confinement": ["R4", "use the outside crate only in its adapter file and call that file (CODEBASE_MAP section 5, crate confinement)"],
  "platform-cfg": ["R3", "take a Platform parameter instead of reading cfg; platform detection belongs in platform.rs or an adapter file listed in R3"],
  "tauri-import": ["R10", "call Rust through src/lib/ipc.ts, the only file that imports @tauri-apps"],
  "preview-import": ["R10", "go through src/lib/ipc.ts; the browser preview is its private stand-in"],
  layer: ["R20", "move the shared piece down to src/components or src/lib, or the code up into its feature (FRONTEND_MAP section 1)"],
  "feature-edge": ["R20", "move the shared piece to src/components or src/lib; if this is a real composition point, list it in FRONTEND_MAP section 3 and scripts/architecture/web.mjs in one commit"],
  "dumping-ground": ["R16", "name the file after the concept it holds, in the module that owns that concept"],
  "map-row": ["R1", "add its row to the map in the same commit"],
};

const attempt = (read) => {
  try {
    return read();
  } catch {
    return null;
  }
};

// ---------------------------------------------------------------------------
// Paths and globs

const globCache = new Map();

function globToRegExp(glob) {
  if (globCache.has(glob)) return globCache.get(glob);
  let body = "";
  for (let i = 0; i < glob.length; i += 1) {
    const c = glob[i];
    if (c === "*" && glob[i + 1] === "*") {
      const slash = glob[i + 2] === "/";
      body += slash ? "(?:.*/)?" : ".*";
      i += slash ? 2 : 1;
    } else if (c === "*") {
      body += "[^/]*";
    } else {
      body += c.replace(/[.+?^${}()|[\]\\]/g, "\\$&");
    }
  }
  // `a/**` also covers `a.rs`, the other way to write a Rust module.
  const regex = glob.endsWith("/**") ? new RegExp(`^${body.slice(0, -3)}(?:/.*|\\.rs)?$`) : new RegExp(`^${body}$`);
  globCache.set(glob, regex);
  return regex;
}

const matchesAny = (globs, file) => globs.some((glob) => globToRegExp(glob).test(file));

// `a::b` is within `a`; segment-wise, so `core::modelx` is not within `core::model`.
const startsWith = (segments, prefix) => prefix.length <= segments.length && prefix.every((s, i) => segments[i] === s);
const modulePath = (text) => text.split("::");
const isWithin = (target, dir) => target === dir || target.startsWith(`${dir}/`);

function underRoot(file, root) {
  return file.startsWith(`${root}/`) ? file.slice(root.length + 1) : null;
}

function sideOf(file) {
  if (underRoot(file, rustRules.root) !== null) return "rust";
  if (underRoot(file, webRules.root) !== null) return "web";
  return null;
}

function lineStarts(text) {
  const starts = [0];
  for (let i = 0; i < text.length; i += 1) if (text[i] === "\n") starts.push(i + 1);
  return starts;
}

function lineAt(starts, offset) {
  let low = 0;
  let high = starts.length - 1;
  while (low < high) {
    const mid = (low + high + 1) >> 1;
    if (starts[mid] <= offset) low = mid;
    else high = mid - 1;
  }
  return low + 1;
}

function countLines(text) {
  if (text.length === 0) return 0;
  return lineStarts(text).length - (text.endsWith("\n") ? 1 : 0);
}

// ---------------------------------------------------------------------------
// Rust source

const IDENT_CHAR = /[A-Za-z0-9_]/;
const CHAR_LITERAL = /'(?:\\(?:x[0-9a-fA-F]{2}|u\{[0-9a-fA-F_]{1,6}\}|.)|[^\\'\r\n])'/uy;
const RAW_STRING_OPEN = /r(#*)"/y;

// Spaces over out[from..to), newlines kept, so offsets and line numbers stay valid.
function blankRange(out, from, to) {
  for (let k = from; k < to; k += 1) if (out[k] !== "\n") out[k] = " ";
}

const endOf = (source, text, from) => (source.indexOf(text, from) === -1 ? source.length : source.indexOf(text, from));

function blockCommentEnd(source, i) {
  let depth = 0;
  let k = i;
  do {
    if (source.startsWith("/*", k)) [depth, k] = [depth + 1, k + 2];
    else if (source.startsWith("*/", k)) [depth, k] = [depth - 1, k + 2];
    else k += 1;
  } while (k < source.length && depth > 0);
  return k;
}

// A comment or literal starting at `i`: [blank from, blank to, resume at], or null.
function rustSpanAt(source, i) {
  const c = source[i];
  if (source.startsWith("//", i)) return [i, endOf(source, "\n", i), endOf(source, "\n", i)];
  if (source.startsWith("/*", i)) return [i, blockCommentEnd(source, i), blockCommentEnd(source, i)];
  if (c === '"') {
    let k = i + 1;
    while (k < source.length && source[k] !== '"') k += source[k] === "\\" ? 2 : 1;
    return [i + 1, k, k + 1];
  }
  if (c === "'") {
    CHAR_LITERAL.lastIndex = i;
    const literal = CHAR_LITERAL.exec(source);
    return literal && [i + 1, i + literal[0].length - 1, i + literal[0].length];
  }
  if ((c === "r" || source.startsWith("br", i)) && !IDENT_CHAR.test(source[i - 1] ?? "")) {
    RAW_STRING_OPEN.lastIndex = c === "b" ? i + 1 : i;
    const open = RAW_STRING_OPEN.exec(source);
    if (!open) return null;
    const end = endOf(source, `"${open[1]}`, RAW_STRING_OPEN.lastIndex);
    return [RAW_STRING_OPEN.lastIndex, end, end + open[1].length + 1];
  }
  return null;
}

/** Comments and the contents of string and char literals become spaces; offsets and newlines stay. */
export function maskRust(source) {
  const out = source.split("");
  let i = 0;
  while (i < source.length) {
    const span = rustSpanAt(source, i);
    if (span) blankRange(out, span[0], span[1]);
    i = span ? Math.max(span[2], i + 1) : i + 1;
  }
  return out.join("");
}

const CFG_TEST = /#\s*\[\s*cfg\s*\(\s*test\s*\)\s*\]/g;
const FILE_CFG_TEST = /#!\s*\[\s*cfg\s*\(\s*test\s*\)\s*\]/;

// The end of the item that starts at `from`: after its `;` or its closing brace.
function itemEnd(code, from) {
  let i = from;
  let depth = 0;
  for (; i < code.length; i += 1) {
    const c = code[i];
    if (c === "(" || c === "[" || c === "{") depth += 1;
    else if (c === ")" || c === "]" || c === "}") {
      depth -= 1;
      if (depth <= 0 && c === "}") return i + 1;
    } else if (c === ";" && depth === 0) return i + 1;
  }
  return code.length;
}

/** Blanks every `#[cfg(test)]` item (a test module, function or use) of masked code. */
export function stripRustTests(masked) {
  if (FILE_CFG_TEST.test(masked)) return { code: masked.replace(/[^\n]/g, " "), ranges: [[0, masked.length]] };
  const out = masked.split("");
  const ranges = [];
  for (const match of masked.matchAll(CFG_TEST)) {
    if (ranges.length && match.index < ranges[ranges.length - 1][1]) continue;
    const end = itemEnd(masked, match.index + match[0].length);
    blankRange(out, match.index, end);
    ranges.push([match.index, end]);
  }
  return { code: out.join(""), ranges };
}

// `context/observer/mod.rs` -> [context, observer]; `lib.rs` -> [].
function moduleOfFile(rel) {
  const parts = rel.replace(/\.rs$/, "").split("/");
  const last = parts[parts.length - 1];
  if (last === "mod" || (parts.length === 1 && (last === "lib" || last === "main"))) parts.pop();
  return parts;
}

function resolveRelative(segments, own) {
  let base = own;
  let i = segments[0] === "self" ? 1 : 0;
  while (segments[i] === "super") {
    if (base.length === 0) return null;
    base = base.slice(0, -1);
    i += 1;
  }
  return [...base, ...segments.slice(i)];
}

const IDENT = /(?:r#)?[A-Za-z_][A-Za-z0-9_]*/y;

// The leaves of a use tree (`a::{b, c::*}`), each with its offset.
function useLeaves(text, base) {
  const leaves = [];
  let i = 0;
  const skipSpace = () => {
    while (i < text.length && /\s/.test(text[i])) i += 1;
  };
  const ident = () => {
    IDENT.lastIndex = i;
    return IDENT.exec(text)?.[0] ?? null;
  };
  const tree = (start) => {
    let prefix = start;
    skipSpace();
    if (text.startsWith("::", i)) i += 2;
    for (;;) {
      skipSpace();
      if (text[i] === "{") {
        i += 1;
        for (;;) {
          skipSpace();
          if (text[i] === "}" || i >= text.length) break;
          tree(prefix);
          skipSpace();
          if (text[i] !== ",") break;
          i += 1;
        }
        i += 1;
        return;
      }
      if (text[i] === "*") {
        leaves.push({ segments: prefix, glob: true, offset: base + i });
        i += 1;
        return;
      }
      const name = ident();
      if (!name) return;
      const at = i;
      i += name.length;
      skipSpace();
      if (text.startsWith("::", i)) {
        i += 2;
        prefix = [...prefix, name.replace(/^r#/, "")];
        continue;
      }
      leaves.push({ segments: name === "self" ? prefix : [...prefix, name], glob: false, offset: base + at });
      if (ident() === "as") {
        i += 2;
        skipSpace();
        i += (ident() ?? "").length;
      }
      return;
    }
  };
  tree([]);
  return leaves;
}

function statementEnd(code, from) {
  let depth = 0;
  for (let i = from; i < code.length; i += 1) {
    if (code[i] === "{") depth += 1;
    else if (code[i] === "}") depth -= 1;
    else if (code[i] === ";" && depth <= 0) return i;
  }
  return code.length;
}

const USE = /\buse\s+/g;
const INLINE_PATH = /(?<![\w$:])(crate|self|super)((?:::[A-Za-z_][A-Za-z0-9_]*)+)/g;
const EXTERN_CRATE = /\bextern\s+crate\s+([A-Za-z_][A-Za-z0-9_]*)/g;

function confinedCrateNames(rules) {
  return rules.crateConfinement.flatMap((entry) => entry.crates);
}

function crateRegex(rules) {
  const names = confinedCrateNames(rules).map((name) => name.replace(/\*/g, "[A-Za-z0-9_]*"));
  return new RegExp(`(?<![\\w$:])(${names.join("|")})\\s*::`, "g");
}

/**
 * What a piece of test-stripped, masked Rust refers to: crate modules (`crate::`,
 * `super::`, `self::` paths, resolved to crate-root segments) and confined outside crates.
 */
export function rustReferences(code, file, rules = rustRules) {
  const own = moduleOfFile(underRoot(file, rules.root) ?? file);
  const starts = lineStarts(code);
  const isConfined = (name) => confinedCrateNames(rules).some((pattern) => globToRegExp(pattern).test(name));
  const modules = [];
  const crates = [];
  const add = (segments, glob, offset) => {
    const line = lineAt(starts, offset);
    if (segments[0] === "crate") modules.push({ path: segments.slice(1), glob, line });
    else if (segments[0] === "self" || segments[0] === "super") {
      const resolved = resolveRelative(segments, own);
      if (resolved) modules.push({ path: resolved, glob, line });
    } else if (segments.length && isConfined(segments[0])) crates.push({ name: segments[0], line });
  };

  const rest = code.split("");
  for (const match of code.matchAll(USE)) {
    const start = match.index + match[0].length;
    const end = statementEnd(code, start);
    for (const leaf of useLeaves(code.slice(start, end), start)) add(leaf.segments, leaf.glob, leaf.offset);
    blankRange(rest, match.index, end);
  }
  const inline = rest.join("");
  for (const match of inline.matchAll(INLINE_PATH)) add([match[1], ...match[2].slice(2).split("::")], false, match.index);
  for (const match of inline.matchAll(crateRegex(rules))) add([match[1]], false, match.index);
  for (const match of inline.matchAll(EXTERN_CRATE)) add([match[1]], false, match.index);
  return { modules, crates };
}

function importProblem(rel, ref, rules) {
  const own = moduleOfFile(rel);
  if (ref.path.length === 0 || startsWith(ref.path, own)) return null;
  if (rules.importableEverywhere.some((m) => startsWith(ref.path, modulePath(m)))) return null;
  if (rules.importExceptions.some((e) => matchesAny(e.files, rel) && startsWith(ref.path, modulePath(e.imports)))) return null;
  for (const restricted of rules.restrictedModules) {
    if (startsWith(ref.path, modulePath(restricted.module)) && !matchesAny(restricted.importers, rel)) {
      return `only ${restricted.importers.join(", ")} may import crate::${restricted.module}`;
    }
  }
  for (const rule of rules.importRules) {
    if (!matchesAny(rule.files, rel) || matchesAny(rule.except ?? [], rel)) continue;
    if (rule.only && !rule.only.some((allowed) => startsWith(ref.path, modulePath(allowed)))) {
      const allowed = [...rule.only, ...rules.importableEverywhere].join(", ");
      return `${rule.files.join(", ")} may import only crate::{${allowed}}`;
    }
    const hit = (rule.forbid ?? []).find((forbidden) => {
      const segments = modulePath(forbidden);
      return startsWith(ref.path, segments) || (ref.glob && startsWith(segments, ref.path));
    });
    if (hit) return `${rule.files.join(", ")} must not import crate::${hit}`;
  }
  return null;
}

function platformCfgViolations(rel, code, rules) {
  const { files, exceptions } = rules.platformCfg;
  if (matchesAny(files, rel) || exceptions.some((e) => matchesAny(e.files, rel))) return [];
  const starts = lineStarts(code);
  const found = [];
  for (const match of code.matchAll(/\bcfg(?:_attr)?\s*!?\s*\(/g)) {
    const open = match.index + match[0].length - 1;
    const args = code.slice(open, itemEnd(code, open));
    const message = `platform cfg outside ${files.join(", ")}`;
    if (/\btarget_os\b|\bwindows\b/.test(args)) found.push({ line: lineAt(starts, match.index), rule: "platform-cfg", message });
  }
  return found;
}

const isRustTestFile = (rel, rules) => matchesAny(rules.testFiles, rel);

/** The violations of one Rust file (path relative to the repository). */
export function checkRustFile(file, source, rules = rustRules) {
  const rel = underRoot(file, rules.root);
  if (rel === null || !rel.endsWith(".rs") || isRustTestFile(rel, rules)) return [];
  const { code } = stripRustTests(maskRust(source));
  const { modules, crates } = rustReferences(code, file, rules);
  const found = [];
  const seen = new Set();
  for (const ref of modules) {
    const text = `crate::${ref.path.join("::")}${ref.glob ? "::*" : ""}`;
    const problem = importProblem(rel, ref, rules);
    if (!problem || seen.has(`${ref.line} ${problem}`)) continue;
    seen.add(`${ref.line} ${problem}`);
    found.push({ line: ref.line, rule: "import-matrix", message: `${problem}; found ${text}` });
  }
  for (const ref of crates) {
    const entry = rules.crateConfinement.find((e) => e.crates.some((c) => globToRegExp(c).test(ref.name)));
    if (entry && !matchesAny(entry.files, rel) && !seen.has(`${ref.line} ${ref.name}`)) {
      seen.add(`${ref.line} ${ref.name}`);
      found.push({ line: ref.line, rule: "crate-confinement", message: `${ref.name} is confined to ${entry.files.join(", ")}` });
    }
  }
  found.push(...platformCfgViolations(rel, code, rules));
  return found.map((v) => ({ file, ...v })).sort((a, b) => a.line - b.line);
}

// ---------------------------------------------------------------------------
// TypeScript source

// Comments become spaces; string literals are kept and listed.
function scanTypeScript(source) {
  const out = source.split("");
  const strings = [];
  const n = source.length;
  let i = 0;
  while (i < n) {
    const c = source[i];
    if (c === "/" && source[i + 1] === "/") {
      while (i < n && source[i] !== "\n") out[i++] = " ";
    } else if (c === "/" && source[i + 1] === "*") {
      const end = source.indexOf("*/", i + 2) === -1 ? n : source.indexOf("*/", i + 2) + 2;
      for (; i < end; i += 1) if (out[i] !== "\n") out[i] = " ";
    } else if (c === '"' || c === "'" || c === "`") {
      let k = i + 1;
      while (k < n && source[k] !== c && (c === "`" || source[k] !== "\n")) k += source[k] === "\\" ? 2 : 1;
      strings.push({ start: i, value: source.slice(i + 1, k) });
      i = k + 1;
    } else {
      i += 1;
    }
  }
  return { code: out.join(""), strings };
}

const SPECIFIER_BEFORE = /(?:\b(?:from|import)\s*|\bimport\s*\(\s*)$/;

function typeScriptImports(file, source) {
  const { code, strings } = scanTypeScript(source);
  const starts = lineStarts(source);
  return strings
    .filter((s) => SPECIFIER_BEFORE.test(code.slice(Math.max(0, s.start - 40), s.start)))
    .map((s) => {
      const relative = s.value.startsWith(".");
      const joined = relative ? path.posix.normalize(path.posix.join(path.posix.dirname(file), s.value)) : s.value;
      const target = relative ? joined.replace(/\.(?:tsx?|jsx?|mjs)$/, "") : joined;
      return { spec: s.value, target, line: lineAt(starts, s.start) };
    });
}

function featureOf(target, rules) {
  const inside = underRoot(target, rules.featureMap.features);
  return inside === null ? null : inside.split("/")[0];
}

function webImportProblem(file, { spec, target }, rules) {
  if (isWithin(target, "@tauri-apps") || target.startsWith("@tauri-apps/")) {
    if (rules.tauriImporters.includes(file)) return null;
    return { rule: "tauri-import", message: `imports ${spec}; only ${rules.tauriImporters.join(", ")} may` };
  }
  if (target === rules.preview.module && !rules.preview.importers.includes(file)) {
    return { rule: "preview-import", message: `imports ${spec}; only ${rules.preview.importers.join(", ")} may` };
  }
  if (rules.importExceptions.some((e) => e.file === file && isWithin(target, e.imports))) return null;
  for (const layer of rules.layerRules) {
    if (!matchesAny(layer.files, file)) continue;
    const hit = layer.forbid.find((forbidden) => isWithin(target, forbidden));
    if (hit) return { rule: "layer", message: `${layer.files.join(", ")} must not import ${hit}; found ${spec}` };
  }
  const from = featureOf(file, rules);
  const to = featureOf(target, rules);
  if (!from || !to || from === to) return null;
  const edge = rules.featureEdges.find((e) => e.from === from && e.to === to);
  if (edge && (!edge.testsOnly || matchesAny(rules.testFiles, file))) return null;
  const where = edge ? "an edge allowed in tests only" : "an edge not listed in FRONTEND_MAP section 3";
  return { rule: "feature-edge", message: `feature ${from} imports feature ${to} (${spec}), ${where}` };
}

/** The violations of one TypeScript file (path relative to the repository). */
export function checkWebFile(file, source, rules = webRules) {
  if (underRoot(file, rules.root) === null || !/\.tsx?$/.test(file)) return [];
  return typeScriptImports(file, source)
    .map((imported) => {
      const problem = webImportProblem(file, imported, rules);
      return problem && { file, line: imported.line, ...problem };
    })
    .filter(Boolean);
}

// ---------------------------------------------------------------------------
// Names, maps and sizes

/** R16: no file or directory named utils, helpers, common or misc under either root. */
export function checkFileName(file) {
  const rel = underRoot(file, rustRules.root) ?? underRoot(file, webRules.root);
  if (rel === null) return [];
  const bad = rel.split("/").find((part) => DUMPING_GROUNDS.includes(part.replace(/\..*$/, "").toLowerCase()));
  if (!bad) return [];
  return [{ file, line: 0, rule: "dumping-ground", message: `"${bad}" is a dumping-ground name (${DUMPING_GROUNDS.join(", ")})` }];
}

const inlineCode = (markdown) => [...markdown.matchAll(/`([^`\n]+)`/g)].map((m) => m[1].trim());
const MOD_DECL = /^[ \t]*(?:pub(?:\([^)\n]*\))?[ \t]+)?mod[ \t]+([A-Za-z_][A-Za-z0-9_]*)[ \t]*;/gm;

/** R1: every `mod` of lib.rs appears as inline code (`name` or `name::…`) in CODEBASE_MAP. */
export function checkModuleMap(libSource, mapText, rules = rustRules) {
  const { code } = stripRustTests(maskRust(libSource));
  const spans = inlineCode(mapText ?? "");
  const starts = lineStarts(code);
  const file = `${rules.root}/${rules.moduleMap.crateRoot}`;
  return [...code.matchAll(MOD_DECL)]
    .filter(([, name]) => !spans.some((s) => s === name || s.startsWith(`${name}::`) || s.startsWith(`${name}/`) || s === `${name}.rs`))
    .map((m) => ({ file, line: lineAt(starts, m.index + m[0].search(/\S/)), rule: "map-row", message: `module ${m[1]} has no row in ${rules.moduleMap.map}` }));
}

/** R1: every src/features/<name> appears as inline code in FRONTEND_MAP. */
export function checkFeatureMap(featureNames, mapText, rules = webRules) {
  const spans = inlineCode(mapText ?? "");
  return featureNames
    .map((name) => `${rules.featureMap.features}/${name}`)
    .filter((dir) => !spans.some((s) => isWithin(s, dir)))
    .map((dir) => ({ file: dir, line: 0, rule: "map-row", message: `${dir} has no row in ${rules.featureMap.map}` }));
}

/** Lines that are not tests: test files count 0, `#[cfg(test)]` items are left out. */
export function productionLines(file, source) {
  const rustRel = underRoot(file, rustRules.root);
  if (rustRel !== null) {
    if (isRustTestFile(rustRel, rustRules)) return 0;
    const { ranges } = stripRustTests(maskRust(source));
    const starts = lineStarts(source);
    const total = countLines(source);
    let tests = 0;
    for (let line = 0; line < total; line += 1) {
      const indent = source.slice(starts[line]).search(/\S|\n|$/);
      const first = starts[line] + indent;
      if (ranges.some(([from, to]) => first >= from && first < to)) tests += 1;
    }
    return total - tests;
  }
  return matchesAny(webRules.testFiles, file) ? 0 : countLines(source);
}

const round1 = (x) => Math.round(x * 10) / 10;

/** R14 rows: files of 400+ production lines, every allowlisted file, and stale entries. Advisory. */
export function largeFileRows(files, allowlist) {
  const entries = new Map(allowlist.map((entry) => [entry.file, entry]));
  const present = new Set(files.map((f) => f.file));
  const rows = [];
  for (const { file, lines } of files) {
    const entry = entries.get(file);
    if (!entry && lines < WATCH_LINES) continue;
    const notes = [];
    let growth = null;
    let status = lines >= ALLOWLIST_LINES ? "needs allowlist entry" : "watch";
    if (entry) {
      status = "allowlisted";
      growth = round1(((lines - entry.baseline) / entry.baseline) * 100);
      if (growth > GROWTH_PERCENT) notes.push(`grew ${growth} % over its baseline of ${entry.baseline}`);
      if (lines < WATCH_LINES && lines < entry.baseline) notes.push(`now under ${WATCH_LINES} lines; the entry can go`);
    }
    rows.push({ file, lines, status, baseline: entry?.baseline ?? null, growth, reason: entry?.reason ?? "", notes });
  }
  for (const entry of allowlist) {
    if (present.has(entry.file)) continue;
    rows.push({ ...entry, lines: null, status: "stale allowlist entry", growth: null, notes: ["the file is gone; remove the entry"] });
  }
  return rows.sort((a, b) => (b.lines ?? -1) - (a.lines ?? -1));
}

export function formatLargeFileReport(rows) {
  const lines = [
    `Large files, production lines only (R14; advisory, never fails): ${WATCH_LINES}+ is "watch", ${ALLOWLIST_LINES}+ needs an allowlist entry, growth above ${GROWTH_PERCENT} % is reported.`,
    "",
    `${"lines".padStart(6)} ${"baseline".padStart(8)} ${"growth".padStart(7)}  ${"status".padEnd(22)} file`,
  ];
  for (const row of rows) {
    const growth = row.growth === null ? "" : `${row.growth > 0 ? "+" : ""}${row.growth}%`;
    lines.push(`${String(row.lines ?? "-").padStart(6)} ${String(row.baseline ?? "").padStart(8)} ${growth.padStart(7)}  ${row.status.padEnd(22)} ${row.file}`);
    for (const note of [row.reason, ...row.notes].filter(Boolean)) lines.push(`${"".padStart(48)}${note}`);
  }
  return lines.join("\n");
}

// ---------------------------------------------------------------------------
// Running

const USAGE = "usage: check-architecture.mjs [--staged | --hook | --report] [--only=rust|web]";

function parseArgs(args) {
  const options = { mode: "tree", only: null };
  const modes = { "--staged": "staged", "--hook": "hook", "--report": "report" };
  for (const arg of args) {
    if (modes[arg] && options.mode === "tree") options.mode = modes[arg];
    else if (/^--only=(rust|web)$/.test(arg) && !options.only) options.only = arg.slice(7);
    else return { error: `unexpected argument ${arg}` };
  }
  return options;
}

const LIB_FILE = `${rustRules.root}/${rustRules.moduleMap.crateRoot}`;

// The violations of `files`, read with `read`. The map rows are checked when lib.rs,
// a feature file or a map is among the files; `listFeatures` names every feature.
function violationsOf(files, read, sides, listFeatures) {
  const found = [];
  const features = new Set();
  let checkModules = false;
  let checkAllFeatures = false;
  for (const file of files) {
    if (file === rustRules.moduleMap.map) checkModules = true;
    if (file === webRules.featureMap.map) checkAllFeatures = true;
    const side = sideOf(file);
    if (!side || !sides.includes(side)) continue;
    if (file === LIB_FILE) checkModules = true;
    const feature = featureOf(file, webRules);
    if (feature) features.add(feature);
    found.push(...checkFileName(file));
    const source = read(file);
    if (source === null) continue;
    found.push(...(side === "rust" ? checkRustFile(file, source) : checkWebFile(file, source)));
  }
  if (checkModules && sides.includes("rust")) {
    found.push(...checkModuleMap(read(LIB_FILE) ?? "", read(rustRules.moduleMap.map)));
  }
  const names = checkAllFeatures ? listFeatures() : [...features];
  if (sides.includes("web") && names.length) found.push(...checkFeatureMap(names, read(webRules.featureMap.map)));
  return found;
}

const rootsOf = (sides) => sides.map((side) => (side === "rust" ? rustRules.root : webRules.root));

function featureNames(io) {
  return [...new Set(io.listFiles(webRules.featureMap.features).map((f) => featureOf(f, webRules)).filter(Boolean))].sort();
}

function largeFiles(io, sides) {
  const files = rootsOf(sides)
    .flatMap((root) => io.listFiles(root))
    .filter((file) => (sideOf(file) === "rust" ? file.endsWith(".rs") : /\.tsx?$/.test(file)))
    .map((file) => ({ file, lines: productionLines(file, io.readFile(file) ?? "") }));
  const allowlist = [
    ...(sides.includes("rust") ? rustRules.largeFiles.map((e) => ({ ...e, file: `${rustRules.root}/${e.file}` })) : []),
    ...(sides.includes("web") ? webRules.largeFiles : []),
  ];
  return largeFileRows(files, allowlist);
}

const where = (v) => (v.line ? `${v.file}:${v.line}` : v.file);

function printViolations(violations, io) {
  for (const v of violations) {
    io.out(`${where(v)}: ${RULES[v.rule][0]} ${v.rule}: ${v.message}`);
    io.out(`    fix: ${RULES[v.rule][1]}`);
  }
}

function hookParagraph(file, violations) {
  const problems = violations.map((v) => `${where(v)} (${RULES[v.rule][0]} ${v.rule}): ${v.message}.`);
  const hints = [...new Set(violations.map((v) => RULES[v.rule][1]))].map((h) => `${h[0].toUpperCase()}${h.slice(1)}.`);
  return [
    `The architecture check found ${violations.length === 1 ? "a violation" : `${violations.length} violations`} after editing ${file}:`,
    ...problems,
    ...hints,
    `Fix the code rather than the check; the rules are in ${RULES_DOC}.`,
  ].join(" ");
}

function runHook(io) {
  const target = attempt(() => JSON.parse(io.readStdin()))?.tool_input?.file_path;
  if (typeof target !== "string" || target === "") return 0;
  const file = path.relative(io.root, path.resolve(io.root, target)).split(path.sep).join("/");
  const side = sideOf(file);
  if (!side || io.readFile(file) === null) return 0;
  const violations = violationsOf([file], io.readFile, [side], () => featureNames(io));
  if (violations.length === 0) return 0;
  io.err(hookParagraph(file, violations));
  return 2;
}

/** Runs the check with `args` against `io` (the file system and git by default); returns the exit code. */
export function run(args, io = nodeIo()) {
  const options = parseArgs(args);
  if (options.error) {
    io.err(`check-architecture: ${options.error}\n${USAGE}`);
    return 2;
  }
  if (options.mode === "hook") return runHook(io);
  const sides = options.only ? [options.only] : ["rust", "web"];
  if (options.mode === "report") {
    io.out(formatLargeFileReport(largeFiles(io, sides)));
    return 0;
  }

  const staged = options.mode === "staged";
  const files = staged ? io.stagedFiles() : rootsOf(sides).flatMap((root) => io.listFiles(root));
  const read = staged ? io.readStaged : io.readFile;
  const violations = violationsOf(files, read, sides, () => featureNames(io));
  printViolations(violations, io);

  if (!staged) {
    const rows = largeFiles(io, sides);
    for (const row of rows.filter((r) => r.status !== "watch" && r.status !== "allowlisted")) {
      io.out(`advisory: ${row.file}: ${row.status}${row.notes.length ? ` (${row.notes.join("; ")})` : ""}`);
    }
    for (const row of rows.filter((r) => r.status === "allowlisted" && r.growth > GROWTH_PERCENT)) {
      io.out(`advisory: ${row.file}: ${row.notes[0]}`);
    }
  }
  const scope = staged ? `${files.length} staged files` : `${files.length} files under ${rootsOf(sides).join(" and ")}`;
  if (violations.length) {
    io.out(`check-architecture: ${violations.length} violation${violations.length === 1 ? "" : "s"} in ${scope}; rules: ${RULES_DOC}`);
    return 1;
  }
  io.out(`check-architecture: no violations in ${scope}${staged ? "" : " (large-file report: --report)"}`);
  return 0;
}

// ---------------------------------------------------------------------------
// The real file system and git

function walk(dir) {
  return (attempt(() => fs.readdirSync(dir, { withFileTypes: true })) ?? []).flatMap((entry) => {
    const full = path.join(dir, entry.name);
    if (entry.isDirectory()) return walk(full);
    return entry.isFile() ? [full] : [];
  });
}

function nodeIo() {
  const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
  const toRepo = (full) => path.relative(root, full).split(path.sep).join("/");
  const git = (args) =>
    execFileSync("git", args, { cwd: root, encoding: "utf8", maxBuffer: 64 * 1024 * 1024, stdio: ["ignore", "pipe", "ignore"] });
  return {
    root,
    listFiles: (dir) => walk(path.join(root, dir)).map(toRepo).sort(),
    readFile: (file) => attempt(() => fs.readFileSync(path.join(root, file), "utf8")),
    stagedFiles: () => git(["diff", "--cached", "--name-only", "--diff-filter=ACMR"]).split("\n").filter(Boolean),
    readStaged: (file) => attempt(() => git(["show", `:${file}`])),
    readStdin: () => fs.readFileSync(0, "utf8"),
    out: (line) => process.stdout.write(`${line}\n`),
    err: (line) => process.stderr.write(`${line}\n`),
  };
}

const invokedDirectly = attempt(() => fs.realpathSync(process.argv[1]) === fs.realpathSync(fileURLToPath(import.meta.url)));
if (invokedDirectly) process.exitCode = run(process.argv.slice(2));
