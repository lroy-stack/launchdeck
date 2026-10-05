# macOS verification record

Running record for the macOS phase described in [NEXT_PHASE.md](NEXT_PHASE.md) (section 8). Everything below was run and seen in this session unless it says **not run**. Branch `macos-verification`, started from `16d6b32`.

## Machine

| | |
| --- | --- |
| Hardware | MacBook Air `Mac14,2`, Apple M2 (arm64), 8 cores, 8 GB RAM |
| System | macOS 15.4.1 (24E263), Darwin 24.4.0 |
| Disk at start | 245 GB APFS container, **260 MB free** (99.9 % used). Freed by hand to 15 GB before anything was built. |

## Toolchain

| Tool | Version | Note |
| --- | --- | --- |
| Xcode Command Line Tools | 16.3 (`com.apple.pkg.CLTools_Executables` 16.3.0.0.1.1742442376), Apple clang 17.0.0, `xcode-select -p` = `/Library/Developer/CommandLineTools` | No full Xcode (`xcodebuild` unavailable). Tauri's prerequisites say the Command Line Tools are enough for a desktop app. |
| Rust used for this record | **1.97.0** (2d8144b78 2026-07-07), cargo 1.97.0, rustfmt 1.9.0, clippy 0.1.97 | Installed beside the machine's default with `rustup toolchain install 1.97.0 --profile minimal --component rustfmt,clippy`, and called as `cargo +1.97.0`: the same compiler as the Linux run, so a difference comes from the system and not from the compiler. |
| Machine's default Rust | 1.83.0 (2024-11-26), left as the default | Older than `rust-version = "1.95"` in `src-tauri/Cargo.toml`; cargo would refuse to build with it. |
| rustup | 1.27.1, updated itself to 1.29.1 during the install | rustup's default self-update. |
| Node / npm | 22.23.3 (nvm) / 10.9.9 | Meets "Node 20 or newer (22 was used)". |

Build setting for this machine only, to fit the disk: Rust is compiled with `CARGO_PROFILE_DEV_DEBUG=0 CARGO_INCREMENTAL=0` in the environment (no debug info, no incremental cache). Nothing in the repository changes; panics still report file and line.

## Section 1: setup

| Command | Result |
| --- | --- |
| `xcode-select -p`, `rustc --version`, `node --version` | See the table above. |
| `npm ci` | Ok: 261 packages in 6 s, 153 MB. `npm audit`: 2 moderate, both in `@vitest/mocker` under `vitest` (development only). Not changed. |
| `npm run typecheck` | Clean (2.9 s). |
| `npm run lint` | Clean (1.8 s). |
| `npm test` | **1 failed, 308 passed** (309 tests, 18 files). See below. |
| `npm run build` | Ok (5.4 s). Vite's usual warning that `index-*.js` is over 500 kB (509 kB). |

### Failure 1: `src/features/feedback/rules.test.tsx`, "an event rule > builds the event pad: a scoped state, and an event that shows Error for 0.7 seconds, and saves what the runtime was tested with"

```
AssertionError: expected '' to be '0.2' // Object.is equality
 ❯ src/features/feedback/rules.test.tsx:557:25
    557|     expect(seconds.min).toBe("0.2");
```

Not predicted. It failed 5 times out of 5 (the full suite twice, the file alone three times), so it is not intermittent on this machine.

Cause, from reading the code: the field's `min` and `max` come from the runtime's limits (`EventRules.tsx:206`, `limits ? limits.minHoldMs / 1000 : undefined`), which arrive in the answer to `review_feedback`. `useFeedbackReview` asks only after 150 ms without a change to the draft (`DEBOUNCE_MS`). The test asserts `min` right after its last `selectOptions`, without waiting, so it passes only if some earlier step left 150 ms of quiet for a review to land. The neighbouring test "holds for a time the runtime accepts: the limits come from Rust" checks the same attribute inside `waitFor`. Nothing here depends on macOS; it depends on how fast the steps run. As far as the code shows, the product is not at fault: the field gets its limits once the answer arrives.

Fixed in the test only (commit `ae2dda4`): it now waits for the bound with `waitFor`, as its neighbour does. After the fix: the file alone 3 times out of 3, then `npm test` **309 passed, 0 failed**; typecheck and lint clean.

## Section 2: first build

