# Engineering rules

The single canonical list of rules for changing LaunchDeck, for people and for coding agents. Where things live is in the maps: [CODEBASE_MAP.md](CODEBASE_MAP.md) (Rust) and [FRONTEND_MAP.md](FRONTEND_MAP.md) (React). How the runtime works is in [overview.md](overview.md).

Each rule says how it is enforced:

* **Checked**: the architecture check (`npm run check:arch`, which runs `scripts/check-architecture.mjs`; both added by package A2) fails on a violation. It is part of `npm run check`, and in Claude Code a hook runs it after every edit.
* **Advisory**: the check reports it and never fails.
* **Review**: no tool catches it; the author and the reviewer do.

## Rules

The first sentence of each rule is its text in `PRODUCTIZATION_PLAN.md` section 5.3; what follows is the reason and how it is enforced.

**R1.** Ownership: every concept has one owner listed in the maps; a new top-level module or `src/features/*` directory needs a map row in the same commit. Code that needs a concept uses its owner instead of keeping a copy. One owner means a change happens in one place and a reader knows where to look. *Checked*: a `mod` in `src-tauri/src/lib.rs` missing from CODEBASE_MAP, or a `src/features/*` directory missing from FRONTEND_MAP, fails.

**R2.** Dependency direction per 5.1. Imports point inward, domain ← services ← adapters ← UI boundary, following the matrix copied into CODEBASE_MAP section 5. The domain stays testable without devices, the operating system or Tauri, and an adapter can be replaced without touching it. *Checked* for non-test Rust code.

**R3.** Platform isolation. `cfg!(target_os…)`, `#[cfg(target_os…)]` and `cfg(windows)` appear only in `platform.rs`, `context/observer/**`, `plugins/external/secrets.rs`, `plugins/keyboard.rs`, `actions/builtin.rs`, and in file-permission code under `cfg(unix)`; logic takes `Platform` as a parameter. Logic that receives the platform can be tested for all three systems on any machine; logic that reads `cfg!` is tested only where it runs. *Checked*.

**R4.** One external system, one adapter file. Each crate that reaches the outside world (MIDI, WebAssembly, the credential store, keyboard input, the foreground application, WebSocket, HTTP, Tauri) is used in exactly one adapter file, listed under crate confinement in CODEBASE_MAP section 5; a new outside system gets one adapter file and a row in that list. The risky surface of each dependency is then one file to read, audit or replace. *Checked*.

**R5.** No MIDI, notes or CCs outside `devices`; `ControlPos` is the only addressing. Raw bytes, note numbers and CC numbers live in `devices/` (today `devices/launchpad_x.rs`), the MIDI transport in `devices/midi/system.rs`, and everything else names a pad by `ControlPos` (`grid`, `top`, `side`, `logo`). A second device model then stays inside `devices`, and nothing above the device layer depends on one controller. *Review*, helped by two checks: `midir` is confined to `devices/midi/system.rs` (R4) and most modules may not import `devices` (R2).

**R6.** Plugins act only through the Plugin trait, host functions, hub writes by the host and the network broker; never devices, renderer or runtime internals; secrets never reach a plugin. Plugins are untrusted code: each of these doors is narrow, checked and logged, and a manifest only asks while the person grants. A secret is applied by the broker to a request for its one origin and is never handed to the module. *Checked* for imports (`plugins/**` may not import `devices`, `renderer`, `core::runtime`); the rest is *review*.

**R7.** The Context Engine imports only `core::model`, outputs only a target for `Runtime::activate`, persists rules and settings only. An application is identified by its stable id in a namespace (bundle id, executable name, `WM_CLASS`), never by its display name, and nothing about which application was in front is written down: no history, no titles, no counts. Auto Context therefore goes through the same activation as a person's click and leaves no record. *Checked* for imports; the rest is *review*.

