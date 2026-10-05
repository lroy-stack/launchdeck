---
name: architecture-review
description: Use before committing a LaunchDeck change that adds, moves or splits code, adds public items or adds imports between modules, to check it against the codebase maps and the engineering rules.
---

# Architecture review

Review the current change against `docs/architecture/ENGINEERING_RULES.md`, `docs/architecture/CODEBASE_MAP.md` and `docs/architecture/FRONTEND_MAP.md`. Do not restate those documents; cite their rule numbers and sections. Report findings; change code only if asked.

## 1. Scope

List the changed files with `git status --short` and `git diff --stat` (for a branch: `git diff --stat main...HEAD`). Review every one, tests included.

## 2. Owner

For each changed file, name its owning module from CODEBASE_MAP section 3 (Rust) or FRONTEND_MAP section 2 (`src/`). A file without an owner, or a new top-level module or `src/features/*` directory without a map row in the same change, is an R1 finding.

## 3. Architecture check

Run `npm run check:arch`. Every violation it prints is a finding (R2, R3, R4, R10, R16, R20). If the script does not exist yet (package A2), check by hand: `rg -n 'use crate::' <changed .rs files>` against CODEBASE_MAP section 5, and `rg -n 'from "\.\./' <changed .ts/.tsx files>` against FRONTEND_MAP sections 1 and 3.

## 4. Size

Compare `git diff --stat` with the R14 thresholds, counting production lines only (test files and `#[cfg(test)]` blocks do not count):

* 400 or more: "watch"; say so.
* 700 or more: the file must be in the check's allowlist with a reason, or have a split plan.
* Growth above 10 % over the allowlisted baseline: say so.

`npm run check:arch -- --report` prints the table once A2 has landed.

## 5. What the diff adds

Run each search and read every hit:

* New public items (R15): each needs a consumer (`lib.rs`/`commands`, `tests/`, `examples/`, the SDK) and a reason in the commit body.
  ```sh
  git diff -U0 | rg '^\+\s*pub '
  ```
* New cross-module edges (R2, R20): each allowed by the matrix or the feature edge list.
  ```sh
  git diff -U0 | rg '^\+.*(use crate::|from "\.\./)'
  ```
* Logic in `mod.rs` (R13): declarations, re-exports, docs and small glue only.
  ```sh
  git diff -- '*mod.rs'
  ```
* Platform code (R3) and outside crates (R4): new `cfg` only where R3 allows; a new `use <crate>::` only in that crate's adapter file.
  ```sh
  git diff -U0 | rg '^\+.*(cfg!?\(|#\[cfg\()'
  ```
* Dumping grounds (R16): no new file or directory named `utils`, `helpers`, `common` or `misc`.
* IPC (R10): a new Tauri command or payload has its Zod schema, `dump_*` fixture, Rust guard and Vitest parse.
* Fixtures (R10, R18): changed only for an intended change, named in the commit, never in a refactor.
  ```sh
  git diff --stat -- examples/ src-tauri/tests/fixtures/
  ```

## 6. Duplicates

For each new function, type or constant, search for the same idea under other names before accepting it (R17):

```sh
rg -n -i '<stem>|<synonym>' src-tauri/src src
```

Examples of stems: `valid`, `parse`, `version`, `ident`, `platform`, `slug`, `write`. Check CODEBASE_MAP section 4 for the owner of the concept. A second implementation of an owned concept is a finding.

## 7. Maps and honesty

* Ownership changed (new module, moved concept, new allowed edge): the maps change in the same commit (R1, R21).
* Every claim in docs or commit messages about what was run is true; anything not run says NOT VERIFIED with the reason (R21).
* Structural moves and behaviour changes are in separate commits (R18).

## 8. Report

One line per finding, most severe first, in this fixed format:

```text
<file>:<line> | <rule> | <what is wrong> | <fix>
```

For example:

```text
src-tauri/src/state/hub.rs:12 | R2 | state imports crate::plugins::PluginRegistry | take the value as a parameter; the registry stays in plugins
```

End with a count of findings, or `No findings.` Name any step you could not run and why.