All from `src-tauri/`, with `cargo +1.97.0` and the environment above.

| Command | Result |
| --- | --- |
| `cargo check --all-targets` | **Ok on the first try: 0 errors, 0 warnings**, 1 min 46 s (315 crates, including the index download). |
| `cargo fmt --check` | Clean. |
| `cargo clippy --all-targets -- -D warnings` | **Clean.** Run twice: the second time after `touch src/lib.rs src/context/observer/macos.rs`, with `-v`, to be sure the crate was linted and not taken from `cargo check`'s cache: 32 `clippy-driver` runs, no warning. |

`src/context/observer/macos.rs` went through the compiler with no change. Evidence that it was compiled and not skipped by a `cfg`: `objc2`, `block2` 0.6.2, `objc2-foundation` 0.3.2 and `objc2-app-kit` 0.3.2 were checked, and the library's metadata (`liblaunchdeck_lib-*.rmeta`) contains `MacObserver`. The vendored bindings match what the adapter assumed: `sharedWorkspace`, `notificationCenter`, `frontmostApplication`, `bundleIdentifier`, `localizedName` and `processIdentifier` are safe methods with no `MainThreadMarker`; `addObserverForName_object_queue_usingBlock` is `unsafe` and takes `&block2::DynBlock<dyn Fn(NonNull<NSNotification>)>`, which `&RcBlock` dereferences to. Cargo features: none added.

Also compiled for the first time on this system, with no error: `keyring` 4.2.0 (Keychain), `midir` 0.11.0 with `coremidi` 0.9.2, `enigo` 0.5.0. Compiled is not run: each of them is still to be seen working (section 4).

Not seen: the warning about the bundle identifier ending in `.app` that NEXT_PHASE.md mentions. It did not appear in `cargo check` or `cargo clippy`; it is presumably printed by Tauri's CLI (`npm run tauri dev` / `build`), to be looked for in section 4.

Read while checking, not changed: the adapter drops its observer token without `removeObserver`, so the block would outlive `run()`. The runtime (`core/runtime.rs`, around line 576) starts the observer once and never stops it, so the block and the thread live as long as the process and this has no effect today.

`src-tauri/target` after `check` and `clippy`: 911 MB; 12 GB left on the disk.

## Section 3: tests

`cargo +1.97.0 test --no-fail-fast` (without `--no-fail-fast` cargo stops at the first failing test program and the list would be incomplete). 2 min 38 s including the build; `src-tauri/target` grew to 4.4 GB, 9.3 GB left on the disk.

**704 passed, 5 failed, 2 ignored, in 22 targets** (the library, the binary, 19 test files, doc-tests). 711 tests: the Linux run had 714 (712 passed, 2 ignored). The 3 not built here are the unit tests of `context/observer/x11.rs`, a module compiled only on Unix other than macOS. The 2 ignored are the same as on Linux: the real credential store test (`secret_store`) and the timing benchmark (`state_characterization`).

| Target | Result |
| --- | --- |
| `--lib` | 555 passed, **2 failed** |
| `--test context_contract` | 0 passed, **1 failed** |
| `--test context_engine` | 12 passed, **2 failed** |
| the other 19 targets | all passed |

### The five failures, before anything was changed

