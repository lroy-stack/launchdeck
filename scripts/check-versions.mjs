#!/usr/bin/env node
// The app version has one source, src-tauri/Cargo.toml: Tauri reads it from
// there, so src-tauri/tauri.conf.json must have no version, and package.json
// keeps a copy that must be equal. Exit 1 if not.

import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const PACKAGE_JSON = "package.json";
const CARGO_TOML = "src-tauri/Cargo.toml";
const TAURI_CONF = "src-tauri/tauri.conf.json";

/** The `version` of Cargo.toml's [package] table, or null. */
export function cargoPackageVersion(toml) {
  let inPackage = false;
  for (const line of toml.split("\n")) {
    const table = /^\s*\[([^\]]+)\]/.exec(line);
    if (table) inPackage = table[1].trim() === "package";
    const version = inPackage && /^\s*version\s*=\s*"([^"]+)"/.exec(line);
    if (version) return version[1];
  }
  return null;
}

/**
 * One sentence per problem: a version missing or different from package.json's,
 * or any version in tauri.conf.json (`tauriVersion` is its raw value, `undefined` when absent).
 */
export function versionProblems({ packageVersion, cargoVersion, tauriVersion }) {
  if (!packageVersion) return [`${PACKAGE_JSON} has no version`];
  const problems = [];
  if (!cargoVersion) problems.push(`${CARGO_TOML} has no [package] version`);
  else if (cargoVersion !== packageVersion) {
    problems.push(`${CARGO_TOML} says ${cargoVersion}, ${PACKAGE_JSON} says ${packageVersion}`);
  }
  if (tauriVersion !== undefined) {
    problems.push(`${TAURI_CONF} has a version (${tauriVersion}); remove it, Tauri reads ${CARGO_TOML}`);
  }
  return problems;
}

const readJson = (file) => JSON.parse(fs.readFileSync(path.join(ROOT, file), "utf8"));

// tauri.conf.json's `version` as written (a version or a path to a package.json), or undefined.
function tauriVersion() {
  if (!fs.existsSync(path.join(ROOT, TAURI_CONF))) return undefined;
  return readJson(TAURI_CONF).version;
}

function main() {
  const versions = {
    packageVersion: readJson(PACKAGE_JSON).version,
    cargoVersion: cargoPackageVersion(fs.readFileSync(path.join(ROOT, CARGO_TOML), "utf8")),
    tauriVersion: tauriVersion(),
  };
  const problems = versionProblems(versions);
  for (const problem of problems) console.error(`versions: ${problem}`);
  if (problems.length) return 1;
  console.log(`versions: ${versions.packageVersion} in ${CARGO_TOML} and ${PACKAGE_JSON}, none in ${TAURI_CONF}`);
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
