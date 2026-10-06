# Release record: LaunchDeck 0.3.0

The validation of the release candidate of 0.3.0, the first external alpha. Everything below was run on this machine on 2026-10-06 unless it says **NOT VERIFIED**. The procedure is [releasing](../contributing/releasing.md); the plan it follows is section 11 of `PRODUCTIZATION_PLAN.md`.

## Release candidate

**`900c08d`** (`900c08d78c5d736107703c18607ad7a749b75d6a`, "docs: link the README to the getting-started, user and contributing pages"), on branch `productization`. It is the last commit before this record. Every check below ran on it with a clean tree. The commit that adds this record changes documentation only (this file, the verification index, README "Verified where", the CHANGELOG, the plan's status column, the install, releasing and manual-check pages and a status note in the history), so the code, the fixtures and the bundle metadata are those of `900c08d`.

Two commits were made by this package before the candidate:

| Commit | What |
| --- | --- |
| `df4dc68` chore: bundle metadata for the alpha | `tauri.conf.json`: category Productivity, short and long description, `macOS.minimumSystemVersion` `11.0`, `macOS.signingIdentity` `-` (ad hoc). No publisher or copyright; the license stays MIT. |
| `900c08d` docs: link the README to … | README links to the getting-started, user and contributing pages that R3 created after R4 wrote the README. |

No `fix:` commit was needed: no check failed.

## Machine and toolchain

| | |
| --- | --- |
| Hardware | MacBook Air, Apple M2 (arm64), 8 GB RAM |
| System | macOS 15.4.1 (24E263) |
| Rust | rustc 1.97.0 (2d8144b78 2026-07-07), cargo 1.97.0 (pinned by `rust-toolchain.toml`); debug builds with `CARGO_PROFILE_DEV_DEBUG=0 CARGO_INCREMENTAL=0`, at most 2 jobs, to fit the disk and memory |
| Node / npm | 22.23.3 / 10.9.9 |
| Tauri | crate 2.12.1, CLI 2.12.1; Wasmtime 48.0.5 |
| Free disk | 7.0 GB at the start, 6.5 GB before the bundle (after removing old test programs and the built examples), 8.1 GB at the end (after removing `src-tauri/target/release`) |

## Checks

| Command | Result |
| --- | --- |
| `npm run check:full` | **All 10 steps passed in 173.6 s**: typecheck 4.7 s, lint 1.9 s, architecture 0.4 s, versions 0.0 s (0.3.0 in `Cargo.toml` and `package.json`, none in `tauri.conf.json`; MIT in LICENSE and 8 declarations), doc links 0.1 s (no broken links in 92 Markdown files), Vitest 19.0 s (37 files, 594 tests passed), cargo fmt 0.8 s, cargo clippy `--all-targets -D warnings` 14.9 s, cargo test `--no-fail-fast` 130.0 s, vite build 1.6 s |
| `cargo test --no-fail-fast` inside `check:full` | **1299 passed, 0 failed, 8 ignored, in 30 targets** (table below) |
| `cargo test --no-fail-fast`, a second time | **1299 passed, 0 failed, 8 ignored**, 90 s |
| `npx vitest run`, five more times after `check:full` | 5 × 594 passed (37 files), 13 to 23 s each |
| `npx vitest run`, twice more while the release build compiled | 2 × 594 passed, 17 and 21 s |
| `cargo test --test artifacts_security`, 5 times | 5 × 108 passed, 0 failed |
| `cargo test --test home_assistant`, 5 times | 5 × 6 passed, 0 failed (about 20 s each) |
| `cargo test --test artifacts_roundtrip`, 5 times | 5 × 1 passed |
| `cargo test --test release_acceptance`, 5 times | 5 × 1 passed (about 2.2 s each) |
| `npm run plugins:validate -- examples/plugins/{hello,counter,home-assistant}` | `example.hello 0.1.0`, `example.counter 0.2.0`, `example.home-assistant 0.1.0`: each valid, **on** |
| `sh scripts/build-example-plugins.sh --check` | **ok** for hello, counter, channels and home-assistant: the modules rebuilt from source with npm match `examples/plugins/SHA256SUMS`, 15 s. npm reached the registry. |
| `node scripts/check-architecture.mjs --report` and `npm run check:arch` | No violation. Large files: no file grew more than 10 % over its baseline; the largest growths are `plugins/external/network.rs` +4.2 % (862 lines), `plugins/external/manifest.rs` +2.9 % (961) and `state/feedback.rs` +2.9 % (639); `plugins/external/host.rs` shrank 21.1 % (933). Files over 400 lines without an allowlist entry are listed as "watch" only (largest: `devices/novation/session.rs`, 692 lines). |
| actionlint on `.github/workflows/*.yml` | **NOT VERIFIED**: actionlint is not installed on this machine, and installing tools was out of bounds for this package. The three workflows (`bundle.yml`, `ci.yml`, `desktop-e2e.yml`) parse as YAML (js-yaml). actionlint 1.7.12 last passed on `ci.yml` on 2026-10-05 ([macOS record](macos-2026-10-05.md#section-5-continuous-integration)). |
| Obsolete claims of plan section 10 | Gone. No match in README, `docs/user`, `docs/getting-started`, `docs/agent-control`, `docs/roadmap`, `docs/plugin-sdk`, `docs/architecture` or `src` for "no CI", "four tests", "expected to fail", "Agent Pack", "Marketplace · Preview", "same bytes run on Windows" or "has not been seen running" (only rule R17, "one Pack concept", and the first-run test that checks the Marketplace placeholder is gone). `macos.rs` and `windows.rs` headers say what was and was not run. `workspace.json` appears in user pages only to say where it is or what it holds, never to edit it. |
| `git log` since `b202509` | No doubled or interleaved commits; `dbc5730` (license check) and `versions:check` agree. |

Flaky tests: **none seen.** The frontend failure seen once on 2026-10-05 while three lanes compiled did not come back in 8 Vitest runs (1 in `check:full`, 5 alone, 2 during the release build); the `home_assistant` ordering race fixed in S6 did not come back in 7 runs (2 full suites, 5 alone).

### Rust tests per target

| Target | Passed | Ignored |
| --- | --- | --- |
| lib (unit tests) | 1000 | 6 |
| `action_library` | 11 | |
| `agent_bridge` | 3 | |
| `agent_control_flows` | 11 | |
| `agent_control_golden` | 2 | |
| `agents_contract` | 1 | |
| `app_info_contract` | 1 | |
| `artifacts_contract` | 2 | |
| `artifacts_roundtrip` | 1 | |
| `artifacts_security` | 108 | |
| `catalog_contract` | 3 | |
| `context_contract` | 1 | |
| `context_engine` | 16 | |
| `devices_contract` | 2 | |
| `feedback_editor` | 12 | |
| `home_assistant` | 6 | |
| `legacy_plaintext_secrets` | 14 | |
| `observables_contract` | 1 | |
| `plugin_install` | 10 | |
| `plugin_network` | 19 | |
| `plugin_sdk` | 14 | |
| `plugin_states` | 19 | |
| `plugins_contract` | 3 | |
| `release_acceptance` | 1 | |
| `secret_store` | 9 | 1 |
| `state_characterization` | 11 | 1 |
| `state_feedback` | 8 | |
| `state_hub` | 10 | |
| the binary, doc-tests | 0 | |
| **Total** | **1299** | **8** |

No test failed. The 8 ignored tests are ignored on purpose: four measurements (`the_save_on_context_activation_is_measured`, three in `devices::novation::measure_tests`), a benchmark (`p9_routing_cost_does_not_grow_with_the_workspace`), the golden regenerator (`regenerate_export_goldens`), the real Launchpad acceptance (`launchpad_acceptance`, needs a unit: (b)) and the real credential store (`the_real_credential_store_keeps_a_secret_out_of_every_plain_file`: on macOS it writes to the login keychain, so it is left to a person, [manual checks](../contributing/manual-checks-macos.md#4-secrets-in-the-keychain)).

## The macOS bundle

`CARGO_BUILD_JOBS=2 CARGO_PROFILE_DEV_DEBUG=0 npm run tauri build -- --bundles app,dmg`, release profile (LTO, one codegen unit, stripped).

**First attempt: the `.app` was built, the `.dmg` failed** after 618 s (release compile 8 min 04 s): `failed to bundle project: error running bundle_dmg.sh`. Tauri's `bundle_dmg.sh` runs an AppleScript that tells Finder to arrange the disk image's window, unless it is given `--skip-jenkins`. The image was attached hidden (`-nobrowse -noautoopen`) and detached again by the script; nothing stayed mounted. That the AppleScript step is the one that failed is inferred (Tauri prints no detail, and the half-made `rw.*.dmg` was left beside the `.app`), not seen. If so, the build **asked Finder to act on the owner's desktop**, which this validation was not meant to do; whether macOS showed an automation prompt for it is not known here.

**Second attempt: built.** When `CI` is set, Tauri passes `--skip-jenkins` to the script (the switch sits beside `TAURI_BUNDLER_DMG_IGNORE_CI` in the CLI), so no AppleScript runs: `CI=true CARGO_BUILD_JOBS=2 CARGO_PROFILE_DEV_DEBUG=0 npm run tauri build -- --bundles app,dmg`, 243 s (release compile 3 min 48 s). [Releasing](../contributing/releasing.md#4-the-bundles) now says to set it.

Inspected without opening the app (nothing was launched or installed):

| Check | Result |
| --- | --- |
| Sizes | `LaunchDeck.app` 16.56 MiB (the `launchdeck` binary 17,225,328 bytes, Mach-O arm64); `LaunchDeck_0.3.0_aarch64.dmg` 7,752,161 bytes (7.39 MiB), UDZO, SHA-256 `20328df8c8ede5b8bf218dcb54c6e1d41da7147e2cbe41815ddacd64226d008f` |
| `plutil -p Contents/Info.plist` | `CFBundleIdentifier` `dev.launchdeck.app`, `CFBundleShortVersionString` and `CFBundleVersion` `0.3.0`, `LSMinimumSystemVersion` `11.0`, `LSApplicationCategoryType` `public.app-category.productivity`, `CFBundleExecutable` `launchdeck`, `NSHighResolutionCapable` 1 |
| `codesign -dv --verbose` | `Identifier=dev.launchdeck.app`, `Signature=adhoc`, `flags=0x10002(adhoc,runtime)`, `TeamIdentifier=not set`, sealed resources (7 files). `codesign --verify --deep --strict`: valid on disk, satisfies its designated requirement |
| Notarization | Skipped by Tauri (no Apple credentials), as planned |
| `hdiutil verify` on the `.dmg` | checksum VALID |
| `Contents/Resources` | `examples/plugins/{hello,counter,home-assistant}/` each with `plugin.json` and its `.wasm`, and `icon.icns`. This settles the resource path left open by D2 (R-11). |
| Inside the `.dmg` (attached read-only and hidden at a scratch mount point, then detached) | `LaunchDeck.app` (byte-identical to the built one, signature valid), an `Applications` link and the volume icon |
| LICENSE | Not a file inside the `.app`. The `.dmg` carries `LICENSE` (MIT) as its software license agreement (`hdiutil imageinfo`: Software License Agreement true), so a person opening the `.dmg` is asked to agree to the MIT text first. |
| Tauri's warning on the identifier | Printed, as expected (R-18): `The bundle identifier "dev.launchdeck.app" set in tauri.conf.json identifier ends with .app. This is not recommended because it conflicts with the application bundle extension on macOS.` Kept, as the plan decides (changing it moves the data folder). |

Afterwards `src-tauri/target/release` (1.7 GB) was removed to give the disk back; the bundle was not kept, shared, installed or opened. A person who walks the manual checks builds it again from `900c08d` with the command above.

## Definition of Done (plan section 14)

"Run here" means run on this machine for this record or by a package of this milestone on this machine, as cited. Letters: (a) GitHub Actions does not start (account billing); (b) no Novation Launchpad attached; (c) no Home Assistant reachable; (d) no Linux desktop; (e) no Windows machine; (f) the manual checks on the Mac not done by a person, and no agent drives the app window.

| # | Item | Status |
| --- | --- | --- |
| 1 | Ownership defined | Run here: `check:arch` green |
| 2 | Codebase map | Run here: map checks in `check:arch`; review of rows done by the packages, not repeated here |
| 3 | Engineering rules | Run here: present, linked from AGENTS.md |
| 4 | Agent instructions | Run here: AGENTS.md 70 lines, 5,174 bytes; CLAUDE.md `@AGENTS.md`; 3 skills; `.agents/skills` links to `.claude/skills`. The skill listing inside Claude Code: needs the owner |
| 5 | Checks active | Run here: `npm run check` includes the architecture step; the hook's exit 2 on a violation is tested in `scripts/check-architecture.test.mjs` (in Vitest). CI step: NOT VERIFIED (a) |
| 6 | Justified refactors | Run here by A5, A6, A7; characterization tests green at the candidate |
| 7 | Tests | Run here: `check:full` green |
| 8 | Context Engine complete | Run here: `context_engine` 16, engine unit tests |
| 9 | Manual override, Resume Auto | Run here in tests; in the window NOT VERIFIED (f) |
| 10 | Edit suspension | Run here in tests; in the window NOT VERIFIED (f) |
| 11 | Honest platform support | Run here: docs match the records; observer evidence from `examples/foreground.rs` ([record](macos-2026-10-05.md#productization-macos-observer-evidence)). Windows NOT VERIFIED (a, e); X11 earlier milestone, not for 0.3.0 (d) |
| 12 | User-configurable | Run here: `context.test.tsx`; in the window NOT VERIFIED (f) |
| 13 | Surface artifact v1 | Run here: format tests, golden `.ldsurface` |
| 14 | Pack artifact v1 | Run here: golden `.ldpack`, `artifacts_contract` |
| 15 | Transactional import | Run here: apply tests |
| 16 | Sanitized export | Run here: export tests |
| 17 | Dependencies | Run here |
| 18 | Inputs | Run here |
| 19 | Provenance | Run here |
| 20 | Missing deps preserved and recover | Run here: `artifacts_roundtrip` and `release_acceptance`, 5 times each plus 2 full suites |
| 21 | Artifact security tests | Run here: `artifacts_security` 108 passed, 5 times plus 2 full suites |
| 22 | Plugin SDK usable | Run here: `plugins:validate` on the three examples; Install in the app NOT VERIFIED (f) |
| 23 | Permissions coherent | Run here: `plugin_network` 19 |
| 24 | Secrets secure | Run here: `secret_store`, `legacy_plaintext_secrets`; the real Keychain NOT VERIFIED (f) |
| 25 | Network works | Run here: `plugin_network`, `home_assistant` against the stand-in; a real Home Assistant NOT VERIFIED (c) |
| 26 | Reference plugins build and run | Run here: `--check` ok, plugin tests load them, `plugins:validate` |
| 27 | Developer docs sufficient | Run here: `docs:links`; review by R3 |
| 28 | First run | Run here: `firstRun.test.tsx`; in the window NOT VERIFIED (f) |
| 29 | Understandable device state | Run here: device tests with simulated units; in the window NOT VERIFIED (f); hardware NOT VERIFIED (b) |
| 30 | Useful starters | Run here: surfaces tests and fixture; in the window NOT VERIFIED (f) |
| 31 | No JSON for core flows | Run here: `release_acceptance` goes through the commands the interface calls; the user pages tell nobody to edit JSON; in the window NOT VERIFIED (f) |
| 32 | Understandable errors | Run here: CommandError and toast tests |
| 33 | Coherent settings | Run here: `settings.test.tsx`; in the window NOT VERIFIED (f) |
| 34 | CI green | NOT VERIFIED (a) |
| 35 | Fast quality command | Run here: the steps of `npm run check` took 41.8 s together inside `check:full` (clippy rebuilding the crate) |
| 36 | Linux bundle | NOT VERIFIED (a, d) |
| 37 | macOS build | Run here: `.app` and `.dmg` built and inspected (above); opened NOT VERIFIED (f) |
| 38 | Windows build | NOT VERIFIED (a, e) |
| 39 | Clean-workspace acceptance | Run here: `release_acceptance`; the walk-through in the window NOT VERIFIED (f) |
| 40 | Pack export/import acceptance | Run here: `artifacts_roundtrip`, `release_acceptance`; in the window NOT VERIFIED (f) |
| 41 | Agent Control tests pass | Run here: `agent_control_flows` 11, `agent_control_golden` 2, `agent_bridge` 3, `agents_contract` 1, lib |
| 42 | Action Library tests pass | Run here: `action_library` 11, `catalog_contract` 3 |
| 43 | Feedback tests pass | Run here: `state_feedback` 8, `feedback_editor` 12, `state_hub` 10, `state_characterization` 11 |
| 44 | Context Engine tests pass | Run here: `context_engine` 16, `context_contract` 1 |
| 45 | Plugin SDK tests pass | Run here: `plugin_sdk` 14, `plugin_states` 19, `plugins_contract` 3 |
| 46 | Network and Home Assistant stand-in tests pass | Run here: `plugin_network` 19, `home_assistant` 6, `secret_store` 9 |
| 47 | Docs match behaviour | Run here: `docs:links`, the grep above; review by R3 |
| 48 | Clean tree | Run here: `git status` clean after this record's commit |
| 49 | Release candidate identified | `900c08d` |
| 50 | Ready for an external alpha tester | **Not yet: needs the owner.** 1-49 are as marked and the MIT license is in place, but the human pass (f) has not been done or recorded |

## NOT VERIFIED, with reasons

What a person must still do on this Mac ([manual checks](../contributing/manual-checks-macos.md)), all (f):

* Open the `.app` (or the `.dmg`, which first shows the MIT license to agree to) and walk the acceptance scenario in the window.
* The app window, the tray, Settings and About, surface management and the damaged-workspace banner.
* Auto Context in the running app, including what LaunchDeck's own window reads as in a bundled app.
* The Keychain: storing, removing and migrating a secret, prompts after a rebuild, a locked keychain; the ignored real-store test.
* Keys pressed for you, with and without Accessibility permission.
* The Install button for the example plugins and Show plugins folder in the app (the files are in the bundle, above).
* Import and export through the file picker and the Downloads prompt (R-09, R-10).
* Quit from the tray and with Cmd+Q.

What cannot be done here:

* GitHub Actions: `ci.yml`, `bundle.yml`, `desktop-e2e.yml` have never run (a). With them: the Linux `.deb`/`.rpm` and the Windows NSIS installer.
* A Launchpad X or Launchpad Mini MK3: `scripts/launchpad-acceptance.sh` has never run on a unit (b).
* A real Home Assistant (c).
* A Linux desktop: the build for 0.3.0, the X11 observer and the desktop suite (d).
* Windows: never built (e).
* Gatekeeper on a downloaded copy: a build made on this Mac carries no quarantine flag, so the "Open Anyway" path of [install](../getting-started/install.md#opening-a-build-on-macos) cannot be seen here.
* macOS desktop end-to-end tests: there is no WebDriver for macOS's web view.
* actionlint for this release candidate (not installed; above).

## Open decisions for the owner

* **License**: MIT is decided (2026-10-05) and in place. Still open: a copyright holder or publisher name for the bundle (`tauri.conf.json` sets neither), and whether the `.dmg` should show the license as an agreement to accept (Tauri does so because `licenseFile` is set).
* **Legacy plaintext secrets** of schema 1-3 plugins: never handed to a module, and Migrate or Remove is offered (D4). Whether to refuse such entries outright in a later release is open.
* **Making CI run**: fix the account's billing or make the repository public; until then every CI line is NOT VERIFIED (a).
* **The bundle identifier** `dev.launchdeck.app` (Tauri warns): kept for 0.3.0; changing it moves the data folder.
* **The release itself**: tagging `v0.3.0`, pushing and publishing are the owner's steps ([releasing](../contributing/releasing.md#6-the-tag)); none was done here.
