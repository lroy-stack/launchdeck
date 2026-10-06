# LaunchDeck: implementation plan

Status: v0.1 prototype plan, kept as written. Agent Control builds on it; see AGENT_CONTROL_PLAN.md. Written after environment inspection and protocol research, before implementation.

> **Where things stand (2026-10-05).** This is the first of eight plans. The milestones built since, in order: Agent Control, the Plugin SDK, the Action Library, state and feedback, the feedback editor, plugin capabilities (network and secrets), and the Context Engine. Each has its own `*_PLAN.md` at the repository root, and the [README](../../../README.md) lists what works today. Several statements below are no longer true and are left as the record of the starting point: the environment did get a display (Xvfb, used by `tools/desktop-e2e`), the architecture has a State Hub, a plugin sandbox and a Context Engine, and the workspace is at schema 4. What is still true: no MIDI hardware has ever been attached, so section 2 remains unverified on a real Launchpad X. The next steps are in [NEXT_PHASE.md](../next-phase-2026-10.md).

## 1. Environment findings

- Node 22.22, npm 10.9, Rust/Cargo 1.97 installed. Root with sudo and apt available.
- WebKitGTK 4.1, GTK3, ayatana appindicator, ALSA dev headers were missing and have been installed via apt, so `cargo check` and `tauri build` can be attempted here.
- No display server and no MIDI hardware in this sandbox. Anything that needs a window or a real Launchpad is validated through the virtual device, unit tests and byte-level protocol tests. Hardware behaviour is documented as untested on real hardware.
- Crates: `midir 0.11` (maintained, cross-platform), `tauri 2.x` (3.0 is alpha, so we stay on 2.x), npm `@tauri-apps/cli` and `@tauri-apps/api` 2.12.

## 2. Protocol research (Launchpad X, official Programmer's Reference Manual)

Confirmed from the manual text:

- SysEx header: `F0 00 20 29 02 0C ... F7`.
- Enter Programmer mode: `F0 00 20 29 02 0C 0E 01 F7` (alternative layout select form: `... 00 7F F7`). Exit: `... 0E 00 F7`.
- LED lighting message: `F0 00 20 29 02 0C 03 <colourspec>... F7`, up to 81 colourspecs. Colourspec types: `00` static (index, palette), `01` flashing (index, colour B, colour A), `02` pulsing (index, palette), `03` RGB (index, r, g, b with 0..127).
- Ports: "LPX MIDI In/Out" (second interface on Windows) is the one that carries Programmer mode input. "LPX DAW In/Out" is for DAW mode.
- Grid indexing is consistent with `note = 10 * row + col`, row 1 at the bottom: lower left 11, upper left 81, lower right 18 (all three appear as examples in the manual).
- In Programmer mode pads and buttons accept either Note or CC for LED control; aftertouch type is configurable (poly, channel, off).
- Programmer mode disables the Setup entry (hold Session).

NOT verifiable from text extraction (the manual uses diagrams):

- Exact CC numbers of top row, side column and logo. Implemented using the layout commonly documented for the MK3 family (top 91..98, side 89..19 downwards, logo 99) and flagged as unverified in code and docs. Auxiliary controls are therefore marked experimental.
- The 128 entry palette (index to colour). Not invented.

Design consequence: static colours always use RGB colourspecs (type 03), which need no palette. Pulse and flash are produced by the renderer as timed RGB frames (software animation strategy). The protocol encoder also implements hardware flashing and pulsing colourspecs, covered by byte-level tests, but they are not used by default until palette indexes are verified on hardware.

## 3. Product assumptions

- The runtime is Rust. React is editor, configuration UI and visualizer only.
- 8x8 grid plus optional auxiliary controls. The core uses control ids and a device capability model, never hardcoded 8x8 maths outside the Launchpad adapter.
- Prototype scope favours a convincing demo path: AI Workspace demo, Development surface, Accessibility surface.
- Shell commands are privileged. They are always shown verbatim in the inspector, only run when a user configured them, and are never reachable from the WebSocket.

## 4. Architecture

```
React UI (Zustand: UI state only)
   | Tauri commands / events
LaunchDeck Core (Rust)
   |- DeviceManager       owns SurfaceDevice instances, input channel
   |- SurfaceRuntime      active surface/page, control lookup, navigation
   |- StateEngine         per-control visual state, bindings, event routing
   |- EventBus            semantic events (tokio broadcast)
   |- ActionEngine        executes semantic actions
   |- Renderer            semantic visual state -> SurfaceFrame (capability aware)
   |- PluginRuntime       registry of plugins (System, Demo)
   |- Persistence         versioned JSON in the app data dir
   |- ExternalEvents      WebSocket on 127.0.0.1 only
Devices: VirtualLaunchpadX, LaunchpadX (midir + SysEx encoder)
```

Key rules: plugins publish semantic actions and states and never see MIDI or coordinates; surfaces bind controls to actions and states; only the device adapter encodes protocol bytes.

## 5. Scope

In: all items in the Definition of Done. Out: marketplace backend, auth, sync, telemetry, full MCP server, other Launchpad models, databases.

## 6. Technical risks

| Risk | Mitigation |
| --- | --- |
| No hardware or display in sandbox | Virtual device, protocol byte tests, document hardware as untested |
| Palette and aux CC numbers unverifiable from text | RGB-only static colours, software animation, aux flagged experimental |
| Tauri Linux build needs system libs | Installed via apt, blockers documented if the build still fails |
| Shell and HTTP actions are a security surface | Explicit config, localhost-only inbound, no remote execution path |
| Keyboard shortcut and volume actions are OS specific | Implement behind a small trait, degrade with a clear error |

## 7. Phases

1. Scaffold (Tauri 2, Vite, React, TS, Zustand, Zod, Vitest, ESLint).
2. Core model, event bus, state engine, renderer, persistence, tests.
3. Device layer: trait, capabilities, virtual device, Launchpad X protocol and midir adapter, tests.
4. Actions, System and Demo plugins, WebSocket events, tests.
5. UI: shell, surface editor, inspector, Live/Edit, onboarding, starter surfaces, agent demo.
6. Docs and examples, tray.
7. Final validation and fixes.

## 8. Acceptance criteria

The 30 Definition of Done items. Verification: `npm run typecheck`, `npm run lint`, `npm test`, `npm run build`, `cargo fmt --check`, `cargo clippy`, `cargo test`, `cargo tauri build` (or documented blocker).
