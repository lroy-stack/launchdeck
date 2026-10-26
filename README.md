# LaunchDeck — a Novation Launchpad as a programmable control surface

Desktop app, started October 2026. Version 0.3.0, a first external alpha. No installer is published yet and there
is no live demo: you build it from source.

## What it is

LaunchDeck turns a Novation Launchpad into a control surface for your computer. Pads open applications, press
shortcuts, send requests and switch pages, and they light up to show state. A simulated Launchpad is built in, so
everything works without hardware.

## What it does

- Surfaces, pages and pads, with a Live mode and an Edit mode and an inspector per pad.
- Action Library: built-in, Agent Control and plugin actions in one searchable list.
- Feedback editor: a pad follows a state or reacts to an event.
- Auto Context: the surface follows the application in front, by your rules.
- Agent Control: shows what AI agents are doing on the pads and lets you approve or reject a request with one
  press. Claude Code and Codex hooks and a local Agent Bridge for any program. Requests are never approved
  automatically.
- Plugins: sandboxed WebAssembly, network access only where you allow it, secrets in the system credential store.
- Export and import of surfaces and packs, with a preview before anything is added.
- Launchpad X and Launchpad Mini MK3 support over USB.

## Status, stated plainly

Built and tested on macOS (Apple M2) only. The Launchpad hardware, Windows, Linux for this release, a real Home
Assistant and the app window itself have not been verified; CI has not run. The detailed matrix is in
`docs/verification/` and the limits are in `docs/user/known-limitations.md`.

## Built with

Tauri 2, Rust (pinned to 1.97.0 by `rust-toolchain.toml`), React 19, TypeScript, Vite, zustand, zod, Wasmtime for
the plugin sandbox, AssemblyScript for the plugin SDK.

## Run it locally

Requires Node (^20.19, ^22.13 or 24+), Rust, and the
[Tauri 2 prerequisites](https://v2.tauri.app/start/prerequisites/) for your system.

```bash
npm ci
npm run tauri dev                                              # the app, with the simulator
LAUNCHDECK_PLUGINS="$PWD/examples/plugins" npm run tauri dev   # the same, with the example plugins
npm run check                                                  # types, lint, architecture rules, tests, cargo fmt and clippy
```

## Layout

```
src/          React interface
src-tauri/    Rust runtime (MIDI, plugins, agent bridge, context)
sdk/          plugin SDK (AssemblyScript)
examples/     example plugins, artifacts and a generic agent client
docs/         user guides, architecture, plugin SDK, verification records
scripts/      checks and build helpers
tools/        desktop end-to-end tooling
```

MIT licence.

---

Versión en español: app de escritorio (Tauri + Rust + React) que convierte un Launchpad de Novation en una
superficie de control programable, con simulador incluido. Es una alpha 0.3.0 probada solo en macOS y sin
instalador todavía.
