#!/usr/bin/env node
// The app version is written in package.json and src-tauri/Cargo.toml (and in
// src-tauri/tauri.conf.json when it has one); they must be equal. Exit 1 if not.

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

/** One sentence per version that is missing or differs from package.json's. */
export function versionProblems({ packageVersion, cargoVersion, tauriVersion }) {
  if (!packageVersion) return [`${PACKAGE_JSON} has no version`];
  const problems = [];
  if (!cargoVersion) problems.push(`${CARGO_TOML} has no [package] version`);
  else if (cargoVersion !== packageVersion) {
    problems.push(`${CARGO_TOML} says ${cargoVersion}, ${PACKAGE_JSON} says ${packageVersion}`);
  }
  if (tauriVersion !== undefined && tauriVersion !== packageVersion) {
    problems.push(`${TAURI_CONF} says ${tauriVersion}, ${PACKAGE_JSON} says ${packageVersion}`);
  }
  return problems;
}

const readJson = (file) => JSON.parse(fs.readFileSync(path.join(ROOT, file), "utf8"));

// tauri.conf.json's `version` is a version or the path of a package.json that holds one.
function tauriVersion() {
  if (!fs.existsSync(path.join(ROOT, TAURI_CONF))) return undefined;
  const version = readJson(TAURI_CONF).version;
  if (typeof version !== "string" || !version.endsWith(".json")) return version;
  return readJson(path.join(path.dirname(TAURI_CONF), version)).version;
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
  console.log(`versions: ${versions.packageVersion} everywhere`);
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
