# Verification records

What was run, where, and what was seen. A record is written while the work happens and is not edited afterwards, except to append; a later record says what changed. README "Verified where", the CHANGELOG and the user guides cite these records, and claim nothing a record does not hold.

| Record | Date | Machine | What it covers |
| --- | --- | --- | --- |
| [macos-2026-10-05.md](macos-2026-10-05.md) | 2026-10-05 | macOS 15.4.1, Apple M2, Rust 1.97.0 | The first build and test run on macOS (sections 1 to 3 of the [next-phase plan](../history/next-phase-2026-10.md)): setup, the first build, the failures found and their fixes, the counts after them. The CI workflow as written and the first run that did not start (account billing). Later appended during productization: the macOS foreground observer seen reporting bundle ids through `src-tauri/examples/foreground.rs` (not in the app), the cost of saving on a context-driven activation, and the app starting under `tauri dev` (the window seen by the owner, nothing checked). |
| [release-0.3.0.md](release-0.3.0.md) | 2026-10-06 | macOS 15.4.1, Apple M2, Rust 1.97.0 | The release candidate of 0.3.0, `900c08d`: `check:full` (1299 Rust tests passed, 0 failed, 8 ignored; 594 Vitest tests), the repeated runs of the artifact security, round-trip, acceptance and Home Assistant suites, the example plugins rebuilt and validated, the macOS `.app` and `.dmg` built and inspected (not opened), the Definition of Done and the full NOT VERIFIED list. |

## Provenance

`macos-2026-10-05.md` was `VERIFICATION.md` at the root of the repository until commit `4ba5305` (2026-10-06) moved it here unedited. Its links were rewritten for the move; its text was not. Paths it names as they were then (`NEXT_PHASE.md`, `docs/context-engine.md`, `docs/devices/…`) now live in [docs/history/](../history/README.md) and the trees by reader.

## Still to be recorded

Each of these is NOT VERIFIED until a record here says otherwise:

* The [manual checks](../contributing/manual-checks-macos.md) on the Mac: the bundle opened, the app window, tray, Auto Context in the app, Keychain, keys pressed for you (reason (f)). The release record of 0.3.0 lists them as not done.
* A physical Launchpad X or Launchpad Mini MK3: the report of `scripts/launchpad-acceptance.sh` ([hardware testing](../contributing/hardware-testing.md)) (reason (b)).
* A real Home Assistant (reason (c)).
* The CI workflows on GitHub Actions ([CI](../contributing/ci.md)) (reason (a)), and with them Linux (d) and Windows (e) for 0.3.0.

The reason letters are those of the README: (a) GitHub Actions does not start, account billing; (b) no Novation Launchpad attached; (c) no Home Assistant reachable; (d) no Linux desktop; (e) no Windows machine; (f) the manual checks on the Mac not done by a person.
