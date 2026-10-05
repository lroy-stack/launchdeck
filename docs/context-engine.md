# Context Engine: the surface follows the application in front

With **Auto Context** on, LaunchDeck shows the surface you chose for the application you are using: your editor comes to the front and the Development surface is on the pads; the terminal comes to the front and it is another one. You write one short rule per application. Nothing else decides: no window titles, no project, no time of day, no script.

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

The engine never talks to a device. Its only output is the activation every manual switch already goes through, so the saved "last surface", the events, the State Hub, the virtual Launchpad and a physical one all follow exactly as they do when you click.

## Using it

**Settings, Auto Context.**

1. Bring the application you want a rule for to the front once, then return to LaunchDeck. It is now in the list under *Add a rule* ("Applications seen in front since LaunchDeck started").
2. Pick it, pick the surface (and a page, or *Any page*), **Add rule**.
3. Switch **Auto Context** to **On**, there or with the switch in the sidebar.

The same panel shows, at all times: whether the observer works and how, which application is in front, which rule matched, and what was done about it, in a sentence:

> Rule 1 matches Visual Studio Code: Development / Git was activated.

## Rules

A rule is one application and one target.

```jsonc
{ "application": { "namespace": "x11", "id": "Code", "displayName": "Visual Studio Code" },
  "surfaceId": "development",
  "pageId": "git" }          // optional
```

* **Ordered, first match wins.** The list is numbered and reordered with ↑ and ↓. Order is the only precedence there is.
* **A page is optional.** Without one, the surface opens on its first page, or keeps the page it shows if it is already on screen.
* Up to 64 rules. No conditions beyond the application, no AND or OR, no nesting, no expressions.
* A rule is checked when it is saved: its surface and page must exist, and its application must have an identity. The error names the rule.

What the list flags:

| Flag | Meaning |
| --- | --- |
| *Never reached* | An earlier rule names the same application. |
| *Its surface no longer exists* | The rule **still wins** for its application and shows nothing: a broken rule does not silently hand over to a lower one. Choose another surface or remove it. |
| *Its page no longer exists* | The surface opens on its first page. |
| *Written on another kind of system* | It names a macOS bundle id on Linux, for example. It never matches here and does no harm. |

## The fallback

What happens for an application that no rule names is always one of two things, chosen under *When no rule matches*:

| Choice | Effect |
| --- | --- |
| **Keep the current surface** (default) | Nothing changes. |
| **Open *a surface*** (and optionally a page) | Every application without a rule shows it. |

Keep is the default because it cannot surprise: switching Auto Context on with one rule changes one thing.

## Application identity

Rules name applications by what the operating system itself uses to tell them apart, never by the name on screen.

| System | Namespace | `id` | Example |
| --- | --- | --- | --- |
| macOS | `macos` | Bundle identifier | `com.microsoft.VSCode` |
| Windows | `windows` | Executable file name, lower case | `code.exe` |
| Linux, X11 | `x11` | The class of `WM_CLASS` | `Code` |

* Two identities are the same application when the namespace is the same and the id is equal ignoring case. The **display name is never compared**: an application that renames itself, or whose name is localized, still matches.
* On Windows the *file name* is used, not the path: the folder changes with every update of many applications.
* An application with no native identity (a macOS process without a bundle, an X window without `WM_CLASS`) is treated as "nothing in front" and cannot have a rule.
* You never type an identity. To see what one is: Settings shows it next to the application's name, and `cargo run --example foreground` prints it as applications come to the front.

## When a rule is applied

At exactly five moments, and never in between:

1. a **different application** comes to the front and stays there;
2. you press **Resume Auto**;
3. you switch Auto Context **on**;
4. you go from **Edit to Live**;
5. you **save a rule or the fallback** (so a rule for the application you were just using takes effect at once).

### Stability

An application counts once it has been in front for **150 ms**. One that flashes past for less (a splash screen, a focus bounce while switching windows) never moves the surface, and a real switch is applied 150 ms after the system reports it. The same application reported again, and a rule whose target is already on screen, do nothing at all.

### LaunchDeck's own window

LaunchDeck in front is **not an application**. The application the rules are asked about stays the last one that was. So opening LaunchDeck to look, to edit a rule or to press a virtual pad switches nothing, and there is no loop to worry about. The same holds for "nothing in front" (the desktop on X11, a window that says nothing about itself): the surface is never reset.

## Manual override

Auto Context does not fight you.

* When you activate a surface or a page yourself (the sidebar, the tray, a navigation pad, a top-row button) and that changes what is shown, the sidebar reads **Manual override**.
* It holds for as long as the same application stays in front, however long that is, and across a visit to the LaunchDeck window.
* It ends when a **different application** comes to the front (that application's rule, or the fallback, then applies), or when you press **Resume Auto** (the current application's rule applies at once).
* Opening what is already shown is not an override.

One rule to remember: *the application that comes to the front decides; until then, your hand does.* To keep a surface regardless of what you use, switch Auto Context off.

## Edit mode

While you edit a surface, nothing is switched automatically, and the sidebar reads **Paused while editing**. Choosing which surface to edit is not a manual override. The observer keeps running, so the panel still shows what is in front and what its rule *would* do.

