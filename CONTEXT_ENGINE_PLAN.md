# Context Engine v1

Goal: the surface on the controller follows the application in front, by rules the person wrote, without ever fighting a choice they made by hand. One context source (the foreground application), one kind of rule (application to surface and optional page).

```text
Platform Foreground Observer      which application is in front (native, per system)
        ↓
ApplicationIdentity               { namespace, id, displayName }
        ↓
ContextSnapshot                   { foregroundApplication }
        ↓
Ordered Context Rules             first match wins, then the explicit fallback
        ↓
Auto / Manual Override machine    decides whether a decision is applied now
        ↓
activate_surface / activate_page  the runtime's existing path, unchanged
        ↓
Renderer / Device
```

The engine never touches MIDI, a device or the renderer. Its only output is a call to the activation the sidebar, the tray and the navigation pads already use.

## 1. LaunchDeck's lifecycle today

| Area | Finding | Consequence |
| --- | --- | --- |
| Activation | One path: `Runtime::activate_surface(surface, page)` and `activate_page(page)`. Callers: the `activate_surface` / `activate_page` commands (sidebar, pages), the tray menu, and `navigate()` for navigation pads and the top-row buttons. Each persists `lastActiveSurface`, publishes `surface.changed` / `page.changed`, writes `surface.active` / `page.active` to the State Hub and emits the runtime state; the 25 Hz render tick does the rest. | Automatic activation calls the same two functions with a marker of who asked. Nothing is bypassed. Every existing caller is a *manual* one. |
| Mode | `Runtime::set_mode(Live or Edit)`; in Edit, pads select and actions do not run. The mode is in the runtime state. | The engine reads the mode there. No second notion of "editing". |
| Persistence | `workspace.json` (schema 4), saved by the runtime on change. `Preferences` holds app settings. Unknown fields are ignored on read. | Context settings are a new optional `context` object in the workspace: additive, no schema bump, no migration. |
| Background | The app keeps running in the tray with the window closed. The runtime is a set of Tokio tasks. | The observer runs in the runtime, not in the web view: it works with the window closed and is never polled from React. |
| State and events | Event bus; State Hub with the native provider `runtime` (`surface.active`, `page.active`). | One more state, `context.mode`, for observability. The hub decides nothing about switching. |
| An older seam | `core/context.rs`: `ContextSnapshot { foreground_app }`, `suggest_surface()`, and `Surface.context.foregroundApps` (strings such as `com.microsoft.VSCode`, `Code`). Never called by the runtime; no namespace, no order, no fallback. | Replaced. `Surface.context` stays readable in the file format (old workspaces keep loading) and is not used. |
| Platform code | None. Everything platform-specific so far comes through crates (`enigo`, `midir`, `keyring`). | The observers are the first native code. They are kept small and behind one trait. |
| Desktop harness | Runs the app under a bare Xvfb: no window manager. | A real `unavailable` case for free; the context suite starts a window manager and real X clients. |

## 2. Platform mechanisms, researched

| System | Identity available | Change notification | Permission | Notes |
| --- | --- | --- | --- | --- |
| **macOS** | `NSRunningApplication.bundleIdentifier` (stable), `localizedName` (display) | `NSWorkspace.didActivateApplicationNotification` on `NSWorkspace.shared.notificationCenter` | None. Window titles would need Screen Recording or Accessibility; they are not read. | `frontmostApplication` is cheap and needs no window list. |
| **Windows** | Executable path of the process that owns the foreground window (`GetForegroundWindow`, `GetWindowThreadProcessId`, `QueryFullProcessImageNameW`) | `SetWinEventHook(EVENT_SYSTEM_FOREGROUND)`, out of context, on a thread with a message loop | None for same-integrity processes. An elevated process cannot be opened by an unelevated LaunchDeck: it is seen as "unknown". | Packaged (UWP) apps all run inside `ApplicationFrameHost.exe`. |
| **Linux, X11** | `WM_CLASS` of the window named by `_NET_ACTIVE_WINDOW` on the root (EWMH) | `PropertyNotify` on the root window | None | Needs a window manager that implements EWMH (all common ones do). |
| **Linux, Wayland** | `app_id`, but only through compositor-specific means | None that is standard | n/a | See below. |