1. `surfaces::tests::contract_fixture_matches_the_default_workspace` (lib): `examples/default-workspace.json is stale`. Running `cargo run --example dump_workspace` into a scratch file (the fixture was not touched) and diffing it with the fixture shows exactly four pads differ: Development, Terminal (`x-terminal-emulator` → `Terminal`), VS Code (`code` → `Visual Studio Code`); Productivity, Files (`xdg-open .` → `Finder`), Screenshot (`shell` `gnome-screenshot --interactive` → `system.keyboardShortcut` `cmd+shift+4`). **Predicted.**
2. `persistence::tests::a_real_v3_workspace_migrates_to_exactly_what_ships_now` (lib): `surface 'development' differs after migration` (the assertion stops at the first surface that differs; Productivity differs too, per the diff above). **Predicted.**
3. `the_context_fixture_has_every_field_the_runtime_emits` (`tests/context_contract.rs`): `examples/context-status.json is out of date, missing ["auto.problems[].kind", "manual.problems[].kind"]`. On macOS every `x11` rule gets a problem `{ "kind": "otherSystem" }` where the fixture, made on Linux, has `null`. **Predicted.**
4. `rules_are_checked_reordered_and_flagged` (`tests/context_engine.rs`): `left: [Some(OtherSystem), Some(OtherSystem), Some(Shadowed { by: 0 }), Some(OtherSystem)]`, `right: [None, None, Some(Shadowed { by: 0 }), None]`. Same cause. **Predicted.**
5. `the_same_application_again_is_a_no_op` (`tests/context_engine.rs`): `assertion 'left == right' failed: nothing was activated, left: 2, right: 0`. **Not predicted, and intermittent**: 1 failure in 10 runs alone, 1 in 5 runs of its target, and the full run above.

   Cause, seen with a temporary `eprintln!` (reverted, never committed): in a failing run the events are the same as in a passing one, one `surface.changed` and one `page.changed`, both for `development`, which is the first activation. Only the baseline differs: 0 instead of 2. `until_shown` polls the runtime's state, which `activate` changes under its lock before it publishes `surface.changed` and `page.changed` on the bus, which delivers them to the interface on another task. The test took its baseline in between. The five repeated observations activated nothing (a second activation of the same surface would publish `page.changed` without `surface.changed`). The same window covers the test's other two counts (`ui::STATE`, `ui::CONTEXT`), which `activate` emits after publishing. Nothing about it is specific to macOS; whether it also happens on Linux could not be checked here.

### Fixes, one commit each

| Commit | Fixes | What changed |
| --- | --- | --- |
| `3002297` | failures 1 and 2 | The starter workspace's system is a parameter: `Platform { Linux, MacOs, Windows }`, `default_workspace_for(platform)`, `default_workspace()` passing `Platform::current()`. `demo_cmd` (a fifth `cfg!`, Windows only, not listed in NEXT_PHASE.md) takes it too, or the Windows runner would still differ from the Linux fixture. The two tests ask for Linux, as their files hold; `examples/dump_workspace.rs` writes the Linux workspace on every system (checked: on this Mac its output is byte for byte `examples/default-workspace.json`). New tests: on every system, the pads that differ per system and that nothing else differs. |
| `093e7c1` | failures 3 and 4 | `ForegroundObserver::namespace()`, by default the namespace of the system LaunchDeck was built for (right for the macOS, Windows and X11 observers and for `NoObserver`, so the product behaves as before everywhere); the scripted observer says `x11`; the engine keeps it next to the observer's status and `status()` uses it instead of `native_namespace()`. `macos.rs`, `windows.rs`, `x11.rs` and `identity.rs` unchanged. New test `a_rule_for_another_system_is_flagged_wherever_this_runs`. `dump_context` on this Mac now writes exactly `examples/context-status.json`. |
| `699abf3` | failure 5 | Test helper `until_heard(surface)`: waits until the last `page.changed` the interface received is for that surface and the last context it was sent is the runtime's current one (sent after the state). Used by the no-op test and by two tests with the same window that had not failed yet (`app_a_shows_surface_a_app_b_shows_surface_b_on_the_launchpad`, `the_interface_is_told_when_the_context_changes_and_only_then`). No sleep added. |

No fixture under `examples/` was regenerated or changed.

### After the fixes

| Run | Result |
| --- | --- |
| `the_same_application_again_is_a_no_op` alone, 50 times | 50 passed (before: 1 failure in 10) |
| `cargo test --test context_engine`, 20 times | 20 × 15 passed (before: 1 failure in 5) |
| `cargo test --no-fail-fast`, 3 full runs | each **715 passed, 0 failed, 2 ignored** (711 before + 6 new tests), 102 s, 66 s, 66 s |
| `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings` | clean after each commit |
| `npm test`, `typecheck`, `lint` | 309 passed; clean |

`src-tauri/src/surfaces/mod.rs` grew from 905 to 1105 lines in `3002297`, mostly from the new per-system tests.

### "Worth watching" in NEXT_PHASE.md

