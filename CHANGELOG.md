# Changelog

Each release of LaunchDeck, grouped by capability. The format follows [Keep a Changelog 1.1.0](https://keepachangelog.com/en/1.1.0/). The app's version is plain numeric; the channel says what kind of release it is ([versions](docs/architecture/versions.md)).

"Run here" means run on macOS 15.4.1, Apple M2, in this milestone. **NOT VERIFIED** gives its reason by letter: (a) GitHub Actions does not start (account billing); (b) no Novation Launchpad (X or Mini MK3) attached; (c) no Home Assistant reachable; (d) no Linux desktop; (e) no Windows machine; (f) the [manual checks on the Mac](docs/history/next-phase-2026-10.md#4-check-by-hand-on-the-mac) not done by a person, and the app window never driven by an agent.

## [Unreleased] — 0.3.0, first external alpha

The first build meant for people outside the project. The release date and the release-candidate commit are added when the release is validated.

### Surfaces and editor
- Rename and delete surfaces and pages. Deleting a surface first lists the Auto Context rules that point to it; the last surface or page cannot be deleted. Run here in tests; the window NOT VERIFIED (f).
- A change is saved before it is shown: when saving fails, the surface on screen stays as it was.
- When the saved surfaces cannot be read, LaunchDeck says so in a banner, keeps the file as `workspace.json.bak` and offers to show the folder. Run here in tests; the banner in the window NOT VERIFIED (f).
- Every error is a sentence, with the system's own text behind **Details**. Empty pages, missing plugins and the activity list say what to do next and use names, not ids.

### Simulator, Launchpad X and Launchpad Mini MK3
- Launchpad Mini MK3 support: found on its own, identified by its answer, taken into Programmer Mode and given back as it was found, on quit too, with its own pulse and flash. A virtual Mini MK3 with the hardware's lighting is used in tests. Supported in code; NOT HARDWARE VERIFIED (b).
- The Launchpad X runs on the same Novation layer as the Mini: Programmer Mode is read back, a unit is identified before it is taken, keeps a stable id across a replug, and is taken back after a power cycle or a takeover. Byte-tested; NOT HARDWARE VERIFIED (b).
- Brightness, sleep and a scrolling test text for both models in **Settings, Devices**. Run here in tests; the window NOT VERIFIED (f), the units NOT HARDWARE VERIFIED (b).
- Device status in words: connected, available, unplugged (it stays listed until it returns), not supported, or failed with the reason. Run here with simulated ports; real units NOT HARDWARE VERIFIED (b).
- The port names each system gives a unit are derived from documents, not observed on any system (b).

### Action Library
- Every provider built into the app reports the app's version.
- The editor's form and Rust agree on every rejected value; a Rust-written fixture (`examples/param-cases.json`) holds the two together.
- A missing setting is named by its kind, not matched by its wording.

### Feedback editor and State Hub
- No change of behaviour. The editor's refusals are sentences, with ids under **Details**.

### Agent Control
- The unused "Agent Pack" manifest is removed; Pack now means a shared file (below).
- Agent notifications moved to **Settings, Notifications**.
- Claude Code hooks were verified live in an earlier milestone; Codex hooks are written, not run.

### Plugins and the SDK
- The example plugins Hello, Counter and Home Assistant ship with the app and install from the **Plugins** screen, which also shows the plugins folder. Run here in tests; the button in the window and the files inside a built bundle NOT VERIFIED (f).
- A plugin that stops says what kind of problem stopped it, in a sentence. `npm run plugins:validate` checks a plugin folder with the app's own validator. Plugin ids starting with `launchdeck.` are reserved.
- A secret that a schema 1-3 plugin kept in plain text is never handed to the plugin again. Its card offers **Migrate** (into the credential store, read back, then removed from the file) or **Remove**. Run here with an in-memory store; the real Keychain NOT VERIFIED (f).
- The example plugins rebuild to the committed bytes (`examples/plugins/SHA256SUMS`, `scripts/build-example-plugins.sh --check`). Run here on macOS with Node 22 only; Linux, Windows and other Node versions NOT VERIFIED (a, d, e). C Ping is not rebuilt here (it needs a clang with wasm32).
- Plugin SDK 0.2.1, ABI 1 and manifest schemas 1 to 4 are unchanged.

### Network and secrets
- The built-in HTTP request follows no redirect, refuses addresses nothing can connect to (such as 0.0.0.0, broadcast and multicast), and goes only where its host name says.
- Secrets stay in the system credential store, or are unavailable; there is no file fallback. The macOS Keychain NOT VERIFIED (f).

### Context Engine
- The macOS observer was seen reporting the application in front by its bundle id (`src-tauri/examples/foreground.rs`, run here). Auto Context in the app window NOT VERIFIED (f).
- A switch made by Auto Context is no longer saved; a person's choice is. After a restart LaunchDeck opens on the surface last chosen.
- New workspaces no longer carry the unused context hint.
- Wayland is not supported, and LaunchDeck says so.

### First run and settings
- A one-screen first run: your Launchpad (or the simulator), what the pads are for (Productivity, Agent Control, Development, Accessibility, Build my own), Start, Skip, or Import a Pack instead. It never runs the demo, removes starters or creates rules.
- Starters: Home (Home Assistant pads that need the plugin before they send anything), Development, Agent Control, Productivity and Accessibility. The sidebar's overview, which was also called Home, is now **Overview**, and the Marketplace placeholder is gone.
- **Settings** in sections: Devices, Auto Context, Notifications, Plugins & permissions, Security, Workspace, Developer, About. About shows every version and copies them for a bug report.
- All of this run here in Vitest; the window NOT VERIFIED (f).

### Surface files and Packs
- Export a surface (`.ldsurface`) or a Pack of surfaces with their Auto Context rules (`.ldpack`) to Downloads. The same workspace gives the same bytes; no secret, grant or runtime state is written, and values that look like credentials are refused unless they become a question for the person importing.
- Import shows a preview built by Rust: what will be added, every program it would launch and every request it would send (shown exactly, on every system), missing plugins, and questions to answer. Nothing is applied until you press Import, and nothing runs on import.
- Hostile files (wrong formats, renamed archives, oversized or deeply nested content, hidden characters, spoofed addresses) are refused with the workspace unchanged (`src-tauri/tests/artifacts_security.rs`). Run here; the file picker and the Downloads prompt in the window NOT VERIFIED (f). Windows-only refusals are tested by passing the system as a parameter; on Windows NOT VERIFIED (e). [Format](docs/artifacts/FORMAT.md), [security](docs/artifacts/security.md).

### Quality and CI
- `npm run check` (types, lint, architecture, versions and license, documentation links, Vitest, cargo fmt and clippy) and `npm run check:full` (with every Rust test and the build); Rust pinned to 1.97.0.
- The large Rust modules are split by responsibility, and the architecture rules are checked on every commit.
- `src-tauri/tests/release_acceptance.rs` runs the release acceptance scenario headless, from a clean data folder to a restart. Run here, 20 times green.
- CI workflows for Linux, macOS and Windows, for bundles and for the desktop suite: written, NOT VERIFIED (a). The desktop acceptance suite for Linux is written, not run (a, d).
- Test counts: the whole Rust suite last recorded green on 2026-10-05 (961 passed, 0 failed, 4 ignored, before the Mini MK3 and acceptance work merged); Vitest 594 passed at 0.3.0. The release candidate's own counts go into [docs/verification/](docs/verification/).

### Versions

| | 0.3.0 |
| --- | --- |
| App | 0.3.0, alpha |
| Plugin SDK | 0.2.1 |
| Plugin ABI | 1 |
| Plugin manifest schemas | 1 to 4 |
| Workspace schema | 4 |
| Agent Bridge protocol | 1 |
| Surface and Pack format | 1 |

### Known limitations

See [known limitations](README.md#known-limitations) and [Verified where](README.md#verified-where) in the README.

### License

MIT, decided by the owner on 2026-10-05: [LICENSE](LICENSE), with the same declaration in `src-tauri/Cargo.toml`, the bundle metadata in `src-tauri/tauri.conf.json`, the root `package.json`, the SDK and the example plugins. `npm run versions:check` holds them together.

## 0.2.0 — 2026-10-04

Internal prototype, not released.
