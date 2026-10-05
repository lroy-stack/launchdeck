# Productization plan: LaunchDeck 0.3.0, first external alpha

This is the controlling document of the productization milestone (branch `productization`). It settles the decisions; implementers do not re-decide them. If the code proves a decision wrong, do the closest correct thing and report it as a deviation. Session rules (cargo wrapper, commits, honesty) are in the session's `SESSION_RULES.md` and apply to every package.

Status words used everywhere: **run here** (done on this Mac in this milestone), **NOT VERIFIED** (with the reason), **written, not run**.

Reasons that recur, referred to by letter:

* (a) GitHub Actions does not start: account billing.
* (b) No Launchpad X attached.
* (c) No Home Assistant reachable.
* (d) No Linux/X11 desktop here; `tools/desktop-e2e` is Linux only.
* (e) No Windows machine.
* (f) The macOS manual checks of NEXT_PHASE.md section 4 have not been done by a person.

## 0. Rule zero and the work packages at a glance

**Rule zero: no new product subsystem during productization.** If a missing subsystem is discovered, it is documented as post-alpha (section 4) unless it blocks the alpha acceptance scenario. Portable Surface/Pack artifacts are the one new subsystem this milestone asks for; everything else is consolidation, hardening, UX and documentation. This plan is the single architectural arbiter: implementers do not re-decide what it settles.

Owner decisions taken on 2026-10-05: the license is **MIT**, everywhere (R-03, in R4); legacy plaintext `config[].secret` values of schema 1-3 manifests are **never used again and migrated only on the person's explicit choice** (R-04, new package D4). After D4, LaunchDeck never hands a plaintext secret stored in `plugins.json` to a plugin, whatever the manifest version: credential store or unavailable.