**R8.** Only providers write hub values; values never persisted; React never evaluates rules. The hub says how things are now, so a saved value would be wrong on the next start. A rule evaluated in React could disagree with what the pad shows; React asks Rust (`explain_control`, `review_feedback`) and renders the answer. *Review*.

**R9.** No duplicated domain evaluation in React; an allowed mirror (`normalizeArgs`, `operatorsFor`, `problemsOf`) names its Rust counterpart and has a Rust-generated fixture with accepted and rejected cases. A mirror exists only so a form can react while the person types; Rust still decides on save. Two evaluators drift apart unless a shared fixture pins them (E1 adds `examples/param-cases.json` for `normalizeArgs`). *Review*.

**R10.** IPC: Zod schema for every payload; only `src/lib/ipc.ts` imports `@tauri-apps`; every Rust-emitted shape has an `examples/*.json` fixture from a `dump_*` example guarded by a Rust test; fixtures are regenerated only for an intended change. The Rust types and the TypeScript types then cannot disagree silently: the Rust test fails when the runtime emits a field the fixture lacks, and Vitest parses the fixture with the schema. *Checked*: `@tauri-apps` outside `ipc.ts`, and `preview.ts` imported by anything but `ipc.ts`; the fixtures are guarded by their tests (CODEBASE_MAP section 6).

**R11.** Persistence owns the schema version and `migrate()`; one file per schema step; features never call migrations; every file write goes through `write_atomic`/`write_private` (the append-only audit is the exception). An old workspace can always be read, a newer one is refused instead of downgraded, and a crash mid-write never leaves half a file. *Review*.

**R12.** Surfaces: model in `core::model`, starters in `surfaces/<starter>.rs`, edits through `Runtime` methods; artifacts never serialize `Workspace` types directly. Every edit is then validated in one place, and the portable file format stays independent of the workspace schema so that either can change without breaking the other. *Review*.

**R13.** `mod.rs` composes: declarations, re-exports, docs, small glue. Logic goes in a file whose name says what it holds. A `mod.rs` that gathers logic becomes the place everything ends up, as `surfaces/mod.rs` did. *Review*.

**R14.** Large modules: 400 production lines is a signal, 700 needs a cohesion justification in the allowlist or a split plan; test lines do not count; split right before the workstream that edits the file. A large file is fine when it holds one concept (the plan keeps `plugins/external/manifest.rs` whole, for example); a file with many responsibilities is split by seam, behaviour unchanged. *Advisory*: the check's report lists files of 400 production lines or more as "watch", expects files of 700 or more in its allowlist with a reason and a baseline, and reports growth above 10 %.

**R15.** Minimal public API: new submodules private with `pub use`; `pub` only when `lib.rs`/`commands`, `tests/`, `examples/` or a documented SDK needs it; a new `pub` item in an existing module is justified in the commit body. Every `pub` item is something other code may come to depend on, and the crate already has over a thousand. Narrowing the existing ones is deferred; new code does not add to them. *Review*.

**R16.** No dumping grounds (`utils`, `helpers`, `common`, `misc`). A helper belongs to the module that owns its concept; a generic bucket hides ownership and attracts unrelated code. *Checked*: a new file or directory with one of these names fails.

**R17.** No parallel systems: search before adding; one Pack concept, one version-requirement implementation (`semver`), one identifier rule per vocabulary. Before writing a type, a parser or a helper, search for an existing one (`rg`) and extend it. Two implementations of one idea disagree sooner or later: today two hand-written version parsers do, and C1 replaces them. *Review*.

**R18.** Structural and product changes in separate commits; characterization green before and after; a refactor never edits fixtures. A pure move is easy to review because nothing may change; mixed with a behaviour change it cannot be reviewed as a move. A fixture diff in a refactor commit means the refactor changed behaviour. *Review*.

**R19.** Tests: inline `#[cfg(test)] mod tests`, over ~300 lines a sibling `tests.rs`, cross-module scenarios in `src-tauri/tests` with `tests/support/*_scenario.rs`, TypeScript tests next to the code. Tests sit where a reader of the code looks for them, and the scenario files are shared with the `dump_*` examples that write the contract fixtures. *Review*.

