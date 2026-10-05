# AGENTS.md

LaunchDeck turns MIDI grid controllers (the Novation Launchpad X, and a built-in virtual one) into programmable physical interfaces: a Rust runtime in a Tauri 2 app (`src-tauri/`) with a React and TypeScript interface (`src/`).

Rust owns the runtime; React only edits, shows and configures. Read this file first, then the rules and maps it links to.

## Layout

* `src-tauri/src/`: the runtime. Ownership of every module: `docs/architecture/CODEBASE_MAP.md`.
* `src/`: the interface. Ownership of every directory: `docs/architecture/FRONTEND_MAP.md`.
* `src-tauri/tests/`: cross-module scenarios; `src-tauri/tests/support/*_scenario.rs` are shared with the `dump_*` examples.
* `examples/`: contract fixtures (`*.json`, written by Rust, parsed by the Vitest suite) and the example plugins.
* `docs/`: product and architecture documentation.
* `PRODUCTIZATION_PLAN.md`: while the productization milestone runs, it settles architectural decisions. Do not re-decide what it settles; if the code proves it wrong, do the closest correct thing and say so.

## Commands

From the repository root:

| Command | What it runs |
| --- | --- |
| `npm run check` | typecheck, lint, architecture check, versions, doc links, Vitest, `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings` |
| `npm run check:web` | the same without cargo, architecture check limited to the web rules |
| `npm run check:full` | `check`, then `cargo test --no-fail-fast` and `npm run build` |
| `npm run check:arch` | the architecture check alone (`-- --report` for the large-file report) |

* `LAUNCHDECK_CARGO` selects the cargo binary these scripts run (default `cargo`).
* `npm run hooks:install` turns on an opt-in pre-commit hook (staged files only: architecture check, `cargo fmt`, ESLint, tsc).
* Single pieces: `npm run typecheck`, `npm run lint`, `npm test` (Vitest), `npm run build`; Rust: `cargo test --manifest-path src-tauri/Cargo.toml [--test <name>]`.
* Regenerate a contract fixture only for an intended change of what Rust emits, and name it in the commit: `cargo run --manifest-path src-tauri/Cargo.toml --example dump_<name> > examples/<fixture>.json` (the example-to-fixture table is in `docs/architecture/ENGINEERING_RULES.md`).

## Hard invariants

* No MIDI, notes or CCs outside `devices` (`src-tauri/src/devices/launchpad_x.rs`); everything else addresses a pad by `ControlPos`.
* Plugins never touch devices, the renderer or runtime internals; they act through the Plugin trait, host functions and the network broker.
* Secrets live only in the system credential store, never in a file, log or audit entry, and never reach a plugin (the legacy schema 1-3 exception is removed by package D4).
* The Context Engine never talks to devices; an application's identity is its stable id (bundle id, executable name, `WM_CLASS`), never its display name; no history of what was in front.
* React never evaluates domain rules: Rust decides (`actions_validate`, `review_feedback`, `explain_control`) and React renders the answer.
* Every IPC payload has a Zod schema and a contract fixture from a `dump_*` example; only `src/lib/ipc.ts` imports `@tauri-apps`.
* One owner per concept: look it up in the maps before adding a type, a helper or a module.
* Platform `cfg` only in `platform.rs` and the adapter files listed in CODEBASE_MAP section 5; logic takes `Platform` as a parameter.
* Structural and product changes go in separate commits; a refactor never edits fixtures.
* Honesty: anything not run is NOT VERIFIED, with the reason. Never describe something as tested on a system, device or service it did not run on.
* No new subsystem without an owning domain: a row in a map, in the same commit.

## Workflow

Before writing code:

1. Name the owning module from CODEBASE_MAP or FRONTEND_MAP.
2. Read the nearby code and its tests.
3. Search (`rg`) for an existing type, function or concept to extend; never add a parallel one.
4. For an IPC change, plan the Zod schema and the contract fixture first.
5. Write tests first where behaviour changes; characterization tests before a structural move.

After the change:

1. Run the relevant tests.
2. Run `npm run check` (`check:web` for a TypeScript-only change while a Rust package runs).
3. Re-read every modified file for creep: unrelated edits, new `pub` items, logic in `mod.rs`, duplicated helpers.
4. Update the maps in the same commit if ownership changed.

Commit messages: `<type>: <description>` (feat, fix, refactor, docs, test, chore, perf, ci), with a body that says what and why.

## Read next

* [docs/architecture/ENGINEERING_RULES.md](docs/architecture/ENGINEERING_RULES.md): rules R1-R21, the workflow, the quality commands, fixture regeneration.
* [docs/architecture/CODEBASE_MAP.md](docs/architecture/CODEBASE_MAP.md): Rust ownership, the import matrix, the contract fixtures, known exceptions.
* [docs/architecture/FRONTEND_MAP.md](docs/architecture/FRONTEND_MAP.md): frontend ownership and the allowed feature-to-feature imports.
* [docs/architecture/overview.md](docs/architecture/overview.md): how the runtime works, end to end.