| WP | Title | Depends on | Owner | Files / domains | Risk | Acceptance |
| --- | --- | --- | --- | --- | --- | --- |
| A1 | Architecture docs and agent entrypoints | — | impl agent | docs/architecture.md -> docs/architecture/overview.md (git mv); docs/architecture/CODEB… | low | maps and rules exist, AGENTS.md ≤ 120 lines, CLAUDE.md imports it, 3 skills |
| A2 | Quality commands, architecture check, hook and toolchain pin | A1 | impl agent | scripts/check-architecture.mjs; scripts/architecture/rust.mjs; scripts/architecture/web… | low | `npm run check` green; `check:arch` fails on a planted violation; hook exits 2 on one |
| A3 | Rust boundary fixes: audit trail, identifier rule, crate::platform | A2 | impl agent | src-tauri/src/agents/audit.rs -> src-tauri/src/audit.rs; src-tauri/src/lib.rs (mod line… | medium | no forbidden import left for audit/identifiers/platform; full tests green |
| A4 | Frontend structure and boundaries | A2 | impl agent | src/** (moves and import updates); src/lib/schema.ts -> src/lib/schema/*.ts; src/lib/fe… | medium | no editor↔feedback cycle; ESLint boundary rule; vitest 309+ green |
| A5 | Decompose surfaces/mod.rs | A3 | impl agent + adversarial review | src-tauri/src/surfaces/** (mod.rs, palette.rs, build.rs, navigation.rs, development.rs,… | medium | pure move; default-workspace fixture byte-identical; surfaces/persistence/golden tests green |
| B0 | Documentation truth pass (macOS facts from VERIFICATION.md) | A2 | impl agent | README.md; docs/context-engine.md; docs/security/secrets.md; docs/product.md; docs/plug… | low | no 'never built / not compiled' claim for macOS left; every claim traceable to VERIFICATION.md |
| A6 | Split lib.rs into commands/ and tray.rs | A3 | impl agent | src-tauri/src/lib.rs; src-tauri/src/commands/{mod,workspace,context,actions,feedback,de… | medium | pure move; same 45 command names and signatures; full tests green |
| R1 | CI workflows within the minutes budget | A2 | impl agent | .github/workflows/ci.yml; .github/workflows/bundle.yml (new); .github/workflows/desktop… | low | workflows pass actionlint; macOS jobs off the per-push path; NOT VERIFIED until Actions runs |
| A7 | Split core/runtime.rs into core/runtime/ | A3 | impl agent + adversarial review | src-tauri/src/core/runtime.rs -> src-tauri/src/core/runtime/{mod,startup,navigation,con… | high | pure move; public paths unchanged; all 715+ Rust tests green |
| B1 | Context Engine hardening and the portable resolver | A5, A7 | impl agent | src-tauri/src/context/observer/macos.rs and windows.rs (header comments only); src-taur… | medium | portable resolver tested; ContextHint no longer emitted; observer evidence recorded or NOT VERIFIED |
| D1 | Plugin host hardening and plugins:validate | A4, A6 | impl agent | src-tauri/src/plugins/external/{host.rs, legacy_secrets.rs (new), mod.rs, discovery.rs,… | medium | typed plugin errors; `plugins:validate` uses the real validator; reserved id prefix |
| D4 | Legacy plaintext secrets: never used, explicit migration | D1 | impl agent + adversarial review | plugins/external/{host,legacy_secrets}.rs, commands/plugins.rs, src/features/plugins, docs/security | high | a schema 1-3 secret from plugins.json never reaches a module; plaintext untouched until Migrate (write, read back, then remove) or Remove; no insecure option |
| D2 | Install the bundled example plugins from the Plugins screen | D1, D4 | impl agent + adversarial review | src-tauri/tauri.conf.json (bundle.resources); src-tauri/src/plugins/external/install.rs… | medium | Hello, Counter, Home Assistant installable from the Plugins screen; tests through the install API |
| D3 | Reproducible example plugins, frozen SDK 0.1 fixtures, author docs | D1 | impl agent | scripts/build-example-plugins.sh; examples/plugins/{hello,counter,channels,home-assista… | medium | example plugins rebuild to SHA256SUMS or NOT VERIFIED; author flow documented |
| C1 | Artifact format core: limits, JSON pre-pass, digest, canonical writer, v1 types | A6 | impl agent | src-tauri/Cargo.toml; src-tauri/Cargo.lock; src-tauri/src/lib.rs (pub mod artifacts); s… | medium | v1 types, limits, JSON pre-pass, digest and canonical writer with tests |
| C2 | Artifact export | C1 | impl agent | src-tauri/src/artifacts/{env.rs, export.rs, review.rs, mod.rs}; src-tauri/src/plugins/s… | medium | export deterministic (same input, same bytes); no secret, grant or runtime state in output |
| C3 | Artifact inspect and the hostile fixture suite | C2 | impl agent + adversarial review | src-tauri/src/artifacts/{inspect.rs, mod.rs}; src-tauri/tests/artifacts_security.rs (ne… | high | every hostile fixture rejected, workspace unchanged; suite green 5 times |
| C4 | Provenance, the commit primitive and the ImportPlan | C3, B1, A7 | impl agent + adversarial review | src-tauri/src/core/model.rs (ImportOrigin, Surface.imported_from); src-tauri/src/core/r… | high | ImportPlan pure; commit primitive saves then swaps; provenance round-trips |
| C5 | Apply, artifact service, commands, contract fixtures and round trip | C4 | impl agent + adversarial review | src-tauri/src/artifacts/{apply.rs, service.rs, mod.rs}; src-tauri/src/commands/{artifac… | high | apply transactional incl. save failure; commands + contract fixtures; export→import round trip |
| C6 | Import and export UI | C5, A4 | impl agent | src/lib/schema/{artifacts.ts (new), index.ts, workspace.ts}; src/lib/ipc.ts; src/lib/pr… | medium | import preview renders the Rust ImportPlan only; export/import flows tested in vitest |
| C7 | Artifact documentation | C5 | impl agent | docs/artifacts/{README.md, FORMAT.md, security.md} (new); docs/user/sharing.md (new); d… | low | docs/artifacts/FORMAT.md matches the code; links check green |
| E1 | Error UX: CommandError, structured issues, no raw text | A4, A6, A7 | impl agent + adversarial review | src-tauri/src/commands/** (CommandError, CmdResult, per-command detail); src-tauri/src/… | high | CommandError {message, detail}; no raw-text sinks left in the UI (grep list empty) |
| E2 | Device status a person can understand | A4, A7 | impl agent | src-tauri/src/devices/{mod.rs, manager.rs, launchpad_x.rs}; src-tauri/src/core/runtime/… | medium | device status in sentences; no MIDI words in normal UX; tests |
| E3 | Surface management, onboarding flag and workspace load notice | A7 | impl agent + adversarial review | src-tauri/src/core/runtime/{editing.rs, mod.rs, startup.rs, tests.rs}; src-tauri/src/pe… | medium | rename/delete surfaces and pages; load-failure notice; onboarding flag command |
| E4 | First run and starter content | E2, E3, A5 | impl agent | src/features/onboarding/{FirstRun.tsx, firstRun.test.tsx} (new); src/app/{App.tsx, Side… | medium | short first run; starter choice through the normal activation path; no surprising auto-config |
| E5 | Settings, About and a single app version source | E3, D2 | impl agent | src/features/settings/** (SettingsView.tsx and section components, settings.test.tsx); … | medium | sectioned Settings; About with versions from one source |
| E6 | Empty states and readable activity | A4 | impl agent | src/features/surfaces/{SurfaceView.tsx, empty.test.tsx (new)}; src/features/editor/Assi… | low | no blank or technical empty state; activity shows names, not ids |
| R2 | Release acceptance automation | C5, D2 | impl agent | src-tauri/tests/release_acceptance.rs (new); src-tauri/tests/support/acceptance_scenari… | medium | `tests/release_acceptance.rs` drives the acceptance scenario headless, green |
| R3 | Documentation restructure and user docs | A2 | impl agent | docs/** (moves and new pages); *_PLAN.md and IMPLEMENTATION_PLAN.md (git mv to docs/his… | low | docs tree (getting-started, user, plugin-sdk, artifacts, architecture); links green |
| R4 | README, CHANGELOG, versions and the 0.3.0 bump | A2 | impl agent | README.md; CHANGELOG.md (new); docs/architecture/versions.md; src-tauri/Cargo.toml; src… | low | README product-first, CHANGELOG, 0.3.0 versions consistent (check) |
| R5 | Release validation and the release record | A2 | impl agent | src-tauri/tauri.conf.json (bundle metadata); docs/verification/release-0.3.0.md (new); … | medium | `check:full` green; macOS bundle if disk allows; release record with NOT VERIFIED list; clean tree |

## 1. Verified current product state

Checked against the code at `95cab94` (five audits, read-only), not against roadmap notes.

| Area | Status | Evidence |
| --- | --- | --- |
| Runtime, surfaces, pages, controls, Live/Edit, inspector | implemented | `core/runtime.rs` (2741 lines, 627 of them tests), `core/model.rs`, `core/validate.rs` |
| Virtual Launchpad X (simulator) | implemented | `devices/virtual_device.rs`; always listed |
| Launchpad X over USB | implemented, byte-tested only | `devices/launchpad_x.rs`; NOT VERIFIED on a unit (b) |
| Action Library, parameters, availability | implemented | `actions/*`, `tests/action_library.rs`, `catalog_contract.rs` |
| State Hub, feedback rules, feedback editor | implemented | `state/*`, `tests/state_*.rs`, `feedback_editor.rs`, `src/features/feedback` |
| Agent Control (demo, Agent Bridge, hooks) | implemented | `agents/*`; Claude Code hooks verified live earlier; Codex written, not run |
| Plugin platform: sandbox, manifest 1-4, ABI 1, SDK 0.2.1, states, network broker, grants, credential-store secrets, lifecycle, logs | implemented | `plugins/external/*`, `tests/plugin_*.rs`, `home_assistant.rs`, `secret_store.rs` |
| Context Engine v1 (identity, ordered first-match rules, fallback, Auto, override, Resume Auto, Edit suspension, 150 ms settle, no history) | implemented | `context/*`, `tests/context_engine.rs` (15), `ContextPanel.tsx`, `context.test.tsx` |
| Persistence and migrations (workspace schema 4) | implemented | `persistence/{mod,legacy,bindings}.rs`; atomic `write_atomic` |
| Portable Surface/Pack artifacts | **missing** | no code, command or UI. `agents/pack.rs` is a dead "Agent Pack" manifest (nothing loads it) that collides with the word Pack |
| First run | partial | only `Preferences.onboardingDone` and a 3-step strip on Home; flag set only from Home |
| Device state UI | partial | `DeviceInfo` has only `connected`; unplugged units vanish; non-X Launchpads invisible; connect failures only logged |
| Error UX | risky | `Result<T, String>` everywhere, raw Rust/WASM/Zod text toasted verbatim |
| Settings | partial | devices, context, a raw WebSocket tester, static security text and reset mixed in `features/devices/SettingsView.tsx` |
| Surface/page rename and delete | missing | no command |
| Workspace load failure | risky | silently falls back to the starters; only a log line names the backup |
| Starter content | partial | 5 starters; "Home" exists only as a webhook template posting to `homeassistant.local`; Development still writes the dead `ContextHint` |
| Plugin install for a non-developer | missing | only `LAUNCHDECK_PLUGINS` or a hand copy into a hidden folder |
| Plugin error UX | partial | typed `ManifestError` flattened to strings; absolute paths shown |
| Example plugin reproducibility | risky | committed `.wasm` are SDK 0.1.x builds that HEAD cannot rebuild; build script rewrites lockfiles; C Ping needs a wasm-capable clang |
| Version handling | duplicated | two hand-rolled parsers (`manifest.rs`, `agents/pack.rs`, which accepts `<>=1.2.3`); plugin versions never compared |
| Agent governance (AGENTS.md, CLAUDE.md, skills, hooks, architecture check) | missing | none exist; 1131 `pub` items vs 23 `pub(crate)` |
| Quality command, toolchain pin | missing | no `npm run check`, no `rust:clippy`, no `rust-toolchain.toml`; this Mac's default rustc is 1.83 (< rust-version 1.95) |
| CI | written, not run | `.github/workflows/ci.yml` (3 systems). Run `37301909131` did not start (a) |
| Bundles | never built | no `tauri build` anywhere; bundle metadata minimal |
| Versioning, CHANGELOG, license | risky | 0.2.0 kept by hand in 3 files, no tags, no CHANGELOG; Cargo.toml and SDK say MIT, README says no license chosen, no LICENSE file |
| Documentation | partial, partly false | README and 4 docs still say macOS "never built / not compiled"; organised by subsystem, 8 historical plans at the root; 0 broken relative links today |

Verified where, today (from VERIFICATION.md and README; nothing new was run by the audits):

| | macOS 15.4.1, Apple M2 | Linux | Windows |
| --- | --- | --- | --- |
| Rust build, fmt, clippy `-D warnings` | run here, clean (Rust 1.97.0) | earlier milestone | NOT VERIFIED (e), never built |
| Rust tests | run here: 715 passed, 0 failed, 2 ignored | earlier milestone | NOT VERIFIED (e) |
| TypeScript typecheck, lint, 309 tests, build | run here | earlier milestone | — |
| Real window, tray, Auto Context running, Keychain, Accessibility (enigo) | NOT VERIFIED (f) | desktop suite, 218 checks, earlier milestone | NOT VERIFIED (e) |
| Launchpad X hardware | NOT VERIFIED (b) | NOT VERIFIED (b) | NOT VERIFIED (b) |
| Home Assistant | mock only; real NOT VERIFIED (c) | mock only | — |
| CI | NOT VERIFIED (a) | | |

## 2. Existing architecture that will remain

Kept as is (extend, never replace):

* The runtime model: Workspace → Surface → Page → Control, edited only through `Runtime` methods and validated by `core::validate`.
* One activation path: `Runtime::activate(.., By)`; the Context Engine only proposes a target.
* State Hub: providers own values, nothing is persisted, Rust judges rules (`review_feedback`, `explain_control`).
* `actions::{params, definition}` as the shared provider-contract kernel; `core::model` as the persisted aggregate.
* Plugin contract: ABI 1, manifest schemas 1-4, SDK 0.2.1, the network broker with exact per-origin grants, secrets only in the credential store.
* External crates confined to one adapter file each (midir, wasmtime, keyring, enigo, objc2/windows-sys/x11rb, tokio-tungstenite, ureq, tauri).
* Persistence: one file per schema step, `write_atomic`, workspace schema 4 (no bump in this milestone).
* The `examples/*.json` contract fixtures written by `dump_*` examples, guarded by Rust tests and parsed by Zod in Vitest.
* Missing-provider behaviour: an assignment or rule whose provider is gone is kept verbatim, shown as missing, and recovers on install.
* Context identity model: bundle id / executable name / WM_CLASS in a namespace (`macos`, `windows`, `x11`), never the display name.
* The single dark theme, the Zod validation of every IPC reply, `preview.ts` as a browser-only stand-in.
* Large cohesive files stay whole: `plugins/external/{manifest,network,sandbox}.rs`, `state/{feedback,hub}.rs`, `context/engine.rs`, `agents/{bridge,model,demo,service}.rs`, `src/lib/feedbackEdit.ts`, and every test-only file.

## 3. Technical debt fixed now

| Debt | Fix | Package |
| --- | --- | --- |
| No governance, no ownership map, no enforced boundaries | AGENTS.md, CLAUDE.md, ENGINEERING_RULES, CODEBASE_MAP, FRONTEND_MAP, 3 skills, architecture check, hook | A1, A2 |
| No quality command, no toolchain pin, no Node engines | `npm run check`, `check:web`, `check:full`, `rust-toolchain.toml`, `engines` | A2 |
| `plugins` imports `agents::audit` | move to `crate::audit` | A3 |
| Identifier rule owned by the WebSocket adapter, duplicated 3 times | `events::valid_ident` single owner; `valid_param` reuses `actions::params` | A3 |
| Platform detection in 4 places | `crate::platform` | A3 |
| editor ↔ feedback import cycle; shared widgets inside features; `schema.ts` with 9 domains; dead `src/lib/feedback.ts` | move widgets to `src/components`, split `src/lib/schema/`, delete dead file, ESLint boundary rule | A4 |
| `surfaces/mod.rs`, 9 responsibilities | one file per responsibility | A5 |
| `lib.rs`, 45 commands plus tray | `commands/`, `tray.rs` | A6 |
| `core/runtime.rs`, ~10 responsibilities | `core/runtime/` by seam | A7 |
| False "not compiled" claims | truth pass from VERIFICATION.md | B0 |
| Foreground example cannot show changes on macOS; dead `ContextHint` emitted | run loop on macOS; stop emitting | B1 |
| Plugin errors as raw strings; legacy secrets code inside `host.rs`; SDK version copied without a test; `launchdeck.` id prefix open | error kinds, `legacy_secrets.rs`, contract test, reserved prefix | D1 |
| Schema 1-3 `config[].secret` handed to the module from plain text | never passed; "Legacy insecure secret" card with Migrate (store, read back, then remove) and Remove; no insecure option | D4 |
| No in-app plugin install | bundled Hello, Counter, Home Assistant with an Install button | D2 |
| Example plugins not reproducible | frozen SDK 0.1 fixtures, rebuilt examples, `SHA256SUMS`, `--check` | D3 |
| Two version parsers | `semver` crate (already locked) for requirements; dead `agents/pack.rs` removed | C1 |
| Save failure keeps the in-memory change | `Runtime::commit_workspace` (save, then swap) | C4 |
| Raw error text in the UI; "is required" text matching | `CommandError {message, detail}`; `Issue.kind` | E1 |
| Thin device state, MIDI words in UX | `DeviceInfo.status` + sentences | E2 |
| No rename/delete of surfaces/pages; silent workspace fallback; read-modify-write of preferences | commands, load notice, `set_onboarding_done` | E3 |
| No first run; starter blurbs duplicated in React; "Home" ambiguous; Marketplace placeholder | first run, Rust as single author, Home starter, Overview view | E4 |
| Settings mixture; no About; app version in 3 files, first-party providers with random versions | sectioned Settings, `app_info`, Cargo.toml as single source | E5 |
| Blank/technical empty states; repository `docs/` paths in the UI; activity feed with ids | guidance + one next step; names | E6 |
| CI never run, no bundle or desktop jobs, macOS minutes on every push | restructured workflows | R1 |
| Docs organised by subsystem, plans at the root, no CHANGELOG | docs tree, CHANGELOG, versions table | R3, R4 |

## 4. Technical debt explicitly deferred

| Item | Why deferred | Revisit |
| --- | --- | --- |
| Split `agents/service.rs` (1970 lines), `plugins/external/capabilities.rs` | one concept each, not on the alpha path | when next edited |
| Mass narrowing of `pub` items | integration tests legitimately use ~35 paths | splits and new code only |
| Move `SLOT_COUNT`/`SOURCE` to `agents::model` | only when `service.rs` is touched | next agents work |
| Third-party plugin install from a file or folder, uninstall, plugin package format, signing, grants bound to a module hash | new subsystem (rule zero) | post-alpha |
| Save dialog for export, file associations, drag-and-drop import | export writes to Downloads instead (no new dependency) | post-alpha |
| Artifact assets (PNG), signatures, starter surfaces as artifacts, bundled sample Packs | not needed for v1; layouts are reserved | format version 2 |
| Appearance / light theme | one theme exists | when a second theme exists |
| Auto-update, MSI, AppImage, x86_64/universal macOS, notarization, Developer ID signing | no certificates; AppImage downloads linuxdeploy at build time | post-alpha |
| Wayland foreground observer | no standard API | not planned |
| macOS observer `removeObserver` on drop | observer lives for the process | if it ever stops |
| Persisted plugin logs | logs would need secret scrubbing guarantees | post-alpha |
| Flag `system.launchApplication` with arguments as privileged in the Action Library | import review already covers it | post-alpha |
| Bundle identifier ending in `.app` | changing it moves every user's data | record the CLI warning only |
| Temp-file cleanup and directory fsync in `write_atomic` | correctness does not depend on it | separate fix |
| Desktop E2E on macOS | `tauri-driver` has no macOS support | when it exists |

## 5. Architecture and modularity plan

### 5.1 Rings and dependency direction

Dependencies point inward: domain ← services ← adapters ← UI.

| Ring | Modules |
| --- | --- |
| Domain | `core::{model, validate}`, `state::{value, address, definition, feedback, eval, lint, index, catalog types}`, `actions::{params, definition}`, `context::{identity, rules, engine, status, portable}`, `agents::{model, visual, observables}`, `surfaces` content, `platform`, `artifacts` (except `service`) |
| Services | `core::runtime` (composition root), `core::{review, explain, observables}`, `state::hub`, `actions::{registry, engine}`, `plugins::{registry, system, demo}`, `plugins::external::{host, manifest, capabilities, discovery, wasm_plugin, install, legacy_secrets}`, `agents::{service, settings, plugin, demo, notify, view}`, `audit`, `persistence`, `artifacts::service` |
| Adapters | `devices::*`, `renderer`, `context::observer::*`, `events::{mod, external}`, `plugins::external::{sandbox, network, secrets}`, `plugins::keyboard`, `actions::builtin`, `agents::{adapter, adapters::*, bridge}` |
| UI boundary | `lib.rs`, `commands/`, `tray.rs`, then React through `src/lib/ipc.ts` |

Forbidden imports (non-test Rust code; enforced by `scripts/check-architecture.mjs`):

* `context/**` imports only `crate::{context, core::model}`.
* `devices/**` only `crate::{devices, core::model}`; `renderer/**` only `crate::{renderer, devices, core::model}`; `events/**` no other crate module.
* `state/**` must not import `devices, renderer, core::runtime, plugins, agents, context, surfaces, persistence`.
* `core/model.rs`, `core/validate.rs` must not import `core::runtime, devices, renderer, plugins, agents, persistence, events`.
* `actions/**` must not import `devices, renderer, core::runtime, agents, context, surfaces, persistence`.
* `plugins/**` must not import `devices, renderer, core::runtime, context, surfaces, agents` (the `agents::audit` exception exists only until A3).
* `agents/**` must not import `devices, renderer, core::runtime, context, surfaces`; `agents/adapters/**` and `agents/bridge.rs` also not `core::model, agents::visual, state::feedback`.
* `surfaces/**` must not import `devices, renderer, core::runtime, plugins::external, persistence, context::engine`.
* `persistence/**` must not import `core::runtime, devices, renderer, context, plugins::external`; feature constants are allowed only in migration step files.
* `artifacts/**` (except `service.rs`) may import only `crate::{artifacts, core::{model, validate}, state::{feedback, address, value}, actions::{definition, params}, context::{identity, rules, portable}, devices` (capability types), `platform, plugins::external::manifest, plugins::system` (action-id constants)`}`. `artifacts/service.rs` may also import `core::runtime` and `persistence`. Only `lib.rs` and `commands/**` import `artifacts`.
* `core::runtime`, `lib.rs`, `commands/**` may import anything.

Crate confinement: midir → `devices/launchpad_x.rs`; wasmtime → `plugins/external/sandbox.rs`; keyring → `plugins/external/secrets.rs`; enigo → `plugins/keyboard.rs`; objc2, block2, objc2_*, windows_sys, x11rb → `context/observer/**`; tokio_tungstenite → `events/external.rs` (tests anywhere); ureq → `actions/builtin.rs`, `plugins/external/network.rs`; tauri → `lib.rs`, `commands/**`, `tray.rs`; semver, sha2 → `artifacts/**` and `plugins/external/manifest.rs`. A new outside system gets exactly one adapter file and a row in this list.

Platform isolation: `cfg!(target_os…)`, `#[cfg(target_os…)]`, `cfg(windows)` appear only in `platform.rs`, `context/observer/**`, `plugins/external/secrets.rs`, `plugins/keyboard.rs`, `actions/builtin.rs`, and file-permission code under `cfg(unix)`. Logic takes `Platform` as a parameter.

### 5.2 Structural refactors (behaviour preserved, characterization first, structural commits separate)

**`surfaces/mod.rs` decomposition (A5).** Public API unchanged: `Platform` (re-exported from `crate::platform`), `starter_surfaces`, `default_workspace`, `default_workspace_for`, `add_agent_control`, `blank_surface`, and `pub mod agent_control`. New private modules:

| File | Holds |
| --- | --- |
| `mod.rs` | module doc, `mod` lines, `pub use` |
| `palette.rs` | the 10 `Rgb` constants |
| `build.rs` | `pub(super)` builders: `g, ctl, plugin, url, shortcut, launch, shell, http, open_surface, open_page, demo_cmd, with_state_visuals, tool_pad` |
| `navigation.rs` | `surface_switchers`, `back_control` |
| `development.rs` | `dev_pad`, `development(platform)` |
| `productivity.rs`, `smart_home.rs`, `accessibility.rs` | one starter each (`big` goes with accessibility) |
| `workspace.rs` | `starter_surfaces`, `default_workspace`, `default_workspace_for`, `blank_surface` |
| `upgrade.rs` | `add_agent_control` (the v1 → v2 content step called by `persistence::migrate`) |
| `tests.rs` | the existing tests, moved verbatim |

`agent_control.rs` replaces `use super::*` with explicit imports. The characterization is the whole-workspace JSON fixture test (`contract_fixture_matches_the_default_workspace`) plus the per-platform tests, the persistence migration tests and `tests/agent_control_golden.rs`. `examples/default-workspace.json` must not change in A5.

**`lib.rs` split (A6).** `commands/{mod, workspace, context, actions, feedback, devices, agents, plugins}.rs` and `tray.rs`; `lib.rs` keeps `run()`, `AppState`, the module list and `generate_handler!`. Command names and signatures unchanged. Later packages add `commands/artifacts.rs` and `commands/app.rs`.

**`core/runtime.rs` split (A7).** `core/runtime/{mod (Runtime, Inner, public types, accessors), startup, navigation, context, input, actions, feedback, observables, devices, editing, tests}.rs`; each child holds `impl Runtime` blocks; public path `crate::core::runtime::*` unchanged.

**Smaller moves.** `agents/audit.rs` → `crate::audit` with `pub use` in `agents`; `valid_ident` → `events`; `crate::platform` (A3). Frontend widgets and schema split (A4). `plugins/external/legacy_secrets.rs` (D1).

### 5.3 Rules (content of `docs/architecture/ENGINEERING_RULES.md`)

R1 Ownership: every concept has one owner listed in the maps; a new top-level module or `src/features/*` directory needs a map row in the same commit (checked). R2 Dependency direction per 5.1 (checked). R3 Platform isolation (checked). R4 One external system, one adapter file (checked). R5 No MIDI, notes or CCs outside `devices`; `ControlPos` is the only addressing. R6 Plugins act only through the Plugin trait, host functions, hub writes by the host and the network broker; never devices, renderer or runtime internals; secrets never reach a plugin. R7 The Context Engine imports only `core::model`, outputs only a target for `Runtime::activate`, persists rules and settings only. R8 Only providers write hub values; values never persisted; React never evaluates rules. R9 No duplicated domain evaluation in React; an allowed mirror (`normalizeArgs`, `operatorsFor`, `problemsOf`) names its Rust counterpart and has a Rust-generated fixture with accepted and rejected cases. R10 IPC: Zod schema for every payload; only `src/lib/ipc.ts` imports `@tauri-apps`; every Rust-emitted shape has an `examples/*.json` fixture from a `dump_*` example guarded by a Rust test; fixtures are regenerated only for an intended change. R11 Persistence owns the schema version and `migrate()`; one file per schema step; features never call migrations; every file write goes through `write_atomic`/`write_private` (the append-only audit is the exception). R12 Surfaces: model in `core::model`, starters in `surfaces/<starter>.rs`, edits through `Runtime` methods; artifacts never serialize `Workspace` types directly. R13 `mod.rs` composes: declarations, re-exports, docs, small glue. R14 Large modules: 400 production lines is a signal, 700 needs a cohesion justification in the allowlist or a split plan; test lines do not count; split right before the workstream that edits the file. R15 Minimal public API: new submodules private with `pub use`; `pub` only when `lib.rs`/`commands`, `tests/`, `examples/` or a documented SDK needs it; a new `pub` item in an existing module is justified in the commit body. R16 No dumping grounds (`utils`, `helpers`, `common`, `misc`) (checked). R17 No parallel systems: search before adding; one Pack concept, one version-requirement implementation (`semver`), one identifier rule per vocabulary. R18 Structural and product changes in separate commits; characterization green before and after; a refactor never edits fixtures. R19 Tests: inline `#[cfg(test)] mod tests`, over ~300 lines a sibling `tests.rs`, cross-module scenarios in `src-tauri/tests` with `tests/support/*_scenario.rs`, TypeScript tests next to the code. R20 Frontend: `shared (components, lib, stores) → features → app`; features import other features only at listed composition points; `src/lib` is pure TypeScript plus IPC, no React; stores hold UI state only. R21 Honesty: anything not run is NOT VERIFIED with a reason; docs and maps change in the same commit as an ownership change.

Agent workflow (in AGENTS.md and the skills): before: owning module from the map, read nearby code, search for existing abstractions, no parallel concept, plan IPC schema and fixture, tests first. After: relevant tests, `npm run check` (with the cargo wrapper), re-read modified files for creep, update the maps if ownership changed.

### 5.4 Governance file layout

| Path | Content |
| --- | --- |
| `AGENTS.md` | ≤ 120 lines and 8 KiB: product one-liner, commands (`npm run check`, `check:web`, `check:full`, `check:arch`, cargo note, fixture regeneration), the hard invariants one line each, the before/after workflow, links. Self-contained (Codex does not expand imports) |
| `CLAUDE.md` | `@AGENTS.md` plus 3-5 Claude-only lines (skills, hook, "read ENGINEERING_RULES before structural changes") |
| `docs/architecture/overview.md` | `docs/architecture.md` moved, unchanged apart from links |
| `docs/architecture/CODEBASE_MAP.md` | "where does it belong" table; ring diagram; one row per Rust module (owns, may depend on, must not depend on, platform code, tests, public consumers); owners of cross-cutting concepts; the import matrix and allowlists ("enforced by the check; change both in one commit"); contract fixtures table; known exceptions with planned fixes. Links to FRONTEND_MAP |
| `docs/architecture/FRONTEND_MAP.md` | one row per `src/` directory and feature, allowed feature-to-feature edges. Separate file so a Rust package and a TypeScript package running at the same time never edit the same map |
| `docs/architecture/ENGINEERING_RULES.md` | R1-R21 and the agent workflow; the single canonical rules document |
| `.claude/skills/{architecture-review, new-feature, refactor-large-module}/SKILL.md` | frontmatter `name` and `description`, < 100 lines, procedural, point to the rules and maps |
| `.agents/skills` | symlink to `../.claude/skills` (Codex) |
| `.claude/settings.json` | one PostToolUse hook, matcher `Edit\|Write\|MultiEdit`: `node "$CLAUDE_PROJECT_DIR"/scripts/check-architecture.mjs --hook`, timeout 10; exit 2 with one paragraph on a violation. No cargo, npm test or tsc in hooks |
| `scripts/check-architecture.mjs` | engine, Node built-ins only; modes: default, `--staged`, `--hook`, `--report`, `--only=rust\|web` |
| `scripts/architecture/rust.mjs`, `web.mjs` | the rules as data, split so that concurrent Rust and web packages edit different files |
| `scripts/check.mjs` | runner for `check`, `check:web`, `check:full`; cargo binary from `LAUNCHDECK_CARGO` (default `cargo`) |
| `scripts/check-versions.mjs`, `scripts/check-docs-links.mjs` | version equality; relative links and anchors in tracked `.md` |
| `.githooks/pre-commit` | opt-in (`npm run hooks:install`); `check:arch --staged`, fmt if `.rs` staged, eslint and tsc if TS staged; < 10 s. Not enabled in this session |
| `rust-toolchain.toml` | root; `channel = "1.97.0"`, `profile = "minimal"`, `components = ["rustfmt", "clippy"]` |

`npm run check`: typecheck, lint, check:arch, versions:check, docs:links, vitest, cargo fmt --check, cargo clippy --all-targets -D warnings. `npm run check:web`: the same without cargo and with `check:arch --only=web` (used by a TypeScript package running next to a Rust package). `npm run check:full`: check + `cargo test --no-fail-fast` + `npm run build`. In this session: `LAUNCHDECK_CARGO=<scratchpad>/cargo.sh npm run check`.

Checks that fail: the import matrix, crate confinement, cfg placement, `@tauri-apps` outside `ipc.ts`, `src/lib` importing React/features/components/stores, `src/stores` importing features/components, a feature edge not in FRONTEND_MAP, `preview.ts` imported outside `ipc.ts`, a new file or directory named `utils/helpers/common/misc`, a `pub mod` in `lib.rs` missing from CODEBASE_MAP, a `src/features/*` directory missing from FRONTEND_MAP. Advisory only: files ≥ 400 production lines ("watch"), ≥ 700 must be in the allowlist with a reason and baseline; growth > 10 % is reported.

## 6. Context Engine completion plan

Context Engine v1 is complete in code and tests; the work is verification, honesty and one pure resolver for Packs. No rebuild, no new context source, no new feature.

| Item | Decision | Package |
| --- | --- | --- |
| Platform claims | macOS: "compiled and clippy-clean on macOS 15.4.1 arm64, Rust 1.97.0; running NOT VERIFIED (f)". Windows: "written, NOT VERIFIED, never compiled (a, e)". X11: "tested on Linux in an earlier milestone; not re-run here (d)". Wayland: not available, said so. Applied to README, `docs/context-engine.md`, observer header comments | B0, B1 |
| macOS evidence without the GUI | `examples/foreground.rs` runs the observer on a spawned thread and pumps the main `NSRunLoop` on macOS (objc2-foundation features `NSRunLoop`, `NSDate`, macOS-only dev-dependency entry; no new crate). An agent runs it, switches apps with `open -a TextEdit/Finder/Terminal`, records the printed identities in VERIFICATION.md as observer evidence. The in-app checklist (NEXT_PHASE 4.2) stays NOT VERIFIED (f) | B1 |
| Obsolete `ContextHint` | `Surface.context` gets `skip_serializing_if = "Option::is_none"`; starters stop setting it; loading still accepts it; `dump_workspace` regenerates the fixture; exports never contain it | B1 |
| Persist cost of context-driven activation | measure `Runtime::persist` over 20 activations here; if the median is ≥ 5 ms, skip the save for `By::Context` activations (Auto recomputes `lastActiveSurface`); record the numbers either way | B1 |
| Portable rules for Packs | new pure `context/portable.rs`: `PortableApplication { name, identities: Vec<(namespace, id)> }`, `resolve(namespace) -> Resolved(ApplicationIdentity) \| Unmapped`, `from_rule`. Any namespace that passes `ApplicationIdentity::validate` is carried losslessly; only the namespace the engine reports (`ContextStatus.namespace`, i.e. `Engine::namespace()`) is matched; display names never match. `ContextRule` and matching are unchanged | B1 |
| Rule provenance in the panel | none in v1 (imported rules are ordinary local rules) | — |

Human checks that remain: NEXT_PHASE 4.2 in the real window, at release candidate (R5 lists them).

## 7. Portable artifacts plan

One new subsystem, `src-tauri/src/artifacts/`. The format is public and independent of the workspace schema. Specification of record; `docs/artifacts/FORMAT.md` (C7) is written from this section and from the code.

### 7.1 Container

A single UTF-8 JSON document (RFC 8259). No archive, no compression, no binary assets. Nothing from an artifact is written to disk except, after confirmation, the workspace through the existing `Store`. Zip-slip, absolute paths, symlinks, hard links, duplicate or case-colliding entry names, compression bombs and extraction races are impossible by construction (no code path exists); hostile fixtures prove it (a ZIP renamed `.ldpack` fails as `notJson`; any `assets` member is refused).

### 7.2 Identifiers and versions

| | Surface artifact | Pack artifact |
| --- | --- | --- |
| Extension | `.ldsurface` | `.ldpack` |
| Media type | `application/vnd.launchdeck.surface+json` | `application/vnd.launchdeck.pack+json` |
| `format` | `launchdeck.surface` | `launchdeck.pack` |
| `formatVersion` | `1` (integer, shared) | `1` |

`formatVersion` is independent of workspace schema 4, manifest schema 4, ABI 1, SDK 0.2.1 and the app version. A reader accepts exactly the versions it knows: greater → `tooNew`, other → `unsupportedFormatVersion`. Never changed in place. Format 1 is documented as an alpha format, not a stability promise. `format` decides the kind; the extension is a hint (mismatch = warning). Export file name: `<slug(name)>-<version>.ld{pack|surface}`, `[a-z0-9.-]`, ≤ 64 characters. UI words: "Surface file" and "Pack". "Pack" means only `launchdeck.pack`: `agents/pack.rs` and `examples/agent-pack/` are deleted (C1) and their docs redirected.

### 7.3 Envelope and inspect order

Top level exactly `{ "format", "formatVersion", "manifest", "content" }` in that order; any other member → `unknownField`, except `signatures` → `unsupportedSignature` and `assets` → `unsupportedAsset`.

Inspect order; each step rejects with a code and nothing later runs:

1. empty → `empty`; > 2 MiB → `tooLarge` (byte count, before parsing).
2. not UTF-8 or starts with a BOM → `notUtf8`.
3. Pre-pass over the token stream (serde `DeserializeSeed` visitor): depth > 32 → `tooDeep`; duplicate key in any object, compared after unescaping → `duplicateKey`; string > 64 KiB or key > 128 B → `stringTooLong`; invalid JSON → `notJson`.
4. Envelope into `Envelope<'a> { format: &str, format_version: u32, manifest: &RawValue, content: &RawValue }` (deny unknown); unknown `format` → `notArtifact`; version checks.
5. Manifest typed parse and validation → `invalidManifest`.
6. Integrity: digest of `content` equals `manifest.integrity.content` → else `integrity`.
7. Content typed parse and validation → `invalidContent`; limits → `tooMany`.
8. Cross-checks: kind rules (`kindMismatch`), recomputed requirements ≤ declared, every plugin provider used by an assignment or state rule is declared, every reference resolves.

A rejection is `{ code, message, at? }`: `message` is a sentence for a person, `at` a JSON pointer (`/content/surfaces/0/pages/1/controls/3/press`). Echoed values are truncated to 40 characters with control characters removed (local helper).

### 7.4 Manifest

`manifest` (deny unknown members):

| Field | Rule |
| --- | --- |
| `id` (required) | lowercase dotted, ≥ 2 segments, each `[a-z][a-z0-9-]*`, 3-64 chars (plugin-id grammar, separate namespace). Export proposes `<slug(author)\|local>.<slug(name)>` |
| `name` (required) | 1-60 chars, trimmed |
| `version` (required) | SemVer `MAJOR.MINOR.PATCH[-PRERELEASE]`, ≤ 32 chars, `semver::Version`; build metadata refused |
| `description` | ≤ 1000 chars, `\n` allowed |
| `author` | `{ name 1-64, url?: https only, ≤ 256, no userinfo }`; shown as text, never fetched |
| `license` | ≤ 64 printable chars, not checked against SPDX |
| `createdWith` (required) | `{ app: "launchdeck", version: <app SemVer> }`, informational |
| `requirements` (required) | 7.7 |
| `testedWith` | ≤ 8 `{ model: [a-z0-9-]{1,32}, kind: "hardware" \| "virtual" }`, informational |
| `dependencies` | ≤ 32, unique `pluginId`, sorted by `pluginId`; 7.6 |
| `inputs` | Pack only, ≤ 32, unique `id`; 7.8 |
| `integrity` (required) | `{ content: "sha256:<64 lowercase hex>" }` |

All text refuses Unicode control characters (Cc; `\n` only where stated) and bidi controls U+202A-U+202E, U+2066-U+2069; names and labels equal their trimmed form.

### 7.5 Content

`content = { surfaces: [SurfaceV1], contextRules?: [ContextRuleV1] }`. Surface artifact: exactly 1 surface, no `contextRules`, no `inputs`. Pack: 1-16 surfaces.

| Type | Shape |
| --- | --- |
| SurfaceV1 | `{ id: [a-z0-9][a-z0-9-]{0,63} unique, name 1-40, description? ≤ 400, icon? 1-8 chars, category [a-z][a-z0-9-]{0,31}, pages 1-32 }`; unknown category imports as `custom` with a warning |
| PageV1 | `{ id: [A-Za-z0-9._:-]{1,64} unique in surface, name 1-40, controls ≤ 128 }` |
| ControlV1 | `{ id (same grammar, unique in page), pos: {kind:"grid",col,row} \| {kind:"top",index} \| {kind:"side",index}, span?: {w 1-8, h 1-8} (grid only), label 1-40, description? ≤ 400, icon? 1-8, visual: {color "#rrggbb", animation?: none\|pulse\|flash}, looks?: {<name 1-40>: Visual} ≤ 32, press?: AssignmentV1, longPress?: AssignmentV1, feedback?: [FeedbackRule] ≤ 32 }`; geometry per `core::validate::validate_control` (coordinates ≤ 15, no overlap, no logo control). `looks` is the public name of the workspace `states` |
| AssignmentV1 | `{ provider 1-64, action 1-64, args: object (≤ 16 keys, depth ≤ 4, ≤ 32 KiB, text ≤ 16 Ki chars), inputs?: {argName: inputId} (Pack only), surfaceRefs?: [argName] }` |
| FeedbackRule | exactly the workspace schema-4 `state::feedback::FeedbackRule` (strict), frozen by a golden test; must pass the extracted `check_rule_shape`; a `show` look must exist in `looks` or be `idle` |
| ContextRuleV1 | `{ application: AppRefV1, surface: <artifact surface id>, page?: <page of that surface> }`, ≤ 64 |
| AppRefV1 | `{ name 1-80, identities: [{namespace, id}] 1-8, unique namespaces, each valid per ApplicationIdentity::validate }` or `{ input: <application input id> }` |

Members equal to their default are omitted on export. The DTOs are their own types in `artifacts/format.rs` (deny unknown everywhere), converted to and from `core::model`.

### 7.6 Dependencies

`{ pluginId (valid_plugin_id), name? 1-64 (display), version: requirement, required: bool }`. Requirements are `semver::VersionReq` (Cargo semantics, bare `1.2.3` = caret; on 0.x the minor is breaking), ≤ 64 chars, ≤ 4 comparators, no pre-release comparators, `*` allowed. Export: a registered plugin provider used by a press/longPress → `required: true, ^<installed>`; used only by state or event feedback → `required: false`; an unregistered ≥ 2-segment provider → `*` with warning "not installed here"; single-segment providers (system, navigation, shell, http, agent-control, demo) are host providers, never dependencies; `legacy` assignments are stripped. Import: every ≥ 2-segment provider used by an assignment or a state rule must be declared (`invalidManifest` otherwise); declared but unused → warning. Status per dependency: `compatible{installed}`, `disabled{installed}`, `incompatible{installed}`, `missing`. No solver, no download.

### 7.7 Device requirements

`requirements = { grid?: {minWidth 1-16, minHeight 1-16}, aux: {top 0-16, side 0-16}, rgb: bool }`, computed by the exporter (max col+span.w / row+span.h; max top/side index + 1; rgb if any colour is not grey). The importer recomputes and rejects declared values weaker than the content. `testedWith` defaults to the models connected at export. Fit is reported per listed device (the virtual device is always there) with the count of positions outside it and never blocks import.

### 7.8 Typed inputs (Pack only)

`InputDef { id [a-z][a-z0-9_]{0,31}, type: path|url|enum|string|application, label 1-40, description? ≤ 280, required (default true), default?, options? (enum: 1-32 {value 1-64, label 1-40}, unique), maxLength? (string: 1-2048, default 256) }`. `secret` and any other type are refused: secret-backed plugin settings stay in the SecretStore and are never inputs. Values `InputValue = {kind:"text", value} | {kind:"application", namespace, id, displayName}` checked in Rust: path 1-1024, no control chars, absolute on this platform; url ≤ 2048, `url` crate, http/https, no userinfo; enum one of the options; string ≤ maxLength, no control chars; application namespace = the engine's namespace and valid. Binding is structured, no template language: `AssignmentV1.inputs {argName: inputId}` replaces the whole arg; `AppRefV1 {input}` takes the identity. Application inputs bind only to rules; the other types only to string args. After substitution, if the provider is installed, `validate_assignment` runs and its issue on the bound param becomes a blocking issue of that input. An optional input without a value removes the bound arg. Export suggests (never applies) a path input for string args starting with `/`, `~/` or `<letter>:\`.

### 7.9 Integrity and future signing

`integrity.content = "sha256:" + 64 lowercase hex` over the bytes of the `content` member as they appear in the file, with insignificant JSON whitespace (0x20, 0x09, 0x0A, 0x0D outside strings) removed. Re-indentation and CRLF do not change it; any change of value, key, order or escape does. It is integrity, not publisher trust; the UI says "Publisher: not verified". Reserved for a later format version: top-level `signatures: [{keyId, algorithm: "ed25519" | "sigstore-bundle", signature}]` over the whitespace-stripped `manifest` bytes (which pin the content digest); v1 refuses the member.

### 7.10 Export

Pure function of (workspace, provider kinds and versions, device models, app version, `ExportRequest`). Never exported: `schemaVersion`, preferences, `lastActiveSurface`, `context.auto`, `context.fallback`, rules whose target is not exported, `Surface.context`, `Surface.importedFrom`, `Control.legacyBindings`, `legacy` assignments (slot emptied, counted), all runtime state (hub values, overlays, held looks, frames, pending actions, provider status, availability), agent sessions, settings, audit, hook token, plugin config, grants, secrets, logs, device ids, timestamps, machine or user identifiers. Blocking export errors: a string arg that is a URL with userinfo; limits exceeded; invalid metadata or inputs. Warnings: every runsCommand/sendsRequest/launchesProgram item ("check it holds no password or token"), navigation to a surface not exported, a plugin not installed, path suggestions.

Determinism: surfaces in workspace order, pages and controls in stored order, rules in workspace order, dependencies by `pluginId`, inputs in request order. Artifact surface id = `importedFrom.sourceSurfaceId` when present and unique, else the local id, de-duplicated with `-2`, `-3`. Canonical writer: object keys sorted by UTF-16 code units (explicitly, whatever serde_json's map does), defaults omitted, 2-space indent, LF, one trailing LF, serde_json escapes and numbers, envelope members in fixed order. Tests: export twice → identical bytes; export → reset → import → export → identical bytes.

Saving: `artifacts_export_save` writes `<Downloads>/<file name>` (Tauri's `download_dir()`; `-2`, `-3` before the extension if taken; never overwrites) with `persistence::write_atomic`. The webview never sends a path. `artifacts_show_export` reveals the last saved file's folder with the `open` crate. No save dialog in v1 (no new dependency); macOS may ask once for Downloads access, and a refusal is shown as a sentence.

### 7.11 Provenance and the commit primitive

`core::model::ImportOrigin { format, artifactId, artifactName, artifactVersion, instance: u32 (1-based), sourceSurfaceId, contentDigest, surfaceDigest }` and `Surface.imported_from: Option<ImportOrigin>` with `#[serde(default, skip_serializing_if = "Option::is_none")]`. No schema bump; existing files and `examples/default-workspace.json` stay byte-identical; a 0.2.x build drops the field and keeps the content. `surfaceDigest` is the SHA-256 of the canonical SurfaceV1 form as written; a different current digest reads "may have been changed since import". Local only: never exported, no timestamps, no paths, no telemetry. The TS `surfaceSchema` gains `importedFrom` (nullish), for display only.

`Runtime::commit_workspace<E>(&self, build: impl FnOnce(&Workspace) -> Result<Workspace, E>) -> Result<(), CommitError<E>>` (`pub(crate)`, in `core/runtime/editing.rs`): under the lock, build the candidate, `store.save(&candidate)`, and only on success swap it in; then rebuild the index, `evaluate_all`, context `settings_changed(was_auto)`, re-activate the first surface if the active one is gone, release the lock, emit workspace and context. Any error returns before the swap. Import and `remove_surface` use it; never `persist()`.

### 7.12 Import flow and the ImportPlan

Pure functions, React renders only: `inspect(bytes, file_name) -> Result<ParsedArtifact, Rejection>` → `plan(&ParsedArtifact, &Workspace, &Environment, &ImportDecisions) -> ImportPlan` → the UI sends decisions to `artifacts_preview` (recomputes the plan) → `artifacts_apply(planId, decisions, planDigest)`. `Environment` is built by `artifacts::service` from existing getters only: `snapshot()`, `catalog()` (provider kind, version, status), `observables()`, `context()` (namespace, recent applications), `list_devices()`, `validate_assignment`, `state_hub().definition`.

Rust types (`artifacts/plan.rs`, serde camelCase):

* `ImportPlan { planId (32 hex, getrandom), planDigest (sha256 of the canonical JSON of effects), source {fileName ≤ 128 sanitized, sizeBytes, contentDigest, formatVersion}, artifact {kind, id, name, version, description, author, license, createdWith, testedWith}, surfaces: [PlannedSurface], contextRules: [PlannedRule], dependencies: [DependencyCheck], inputs: [PlannedInput], review: [ReviewItem], existing: [ExistingInstance], compatibility, warnings: [{code, message, at?}], decisions, suggested, blockers: [{code, message, input?}], ready: bool, effects: PlanEffects }`.
* `PlanEffects { instance, addSurfaces [{sourceId, localId, name}], replaceSurfaces [{sourceId, localId}], addRules [{index, application, surfaceLocalId, pageId}], removedSlots [Location], inputValues {id: InputValue} }`: exactly what apply writes, and the only digested part.
* `PlannedSurface { sourceId, localId, name, category, pages, controls, action: add | replace{previousVersion, locallyEdited}, idChanged, sameNameAs?, missingProviders }`.
* `PlannedRule { index, applicationName, application?, surfaceLocalId, pageId?, outcome: add{position} | notSelected | skipConflict{localRule} | skipShadowed{by} | skipNoIdentity | skipCapacity | needsInput{input} }`.
* `DependencyCheck { pluginId, name, requirement, required, status, usedBy }`.
* `PlannedInput { def, value?, issues: [String], usedBy: [Location] }`.
* `ReviewItem { at: Location{surfaceSourceId, pageId, controlId, slot: press|longPress}, label, kind: runsCommand | sendsRequest | launchesProgram | instructsAgent, text }`.
* `ExistingInstance { instance, version, relation: same|newer|older, identicalContent, surfaces [{localId, name, sourceId, locallyEdited}] }`.
* `Compatibility { namespace?, requirements, devices [{name, model, kind, connected, fits, outside, rgbOk}] }`.
* `ImportDecisions { resolution?: importAsNew | replace{instance}, runs?: keep | remove, inputs: {id: InputValue}, contextRules: [index], confirmOverwriteEdits: bool }`. `suggested`: importAsNew, runs remove, input defaults, **no context rules** (rules are opt-in, listed unchecked).
* `ImportReport { surfacesAdded, surfacesReplaced, rulesAdded, rulesSkipped, slotsRemoved, missingDependencies, autoContext: bool }`.
* Blockers: `inputRequired`, `inputInvalid`, `runsDecisionNeeded`, `resolutionNeeded` (only when `existing` is non-empty), `overwriteEditsNotConfirmed`, `workspaceFull` (> 64 surfaces).

### 7.13 ID remapping

A table maps source surface id → local id, built in artifact order. New surface: the source id if free, else `<id>-2`, `-3`… (the `create_surface` rule). Replaced surface: the existing local id. Rewritten through the table: `ContextRuleV1.surface` and every arg named in `surfaceRefs`. Kept verbatim: page and control ids, all other args. A `navigation.openSurface` arg not in `surfaceRefs` keeps its value (warning `externalSurfaceReference`). The exporter records `surfaceRefs` for every arg whose definition is a `reference` to a surface (plugin params included); the importer never consults the catalog to find references. Local rules and navigation are never rewritten.

### 7.14 Conflicts and re-import

* A. Same surface id: remapped (info `idChanged`), not a choice.
* B. Same display name: name kept (info `sameName`); the sidebar shows an "Imported" badge from `importedFrom`.
* C. Same artifact already imported (local surfaces whose `importedFrom` has the same format and `artifactId`): `existing` lists instances with relation by SemVer precedence, identical content and locally-edited flags; `resolution` must be chosen; `importAsNew` suggested (new instance, fresh ids); `replace{instance}` replaces matching surfaces in place (same local id and position, new `importedFrom`), adds unmatched incoming surfaces to that instance, keeps surfaces the new version lacks (warning `keptFromPreviousVersion`), and is blocked until `confirmOverwriteEdits` if any replaced surface was edited. Downgrades and identical re-imports are shown and remain explicit choices. No merge, no automatic update, no update check.
* D. Context rules: only rules the person selected are added; a resolved identity that `is()` an existing local rule's identity → `skipConflict` (local wins); a second incoming rule for the same identity → `skipShadowed`; no identity for this namespace → `skipNoIdentity` (or `needsInput` when bound to an application input); past 64 total → `skipCapacity`. Added rules are appended after all local rules in artifact order. Auto and the fallback never change. Rules are checked with `context::rules::checked(new, previous, workspace_after_import)`.
* E. Duplicate surface, page or control ids inside the artifact reject it.

### 7.15 Actions that run things

Classified while planning: `runsCommand` (`shell.execute`, text = the command), `sendsRequest` (`http.request`, text = `METHOD URL` + body length; also any future `privileged: true` definition), `launchesProgram` (`system.launchApplication` with non-empty `args` or an `app` containing `/` or `\`; ids from constants exported by `plugins/system.rs`). These three need the decision `runs`: `remove` (suggested; slots emptied, label/looks/feedback kept) or `keep`. `instructsAgent` (`agent-control.newTask`, `agent-control.continue`) is shown, needs no decision, always kept. Plugin actions are never review items (plugins cannot be privileged; network stays behind grants that import never changes). Nothing is executed during import: no action, test run, install, rescan, fetch or grant change; logs record only codes and ids.

### 7.16 Missing or incompatible dependencies

Never blocking. Content is stored verbatim (never normalized against a local version). Missing/disabled: pads use the existing missing/unavailable UI; state rules see `unavailable`; the plan says "Some pads will not work until you install X <requirement>". Recovery needs no import step: install (D2's Install button, which the import view offers when the missing id is a bundled example), rescan, and the provider comes up. Incompatible or compatible: each assignment goes through `validate_assignment` and rule addresses through hub definitions; failures are warnings (`invalidForInstalledVersion`) with locations, kept verbatim. Unknown single-segment provider → warning `unknownHostProvider`. Unregistered event source → info `externalEventSource`.

### 7.17 Transactional apply

`artifacts_apply(planId, decisions, planDigest)`: (1) unknown pending → `expired`; (2) build the Environment; (3) `runtime.commit_workspace(|current| { plan = plan::build(..); if plan.planDigest != planDigest → Err(Stale(plan)); if blockers → Err(Blocked(plan)); candidate = apply::build(..)?; apply::verify(&candidate, current)?; Ok(candidate) })`. `apply::verify` runs `validate_control` for every control of every touched page, `context::rules::checked`, and surface id uniqueness. Outcome: `{status:"applied", report} | {status:"stale", plan} | {status:"blocked", plan} | {status:"failed", message} | {status:"expired"}`. On every non-applied outcome the in-memory workspace and `workspace.json` are unchanged. One pending import at a time; a new inspect replaces it; dropped on success or `artifacts_discard`.

### 7.18 Limits (`artifacts/limits.rs`, also sent to the UI)

File ≤ 2 MiB; JSON depth ≤ 32, string ≤ 64 KiB, key ≤ 128 B. Surfaces: 1 (surface artifact), 1-16 (pack); pages 1-32; controls ≤ 128 per page, ≤ 2048 per artifact; looks ≤ 32 (name 1-40); feedback rules ≤ 32 per control, where ≤ 8, hold 200-60000 ms or latch; context rules ≤ 64; identities 1-8 (id ≤ 255, namespace ≤ 16); dependencies ≤ 32; inputs ≤ 32; enum options ≤ 32; string maxLength ≤ 2048; path ≤ 1024; url ≤ 2048; bindings ≤ 256; args ≤ 16 keys, depth ≤ 4, ≤ 32 KiB, text ≤ 16 Ki chars; artifact name 1-60, description ≤ 1000, author 1-64, url ≤ 256, license ≤ 64; surface/page name and label 1-40; control description ≤ 400; icon 1-8 chars; input label 1-40, description ≤ 280; ids ≤ 64; version ≤ 32; requirement ≤ 64 with ≤ 4 comparators; testedWith ≤ 8; assets 0. After import: ≤ 64 surfaces (blocker), rules past 64 skipped. Existing editor limits are reused by reference.

### 7.19 Module layout, commands and TypeScript contract

`artifacts/{mod, limits, json (pre-pass, digest, canonical writer), format (DTOs, conversion, structural validation), inspect, export, env, plan, apply, service}.rs`, unit tests beside each. Core gains only `ImportOrigin` and `commit_workspace`; `state/feedback.rs` gains `check_rule_shape`; `context/portable.rs` (B1). Cargo: `semver = "1.0.28"`, `sha2 = "0.10.9"` (both already locked), serde_json feature `raw_value`. No `zip`, no dialog plugin.

Commands (`commands/artifacts.rs`; `AppState` gains `artifacts: ArtifactService`):

| Command | Returns |
| --- | --- |
| `artifacts_limits()` | `ArtifactLimits` |
| `artifacts_export_preview(request: ExportRequest)` | `ExportPreview` |
| `artifacts_export_save(request: ExportRequest)` | `ExportSaved { fileName, folder, sizeBytes, contentDigest }` |
| `artifacts_show_export()` | `()` (reveals the last saved export) |
| `artifacts_inspect(request: tauri::ipc::Request)` | `InspectOutcome = {status:"plan", plan} \| {status:"rejected", rejection}`; raw body = file bytes, header `x-launchdeck-file-name` = `encodeURIComponent(name)` |
| `artifacts_preview(planId, decisions)` | `PreviewOutcome = {status:"plan", plan} \| {status:"expired"}` |
| `artifacts_apply(planId, decisions, planDigest)` | `ApplyOutcome` |
| `artifacts_discard(planId)` | `()` |

`ExportRequest { kind: surface|pack, surfaceIds, metadata {id, name, version, description?, author?, license?}, includeContextRules, inputs: [InputDef], bindings: [{at: Location, arg, input}], ruleInputs: [{rule, input}], extraIdentities: [{rule, namespace, id}] }`. `ExportPreview { fileName, sizeBytes, contentDigest, manifest, surfaces [{localId, artifactId, name, pages, controls}], contextRules, dependencies, requirements, testedWith, review, stripped {legacyBindings, legacyActions, contextHints}, suggestions [{at, arg, value, suggestedType}], parameters [{at, arg, value}], warnings, errors }`.

TypeScript: `src/lib/schema/artifacts.ts` (Zod for every shape), `api.artifacts.*` in `ipc.ts`, UI in `src/features/artifacts/`. Contract fixtures: `cargo.sh run --example dump_artifacts` writes `examples/artifacts/{development.ldpack, export-preview.json, import-plan.json, import-plan-missing.json, inspect-rejected.json}`, guarded by `tests/artifacts_contract.rs`, parsed by Vitest.

### 7.20 Hostile fixtures (release blocker)

`src-tauri/tests/artifacts_security.rs` runs each case through `ArtifactService` on a `Runtime` with a real `Store` in a tempdir and asserts the rejection code and `at`, that `workspace.json`, `plugins.json` and `permissions.json` are byte-identical, and that `snapshot().workspace` is unchanged. Files in `src-tauri/tests/fixtures/artifacts/hostile/` (`.gitattributes`: `-text`); large or deep cases generated in code.

* Bytes: empty; 2 MiB + 1; truncated; garbage; UTF-8 BOM; invalid UTF-8; ZIP local-header bytes renamed `.ldpack`; top-level array.
* Envelope: missing format; `launchdeck.workspace`; formatVersion 2, 0, `"1"`; unknown top-level member; `signatures`; `assets` (SVG with `<script>`, base64 `MZ`, name `../../x`); a raw `workspace.json`.
* JSON: duplicate `content`; duplicate key in a control; duplicate look name via `\u0061` escape; 10 000 nested arrays in args; 70 KiB string; `1e400`.
* Integrity: one-character content edit; missing integrity; uppercase hex; `md5:`; whitespace-only reformat **accepted** (control case).
* Manifest: ids `../evil`, `Acme.Pack`, `single`, `/abs`, `C:\x`; versions `1.0`, `01.0.0`, `1.0.0+build`; empty or 61-char name; label with U+202E; control char in description; author url `javascript:`, `http://`, `https://u:p@h`.
* Dependencies: invalid pluginId; `>>1`; pre-release comparator; duplicate; 33; host provider `system`; undeclared `acme.notes`.
* Limits and geometry: 17 surfaces; 33 pages; 129 controls on a page; 2049 controls; 33 looks; 33 feedback rules; 65 rules; 9 identities; col 16; span 9; logo control; overlap; duplicate surface/page/control ids.
* References: rule to a surface not in the pack; rule page not in its surface; `surfaceRefs` to a missing arg, a non-string or an unknown surface; binding to an undeclared input; application input bound to an arg; text input bound to a rule; `show` naming a missing look.
* Feedback: hold 10 ms; 9 where entries; both show and showFromValue; invalid state provider id.
* Assignments: args not an object; extra member; args depth 5; 33 KiB args.
* Inputs: type `secret`; enum without options; default not an option; duplicate id; 33 inputs.
* Requirements understated; kind: a surface artifact with rules, with inputs, with 2 surfaces.
* Apply stage: stale digest after a workspace change; runs decision missing with `shell.execute`; required input missing; invalid input (relative path, `ftp:` URL); replace of an edited instance without confirmation; workspaceFull; save failure (a non-empty directory at `workspace.json.tmp`); expired planId.
* Positive control: a valid Pack with `shell.execute` and `runs=remove` imports with the slot emptied, and a recording provider and keyboard see zero calls.

## 8. Plugin ecosystem hardening plan

No SDK redesign. Keep ABI 1, manifest schemas 1-4, SDK 0.2.1 (bump the SDK minor only if SDK code changes; it does not in this plan), the four independent version numbers, the compatibility rule (newer SDK minor before 1.0 refused, older accepted).

| Item | Decision | Package |
| --- | --- | --- |
| Error kinds | `kind: incompatible \| invalidManifest \| invalidModule \| duplicate \| initFailed \| stopped` on `Issue` and `ExternalInfo`, mapped from `ManifestError` variants and load/trap paths; the card shows one sentence per kind, raw reason and path under Details; `dump_plugins` regenerates `examples/plugins-overview.json` | D1 |
| Legacy secrets code | `plugins/external/legacy_secrets.rs` (structural commit) | D1 |
| SDK version copies | Rust test: `sdk/assemblyscript/package.json` version == `SDK_VERSION` | D1 |
| First-party id prefix | refuse external ids starting with `launchdeck.` | D1 |
| Validate command | `src-tauri/examples/validate_plugin.rs` driving the real `PluginHost::scan()` over a temp copy, in-memory grants and secrets; `npm run plugins:validate -- <folders>`; exit 1 on failure; warns when manifest `sdk` < `SDK_VERSION` | D1 |
| Reference set | Hello (minimal), Counter (stateful), Home Assistant (network + secret), bundled with the app; Channels and C Ping stay as "SDK conformance samples", not bundled; C Ping leaves the default build list | D2, D3 |
| In-app install | `bundle.resources` (map form) ships `plugin.json` + `.wasm` of the three; `plugins/external/install.rs` validates with `discovery::load_dir`, copies to `<app_data>/plugins/<id>/` through a temp dir + rename, refuses an installed id or a symlink, then `scan()`. The explicit click installs and enables (as dropping a folder does today); no grant is implied. Commands `plugins_examples`, `plugins_install_example`, `plugins_reveal_folder`. Debug builds fall back to `<repo>/examples/plugins` when the resource dir lacks them | D2 |
| Reproducibility | frozen SDK 0.1 modules → `src-tauri/tests/fixtures/plugins/sdk-0.1/` with PROVENANCE and sha256, loaded by a test; examples rebuilt with SDK 0.2.1 through `npm ci`, manifest `sdk` stamped, `examples/plugins/SHA256SUMS`, `scripts/build-example-plugins.sh --check` | D3 |
| Grants and secrets bound to the plugin id only | documented in `docs/plugin-sdk/security.md` and `docs/plugins/permissions.md`; module hashing deferred | D3 |
| Author flow | one page: create (copy hello; depend on the SDK by path or a packed tarball), build, validate (`plugins:validate`), link (`LAUNCHDECK_PLUGINS`), test (Test run), logs (Show log), package (zip `plugin.json` + `.wasm`; no checksums or signing yet) | D3 |
| Legacy schema 1-3 `config[].secret` stored in plain text | **user decision** (risk R-04); unchanged until then, listed as a known limitation | — |
| Uninstall, file/folder install, package format | deferred | — |

Human/external: Keychain (4.3) NOT VERIFIED (f); real Home Assistant NOT VERIFIED (c).

## 9. Onboarding and release plan

| Topic | Decision | Package |
| --- | --- | --- |
| Error UX | Rust command errors become `CommandError { message: sentence for people, detail?: ids/OS/WASM text }` with `From<String>`, so conversion is gradual; React shows the message plus a Details disclosure; an error toast with details stays until dismissed; React writes no per-error wording; UI-originated failures (reply mismatch) get one UI sentence with the Zod text as detail. Convert `core/validate.rs`, unknown surface/page/control, name length, and the AssignmentEditor/LiveFeedback/Access/fatal-screen sinks. `Issue.kind: missing \| invalid` replaces `endsWith(' is required')` (text kept as a fallback for one release); `examples/param-cases.json` from `dump_param_cases` guards the `normalizeArgs` mirror on rejected inputs | E1 |
| Device state | `DeviceInfo.status: connected \| available \| unplugged \| unsupported \| failed` and `problem?: sentence`; unplugged hardware stays listed until it returns; other Launchpad-named ports listed as unsupported by name only, never opened (written, not run (b)); connect failures recorded. Wording in `src/lib/devices.ts`, one `DeviceStatusLine` component, one `primaryDevice` selector; the virtual device reads "Simulator"; no MIDI or "Programmer mode" words in normal UI (capabilities under Technical details) | E2 |
| Surface management | `remove_surface` (refuse the last; removes rules that target it and resets a fallback that targets it, listed in the confirmation; through `commit_workspace`), `update_surface` (name, description, icon), `rename_page`, `remove_page` (refuse the last page) | E3 |
| Workspace load | `LoadOutcome { workspace, notice? {kind: damaged \| tooNew, backup} }`; `Snapshot.notices`; persistent banner "Your surfaces could not be opened, so LaunchDeck started fresh. Your file was kept as workspace.json.bak." with Show folder and Details | E3 |
| First run | one screen while `onboardingDone` is false: device status, purpose radiogroup (Productivity, Agent Control, Development, Accessibility, Build my own; text from `Surface.description`/`icon`), Start and Skip, "Import a Pack instead". Start activates the surface (Live); Build my own = `create_surface` → Edit → Library. `set_onboarding_done(true)` last. Never runs the demo, removes starters or creates rules. Productivity/Accessibility cards mention the macOS Accessibility prompt | E4 |
| Starters | Home, Development, Agent Control, Productivity, Accessibility. Smart Home is shown as **Home** (id `smart-home` kept); its webhook pads become `example.home-assistant` actions with example entity ids, so they read "needs the Home Assistant plugin" until it is installed and configured, and nothing is sent to the network on first press. The sidebar view "Home" becomes **Overview**. React's `BLURBS` are deleted (Rust is the only author). The Marketplace placeholder is removed. Starters carry no context rules | E4 |
| Settings | `features/settings/SettingsView.tsx` with sections: Devices, Auto Context, Notifications (moved from Agents), Plugins & permissions, Security, Workspace (reset, Import/Export), Developer (collapsed: WebSocket tester, data folder, show the welcome again), About (`app_info`, "Copy for a bug report"). No Appearance section | E5 |
| Versioning | app **0.3.0**, plain numeric (pre-release suffixes break MSI, CFBundleShortVersionString, RPM and semver ranges), labelled "alpha" in About, README, CHANGELOG and the GitHub pre-release flag. `src-tauri/Cargo.toml` is the single source: `version` removed from `tauri.conf.json`; `package.json` kept equal by `versions:check`; every first-party built-in provider reports `CARGO_PKG_VERSION`. Unchanged: SDK 0.2.1, ABI 1, manifest schema 4, workspace schema 4, bridge protocol 1, artifact format 1 (integers are compatibility numbers, not 1.0 claims). Table in `docs/architecture/versions.md` and in About | E5, R4 |
| Empty states | one sentence + one next step: no plugins (Install examples / Show plugins folder), blank Live page ("Switch to Edit to add pads"), missing action ("Needs the plugin <name>" + Open Plugins, id under Details), activity feed with names, category labels, no repository `docs/` paths in rendered text (test) | E6, D2 |
| Bundles in the alpha | macOS arm64 `.app`/`.dmg` built here, ad-hoc signed (`signingIdentity "-"`), `minimumSystemVersion "11.0"`; Linux `.deb`/`.rpm` and Windows NSIS from CI only, NOT VERIFIED (a). Not shipped: MSI, AppImage, x86_64/universal macOS, auto-update. Identifier `dev.launchdeck.app` kept | R5, R1 |
| License | MIT, decided by the owner on 2026-10-05: root `LICENSE`, `license` in Cargo.toml, the root and example `package.json` files and the SDK, `bundle.license` and `bundle.licenseFile` in tauri.conf.json, README section | done |
| CHANGELOG | Keep a Changelog 1.1.0, by capability: `[Unreleased] — 0.3.0, first external alpha` (capabilities, verified where, versions, link to known limitations, license status) and a `0.2.0 — 2026-10-04, internal prototype, not released` line; date and RC commit fixed at release | R4 |

## 10. Documentation plan

Target tree (moves with `git mv` in one structural commit, links fixed, `docs:links` green; content in later commits):

```
README.md                          product first, ≤ ~120 lines
CHANGELOG.md
AGENTS.md, CLAUDE.md, PRODUCTIZATION_PLAN.md (until the milestone closes)
docs/README.md                     index by audience
docs/getting-started/{install, first-run, build-from-source}.md
docs/user/{surfaces-and-editor, action-library, feedback-editor, context-engine, agent-control,
          plugins, sharing, launchpad-x, known-limitations}.md
docs/plugin-sdk/                   as is (+ author flow, versions row, Windows wording)
docs/agent-control/{bridge-protocol, adapter-sdk, domain-model, surfaces, security}.md (integrators)
docs/artifacts/{README, FORMAT, security}.md
docs/architecture/{overview, CODEBASE_MAP, FRONTEND_MAP, ENGINEERING_RULES, versions,
                   state-and-feedback, observables, surface-format, secrets,
                   launchpad-x-protocol, plugins}.md, docs/architecture/actions/*.md
docs/roadmap/{marketplace, mcp}.md (marketplace reduced to hosting/listing/signing ideas)
docs/contributing/{testing, releasing, ci, manual-checks-macos}.md
docs/verification/{macos-2026-10-05.md (VERIFICATION.md moved, unedited), release-0.3.0.md}
docs/history/{plans/*_PLAN.md + IMPLEMENTATION_PLAN.md, next-phase-2026-10.md}
```

What moves where: the 8 `*_PLAN.md` and `IMPLEMENTATION_PLAN.md` → `docs/history/plans/` unedited; `NEXT_PHASE.md` → `docs/history/next-phase-2026-10.md` (its section 4 checklist becomes `docs/contributing/manual-checks-macos.md`, extended with the acceptance scenario); `VERIFICATION.md` → `docs/verification/macos-2026-10-05.md`; mixed pages split into a user half and an architecture half (`surfaces.md` → `user/surfaces-and-editor.md` + `architecture/surface-format.md`; `context-engine.md` → `user/context-engine.md` + architecture notes; `devices/launchpad-x.md` → `user/launchpad-x.md` + `architecture/launchpad-x-protocol.md`; `plugins.md` → `architecture/plugins.md`; `security/secrets.md` → `architecture/secrets.md`; `actions/{definitions, parameters, providers, availability}.md` → `architecture/actions/`; `actions/{overview, action-library}.md` → `user/action-library.md`; `feedback-editor.md` → `user/`; `agent-control/overview.md` → `user/agent-control.md`; `agent-control/mcp.md` → `roadmap/`); plan references in 5 Rust comments and docs updated to the new paths.

Obsolete claims to remove (only from VERIFICATION.md facts): README:7, :52, :61, :64, :69 ("no CI", "four tests expected to fail"), :213, :221; `docs/context-engine.md:123-127`; `docs/security/secrets.md:102`; `docs/product.md:59`; `macos.rs:12-14`, `windows.rs:16`; `docs/plugin-sdk/README.md:3` and `packaging.md:12` ("same bytes run on Windows"); README "Agent Pack loading", the Agent Pack sections of `docs/agent-control/*` and `docs/roadmap/marketplace.md`; "Marketplace · Preview" in the UI; JSON-editing instructions in user docs (README:136, `surfaces.md` authoring). User docs show UI paths only.

README "Verified where" is rewritten only from `docs/verification/*` and what was run in this milestone (R5's record): macOS row = built and tested here with counts; app window, tray, Keychain, Accessibility, Auto Context running = NOT VERIFIED (f) unless the human pass happened and was recorded; Windows never built (a, e); Linux = earlier milestone, CI not run (a); hardware (b), Home Assistant (c).

## 11. CI and release validation plan

| Where | What | Status |
| --- | --- | --- |
| Here, every package | relevant cargo test targets through the wrapper, fmt, clippy `-D warnings`, `npm run check` or `check:web` | run here |
| Here, R5 | `npm run check:full`; `artifacts_security`, `artifacts_roundtrip`, `release_acceptance` 5 times each; `plugins:validate` on the examples; `build-example-plugins.sh --check` (if npm can fetch); actionlint on all workflows; macOS `tauri build --bundles app,dmg` if ≥ 6 GB free and no other cargo process, then `plutil -p Info.plist`, `codesign -dv`, `hdiutil verify` | run here |
| CI `ci.yml` | Linux on push and PR; macOS and Windows on pull_request and `workflow_dispatch` (Windows `continue-on-error`); steps call the npm scripts (check pieces, build, `rust:test`); Architecture step; Linux extras: real Secret Service test (`scripts/test-secret-service.sh`), SDK contract rebuild (rebuild examples and net-probe with the current SDK, re-run plugin targets, `git diff --stat` informational only) | written, NOT VERIFIED (a) |
| CI `bundle.yml` | `workflow_dispatch` and `v*` tags: Linux deb+rpm (+`dpkg-deb -I/-c`, `rpm -qip`), macOS app+dmg (+`codesign -dv`, `hdiutil verify`), Windows nsis (continue-on-error); artifacts kept 14 days | written, NOT VERIFIED (a) |
| CI `desktop-e2e.yml` | `workflow_dispatch` + weekly cron, Linux: tauri-driver, `npx tauri build --no-bundle`, `tools/desktop-e2e/run.py` incl. the new acceptance suite; screenshots and logs on failure | written, NOT VERIFIED (a, d) |
| A person on this Mac at RC | install the `.dmg` built here; move `~/Library/Application Support/dev.launchdeck.app` aside (never delete); walk the acceptance scenario in the real window; NEXT_PHASE 4.1-4.4 (tray, Auto Context, Keychain, Accessibility); record seen and not seen | NOT VERIFIED (f) until done |
| Cannot be done here | GitHub Actions (a); Launchpad hardware (b); real Home Assistant (c); Linux window and desktop suite (d); Windows (e); Gatekeeper on a downloaded copy (a local build is not quarantined); macOS desktop E2E (no tauri-driver) | stated in the release record |

The headless acceptance test (`tests/release_acceptance.rs`) drives the real runtime with `RuntimeConfig` (temp store, temp plugin folders, in-memory secrets, scripted observer, fake keyboard): clean start, onboarding flag, starter choice, Action Library assignment, feedback rule, Context Engine rule matched, Counter installed through the install API, pad press changes state, export Pack, reset, import without the plugin (dependency missing, pads missing, opted-in rules added, Auto unchanged), install, rescan, recovery, restart from the same folder, everything equal.

## 12. Implementation order

One shared tree, branch `productization`, no worktrees. Packages run in this order; at most two at once, only when their files are disjoint and at most one touches Rust (a pair marked ‖). Cargo for every package: `/private/tmp/claude-501/-Users-lr0y-Movies-launchdeck/de5fb0f6-fd95-4c41-bf94-4c11fa0f3ba8/scratchpad/cargo.sh` run from `src-tauri/`. Packages worth an adversarial review after implementation: A5, A7, D4, D2, C3, C4, C5, E1, E3.

| # | Id | Package | Rust | Depends on | ‖ | Status |
| --- | --- | --- | --- | --- | --- | --- |
| 1 | A1 | Architecture docs and agent entrypoints | no | — | | done |
| 2 | A2 | Quality commands, architecture check, hook, toolchain pin | no | A1 | | done |
| 3 | A3 | Rust boundary fixes: audit, identifiers, platform | yes | A2 | A4 | done |
| 4 | A4 | Frontend structure and boundaries | no | A2 | A3 | done |
| 5 | A5 | Decompose `surfaces/mod.rs` | yes | A3 | B0 | done |
| 6 | B0 | Documentation truth pass | no | A2 | A5 | done |
| 7 | A6 | Split `lib.rs` into `commands/` and `tray.rs` | yes | A3 | R1 | done |
| 8 | R1 | CI workflows within the minutes budget | no | A2 | A6 | done, NOT VERIFIED (a) |
| 9 | A7 | Split `core/runtime.rs` | yes | A3 | | done |
| 10 | B1 | Context Engine hardening and portable resolver | yes | A5, A7 | | done |
| 11 | D1 | Plugin host hardening and `plugins:validate` | yes | A4, A6 | | done |
| 11b | D4 | Legacy plaintext secrets: never used, explicit migration | yes | D1 | | done; real Keychain and the card in the app NOT VERIFIED (f) |
| 12 | D2 | Install bundled example plugins | yes | D1, D4 | | done; the resource path inside a built bundle NOT VERIFIED until R5; the Install button and Show plugins folder in the app NOT VERIFIED (f) |
| 13 | D3 | Reproducible example plugins and author docs | yes | D1 | | done; `--check` reproduced the four modules on macOS only (Linux, Windows, other Node versions NOT VERIFIED (a, d, e)); C Ping not rebuilt (no wasm-capable clang here); the in-app steps of the author flow NOT VERIFIED (f) |
| 14 | C1 | Artifact format core | yes | A6 | | done; tests run here on macOS only (CI NOT VERIFIED (a)); `format` is a directory of private files (`artifacts/format/`); descriptions may break lines (the editor's description is multi-line); a grid control may not reach past 16 columns or rows; every count over a limit is `tooMany`, also in the manifest |
| 15 | C2 | Artifact export | yes | C1 | | todo |
| 16 | C3 | Artifact inspect and hostile fixtures | yes | C2 | | todo |
| 17 | C4 | Provenance, commit primitive, ImportPlan | yes | C3, B1, A7 | | todo |
| 18 | C5 | Apply, service, commands, round trip | yes | C4 | | todo |
| 19 | C6 | Import and export UI | no | C5, A4 | C7 | todo |
| 20 | C7 | Artifact documentation | no | C5 | C6 | todo |
| 21 | E1 | Error UX and structured issues | yes | A4, A6, A7 | | todo |
| 22 | E2 | Device status | yes | A4, A7 | | todo |
| 23 | E3 | Surface management, onboarding flag, load notice | yes | A7 | | todo |
| 24 | E4 | First run and starter content | yes | E2, E3, A5 | | todo |
| 25 | E5 | Settings, About, single version source | yes | E3, D2 | | todo |
| 26 | E6 | Empty states and readable activity | no | A4 | R2 | done |
| 27 | R2 | Release acceptance automation | yes | C5, D2 | E6 | todo |
| 28 | R3 | Documentation restructure and user docs | yes (comments) | A2 | | todo |
| 29 | R4 | README, CHANGELOG, versions, 0.3.0 | yes | A2 | | todo |
| 30 | R5 | Release validation and record | yes | A2 | | todo |

R3, R4 and R5 declare only A2 so that they still run if a feature package fails; they are last in the order and describe only what is in the tree.

## 13. Risk register

| Id | Risk | Likelihood / impact | Mitigation |
| --- | --- | --- | --- |
| R-01 | Disk (8 GB free) fills during builds (extra crates, release LTO build) | medium / high | no new crates beyond locked ones; no dialog plugin; release build only in R5 with ≥ 6 GB free; never `cargo clean`; if < 2 GB after the bundle, remove only `target/release` |
| R-02 | 8 GB RAM and LTO `codegen-units = 1` make the release build slow or fail | medium / medium | build alone; record NOT VERIFIED if it fails |
| R-03 | License contradiction (Cargo/SDK MIT vs README none) | decided | MIT everywhere, root LICENSE, README section (R4) |
| R-04 | Schema 1-3 manifests can still store a `config[].secret` in plain text, against "credential store or nothing" | decided | never handed to a module; explicit Migrate (write, read back, then remove) or Remove; plaintext untouched on any failure (D4) |
| R-05 | CI never ran (billing) | certain / medium | full local gate; workflows linted with actionlint; NOT VERIFIED everywhere; the owner can fix billing or make the repo public |
| R-06 | Runtime/surfaces/lib splits change behaviour | low / high | characterization tests green before and after; fixtures must not change; review of A5, A7 |
| R-07 | Two agents in one tree commit each other's files | medium / medium | path-scoped commits; pairs only with disjoint files and one Rust side; separate rule and map files |
| R-08 | Hostile artifact changes user configuration | low / critical | inspect before typed parsing; plan/apply pure; commit after save only; security suite as release blocker; review C3-C5 |
| R-09 | `<input type=file>` and raw-body IPC behave differently in WKWebView | low / medium | headless tests cover Rust; human pass checks the real picker; fallback: base64 JSON arg |
| R-10 | macOS asks for Downloads access on export; refusal | medium / low | sentence with the System Settings path; reveal button; save dialog deferred |
| R-11 | Bundled resources resolve differently in dev, tests and the bundle | medium / medium | install takes the examples root as a parameter; debug fallback to `examples/plugins`; checked in the R5 bundle |
| R-12 | Rebuilding example plugins needs npm network or changes behaviour | medium / medium | freeze old modules first; dump fixtures diffed; if npm cannot fetch, keep committed modules and record NOT VERIFIED |
| R-13 | Error-message rewording breaks "needed" prompts | medium / low | `Issue.kind` lands in E1 before rewording |
| R-14 | Fixture churn hides regressions | medium / medium | regenerate only through `dump_*` for an intended change, named in the commit; TS tests re-run |
| R-15 | Keychain prompts on rebuilt binaries, Accessibility silence | medium / medium | human checklist; documented in known limitations |
| R-16 | Gatekeeper blocks the ad-hoc signed app downloaded by a tester | high / medium | getting-started explains "Open Anyway"; not verifiable on a local build |
| R-17 | Scope creep beyond rule zero | medium / high | deferred list (section 4); reviewers reject new subsystems |
| R-18 | Tauri CLI warning on the `.app` identifier | certain / low | record only |
| R-19 | SF-1, found by a background security review of `9f70375`: artifact review and the runtime parse `system.launchApplication` differently (empty list arguments; on Windows `cmd /C start` lets `& | < > ^ %` in an unreviewed `app` chain commands) | certain / critical for imported files | fixed in C3 first: one shared launch parser in `plugins::system`, strict review rule, runtime refuses NUL, line breaks and Windows cmd metacharacters; differential tests and hostile fixtures; Windows NOT VERIFIED (e) |
| R-20 | SF-2, found by a background security review of `8f1b25a`: an argument bound to a Pack input kept the exporter's local value in the file (webhook ids, tokens in URLs, home paths) | certain / high | fixed in C3 after SF-1: bound arguments exported as the input default or empty; credential-shaped values refused unless bound; preview names them without values |

## 14. Definition of Done

Who checks: **A** automated here, **CI** only in CI (NOT VERIFIED until it runs (a)), **H** a person on this Mac, **HW** hardware or an external service.

| # | Item | Concrete check | Who |
| --- | --- | --- | --- |
| 1 | Ownership defined | every `lib.rs` module in CODEBASE_MAP, every feature in FRONTEND_MAP; `check:arch` ownership check green | A |
| 2 | Codebase map | maps match the tree (check) and review of rows | A + review |
| 3 | Engineering rules | ENGINEERING_RULES.md R1-R21 present; AGENTS.md links it | A |
| 4 | Agent instructions | AGENTS.md ≤ 120 lines / 8 KiB, CLAUDE.md `@AGENTS.md`, 3 skills with frontmatter, `.agents/skills` symlink | A; skill listing in Claude Code H |
| 5 | Checks active | `npm run check` includes check:arch; hook exits 2 on a sample violation; CI step present | A; CI |
| 6 | Justified refactors | A5, A6, A7 commits are pure moves; characterization tests green; fixtures unchanged | A |
| 7 | Tests | `npm run check:full` green | A |
| 8 | Context Engine complete | `tests/context_engine.rs`, engine unit tests | A |
| 9 | Manual override, Resume Auto | tests; NEXT_PHASE 4.2 in the window | A; H |
| 10 | Edit suspension | `edit_mode_suspends_activation_and_leaving_it_evaluates_once`; window | A; H |
| 11 | Honest platform support | docs table = verification record; observer evidence from the foreground example | A; Windows CI; X11 (d) |
| 12 | User-configurable | `context.test.tsx`; window | A; H |
| 13 | Surface artifact v1 | format tests, golden `.ldsurface` | A |
| 14 | Pack artifact v1 | golden `.ldpack`, contract fixture | A |
| 15 | Transactional import | apply tests incl. stale, blocked, save failure | A |
| 16 | Sanitized export | strip tests; no secret/grant/runtime state in output | A |
| 17 | Dependencies | dependency status tests | A |
| 18 | Inputs | input validation and binding tests | A |
| 19 | Provenance | `importedFrom` round trip; fixture unchanged without it | A |
| 20 | Missing deps preserved and recover | `artifacts_roundtrip`, `release_acceptance` | A |
| 21 | Artifact security tests | `artifacts_security` green 5 times (release blocker) | A |
| 22 | Plugin SDK usable | author-flow page walked here with `plugins:validate`; in-app Install | A; H |
| 23 | Permissions coherent | `plugin_network` tests; docs | A |
| 24 | Secrets secure | `secret_store` tests; D4 tests (no plaintext secret reaches a module, migration failure leaves it untouched); Keychain real store | A; H (4.3) |
| 25 | Network works | `plugin_network`, `home_assistant` (mock) | A; real HA (c) |
| 26 | Reference plugins build and run | `build-example-plugins.sh --check`; plugin tests load them; `plugins:validate` | A (npm network permitting) |
| 27 | Developer docs sufficient | review + `docs:links` | A + review |
| 28 | First run | `firstRun.test.tsx`; window | A; H |
| 29 | Understandable device state | device tests; window; hardware | A; H; HW (b) |
| 30 | Useful starters | surfaces tests + fixture; window | A; H |
| 31 | No JSON for core flows | acceptance steps all through commands the UI calls; user docs grep for JSON-editing instructions empty; window | A; H |
| 32 | Understandable errors | CommandError and toast tests; no raw-text sinks left (grep list in E1) | A |
| 33 | Coherent settings | `settings.test.tsx`; window | A; H |
| 34 | CI green | Actions run | CI (a) |
| 35 | Fast quality command | `npm run check` < ~60 s warm | A |
| 36 | Linux bundle | `bundle.yml` | CI (a) |
| 37 | macOS build | `tauri build --bundles app,dmg` + inspection (R5) | A (disk permitting) |
| 38 | Windows build | `bundle.yml` NSIS | CI (a, e) |
| 39 | Clean-workspace acceptance | `release_acceptance`; window walk-through | A; H |
| 40 | Pack export/import acceptance | `artifacts_roundtrip`, `release_acceptance`; window | A; H |
| 41 | Agent Control tests pass | `agent_control_flows`, `agent_control_golden`, `agent_bridge`, `agents_contract`, lib | A |
| 42 | Action Library tests pass | `action_library`, `catalog_contract` | A |
| 43 | Feedback tests pass | `state_feedback`, `feedback_editor`, `state_hub`, `state_characterization` | A |
| 44 | Context Engine tests pass | `context_engine`, `context_contract` | A |
| 45 | Plugin SDK tests pass | `plugin_sdk`, `plugin_states`, `plugins_contract` | A |
| 46 | Network + Home Assistant mock tests pass | `plugin_network`, `home_assistant`, `secret_store` | A |
| 47 | Docs match behaviour | review; `docs:links`; grep for the obsolete claims of section 10 empty | A + review |
| 48 | Clean tree | `git status` clean after R5 | A |
| 49 | Release candidate identified | R5 record names the commit | A |
| 50 | Ready for an external alpha tester | 1-49 as marked, MIT license in place (R4), and the human pass recorded | H + user |