**R20.** Frontend: `shared (components, lib, stores) → features → app`; features import other features only at listed composition points; `src/lib` is pure TypeScript plus IPC, no React; stores hold UI state only. Rust is the source of truth, so the store mirrors what Rust pushes and adds only visual choices (view, selection, toasts). *Checked*: `src/lib` importing React, features, components or stores; `src/stores` importing features or components; a feature-to-feature edge not listed in FRONTEND_MAP.

**R21.** Honesty: anything not run is NOT VERIFIED with a reason; docs and maps change in the same commit as an ownership change. "Written, not run" is an acceptable status; claiming a test on a system, device or service it never ran on is not. A map is only useful while it matches the tree. *Review* (R1's check catches a missing map row).

## Agent workflow

Before writing code:

1. Name the owning module from the maps. If no row fits, read "Where does it belong?" in CODEBASE_MAP; a new top-level module or feature directory needs its map row (R1).
2. Read the nearby code and its tests.
3. Search for existing abstractions (`rg` the type, function and concept names) and extend them; do not add a parallel concept (R17).
4. For a new or changed IPC payload, plan the Zod schema and the contract fixture with its `dump_*` example and Rust guard (R10).
5. Write the tests first where behaviour changes; add characterization tests before a structural move (R18).

After the change:

1. Run the relevant tests (the Rust test targets you touched, Vitest).
2. Run `npm run check` (`npm run check:web` when only TypeScript changed and a Rust package is running at the same time).
3. Re-read every modified file for creep: unrelated edits, new `pub` items (R15), logic in `mod.rs` (R13), duplicated helpers.
4. Update the maps if ownership changed, in the same commit (R21).

## Quality commands

| Command | Runs |
| --- | --- |
| `npm run check` | typecheck, lint, `check:arch`, `versions:check`, `docs:links`, Vitest, `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings` |
| `npm run check:web` | the same without the two cargo steps, with the architecture check limited to the web rules (`--only=web`); for a TypeScript package running next to a Rust package |
| `npm run check:full` | `check`, then `cargo test --no-fail-fast` and `npm run build` |
| `npm run check:arch` | the architecture check alone; `npm run check:arch -- --report` prints the large-file report |

`LAUNCHDECK_CARGO` selects the cargo binary these scripts run (default `cargo`), for example a wrapper that pins the toolchain and the target directory.

Each piece also runs alone: `npm run typecheck`, `npm run lint`, `npm test`, `npm run versions:check`, `npm run docs:links`, and for Rust `npm run rust:fmt`, `rust:clippy`, `rust:test` (arguments after `--` reach cargo, for example `npm run rust:test -- --test context_engine`). `npm run hooks:install` turns on an opt-in pre-commit hook that checks only what is staged: the architecture check, `cargo fmt` when Rust is staged, ESLint and tsc when TypeScript is staged.

## Regenerating a contract fixture

Only for an intended change of what Rust emits, never to make a failing test pass. From the repository root:

```sh
cargo run --manifest-path src-tauri/Cargo.toml --example dump_<name> > examples/<fixture>.json
```

| Example | Fixture |
| --- | --- |
| `dump_workspace` | `examples/default-workspace.json` |
| `dump_agents` | `examples/agents-snapshot.json` |
| `dump_plugins` | `examples/plugins-overview.json` |
| `dump_plugin_examples` | `examples/plugin-examples.json` |
| `dump_catalog` | `examples/action-catalog.json` |
| `dump_observables` | `examples/observables.json` |
| `dump_context` | `examples/context-status.json` |
| `dump_app_info` | `examples/app-info.json` |

Then run the Rust guard test and Vitest again, and name the regenerated fixture and the reason in the commit message. A structural commit never regenerates a fixture (R18).