| Item | Result |
| --- | --- |
| Tests that reach a local server through `localhost` | Pass: `localhost_is_resolved_and_reached_when_granted`, `a_grant_is_for_one_scheme_one_host_and_one_port`, `a_name_that_resolves_outside_its_reach_is_refused`, `bad_urls_wrong_schemes_and_other_ports_never_reach_a_server`. |
| Stopping a runaway plugin on Apple silicon | Pass: `an_endless_loop_runs_out_of_fuel_and_is_stopped`, `an_endless_loop_is_also_stopped_by_the_wall_clock`, `runaway_recursion_is_a_stack_overflow_trap`. |
| `#[cfg(unix)]` tests, first time on macOS | All 7 pass: `private_files_are_readable_by_the_owner_only`, `the_audit_log_is_readable_by_the_owner_only`, `symbolic_links_are_refused`, `shell_reports_output_and_failure`, `shell_runs_through_the_same_path`, `failing_command_ends_in_the_error_state`, `unbound_pad_goes_running_then_success_and_emits_action_events`. |

## Section 5: continuous integration

`.github/workflows/ci.yml` (commit `b0f13ba`), from the draft in NEXT_PHASE.md. Checked on 2026-10-05 with `gh api` against the actions' releases and `actions/runner-images`:

| Draft | Now | Why |
| --- | --- | --- |
| `actions/checkout@v7`, `actions/setup-node@v7`, `Swatinem/rust-cache@v2` | unchanged | Current majors: v7.0.1, v7.0.0, v2.9.2. |
| `ubuntu-24.04`, `macos-latest`, `windows-latest` | unchanged | `ubuntu-latest` still means 24.04 and moves to 26.04 in November 2026 (runner-images #14748, open). `macos-latest` is macOS 26 on arm64 (#14167, closed). `windows-latest` is Windows Server 2025. |
| `dtolnay/rust-toolchain@stable` | `@1.97.0` | Stable is 1.99.0 (2026-09-28). 1.97.0 is what was verified on Linux and here; a newer clippy with `-D warnings` could fail for reasons unrelated to any system. Moving is a change of its own. |
| Linux packages | `pkg-config` and `libdbus-1-dev` added | `cargo tree --target x86_64-unknown-linux-gnu`: `libdbus-sys` (through `tauri` → `tao`) links D-Bus with pkg-config; it reached the container only as a dependency of the GTK packages. `alsa-sys` (`midir`) and `xkbcommon` (`enigo`) are covered by the draft's list. Nothing uses OpenSSL or libxdo; `libssl-dev` and `libxdo-dev` stay as Tauri's documented prerequisites. |
| `cargo test` | `cargo test --no-fail-fast` | Every failure is listed, not just those of the first failing program (Windows, section 5 item 2). |
| step order | fast checks (types, lint, fmt, clippy) first, then tests and builds | So that a later check (an architecture check, for example) has an obvious place. Rust does not need the built interface: `cargo clippy --all-targets` passed here with `dist/` moved away. |
| | `permissions: contents: read`, `concurrency` cancelling superseded runs, `CARGO_PROFILE_DEV_DEBUG: line-tables-only`, `core.autocrlf false` on Windows, 60 min timeout | No step needs a secret or a token. |

Checked: the YAML parses, and `actionlint` 1.7.12 reports nothing.

**Not run.** The workflow runs on a push to `main` or on a pull request; nothing has been pushed.

Not done from section 5: the desktop suite job (Linux, on demand or nightly, item 3).

## Predictions of NEXT_PHASE.md

| Prediction | Status |
| --- | --- |
| Section 1: "expected: clean, 309 tests. Nothing here is known to depend on the system." | **Corrected.** Typecheck, lint and build are clean; one test of 309 fails, from a race between the test and a 150 ms debounce, not from the system. |
| Section 2: "If this fails, it is expected to fail in one place: `src/context/observer/macos.rs`", most likely for a missing Cargo feature, a block type, a `MainThreadMarker` or `unused_unsafe` | **Did not fail.** `macos.rs` compiled and passed clippy as written; none of the four expected kinds of error appeared. The feature list in `Cargo.toml` was already complete. |
| Section 2: Tauri warns that `dev.launchdeck.app` ends in `.app` | Not seen in `cargo check` or `clippy`; still to be looked for under Tauri's CLI. |
| Section 3: four tests fail on macOS, all from values recorded on Linux | **Confirmed**, all four, for the reasons given. |
| Section 3: "Anything else that fails is new information" | One more, intermittent: `the_same_application_again_is_a_no_op`, a race in the test's baseline, not a fault of the product and not specific to macOS. |
| Section 3: `localhost`, fuel and wall clock on Apple silicon, `#[cfg(unix)]` tests (no prediction / expected to pass) | All pass. |
