#!/usr/bin/env node
// The app version has one source, src-tauri/Cargo.toml: Tauri reads it from
// there, so src-tauri/tauri.conf.json must have no version, and package.json
// keeps a copy that must be equal. The license is MIT everywhere: every file
// that declares one says MIT, and LICENSE is the MIT text. Exit 1 if not.

import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const PACKAGE_JSON = "package.json";
const CARGO_TOML = "src-tauri/Cargo.toml";
const TAURI_CONF = "src-tauri/tauri.conf.json";
const LICENSE_FILE = "LICENSE";
const LICENSE = "MIT";
const PACKAGES_WITH_LICENSE = [PACKAGE_JSON, "sdk/assemblyscript/package.json"];
const EXAMPLE_PLUGINS = "examples/plugins";

/** A string field of Cargo.toml's [package] table, or null. */
export function cargoPackageField(toml, field) {
  const pattern = new RegExp(`^\\s*${field}\\s*=\\s*"([^"]+)"`);
  let inPackage = false;
  for (const line of toml.split("\n")) {
    const table = /^\s*\[([^\]]+)\]/.exec(line);
    if (table) inPackage = table[1].trim() === "package";
    const value = inPackage && pattern.exec(line);
    if (value) return value[1];
  }
  return null;
}

/** The `version` of Cargo.toml's [package] table, or null. */
export const cargoPackageVersion = (toml) => cargoPackageField(toml, "version");

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

/**
 * One sentence per problem: a declaration (`{file, license}`) that is missing
 * or not MIT, or a LICENSE file (`licenseText`, null when absent) that is not the MIT text.
 */
export function licenseProblems({ declarations, licenseText }) {
  const problems = [];
  if (licenseText === null) problems.push(`${LICENSE_FILE} is missing`);
  else if (!licenseText.startsWith(`${LICENSE} License`)) problems.push(`${LICENSE_FILE} is not the ${LICENSE} license text`);
  for (const { file, license } of declarations) {
    if (license === undefined || license === null) problems.push(`${file} declares no license; it should say ${LICENSE}`);
    else if (license !== LICENSE) problems.push(`${file} says ${license}; the license is ${LICENSE}`);
  }
  return problems;
}

const readJson = (file) => JSON.parse(fs.readFileSync(path.join(ROOT, file), "utf8"));

// tauri.conf.json's `version` as written (a version or a path to a package.json), or undefined.
function tauriVersion() {
  if (!fs.existsSync(path.join(ROOT, TAURI_CONF))) return undefined;
  return readJson(TAURI_CONF).version;
}

// Every declaration of the license: Cargo.toml, tauri.conf.json's bundle, and
// the package.json files of the app, the SDK and each example plugin.
function licenseDeclarations() {
  const plugins = fs
    .readdirSync(path.join(ROOT, EXAMPLE_PLUGINS), { withFileTypes: true })
    .filter((entry) => entry.isDirectory())
    .map((entry) => `${EXAMPLE_PLUGINS}/${entry.name}/package.json`)
    .filter((file) => fs.existsSync(path.join(ROOT, file)));
  return [
    { file: CARGO_TOML, license: cargoPackageField(fs.readFileSync(path.join(ROOT, CARGO_TOML), "utf8"), "license") },
    { file: `${TAURI_CONF} (bundle)`, license: readJson(TAURI_CONF).bundle?.license },
    ...[...PACKAGES_WITH_LICENSE, ...plugins].map((file) => ({ file, license: readJson(file).license })),
  ];
}

function readLicenseText() {
  const file = path.join(ROOT, LICENSE_FILE);
  return fs.existsSync(file) ? fs.readFileSync(file, "utf8") : null;
}

function main() {
  const versions = {
    packageVersion: readJson(PACKAGE_JSON).version,
    cargoVersion: cargoPackageVersion(fs.readFileSync(path.join(ROOT, CARGO_TOML), "utf8")),
    tauriVersion: tauriVersion(),
  };
  const declarations = licenseDeclarations();
  const problems = [...versionProblems(versions), ...licenseProblems({ declarations, licenseText: readLicenseText() })];
  for (const problem of problems) console.error(`versions: ${problem}`);
  if (problems.length) return 1;
  console.log(`versions: ${versions.packageVersion} in ${CARGO_TOML} and ${PACKAGE_JSON}, none in ${TAURI_CONF}`);
  console.log(`versions: ${LICENSE} in ${LICENSE_FILE} and ${declarations.length} declarations`);
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
