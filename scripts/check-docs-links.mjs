#!/usr/bin/env node
// Relative links in every tracked Markdown file must resolve: the file or directory
// exists (with the exact case, as on GitHub), and an anchor (`#part`, `file.md#part`)
// matches a heading of the target as GitHub slugs it. Links in fenced or inline code
// and external URLs are skipped. Exit 1 listing each broken link.

import { execFileSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");

const FENCE_OPEN = /^ {0,3}(`{3,}|~{3,})/;
const FENCE_CLOSE = /^ {0,3}(`{3,}|~{3,})\s*$/;
const INLINE_CODE = /(`+)(?:.*?[^`])?\1(?!`)/g;
const INLINE_LINK = /!?\[(?:[^\]\\]|\\.)*\]\(\s*(<[^>\n]*>|[^)\s]+)(?:\s+(?:"[^"]*"|'[^']*'|\([^)]*\)))?\s*\)/g;
const REFERENCE = /^ {0,3}\[[^\]]+\]:\s*(<[^>\n]*>|\S+)/;
const ATX = /^ {0,3}#{1,6}(?:[ \t]+(.*?))?(?:[ \t]+#+)?[ \t]*$/;
const SETEXT = /^ {0,3}(?:=+|-+)[ \t]*$/;
const HTML_ANCHOR = /<a\s[^>]*\b(?:name|id)\s*=\s*["']([^"']+)["']/gi;
const EXTERNAL = /^(?:[a-z][a-z0-9+.-]*:|\/\/)/i;

// The lines outside fenced code and YAML front matter: { raw, text (inline code blanked), line }.
function proseLines(markdown) {
  const lines = markdown.split("\n");
  const frontMatterEnd = lines[0] === "---" ? lines.indexOf("---", 1) : -1;
  const prose = [];
  let fence = null;
  for (let i = frontMatterEnd + 1; i < lines.length; i += 1) {
    const raw = lines[i];
    if (fence) {
      const close = FENCE_CLOSE.exec(raw);
      if (close && close[1][0] === fence[0] && close[1].length >= fence.length) fence = null;
      continue;
    }
    const open = FENCE_OPEN.exec(raw);
    if (open) fence = open[1];
    else prose.push({ raw, text: raw.replace(INLINE_CODE, (code) => " ".repeat(code.length)), line: i + 1 });
  }
  return prose;
}

/** A heading's anchor as GitHub writes it (github-slugger on the rendered text). */
export function slugify(heading) {
  const rendered = heading
    .replace(/!?\[([^\]]*)\]\([^)]*\)/g, "$1")
    .replace(/<[^>]+>/g, "")
    .replace(/(^|[^\p{L}\p{N}_])_+([^_]+?)_+(?=[^\p{L}\p{N}_]|$)/gu, "$1$2");
  return rendered
    .trim()
    .toLowerCase()
    .replace(/[^\p{L}\p{M}\p{N}\p{Pc} -]/gu, "")
    .replace(/ /g, "-");
}

/** Every anchor of a Markdown file: its headings (duplicates numbered -1, -2…) and HTML anchors. */
export function anchorsOf(markdown) {
  const anchors = new Set();
  const seen = new Map();
  const addHeading = (text) => {
    const slug = slugify(text);
    const count = seen.get(slug) ?? 0;
    seen.set(slug, count + 1);
    anchors.add(count === 0 ? slug : `${slug}-${count}`);
  };
  const prose = proseLines(markdown);
  prose.forEach(({ raw, line }, i) => {
    const next = prose[i + 1];
    const atx = ATX.exec(raw);
    if (atx) addHeading(atx[1] ?? "");
    else if (raw.trim() && next?.line === line + 1 && SETEXT.test(next.raw)) addHeading(raw);
    for (const match of raw.matchAll(HTML_ANCHOR)) anchors.add(match[1]);
  });
  return anchors;
}

const destination = (text) => (text.startsWith("<") ? text.slice(1, -1) : text);

/** The link destinations of a Markdown file outside code, with their line numbers. */
export function linksOf(markdown) {
  return proseLines(markdown).flatMap(({ text, line }) => {
    const inline = [...text.matchAll(INLINE_LINK)].map((m) => ({ target: destination(m[1]), line }));
    const reference = REFERENCE.exec(text);
    return reference ? [...inline, { target: destination(reference[1]), line }] : inline;
  });
}

const decode = (text) => {
  try {
    return decodeURIComponent(text);
  } catch {
    return text;
  }
};

function linkProblem(file, target, { exists, anchorsFor }) {
  if (EXTERNAL.test(target)) return null;
  const hash = target.indexOf("#");
  const linked = decode((hash === -1 ? target : target.slice(0, hash)).split("?")[0]);
  const anchor = hash === -1 ? "" : decode(target.slice(hash + 1)).toLowerCase();
  const resolved = linked === "" ? file : path.posix.normalize(linked.startsWith("/") ? linked.slice(1) : path.posix.join(path.posix.dirname(file), linked));
  if (resolved === ".." || resolved.startsWith("../")) return `${resolved} is outside the repository`;
  if (resolved !== "." && !exists(resolved.replace(/\/$/, ""))) return `${resolved} does not exist`;
  if (!anchor || !resolved.endsWith(".md")) return null;
  return anchorsFor(resolved).has(anchor) ? null : `${resolved} has no heading with the anchor #${anchor}`;
}

/** Every broken relative link of `files`: { file, line, target, reason }. */
export function brokenLinks(files, { read, exists }) {
  const anchors = new Map();
  const anchorsFor = (file) => {
    if (!anchors.has(file)) anchors.set(file, anchorsOf(read(file) ?? ""));
    return anchors.get(file);
  };
  return files.flatMap((file) => {
    const markdown = read(file);
    if (markdown === null) return [];
    return linksOf(markdown)
      .map(({ target, line }) => ({ file, line, target, reason: linkProblem(file, target, { exists, anchorsFor }) }))
      .filter((link) => link.reason);
  });
}

// Exists with exactly this case: macOS ignores case, GitHub does not.
function existsExactly(relative) {
  let dir = ROOT;
  for (const part of relative.split("/")) {
    if (part === "." || part === "") continue;
    let names;
    try {
      names = fs.readdirSync(dir);
    } catch {
      return false;
    }
    if (!names.includes(part)) return false;
    dir = path.join(dir, part);
  }
  return true;
}

function main() {
  const files = execFileSync("git", ["ls-files", "-z", "*.md"], { cwd: ROOT, encoding: "utf8" }).split("\0").filter(Boolean);
  const read = (file) => {
    try {
      return fs.readFileSync(path.join(ROOT, file), "utf8");
    } catch {
      return null;
    }
  };
  const broken = brokenLinks(files, { read, exists: existsExactly });
  for (const link of broken) console.error(`${link.file}:${link.line}: ${link.target}: ${link.reason}`);
  if (broken.length) {
    console.error(`docs:links: ${broken.length} broken link${broken.length === 1 ? "" : "s"}`);
    return 1;
  }
  console.log(`docs:links: no broken links in ${files.length} Markdown files`);
  return 0;
}

const invokedDirectly = (() => {
  try {
    return fs.realpathSync(process.argv[1]) === fs.realpathSync(fileURLToPath(import.meta.url));
  } catch {
    return false;
  }
})();

if (invokedDirectly) process.exitCode = main();
