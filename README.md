# LaunchDeck

LaunchDeck turns a Novation Launchpad into a programmable control surface for your computer. Pads open applications, press shortcuts, send requests and switch pages. They light up to show the state of things, on your computer or outside it through plugins you allow. The surface can follow the application in front, and Agent Control puts AI agents on the pads, so you can see what each one is doing and approve or reject a request with one press. A simulated Launchpad is built in, so all of it works without hardware.

**Status: 0.3.0, the first external alpha.** Built and tested on macOS 15.4.1 (Apple M2). Nobody has yet walked through the app window, a real Launchpad, Windows, a Linux desktop or a real Home Assistant for this release: see [Verified where](#verified-where) and the [known limitations](#known-limitations). License: [MIT](LICENSE).

## Install

There is no installer for 0.3.0 yet; it is added when the release is validated. Until then, build it from source. You need Node ^20.19, ^22.13 or 24+, Rust 1.97.0 (pinned by `rust-toolchain.toml`) and the [Tauri 2 prerequisites](https://v2.tauri.app/start/prerequisites/) for your system (on macOS, the Xcode Command Line Tools are enough).

```bash
npm install
npm run tauri dev                                              # the app, with the simulator
LAUNCHDECK_PLUGINS="$PWD/examples/plugins" npm run tauri dev   # the same, with the example plugins
```

On macOS LaunchDeck keeps its data in `~/Library/Application Support/dev.launchdeck.app/`.

## Try it in two minutes, without a Launchpad

1. Start LaunchDeck. The first-run screen shows the simulator as your Launchpad and asks what the pads are for. Choose **Development** and press **Start with Development**.
2. Click the pads on screen. They take the same path a press on a physical Launchpad takes, and light up the same way.
3. Switch the surface to **Edit**, select an empty pad and give it an action from the **Action Library**. Back in **Live**, press it.
4. Open **Agents** and press **Run Agent Demo**. Four simulated agents start; when APPROVE flashes, press it.

## What works

| Capability | In this release | Guide |
| --- | --- | --- |
| Surfaces, pages and pads; Live and Edit; an inspector for each pad; rename and delete surfaces and pages | Built and tested | [Surfaces and the editor](docs/user/surfaces-and-editor.md) |
| Simulator (a virtual Launchpad X) | Built and tested; always connected | [Surfaces and the editor](docs/user/surfaces-and-editor.md) |
| Launchpad X and Launchpad Mini MK3 over USB, with brightness, sleep and a scrolling test text in **Settings, Devices** | Supported in code and tested against simulated units; **NOT HARDWARE VERIFIED** (b) | [Launchpad X](docs/user/launchpad-x.md), [Launchpad Mini MK3](docs/user/launchpad-mini-mk3.md) |
| Action Library: every built-in, Agent Control and plugin action in one searchable list, set up in a form | Built and tested | [Action Library](docs/user/action-library.md) |
| Feedback editor: a pad follows a state or reacts to an event, with looks made in place | Built and tested | [Feedback editor](docs/user/feedback-editor.md) |
| Auto Context: the surface follows the application in front, by your rules, with a manual override | Built and tested; the macOS observer seen reporting applications outside the app; in the app window **NOT VERIFIED** (f) | [Auto Context](docs/user/context-engine.md) |
| Agent Control: demo agents, Claude Code and Codex hooks, and the Agent Bridge for any local program | Built and tested; Claude Code hooks verified live in an earlier milestone; Codex hooks written, not run | [Agent Control](docs/user/agent-control.md) |
| Plugins: sandboxed WebAssembly, examples installed from the Plugins screen, network access only where you allow it, secrets in the system credential store | Built and tested; the Keychain itself **NOT VERIFIED** (f) | [Plugins](docs/user/plugins.md), [Plugin SDK](docs/plugin-sdk/README.md) |
| Home Assistant example plugin and the Home starter | Tested against a stand-in server only; a real Home Assistant **NOT VERIFIED** (c) | [Home Assistant](examples/plugins/home-assistant/README.md) |
| Sharing: export a surface or a Pack to a file, import one with a preview of everything it would add | Built and tested | [Sharing surfaces](docs/user/sharing.md) |
| First run, Settings in sections, About with every version | Built and tested | [Versions](docs/architecture/versions.md) |

"Built and tested" means built and run in automated tests on macOS in this milestone, as recorded below. It does not mean a person has used it in the window yet.

## Verified where

From the [macOS verification record](docs/verification/macos-2026-10-05.md) and what the packages of this milestone recorded as run. Letters give the reason, listed under the table.

| | macOS 15.4.1, Apple M2 | Linux | Windows |
| --- | --- | --- | --- |
| Rust build, fmt, clippy `-D warnings` (Rust 1.97.0) | Run here, clean, at 0.3.0 | Not built for 0.3.0 (a, d); clean in an earlier milestone | NOT VERIFIED (e): never built |
| Rust tests | Whole suite last recorded green on 2026-10-05: 961 passed, 0 failed, 4 ignored (commit `e24e4f3`, before the Mini MK3 and acceptance work merged); the contract tests run here at 0.3.0 | 712 passed in an earlier milestone ([record](docs/history/next-phase-2026-10.md#0-where-things-stand)); not run for 0.3.0 (a, d) | NOT VERIFIED (e) |
| TypeScript typecheck, lint, tests | Run here at 0.3.0: 594 tests passed | 309 tests in an earlier milestone; not run for 0.3.0 (a, d) | NOT VERIFIED (e) |
| Release acceptance scenario, headless (`src-tauri/tests/release_acceptance.rs`) | Run here, 20 times green | NOT VERIFIED (a, d) | NOT VERIFIED (a, e) |
| App window, tray, Auto Context in the app, Keychain, keys pressed for you (Accessibility) | NOT VERIFIED (f): the window was started once and seen by the owner, nothing checked | Desktop suite 218 of 218 in an earlier milestone; the new acceptance suite written, not run (a, d) | NOT VERIFIED (e) |
| Auto Context observer on its own | Seen reporting applications by bundle id ([record](docs/verification/macos-2026-10-05.md#productization-macos-observer-evidence)) | X11 in an earlier milestone; Wayland not supported | NOT VERIFIED (e): never compiled |
| Launchpad X, Launchpad Mini MK3 | NOT HARDWARE VERIFIED (b) | NOT HARDWARE VERIFIED (b) | NOT HARDWARE VERIFIED (b) |
| Home Assistant | Stand-in server only; a real one NOT VERIFIED (c) | Stand-in server only, earlier milestone | NOT VERIFIED (e) |
| CI (`.github/workflows`) | NOT VERIFIED (a) | NOT VERIFIED (a) | NOT VERIFIED (a) |

(a) GitHub Actions does not start: account billing. (b) No Novation Launchpad (X or Mini MK3) attached; the [hardware test](docs/contributing/hardware-testing.md) has never run on a unit. (c) No Home Assistant reachable. (d) No Linux desktop here. (e) No Windows machine. (f) The [checks a person does on the Mac](docs/history/next-phase-2026-10.md#4-check-by-hand-on-the-mac) (sections 4.1 to 4.6) have not been done, and no agent drove the app window.

## Security in short

* **Nothing arriving over the network can start an action.** The local WebSocket binds to `127.0.0.1` and accepts events and the Agent Bridge only; events change how pads look, never what runs.
* **A permission request is never approved automatically.** Only a person presses APPROVE, and agents are read-only until you allow control.
* **Plugins are sandboxed** WebAssembly with no files, shell, sockets or MIDI. A plugin reaches the network only through LaunchDeck, only where you allowed, and never sees a secret's value ([plugin security](docs/plugin-sdk/security.md), [where secrets are kept](docs/architecture/secrets.md)).
* **A shared file is reviewed before anything is added**, and importing it runs nothing ([artifact security](docs/artifacts/security.md)).
* **No telemetry, no cloud, no silent installs.**

## Known limitations

* **Alpha, macOS first.** Windows has never been built, and nothing in 0.3.0 has been built on Linux. CI exists but has not run (a).
* **No Launchpad has been attached.** Both models follow Novation's manuals and are byte-tested against simulated units; the port names each system gives a unit are derived from documents, not observed.
* **Hook adapters can only approve and reject.** Codex has never been run. The Agent Bridge is not authenticated: any local program can register an agent, read-only until you allow control.
* **Plugins are not signed**, and their one capability is HTTP to addresses you allowed. Outside state is polled, not pushed. Install only plugins you trust.
* **Secrets need a system credential store**; without one they are unavailable, never written to a file.
* **Auto Context matches applications only** (no window titles), and Wayland is not supported.
* **Keys pressed for you** need Accessibility permission on macOS.

## Documentation

* **Using LaunchDeck**: the user guides in [docs/user/](docs/user/), starting with [surfaces and the editor](docs/user/surfaces-and-editor.md).
* **Writing plugins**: the [Plugin SDK](docs/plugin-sdk/README.md) and the [example plugins](examples/plugins/README.md).
* **Connecting agents**: the [bridge protocol](docs/agent-control/bridge-protocol.md), the [adapter SDK](docs/agent-control/adapter-sdk.md) and the [example client](examples/generic-agent/README.md).
* **Sharing files**: the [Surface and Pack format](docs/artifacts/README.md).
* **How it is built**: the [architecture overview](docs/architecture/overview.md), the [codebase map](docs/architecture/CODEBASE_MAP.md), the [engineering rules](docs/architecture/ENGINEERING_RULES.md) and the [versions](docs/architecture/versions.md).
* **What was run, and where**: the [verification records](docs/verification/) and the [changes in each release](CHANGELOG.md).
* **History**: the plans and records of earlier milestones in [docs/history/](docs/history/).

## Contributing

Run `npm run check` before you send a change: types, lint, the architecture rules, versions and license, documentation links, the TypeScript tests, and the Rust format and lints. `npm run check:full` adds every Rust test and the build. [AGENTS.md](AGENTS.md) says where code goes.

## License

LaunchDeck is released under the [MIT License](LICENSE). The plugin SDK and the example plugins are MIT too.