**Wayland.** The core protocol gives a client no way to learn which application has focus, by design. `ext-foreign-toplevel-list-v1` lists toplevels with `app_id` and title and "does not expose which toplevel is focused or activated" (wayland.app, read 4 Oct 2026). What exists is per compositor: wlroots compositors (Sway, Hyprland, river, labwc) offer `wlr-foreign-toplevel-management`, which does carry an `activated` state; GNOME offers nothing to ordinary clients and needs a Shell extension that exposes the focused window over D-Bus; KDE needs a KWin script reached over D-Bus ("each compositor will need its own unique implementation", J. Sissel, 15 Nov 2025). An X connection through XWayland only ever sees other X clients, so it would report "nothing in front" for every native Wayland window: a wrong answer, not a limited one.

**Crates considered.**

| Crate | Version, date | What it does | Decision |
| --- | --- | --- | --- |
| `active-win-pos-rs` | 0.11.0, 26 May 2026 | Polling. Returns title, position, process path, app name. On macOS walks the whole window list (`CGWindowListCopyWindowInfo`) every call and returns the bundle *path*, not the bundle id. | Not used: collects titles and geometry the engine must not have, no bundle id, no events. |
| `x-win` | 5.8.0, 2026 | Polling. Titles, positions, memory use, browser URLs, icons; on GNOME Wayland installs a Shell extension. Pulls `image`, `zbus`, `xcb`. | Not used: far more than is needed, and installs things on the user's system. |
| `x11rb` | 0.14.0, 16 Jul 2026 | Pure-Rust X11 protocol client, no C library. | **Used** for X11. |
| `objc2-app-kit` | 0.3.2 | Generated AppKit bindings. Already in the dependency tree through Tauri. | **Used** for macOS. |
| `windows-sys` | 0.61.2 | Raw Win32 bindings. Already in the dependency tree. | **Used** for Windows. |

Other products, for behaviour rather than code: Elgato Stream Deck "Smart Profiles" switch the profile "based on the program in focus" (help article updated 24 Jun 2026); the profile of the application that gains focus wins over a profile picked by hand earlier.

## 3. Selected implementation

One trait, three native adapters, each reading **only** the application identity:

```rust
pub trait ForegroundObserver: Send {
    /// What it can do here, decided once at start.
    fn status(&self) -> ObserverStatus;
    /// Run until `stop`, sending each change. Called on its own thread.
    fn run(self: Box<Self>, sink: Sink);
}

pub enum Seen { Application { identity: ApplicationIdentity, own: bool }, Nothing }
pub enum ObserverStatus { Available { mechanism }, Degraded { mechanism, reason }, Unavailable { reason } }
```

| System | Mechanism | Events or polling |
| --- | --- | --- |
| macOS | `NSWorkspace` activation notification; the handler reads `frontmostApplication` | Events. Plus a read every 2 s as a safety net for a missed notification. |
| Windows | `SetWinEventHook(EVENT_SYSTEM_FOREGROUND)` on a thread with a message loop; the callback reads the foreground window's process | Events. Plus the same 2 s safety read (the hook is known to miss a restore from minimised). |
| Linux X11 | `PropertyNotify` for `_NET_ACTIVE_WINDOW` on the root window | Events only. |
| Linux Wayland, anything else | none | Unavailable, with the reason. |

The 2 s safety read is not what makes switching fast; events do that. It is one cheap call that makes a lost event self-correcting. Nothing polls from React, and nothing polls at a rate chosen to look instant.

`own` is true when the foreground process is LaunchDeck itself, compared by process id (the same test on every system).

A scripted observer implements the same trait for tests: the engine and the runtime are tested with no display at all.

## 4. Platform limits, stated as they are

| Environment | Status | What the person is told |
| --- | --- | --- |
| macOS | Available | |
| Windows | Available | Packaged (Store) apps share one identity (`applicationframehost.exe`). System surfaces that take the foreground (Start, the task switcher) count as applications. An elevated app is not identifiable from an unelevated LaunchDeck. |
| Linux, X11 with an EWMH window manager | Available | |
| Linux, X11 without a window manager that reports the active window | Unavailable | "This X session has no window manager that reports the active window." |
| Linux, Wayland (any compositor) | Unavailable | "Wayland does not let an application see which application is in front. Auto Context needs an X11 session." Not implemented per compositor in v1: wlroots (protocol exists, untested here), GNOME (needs a Shell extension), KDE (needs a KWin script). |
| No display, or the connection fails | Unavailable | The error, in words. |

Unavailable never blocks start: the engine reports it, Auto can still be switched on (it does nothing), and manual switching is exactly as before.

