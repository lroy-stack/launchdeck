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

## Predictions of NEXT_PHASE.md

| Prediction | Status |
| --- | --- |
| Section 1: "expected: clean, 309 tests. Nothing here is known to depend on the system." | **Corrected.** Typecheck, lint and build are clean; one test of 309 fails, from a race between the test and a 150 ms debounce, not from the system. |
| Section 2: "If this fails, it is expected to fail in one place: `src/context/observer/macos.rs`", most likely for a missing Cargo feature, a block type, a `MainThreadMarker` or `unused_unsafe` | **Did not fail.** `macos.rs` compiled and passed clippy as written; none of the four expected kinds of error appeared. The feature list in `Cargo.toml` was already complete. |
| Section 2: Tauri warns that `dev.launchdeck.app` ends in `.app` | Not seen in `cargo check` or `clippy`; still to be looked for under Tauri's CLI. |
| Section 3: exactly four tests fail, all from Linux-recorded values | Not run yet |
