#!/usr/bin/env node
// Runs cargo on src-tauri/Cargo.toml from anywhere in the repository:
//
//   node scripts/cargo.mjs <subcommand> [arguments...]
//
// becomes `$LAUNCHDECK_CARGO <subcommand> --manifest-path <repo>/src-tauri/Cargo.toml
// [arguments...]`, with every argument (`--` and what follows included) passed through.
// LAUNCHDECK_CARGO names the cargo binary or a wrapper script (default `cargo`); it is
// one executable path, not a command line. Exits with cargo's exit code.

import { spawnSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const USAGE = "usage: node scripts/cargo.mjs <subcommand> [arguments...]";

/** The command and arguments that run cargo `args` on the repository's manifest. */
export function cargoInvocation(args, env = process.env, root = ROOT) {
  const toolchain = args.filter((arg, i) => arg.startsWith("+") && args.slice(0, i).every((a) => a.startsWith("+")));
  const [subcommand, ...rest] = args.slice(toolchain.length);
  if (!subcommand) throw new Error(USAGE);
  const manifest = path.join(root, "src-tauri", "Cargo.toml");
  return { command: env.LAUNCHDECK_CARGO || "cargo", args: [...toolchain, subcommand, "--manifest-path", manifest, ...rest] };
}

function main(args) {
  let invocation;
  try {
    invocation = cargoInvocation(args);
  } catch (error) {
    console.error(error.message);
    return 2;
  }
  const result = spawnSync(invocation.command, invocation.args, { stdio: "inherit" });
  if (result.error) {
    console.error(`cargo.mjs: cannot run ${invocation.command}: ${result.error.message} (set LAUNCHDECK_CARGO to the cargo to use)`);
    return 127;
  }
  return result.status ?? 1;
}

const invokedDirectly = (() => {
  try {
    return fs.realpathSync(process.argv[1]) === fs.realpathSync(fileURLToPath(import.meta.url));
  } catch {
    return false;
  }
})();

if (invokedDirectly) process.exitCode = main(process.argv.slice(2));