**Built and run here:** Linux X11 only. **macOS and Windows adapters cannot be compiled in this environment** (no Rust standard library for those targets can be downloaded). They are written against the exact signatures in the vendored `objc2-app-kit 0.3.2`, `objc2-foundation 0.3.2`, `block2 0.6.2` and `windows-sys 0.61.2`, kept under 100 lines each, and are reported as not compiled and not run.

## 5. Application identity

```rust
pub struct ApplicationIdentity {
    pub namespace: String,     // "macos" | "windows" | "x11"
    pub id: String,            // stable, native
    pub display_name: String,  // presentation only
}
```

| Namespace | `id` | Example | `display_name` |
| --- | --- | --- | --- |
| `macos` | Bundle identifier | `com.microsoft.VSCode` | Localized name |
| `windows` | Executable file name, lower case | `code.exe` | The file name without `.exe` |
| `x11` | The class part of `WM_CLASS` | `Code` | The class |

* **Matching** is `namespace` equal and `id` equal ignoring ASCII case. `display_name` is never compared: an application that renames itself, or a localized name in another language, still matches.
* **Valid** means: namespace `[a-z][a-z0-9-]{0,15}`, id 1 to 255 characters with no control characters, display name at most 80. An invalid identity is refused when a rule is saved, with the reason.
* The namespace is not a closed list in the file, so a rule written on another system loads and simply never matches here. The interface says so on that rule.
* An application with no native identity (a macOS process without a bundle, an X window without `WM_CLASS`) is "nothing": it cannot be the subject of a rule.
* Why the executable name and not the path on Windows: the path changes with every update of many apps (`…\app-1.94.2\Code.exe`); the name does not.

## 6. Rules

```jsonc
"context": {
  "auto": true,
  "rules": [
    { "application": { "namespace": "x11", "id": "Code", "displayName": "Code" },
      "surfaceId": "development", "pageId": "main" }      // pageId optional
  ],
  "fallback": { "kind": "keep" }                          // or { "kind": "surface", "surfaceId": "home", "pageId": null }
}
```

Declarative, one condition, one target. No expressions, no AND or OR, no titles, no nesting. At most 64 rules.

## 7. Precedence

Ordered, **first match wins**. The list is shown numbered and reordered with Up and Down. Two rules for the same application are allowed and the second is flagged as unreachable.

A rule whose surface no longer exists **still wins** and activates nothing: a broken rule does not silently hand over to a lower one. The diagnostics say "Rule 2 matches, but its surface no longer exists", and the rule is flagged in the list. A rule whose page no longer exists opens the surface on its first page and says so.

## 8. Fallback

Explicit, one of two, chosen in the interface:

* **Keep the current surface** (the default). An application without a rule changes nothing.
* **Open a surface** (and optionally a page): every application without a rule shows it.

Keep is the default because it is the one that cannot surprise: switching Auto on with a single rule changes exactly one thing. The product example ("no rule matches, Home") is one select away.

## 9. Manual override

Auto applies a decision at exactly these moments, and at no other:

1. the **accepted application changes** to a different one;
2. **Resume Auto** is pressed;
3. Auto is switched **on**;
4. the mode goes from **Edit to Live**;
5. the **rules or the fallback are saved** (so a rule written for the application just used takes effect without leaving and coming back).

Because nothing is applied between those moments, a choice made by hand is never undone "immediately": it holds for as long as the same application stays in front.

* **Manual override** begins when, with Auto on and in Live mode, a surface or page is activated by anything other than the engine (sidebar, tray, a navigation pad, a top-row button) and that actually changes what is shown.
* It **ends** when a *different* application is accepted (rule 1 then applies: the new application's rule, or the fallback), or when **Resume Auto** is pressed (the current application's rule is applied at once).
* While it lasts, moments 4 and 5 do not apply anything. Switching Auto off and on again starts afresh.
* LaunchDeck's own window never counts as an application (section 10), so going to the LaunchDeck window, picking a surface by hand and returning to the *same* application keeps the pick. Moving on to another application ends it. This is the behaviour of the comparable products, and the only rule the person has to remember is: *the application that comes to the front decides; your hand decides until then.*

## 10. LaunchDeck itself, "nothing", and loops

The **accepted** application only ever changes to a real, other application.

