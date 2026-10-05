---
name: refactor-large-module
description: Use when splitting a large LaunchDeck file or moving code between modules without changing behaviour, such as the planned splits of the runtime or lib.rs.
---

# Refactor a large module

A structural refactor changes where code lives and nothing else. The rules behind each step are R13, R14, R15 and R18 in `docs/architecture/ENGINEERING_RULES.md`; the module rows are in `docs/architecture/CODEBASE_MAP.md`. If `PRODUCTIZATION_PLAN.md` section 5.2 already plans this split, follow its file list and public API exactly.

## 1. Plan the seams

1. Read the whole file. List its responsibilities and give each a target file named after what it holds (no `utils`, `helpers`, `common`, `misc`: R16).
2. List the public API that must survive: every `pub` item used by `lib.rs`, other modules, `src-tauri/tests/` and `src-tauri/examples/`:
   ```sh
   rg -n '<module path>::' src-tauri/src src-tauri/tests src-tauri/examples
   ```
3. Write the plan down: file → items it receives.

## 2. Characterization first

1. List the tests that pin the module's behaviour: its inline tests, the `src-tauri/tests` targets that use it, and the contract fixture guards (CODEBASE_MAP section 6).
2. If some behaviour is not pinned, add characterization tests that record what the code does today, in their own commit, before moving anything.
3. Run them and record the counts:
   ```sh
   cargo test --manifest-path src-tauri/Cargo.toml --no-fail-fast 2>&1 | rg '^test result'
   npm test
   ```

## 3. Move mechanically

* Cut and paste whole items. The only edits allowed are `use` lines, visibility (`pub(super)`, `pub(crate)`) and `mod` declarations.
* No renames, no new helpers, no logic changes, no rewording of messages, no "while I am here" fixes. Note them for a later product commit instead.
* Keep public paths: the old path keeps compiling through `pub use` in the parent, so no caller changes.
* New submodules are private, re-exported with `pub use` (R15).
* `mod.rs` ends with declarations, re-exports, docs and small glue only (R13).
* Tests move with their code, verbatim; a test-only file over ~300 lines goes to a sibling `tests.rs` (R19).

## 4. Fixtures unchanged

```sh
git diff --stat -- examples/ src-tauri/tests/fixtures/
```

must print nothing. A refactor never regenerates a fixture; a diff here means behaviour changed (R18).

## 5. Full gate

```sh
cargo fmt --manifest-path src-tauri/Cargo.toml --check
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
cargo test --manifest-path src-tauri/Cargo.toml --no-fail-fast
npm run check
```

(`LAUNCHDECK_CARGO` names the cargo binary the npm scripts use; use the same one here. Before package A2 lands, run `npm run typecheck`, `npm run lint` and `npm test` instead of `npm run check`.) The test counts equal those of step 2: same passed, same ignored, zero failed.

## 6. Commit

* The move is its own commit, `refactor: <what moved where>`, with a body that says it is a pure move, names the characterization tests and gives the counts before and after. No product change in it (R18).
* Update the CODEBASE_MAP rows for the new files in the same commit (R21).
* Check the commit holds only the intended paths: `git show --stat HEAD`.

Finish with the `architecture-review` skill.
