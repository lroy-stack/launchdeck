#!/usr/bin/env node
// The quality gate, one step after the other, stopping at the first failure:
//
//   node scripts/check.mjs          npm run check       typecheck, lint, architecture, versions,
//                                                       doc links, Vitest, cargo fmt, cargo clippy
//   node scripts/check.mjs --web    npm run check:web   the same without cargo, architecture
//                                                       limited to the web rules
//   node scripts/check.mjs --full   npm run check:full  check, then cargo test and vite build
//
// Cargo runs through scripts/cargo.mjs, so LAUNCHDECK_CARGO selects the binary.

import { spawnSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const MODES = { "": "check", "--web": "web", "--full": "full" };

const script = (file, ...args) => ({ script: file, args });
const tool = (pkg, bin, ...args) => ({ tool: [pkg, bin], args });

/** The steps of `mode` ("check", "web" or "full"), in order. */
export function stepsFor(mode) {
  const fast = [
    { name: "typecheck", ...tool("typescript", "tsc", "--noEmit") },
    { name: "lint", ...tool("eslint", "eslint", ".") },
    { name: "architecture", ...script("scripts/check-architecture.mjs", ...(mode === "web" ? ["--only=web"] : [])) },
    { name: "versions", ...script("scripts/check-versions.mjs") },
    { name: "doc links", ...script("scripts/check-docs-links.mjs") },
    { name: "vitest", ...tool("vitest", "vitest", "run") },
  ];
  if (mode === "web") return fast;
  const cargo = [
    { name: "cargo fmt", ...script("scripts/cargo.mjs", "fmt", "--check") },
    { name: "cargo clippy", ...script("scripts/cargo.mjs", "clippy", "--all-targets", "--", "-D", "warnings") },
  ];
  if (mode === "check") return [...fast, ...cargo];
  return [
    ...fast,
    ...cargo,
    { name: "cargo test", ...script("scripts/cargo.mjs", "test", "--no-fail-fast") },
    { name: "vite build", ...tool("vite", "vite", "build") },
  ];
}

// The JavaScript file behind a package's command, run with this Node (no shell, any OS).
function binPath(pkg, bin) {
  const dir = path.join(ROOT, "node_modules", pkg);
  const manifest = JSON.parse(fs.readFileSync(path.join(dir, "package.json"), "utf8"));
  return path.join(dir, typeof manifest.bin === "string" ? manifest.bin : manifest.bin[bin]);
}

const seconds = (since) => `${((Date.now() - since) / 1000).toFixed(1)} s`;

function runStep(step) {
  let entry;
  try {
    entry = step.script ? path.join(ROOT, step.script) : binPath(...step.tool);
  } catch {
    console.error(`check: ${step.tool[0]} is not installed; run npm ci`);
    return 1;
  }
  const result = spawnSync(process.execPath, [entry, ...step.args], { cwd: ROOT, stdio: "inherit" });
  return result.error ? 1 : (result.status ?? 1);
}

function main(args) {
  const mode = MODES[args.join(" ")];
  if (!mode) {
    console.error("usage: node scripts/check.mjs [--web | --full]");
    return 2;
  }
  const steps = stepsFor(mode);
  const started = Date.now();
  for (const [index, step] of steps.entries()) {
    const since = Date.now();
    console.log(`\ncheck: [${index + 1}/${steps.length}] ${step.name}`);
    const code = runStep(step);
    if (code !== 0) {
      console.error(`\ncheck: ${step.name} FAILED (exit ${code}) after ${seconds(since)}; stopped`);
      return code;
    }
    console.log(`check: ${step.name} passed in ${seconds(since)}`);
  }
  console.log(`\ncheck: all ${steps.length} steps passed in ${seconds(started)} (${mode})`);
  return 0;
}

const invokedDirectly = (() => {
  try {
    return fs.realpathSync(process.argv[1]) === fs.realpathSync(fileURLToPath(import.meta.url));
  } catch {
    return false;
  }
})();

if (invokedDirectly) process.exitCode = main(process.argv.slice(2));