Going back to **Live** applies the current application's rule once (unless a manual override from before is still active). If you just edited a surface and want to try it while another application has a rule, click it in the sidebar: that is a manual override and holds.

## Platform support

| Environment | Auto Context | How | Tested here |
| --- | --- | --- | --- |
| Linux, X11, with a window manager | **Available** | The window manager's `_NET_ACTIVE_WINDOW` (EWMH); `PropertyNotify` events, no polling | Yes, on Linux in an earlier milestone: real X server, openbox, real windows (`tools/desktop-e2e`, suite `context`) |
| macOS | Compiled; running **NOT VERIFIED** | `NSWorkspace` activation notifications; a read every 2 s as a safety net | Compiled and clippy-clean; not seen running |
| Windows | Written, **never compiled** | `SetWinEventHook(EVENT_SYSTEM_FOREGROUND)`; a read every 2 s as a safety net | No |
| Linux, Wayland (any compositor) | **Not available** | Wayland gives applications no way to learn which application has focus. Per compositor: wlroots (Sway, Hyprland) has a protocol that is not implemented here; GNOME needs a Shell extension; KDE needs a KWin script. | The refusal and its message: yes |
| Linux, X11 without an EWMH window manager | **Not available** | Nothing reports the active window | Yes |

On macOS LaunchDeck is built, and the Rust (715 passed, 0 failed, 2 ignored) and TypeScript (309) tests pass on macOS 15.4.1, Apple M2, Rust 1.97.0; the macOS adapter compiled there as written and passed clippy, but the Auto Context observer has not been seen running and the app window has not been checked by hand (the check is [4.2 in NEXT_PHASE.md](../NEXT_PHASE.md#42-auto-context); the record is [VERIFICATION.md](../VERIFICATION.md)). The Windows adapter could not be built in the environment it was written in (the Rust standard library for that target could not be downloaded) and has never been compiled. Both are small, written against the exact signatures of the binding crates already in the dependency tree, and share the identity code that *is* tested. Treat them as unverified until someone runs them.

**Known limits where it does work**

* Windows: packaged (Store) applications all run inside `ApplicationFrameHost.exe` and so share one identity. System surfaces that take the foreground (Start, the task switcher) count as applications. An application running elevated cannot be identified by an unelevated LaunchDeck and reads as "nothing".
* X11: whether a window manager is present is checked once, at start.
* No application icons, and no list of installed applications: the picker offers what has been in front since LaunchDeck started.

**When it is not available**, LaunchDeck starts and works exactly as before. The panel says *Not available* and why; the sidebar reads *Not available here* if you switch Auto Context on; switching surfaces by hand, pads, plugins and everything else are untouched.

## Privacy

Which application you use is nobody's business, including LaunchDeck's files.

| | |
| --- | --- |
| **Saved** (`workspace.json`, under `context`) | Whether Auto Context is on, the rules (each with the one identity it names), the fallback. |
| **In memory only**, gone when LaunchDeck closes | The application in front, the last one accepted, the up-to-12 applications offered when adding a rule, the last decision. |
| **Never read** | Window titles, window contents, URLs, documents, the list of running or installed applications. |
| **Never written or sent** | Any history, timeline, duration or count of application use. No analytics. Nothing in the audit trail or the logs. |

Plugins have no access to any of it: no API reads the context, adds a rule or activates a surface through the engine. The only thing published to the State Hub is `context.mode`, one word (`auto`, `manual`, `paused`, `off`, `unavailable`), so a pad can show whether Auto Context is in charge.

The observer runs whenever LaunchDeck does and the system allows it, including with Auto Context off, so that the rule picker has something to offer. It holds one identity at a time.

## For developers

| Piece | Where |
| --- | --- |
| Identity | `src-tauri/src/context/identity.rs` |
| Rules and their evaluation (pure) | `context/rules.rs` |
| The state machine (pure; time is passed in) | `context/engine.rs` |
| Observers | `context/observer/`: `x11.rs`, `macos.rs`, `windows.rs`, and `scripted.rs` for tests |
| What the interface is told | `context/status.rs`; Tauri event `context-changed`; commands `context_status`, `context_update`, `context_resume` |
| Runtime integration | `core/runtime.rs`: `activate()` with an origin, `on_foreground`, `settle_context` |
| Interface | `src/features/context/` |

Tests: the engine as a state machine (`cargo test --lib context::`), the runtime with a scripted observer and the virtual Launchpad (`cargo test --test context_engine`), the status contract (`examples/context-status.json`, `cargo test --test context_contract`), the interface (`npm test`), and the real observer on a real X server (`python3 tools/desktop-e2e/run.py context`).

Measured on Linux/X11: the surface is on screen about 210 ms after the window manager is asked to activate a window (release build; 215 to 275 ms in a debug build), of which 150 ms is the stability wait. The observer uses no CPU while nothing changes (0 clock ticks in 10 s) and about 0.4 ms of CPU per change of application; evaluating a full list of 64 rules takes about 1.5 microseconds (both in a debug build).

The research and the decisions behind this are in [CONTEXT_ENGINE_PLAN.md](../CONTEXT_ENGINE_PLAN.md).

## Not built

Window-title matching, the current project or Git repository, agent or plugin state as context, time or network or location, conditions combined with AND or OR, scripts, context provided by plugins, any history of use, and per-compositor Wayland support.
