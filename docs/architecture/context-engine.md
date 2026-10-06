# Context Engine

How Auto Context is built. What a person sees and does is in [Auto Context](../user/context-engine.md); the research and decisions behind it are in the [Context Engine plan](../history/plans/CONTEXT_ENGINE_PLAN.md).

```text
Foreground observer (per system)     which application is in front
        ↓
Application identity                  { namespace, id, displayName }
        ↓
Ordered rules                         first match wins, then the fallback
        ↓
Auto / Manual Override                is this a moment to apply it?
        ↓
The runtime's own activation          the same one the sidebar uses
        ↓
Renderer and devices
```

The engine never talks to a device. Its only output is `Runtime::activate`, the activation every manual switch already goes through, so the events, the State Hub, the virtual Launchpad and a physical one all follow exactly as they do on a click. What Auto shows is not saved as `lastActiveSurface` (a person's choice is), so the workspace file is not rewritten at every switch of application: a save measured about 6 ms on macOS, above the 5 ms the productization plan allowed ([record](../verification/macos-2026-10-05.md#persist-cost-of-a-context-driven-activation)).

## Rules

A rule is one application identity and one target, saved in the workspace's `context`:

```jsonc
{ "application": { "namespace": "x11", "id": "Code", "displayName": "Visual Studio Code" },
  "surfaceId": "development",
  "pageId": "git" }          // optional
```

Ordered, first match wins; at most 64; no conditions beyond the application. A rule is checked when it is saved (its surface and page exist, its application has an identity). A rule whose surface no longer exists still wins for its application and shows nothing, so a broken rule never hands over to a lower one silently. The fallback is `keep` (default) or a surface and optional page.

## Identity

| System | Namespace | `id` | Example |
| --- | --- | --- | --- |
| macOS | `macos` | Bundle identifier | `com.microsoft.VSCode` |
| Windows | `windows` | Executable file name, lower case | `code.exe` |
| Linux, X11 | `x11` | The class of `WM_CLASS` | `Code` |

Two identities are equal when the namespace is the same and the id is equal ignoring case; the display name is never compared. On Windows the file name is used, not the path, which changes with every update of many applications. An application with no native identity (a macOS process without a bundle, an X window without `WM_CLASS`) is "nothing in front" and cannot have a rule. LaunchDeck's own window is never an application: the application the rules are asked about stays the last one that was.

## When a rule is applied

At five moments only: a different application accepted after it has been in front for 150 ms; Resume Auto; Auto Context switched on; Edit left for Live; a rule or the fallback saved. A manual activation that changes what is shown is a Manual Override, held until a different application is accepted or Resume Auto. While a surface is edited nothing is applied, and the observer keeps running. The state machine (`context/engine.rs`) is pure and is given the time; it has no clock and no thread.

## Observers

| System | How | Status |
| --- | --- | --- |
| Linux, X11 with an EWMH window manager | `_NET_ACTIVE_WINDOW`, `PropertyNotify` events, no polling (`context/observer/x11.rs`) | Run on a real X server with openbox in an earlier milestone (`tools/desktop-e2e`, suite `context`); not run for 0.3.0 (no Linux desktop here, CI does not run) |
| macOS | `NSWorkspace` activation notifications, and a read every 2 s as a safety net (`context/observer/macos.rs`) | Compiled and clippy-clean on macOS 15.4.1, Apple M2, Rust 1.97.0. Seen reporting the application in front by bundle id through `src-tauri/examples/foreground.rs`, outside the app ([record](../verification/macos-2026-10-05.md#productization-macos-observer-evidence)). In the running app: NOT VERIFIED, the manual check has not been done |
| Windows | `SetWinEventHook(EVENT_SYSTEM_FOREGROUND)`, and a read every 2 s (`context/observer/windows.rs`) | Written, never compiled: NOT VERIFIED, no Windows machine and CI does not run |
| Linux, Wayland | none | Not available: Wayland gives applications no way to learn which application has focus. wlroots compositors have a protocol that is not implemented; GNOME would need a Shell extension and KDE a KWin script |
| Linux, X11 without an EWMH window manager | none | Not available; checked once at start |

The macOS and Windows adapters share the identity code, which is tested on every system. The observer runs whenever LaunchDeck does and the system allows it, including with Auto Context off, so that the rule picker has something to offer; it holds one identity at a time on its own thread (`foreground-observer`). `cargo run --example foreground` (from `src-tauri/`) prints identities as applications come to the front, running the observer as the app does.

## Privacy

Saved: whether Auto Context is on, the rules and the fallback. In memory only: the application in front, the last one accepted, the up to 12 applications offered when adding a rule, the last decision. Never read: window titles or contents, URLs, documents, the list of running or installed applications. Never written or sent: any history, timeline, duration or count of use, in any file, log or the audit trail. Plugins cannot read the context, add a rule or activate a surface through the engine; the State Hub gets one word, `context.mode` (`auto`, `manual`, `paused`, `off`, `unavailable`).

## Where it is

| Piece | Where |
| --- | --- |
| Identity | `src-tauri/src/context/identity.rs` |
| Rules and their evaluation (pure) | `context/rules.rs` |
| The state machine (pure; time is passed in) | `context/engine.rs` |
| Observers | `context/observer/`: `x11.rs`, `macos.rs`, `windows.rs`, and `scripted.rs` for tests |
| Rules in a shared file, resolved for this system | `context/portable.rs` |
| What the interface is told | `context/status.rs`; Tauri event `context-changed`; commands `context_status`, `context_update`, `context_resume` |
| Runtime integration | `core/runtime/navigation.rs`: `activate()` with an origin; `core/runtime/context.rs`: `on_foreground`, `settle_context` |
| Interface | `src/features/context/` |

Tests: the engine as a state machine (`cargo test --lib context::`), the runtime with a scripted observer and the virtual Launchpad (`cargo test --test context_engine`), the status contract (`examples/context-status.json`, `cargo test --test context_contract`), the interface (`npm test`), and the real observer on a real X server (`python3 tools/desktop-e2e/run.py context`).

Measured on Linux/X11 in an earlier milestone: the surface is on screen about 210 ms after the window manager is asked to activate a window (release build; 215 to 275 ms in a debug build), of which 150 ms is the stability wait. The observer uses no CPU while nothing changes (0 clock ticks in 10 s) and about 0.4 ms of CPU per change of application; evaluating 64 rules takes about 1.5 microseconds (both in a debug build).

## Not built

Window-title matching, the current project or Git repository, agent or plugin state as context, time, network or location, conditions combined with AND or OR, scripts, context provided by plugins, any history of use, and per-compositor Wayland support.