* **LaunchDeck in front** (`own`): ignored. The accepted application stays what it was. So opening the window to look, to edit rules or to press a virtual pad switches nothing, and there is no loop to protect against: the engine's own activation cannot produce a foreground change that the engine acts on.
* **Nothing in front** (the desktop on X11, a window with no identity, the lock screen where it is not an application): ignored the same way. The surface is not reset.
* **An observer error after start**: the status becomes Degraded with the reason; the accepted application stays; manual switching works.
* **The application in front exits**: whatever comes to the front next is an ordinary change.

## 11. Edit mode

* While the mode is Edit, **nothing is activated automatically**. The observer keeps running and the accepted application keeps updating.
* Activating surfaces by hand in Edit (choosing what to edit) is not a manual override.
* On **Edit to Live**, the engine applies the current application's decision once, unless a manual override from before is still active.

## 12. Stability

Two values are kept apart: **observed** (the last thing the observer saw) and **accepted** (what rules are evaluated against). An observation becomes accepted when it has stayed the same for **150 ms**. A newer observation restarts the wait; one that returns to the accepted application cancels it.

So a window that flashes in front for less than that (a splash screen, a focus bounce while switching) never moves the surface, and a real switch is applied 150 ms after the event, below what reads as lag. Seeing the accepted application again is a no-op, as is a decision whose target is already what is shown.

## 13. Persistence and privacy

Persisted, in `workspace.json` under `context`: the Auto preference, the rules (each with the identity it names), the fallback. **Nothing else.**

Never persisted, never logged, never sent anywhere: which application is or was in front, when, for how long, in what order. No window title is read at all, on any system. The observed and accepted applications, and the short list of "recently seen" applications that feeds the rule picker (12, most recent first), live in memory and are gone when LaunchDeck closes. No analytics. The audit trail is not written to. Plugins get no API: they cannot read the context, add rules or activate through it.

`context.mode` in the State Hub carries one word (`off`, `auto`, `manual`, `paused`, `unavailable`), never an application.

## 14. Performance

The observer blocks on the system's event source; between changes it costs nothing. Per accepted change: one comparison per rule (at most 64) and at most one activation. To be measured and reported: time from an X11 focus change to the activation, and CPU of an idle app with the observer running.

## 15. Tests

Written against the engine's interface **before** its implementation:

| Case | Expectation |
| --- | --- |
| App A in front | Surface A is activated |
| App B in front | Surface B is activated |
| An application without a rule | The fallback: nothing with *keep*, the surface with *surface* |
| A manual switch | Override; the same application seen again changes nothing |
| The same application reported again | No-op: no activation, no state change |
| Edit mode | No automatic activation; the accepted application still updates |
| Edit to Live | Evaluated once |
| Observer unavailable | Status says why; manual activation works; Auto does nothing |

Plus: identity (normalised match, display name ignored, invalid refused); rules (ordered first match, reorder, page target, missing surface, missing page, removed surface); stability (a transient under 150 ms is ignored, a return to the accepted application cancels); override (ends on a different application, Resume applies at once, LaunchDeck in front does not end it); restart (rules and Auto persist, nothing about the foreground is in the file).

Levels: the engine as a pure state machine with time passed in; the runtime with a scripted observer and the virtual Launchpad (the LED changes); the X11 observer against a real X server with real windows (`tools/desktop-e2e`, suite `context`); the interface with Testing Library.

## 16. Phases

1. Identity, rules, evaluation: types, validation, tests.
2. The engine state machine: tests first, then the implementation.
3. The observer trait, the scripted observer, the X11 observer; the macOS and Windows adapters.
4. Runtime integration: one activation path with an origin, the settle timer, `context.mode`, commands, persistence.
5. Interface: Settings, Context; the Auto control and the override notice where surfaces are shown; diagnostics.
6. Desktop suite, documentation, measurements, full validation.

## 17. Done when

* The observer is behind one trait, with a real X11 implementation and honest statuses elsewhere.
* Rules persist stable identities; matching ignores display names.
* Ordered first match, explicit fallback, both visible and editable.
* Auto can be switched on and off and the choice persists.
* A manual switch is not undone until a different application comes to the front or Resume Auto is pressed.
* Edit suspends; Edit to Live evaluates once.
* A repeated observation and a decision that changes nothing cause no activation.
* The interface shows the observer's status, the application in front, the rule that matched and what it did.
* Nothing about foreground applications is written to disk.
* Every existing suite still passes; the context desktop suite passes on X11.
