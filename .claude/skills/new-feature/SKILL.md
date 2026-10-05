---
name: new-feature
description: Use when adding behaviour to LaunchDeck (a command, an action provider, a state or event, a starter surface, a view or panel), to put it in the owning module and land it with tests, schema and contract fixture.
---

# New feature

The rules are in `docs/architecture/ENGINEERING_RULES.md`; where things go is in `docs/architecture/CODEBASE_MAP.md` (Rust) and `docs/architecture/FRONTEND_MAP.md` (`src/`). This skill is the order to work in.

During the productization milestone, rule zero of `PRODUCTIZATION_PLAN.md` applies: no new product subsystem. If the feature needs one, stop and say so.

## Before writing code

1. **Owner.** Find the row in "Where does it belong?" (CODEBASE_MAP section 1), then the module in section 3 and the concept owner in section 4. For UI, the directory in FRONTEND_MAP section 2. Write the owner down. If nothing fits, a new module or `src/features/*` directory needs its map row in the same commit (R1); prefer extending an existing owner.
2. **Nearby code.** Read the owning file, its tests, and the code that calls it (`rg -n '<type or fn>' src-tauri/src src`).
3. **Existing abstractions.** Search for what already does part of the job and extend it (R17):
   ```sh
   rg -n -i '<concept>|<synonym>' src-tauri/src src
   ```
   No parallel concept: one identifier rule per vocabulary, one version-requirement implementation, one owner per type.
4. **IPC plan** (only if Rust sends a new or changed shape to the interface), per R10:
   * the Rust type (serde, camelCase like its neighbours) and the command in its file (`lib.rs` until A6, then `commands/<feature>.rs`);
   * the Zod schema (`src/lib/schema.ts` until A4, then `src/lib/schema/<domain>.ts`) and the `api.*` call in `src/lib/ipc.ts`;
   * the contract fixture: a scenario in `src-tauri/tests/support/`, a `dump_*` example, a Rust guard test, and a Vitest test that parses the fixture (CODEBASE_MAP section 6 shows the existing ones).
5. **Tests first.** Write the failing test before the code (R19): an inline `#[cfg(test)] mod tests`, a scenario in `src-tauri/tests/`, or a Vitest file next to the TypeScript code. Run it and see it fail for the right reason.

## Writing it

6. The smallest change in the owning module. New submodules are private with `pub use`; a new `pub` item needs a consumer and a line in the commit body (R15).
7. Keep the invariants: no MIDI outside `devices` (R5); plugins only through their doors (R6); the Context Engine only proposes (R7); React never evaluates rules (R8, R9); platform code only where R3 allows; an outside crate only in its adapter file (R4).

## After the change

8. Run the relevant tests:
   ```sh
   cargo test --manifest-path src-tauri/Cargo.toml --lib
   cargo test --manifest-path src-tauri/Cargo.toml --test <target>
   npm test
   ```
   (`LAUNCHDECK_CARGO` names the cargo binary the npm scripts use; use the same one here.)
9. Run `npm run check` (`npm run check:web` for a TypeScript-only change while a Rust package runs). Before package A2 lands: `npm run typecheck`, `npm run lint`, and `cargo fmt --check` and `cargo clippy --all-targets -- -D warnings` in `src-tauri/`.
10. Re-read every modified file (`git diff`) for creep: unrelated edits, new `pub` items, logic in `mod.rs`, a helper that duplicates one elsewhere, leftover debug output.
11. If ownership changed, update CODEBASE_MAP or FRONTEND_MAP in the same commit (R21).
12. If a fixture changed, it was regenerated with its `dump_*` example for this intended change, and the commit says so.
13. Commit the product change on its own; a structural move it needed goes in a separate, earlier commit (R18). Say NOT VERIFIED, with the reason, for anything you could not run (R21).

Finish with the `architecture-review` skill.
