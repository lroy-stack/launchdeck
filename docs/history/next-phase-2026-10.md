# Next phase: run it where it is meant to run

Written 2026-10-05, at commit `6a9224e` plus the documentation update that added this file. Meant to be read on the Mac, top to bottom, with the repository open.

**The short version.** Eight milestones of LaunchDeck were built and tested in one Linux container that had no Mac, no Windows, no Launchpad and no Home Assistant. The code that exists only for those targets has never run, and some of it has never been compiled. Before anything new is built, this phase makes the existing claims true on real targets:

1. Build and test on macOS, and fix what breaks (sections 1 to 3).
2. Check by hand what only a real machine can show: Auto Context, the Keychain, synthetic keys, a physical Launchpad X, a real Home Assistant (section 4).
3. Add continuous integration on Linux, macOS and Windows, so that this never has to be done by hand again (section 5).

No new features. The phase is done when the "Verified where" table in the [README](../../README.md#verified-where) can be rewritten from things that were run.

Throughout this file, **"expected"** means predicted by reading the code, not observed. Treat every expectation as a hypothesis to confirm or correct.

## 0. Where things stand

| | |
| --- | --- |
| Last full run (Linux, commit `6a9224e`) | Rust: 712 tests passed, 0 failed, 2 ignored, in 21 targets. TypeScript: 309 tests in 18 files. `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `tsc`, `eslint`: clean. Production build: ok. Desktop run on the release build: 218 of 218 checks. |
| The 2 ignored Rust tests | A benchmark that prints timings, and the test that writes to a real credential store (section 4.3). |
| Toolchain used | Rust 1.97.0, Node 22.22, Tauri 2, Wasmtime 48, `keyring` 4.2.0. `Cargo.lock` and `package-lock.json` are committed. |
| Written for one system, never through a compiler | `src-tauri/src/context/observer/macos.rs` and `windows.rs`, with their target-specific dependencies in `src-tauri/Cargo.toml`. |
| Shared code that has only ever been built for Linux | Everything else. On macOS and Windows it reaches things that have never run: the Keychain and Credential Manager (through `keyring`), CoreMIDI and WinMM (through `midir`), synthetic keys (through `enigo`), the tray, the bundle. |
| Never run against the real thing | The Launchpad X adapter (byte-tested against Novation's manual) and the Home Assistant example (tested against a stand-in server). |

The three most recent milestones and where each is described:

| Milestone | Commit | Read |
| --- | --- | --- |
| Plugin capabilities: network through a broker, per-address grants | `4c64e1c`, `49e0f44` | [docs/plugins/permissions.md](../user/plugins.md), [docs/plugin-sdk/network.md](../plugin-sdk/network.md) |
| Secrets: the system credential store or nothing | `35c5e01` | [docs/security/secrets.md](../architecture/secrets.md) |
| Context Engine v1: the surface follows the application in front | `6a9224e` | [docs/context-engine.md](../user/context-engine.md) |

## 1. Set up the Mac

Prerequisites (Tauri's own list, page updated 20 August 2026: <https://v2.tauri.app/start/prerequisites/>):

```bash
xcode-select --install                                           # Command Line Tools are enough for a desktop app
curl --proto '=https' --tlsv1.2 https://sh.rustup.rs -sSf | sh   # Rust stable (1.97 was used)
# Node 20 or newer (22 was used), from nodejs.org or your version manager
```

Then:

```bash
git clone https://github.com/lroy-stack/launchdeck.git && cd launchdeck
npm ci
npm run typecheck && npm run lint && npm test    # expected: clean, 309 tests. Nothing here is known to depend on the system.
npm run build                                    # type check and production build of the interface
```

Plain `cargo` commands (`check`, `test`, `clippy`) do not need the built interface: without Tauri's CLI they are development builds, which load from the dev server instead of embedding `dist/`. Only `npm run tauri build` embeds it, and it runs `npm run build` itself.

The first Rust build compiles Wasmtime and Tauri and takes several minutes; `cargo test` then links about twenty test programs, each with all of Tauri in it, so expect several gigabytes under `src-tauri/target`.

## 2. First build: what has never been compiled

```bash
cd src-tauri
cargo check --all-targets
```

If this fails, it is expected to fail in one place: `src/context/observer/macos.rs` (89 lines). It was written against the vendored sources of `objc2-app-kit` 0.3.2, `objc2-foundation` 0.3.2 and `block2` 0.6.2 without a compiler for the target. What it uses:

| Call | Gated by (Cargo feature) |
| --- | --- |
| `NSWorkspace::sharedWorkspace()`, `.notificationCenter()` | `objc2-app-kit/NSWorkspace`, `objc2-foundation/NSNotification` |
| `.frontmostApplication()`, `.bundleIdentifier()`, `.localizedName()` | `objc2-app-kit/NSRunningApplication`, `objc2-foundation/NSString` |
| `.processIdentifier()` | `objc2-app-kit/libc` |
| `NSWorkspaceDidActivateApplicationNotification` | `objc2-app-kit/NSWorkspace`, `objc2-foundation/NSString`, `NSNotification` |
| `addObserverForName_object_queue_usingBlock` with a `block2::RcBlock` | `objc2-foundation/block2`, `NSOperation`, `NSNotification` |

How to read a failure:

* **"no method named …" or "cannot find … in crate"**: almost always a missing Cargo feature. The item's page on docs.rs names the feature; add it to the `[target.'cfg(target_os = "macos")'.dependencies]` section.
* **A type mismatch on the block**: the method wants `&block2::DynBlock<dyn Fn(NonNull<NSNotification>)>`; an `RcBlock` dereferences to it. Adjust the closure's argument type to what the compiler names.
* **The compiler asks for a `MainThreadMarker`**: the binding considers the call main-thread-only. Then the registration has to happen on the main thread (from Tauri's `setup`, which runs there) instead of on the observer's own thread. That is a change to how `MacObserver` starts, not to the engine.
* **`unused_unsafe` from clippy**: the binding marks the call safe; remove the `unsafe` block.

Rules for fixing it:

* Change the adapter and its dependencies, not the engine. `context/engine.rs`, `rules.rs` and `identity.rs` are pure, tested and the same on every system.
* Read the application's identity and nothing else. Do not reach for window lists, window titles or the Accessibility API to "make it work": they need Screen Recording or Accessibility permission and they read things this project has decided never to read ([privacy](../user/context-engine.md#privacy)).
* When it compiles and has been seen working, change the header comment of `macos.rs` and the platform table in `docs/context-engine.md` to say so, with the macOS version it was run on.

Then:

```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings      # code that only exists on macOS gets its first lint here
```

Tauri will also print a warning that the bundle identifier `dev.launchdeck.app` ends in `.app`, which it discourages on macOS. It is harmless for now. Changing the identifier moves the data folder on every system, so that is a decision to take deliberately, not in passing.

## 3. Tests expected to fail on macOS

```bash
cd src-tauri && cargo test
```

Four tests are expected to fail on macOS (and on Windows) **without anything being wrong with the product**. They compare against values recorded on Linux. I found them by reading the code while writing this file; they have not been seen failing.

| Test | Why |
| --- | --- |
| `surfaces::tests::contract_fixture_matches_the_default_workspace` (lib) | `default_workspace()` picks a few pads by system with `cfg!` (which terminal, editor, file manager and screenshot shortcut). `examples/default-workspace.json` holds the Linux ones and is compared exactly. |
| `persistence::tests::a_real_v3_workspace_migrates_to_exactly_what_ships_now` (lib) | The schema 3 fixture is a Linux workspace; after migration it is compared, surface by surface, with what `default_workspace()` builds on this system. |
| `the_context_fixture_has_every_field_the_runtime_emits` (`tests/context_contract.rs`) | `context::status()` reads the system's namespace with `cfg!` (`macos` here). The fixture and the scenario use `x11` identities, so on macOS the namespace differs and every rule is flagged "for another system". |
| `rules_are_checked_reordered_and_flagged` (`tests/context_engine.rs`) | The same cause: it expects no problem on rules written with `x11` identities. |

The fix, in both cases, is to stop reading the build target inside logic that tests compare exactly:

* **Starter workspace.** Make the system a parameter: `default_workspace_for(platform)`, with `default_workspace()` passing the current one. The two tests above ask for the Linux variant explicitly, which is what their fixtures are, and one small new test per system checks the pads that differ. One fixture, and all three variants tested on every machine.
* **Context namespace.** The namespace is a property of whatever reports the identities, so let the observer say it (for example `ForegroundObserver::namespace()`), keep it in the engine next to the observer's status, and have `status()` use that instead of `native_namespace()`. The scripted observer used by tests says `x11`. The fixture stops depending on where it is generated, and "a rule for another system" becomes testable on any system.

Do not regenerate the fixtures on the Mac to make the tests pass: that only moves the failure to Linux.

Worth watching, with no prediction either way:

* Three tests reach a local test server through the name `localhost` (`tests/plugin_network.rs`, and two in `plugins/external/network.rs`). They depend on how the system resolves that name.
* Tests that stop a runaway plugin by fuel and by wall clock run on a different CPU architecture (Apple silicon) for the first time.
* Tests marked `#[cfg(unix)]` (owner-only permissions on the files LaunchDeck writes, a symlinked plugin being refused, shell actions) run on macOS for the first time. Expected to pass.

Anything else that fails is new information. Write down the test name and the message before fixing it.

## 4. Check by hand, on the Mac

```bash
LAUNCHDECK_PLUGINS="$PWD/examples/plugins" npm run tauri dev
```

The app's data is in `~/Library/Application Support/dev.launchdeck.app/`. Delete that folder to start clean.

### 4.1 The app itself

- [ ] The window opens, the virtual Launchpad is connected, **Agents, Run Agent Demo** animates the pads, APPROVE flashes and works.
- [ ] The tray icon appears in the menu bar; closing the window leaves the runtime alive; the tray menu brings the window back and quits.
- [ ] **Plugins** lists Hello, Counter, Channels, Home Assistant and C Ping as on. (The `.wasm` files are committed; nothing needs building. The C example cannot be rebuilt with Apple's clang, which has no `wasm32` target; `brew install llvm lld` if you ever need to.)

### 4.2 Auto Context

In the app, not with the example program: `cargo run --example foreground` runs the observer on the main thread of a program that has no run loop, and on macOS `NSWorkspace` delivers its notifications and refreshes `frontmostApplication` through the main run loop. The example is therefore expected to print the first application and never notice a change. That would say nothing about the adapter. (Making the example pump a run loop is a small, separate fix.)

**Settings, Auto Context** should read *Available*, "Watching through macOS application notifications."


- [ ] Bring Terminal, a browser and an editor to the front once each; back in LaunchDeck they are listed under *Add a rule* with bundle identifiers as their identity (`com.apple.Terminal`, `com.apple.Safari`, `com.microsoft.VSCode`…), never a window title.
- [ ] Application A has a rule: switching to it shows surface A. Application B: surface B.
- [ ] An application without a rule: nothing changes with the fallback on *Keep*; the fallback surface appears when one is set.
- [ ] Pick a surface by hand while A is in front: the sidebar reads **Manual override**, and going to LaunchDeck and back to A does not undo it.
- [ ] **Resume Auto** applies A's rule at once. Switching to B ends an override too.
- [ ] In **Edit** mode nothing switches and the sidebar reads *Paused while editing*; back in **Live** the current application's rule is applied once.
- [ ] Quit and reopen: the rules and the Auto switch are kept; `workspace.json` has a `context` entry with rules and no history of any kind.

Things only macOS can answer. Record what happens; none has an expected answer yet:

* Under `npm run tauri dev` the binary has no bundle, so LaunchDeck's own window should read as *nothing in front* rather than *LaunchDeck*. The engine treats both the same way (neither is an application), but the panel's wording differs from a bundled build (`npm run tauri build`, then open the `.app`). Check both.
* What comes to the front, as far as `NSWorkspace` is concerned, for Spotlight, the Cmd+Tab switcher, Mission Control, a full-screen application, and a menu-bar-only application's popover. If a system surface counts as an application and causes a visible flicker of surfaces, that is a finding for the limits list in `docs/context-engine.md`.
* How fast it feels. On Linux the surface is on screen about 210 ms after the switch, of which 150 ms is the deliberate wait.

### 4.3 Secrets in the Keychain

- [ ] **Plugins, Home Assistant, Secrets** reads "Kept in your system's credential store (Keychain)." and offers a field (not *Unavailable*).
- [ ] Save any value as the token. In **Keychain Access**, login keychain, search `LaunchDeck plugin`: one entry, `LaunchDeck plugin example.home-assistant`, account `token`.
- [ ] `grep -r "<the value>" ~/Library/Application\ Support/dev.launchdeck.app/` finds nothing.
- [ ] Remove the secret in the app: the entry is gone from Keychain Access.
- [ ] `cd src-tauri && cargo test --test secret_store -- --ignored`. This is the one test that touches your real Keychain: it writes, reads, replaces and removes that same entry. Run it knowingly.

Record:

* Whether macOS asks for permission ("… wants to use your confidential information stored in …") when a **rebuilt** binary reads an entry an earlier build wrote. An unsigned debug binary changes with every build, and the Keychain ties access to the program. If it asks: what the app shows while the question is open, and what happens on *Deny*. The only acceptable outcomes are "the secret is used" or "the request is not sent and the card says why". A crash, a hang without explanation or any plain copy of the value is a bug.
* With the login keychain locked (Keychain Access, File, Lock Keychain) and LaunchDeck restarted: does macOS offer to unlock it, or does the app say *Unavailable* with "The login keychain could not be reached…"? Either is acceptable; write down which.

### 4.4 Keyboard shortcuts and media keys

These go through `enigo` and need **System Settings, Privacy & Security, Accessibility**. Under `npm run tauri dev` the permission is asked for the terminal that started it; for a built `.app`, for LaunchDeck.

- [ ] On the Accessibility surface: volume up, volume down, mute, play/pause. On the Productivity surface: Copy, Paste, Undo, Redo and Search (keyboard shortcuts; `mod` is Cmd).
- [ ] Without the permission: note whether the pad turns red with a message or whether nothing happens at all. Silence would be a finding: the person should be told why.
- [ ] The starter pads that are different on macOS, none of which has ever been pressed: Development, **Terminal** and **VS Code** (start the applications named `Terminal` and `Visual Studio Code`); Productivity, **Files** (Finder) and **Screenshot** (Cmd+Shift+4). Delete the data folder first, so that the starter workspace is the one this system builds.

### 4.5 A physical Launchpad X

Follow [docs/devices/launchpad-x.md, "Trying it on real hardware"](../user/launchpad-x.md#trying-it-on-real-hardware). What is unverified and needs an answer:

- [ ] **Settings, Rescan** finds it, and under which port names macOS lists it (the adapter expects something containing `LPX MIDI`).
- [ ] **Connect** switches it to Programmer mode and the grid mirrors the virtual Launchpad, pad for pad, with the right colours.
- [ ] Pressing a pad in Live mode runs its action; press and release both arrive (try a long press: 500 ms).
- [ ] **The top row, the side column and the logo.** Their controller numbers (top CC 91 to 98, side CC 89 down to 19, logo 99) were taken from what is commonly documented for the MK3 family, not from the manual's text. A MIDI monitor shows what the device really sends. If they differ, change `led_index` and `pos_from_cc` in `src-tauri/src/devices/launchpad_x.rs` and the two tests beside them; nothing else knows these numbers.
- [ ] **Pulse and flash** are drawn by LaunchDeck at 25 frames a second as RGB updates. Run the Agent Demo and look for stutter on running (pulse) and approval (flash) pads.
- [ ] Unplug while connected: the app falls back to the virtual device and says so. Plug in again: it is found within a few seconds.
- [ ] **Disconnect** clears the LEDs and returns the device to its normal mode.

Record the firmware version and the numbers you saw. With that, the "unverified" notes in `docs/devices/launchpad-x.md` and the README can be replaced by facts.

### 4.6 A real Home Assistant

Follow [examples/plugins/home-assistant/README.md](../../examples/plugins/home-assistant/README.md#set-it-up): address, **Allow**, token, a pad with **Toggle** and feedback from **Entity state**.

- [ ] The request under **Network access** shows the exact address and, for an address on your LAN, *Your local network*.
- [ ] The pad shows the light's real state, goes Pending on a press, and ends On or Off as the light does.
- [ ] Change the light from Home Assistant's own interface: the pad follows within about 5 seconds (it polls).
- [ ] **Revoke** the address: the next press sends nothing and the card says why. **Allow** again: it recovers.

Record:

* Whether macOS asks for **Local Network** access the first time (macOS 15 and later do, for programs that talk to devices on the LAN). A request that times out with no visible reason usually means that permission was denied: System Settings, Privacy & Security, Local Network.
* Address by IP and by name (`homeassistant.local`): both, and what the prompt says for each.
* If your installation uses HTTPS: what happens, including with a certificate the system does not trust.
* Any difference between the real API and the stand-in (`tools/desktop-e2e/lib/mock_home_assistant.py`, `src-tauri/tests/support/mock_home_assistant.rs`). A difference means the stand-in is wrong and the tests that use it prove less than they say; fix the stand-in first, then the plugin.

## 5. The milestone: continuous integration on three systems

Three milestones in a row ended with "written, not run on its target". CI is what stops that.

**Done means**

1. Every push and pull request runs, on Linux and macOS: `npm ci`, `typecheck`, `lint`, `npm test`, `npm run build`, `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test`. All green, with the four tests of section 3 fixed properly.
2. Windows runs the same steps. `windows.rs` compiles, and whatever tests fail there are either fixed or listed by name in the README. Start it as allowed-to-fail and tighten it; do not hide it.
3. The desktop suite (`tools/desktop-e2e`) runs on Linux on demand or nightly, not on every push: it needs a release build and takes minutes.
4. The README's "Verified where" table and the platform notes in the docs are rewritten from CI and from section 4, and say nothing that was not run.

**A starting point.** This workflow is a draft and has not been run. Save it as `.github/workflows/ci.yml`.

```yaml
name: CI
on:
  push: { branches: [main] }
  pull_request:

jobs:
  test:
    strategy:
      fail-fast: false
      matrix:
        os: [ubuntu-24.04, macos-latest, windows-latest]
    runs-on: ${{ matrix.os }}
    continue-on-error: ${{ matrix.os == 'windows-latest' }}   # remove once Windows is green
    steps:
      - uses: actions/checkout@v7
      - uses: actions/setup-node@v7
        with: { node-version: 22, cache: npm }
      - uses: dtolnay/rust-toolchain@stable
        with: { components: "rustfmt, clippy" }
      - uses: Swatinem/rust-cache@v2
        with: { workspaces: src-tauri }
      - name: Linux packages
        if: runner.os == 'Linux'
        run: |
          sudo apt-get update
          sudo apt-get install -y libwebkit2gtk-4.1-dev libgtk-3-dev libayatana-appindicator3-dev \
            librsvg2-dev libasound2-dev libxdo-dev libssl-dev libxkbcommon-dev
      - run: npm ci
      - run: npm run typecheck
      - run: npm run lint
      - run: npm test
      - run: npm run build
      - run: cargo fmt --manifest-path src-tauri/Cargo.toml --check
      - run: cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
      - run: cargo test --manifest-path src-tauri/Cargo.toml
```

Notes on the draft, checked on 2026-10-05:

* `ubuntu-24.04` is pinned on purpose. `ubuntu-latest` is being moved to Ubuntu 26.04 between 19 October and 19 November 2026 ([runner-images #14748](https://github.com/actions/runner-images/issues/14748)), and nobody has built this against 26.04's WebKitGTK yet. Move when you choose to, not when the label does.
* `macos-latest` is macOS 26 on Apple silicon since July 2026 ([runner-images #14167](https://github.com/actions/runner-images/issues/14167)).
* `actions/checkout` and `actions/setup-node` are at major version 7, `Swatinem/rust-cache` at 2.9.x. Check before pinning.
* The package list is what the development container had installed; a clean runner may need one more or one fewer.
* A hosted runner has no credential store that answers, no window manager and no MIDI device. The tests already treat all three as normal (secrets in memory, a scripted observer, the virtual Launchpad), which is why plain `cargo test` is enough. The real-store test stays ignored in CI; on Linux `scripts/test-secret-service.sh` can run in a job of its own (it needs `dbus` and `gnome-keyring`).
* On Windows, expect more than the four tests of section 3: nothing has been tried there, and the tests marked `#[cfg(unix)]` do not run at all, so some behaviour has no test on that system. List what fails before fixing anything.
* Nothing in CI should ever need a secret, a token or network access beyond package registries.

**The desktop job** (Linux only, `workflow_dispatch` or a schedule): install what `tools/desktop-e2e/README.md` lists (`tauri-driver`, `webkit2gtk-driver`, `xvfb`, `dbus`, `gnome-keyring`, `openbox`, `xterm`, `wmctrl`), run `npx tauri build --no-bundle`, then `python3 tools/desktop-e2e/run.py`, and upload `tools/desktop-e2e/.work/shots` and `logs` as artifacts when it fails.

## 6. What cannot be done from the Mac

* **The desktop suite.** `tauri-driver` supports Linux and Windows only; there is no WebDriver for macOS's web view (Tauri's WebDriver page, updated 29 June 2026: <https://v2.tauri.app/develop/tests/webdriver/>). The 218 checks stay a Linux job. Section 4 is the macOS substitute, by hand.
* **`scripts/test-secret-service.sh`.** Linux only. On macOS the same test runs directly against the Keychain (section 4.3).
* **The X11 observer and the Wayland refusal.** Linux only; covered by the `context` desktop suite.
* **Windows.** `windows.rs`, Credential Manager, `ApplicationFrameHost.exe` and elevated applications (see the limits in `docs/context-engine.md`) need a Windows machine or the CI runner. From the Mac, the most that can be done is `rustup target add x86_64-pc-windows-msvc && cargo check --target x86_64-pc-windows-msvc`, which may or may not get far without the Windows SDK. CI is the realistic route.

## 7. Rules that do not change

These were decided in earlier milestones and bind this phase too. A fix that needs to break one is the wrong fix.

**Secrets**

* The system's credential store or nothing. No plain-file fallback, no environment switch that brings one back, no new plaintext storage in schema 4 or later. A test fails if a new kind of store appears.
* If the store cannot be reached, what needs the secret does not work and the interface says why.
* A secret's value is never given to the plugin, never logged, never in the workspace, a manifest, plugin state or the audit trail.
* Nothing the person stored is deleted silently. Moving the old `secrets.json` is an explicit step.
* No claim of secure deletion beyond what a filesystem can give.

**Plugins**

* Untrusted. No raw sockets, no WASI, no unrestricted HTTP, no direct access to localhost, the LAN or system network APIs.
* A manifest asks; only the person grants. Grants are stored apart from manifests and are never widened automatically. An update that asks for more needs a new approval for what is new.
* A redirect is authorized like a new request.
* Authorization headers and sensitive response bodies are not logged.

**Context Engine**

* It never talks to MIDI or a device. Its only output is the activation a person's click uses.
* Identity is the stable identifier (bundle id, executable name, `WM_CLASS`), never the display name.
* Persisted: the rules, the settings, the identifiers the rules name. Never: window titles, a history or timeline of applications, usage counts. No analytics.
* Plugins cannot create rules, read the foreground or its history, or activate a surface through the engine.
* v1 is "application in front to surface and page", ordered, first match wins. Not in v1, and not to be slipped in while porting: window-title matching, project or Git detection, agent state as context, context from plugins, schedules, network or location, AND/OR conditions, scripts, AI classification.

**Honesty**

* Nothing is described as tested on a system, a device or a service it was not run on. "Written, not run" is an acceptable status; a guess presented as a result is not.
* Wayland is not supported and is said not to be. No universal Linux claim.

## 8. Write down what you find

Keep a short record as you go (a `VERIFICATION.md` at the root, or the pull request description): the macOS version and hardware, each checkbox of section 4 with what was seen, every test that failed and why, every answer to a "record" item. Then update, from that record only:

| File | What to change |
| --- | --- |
| `README.md` | The status line, "Verified where", "Known limitations". |
| `docs/context-engine.md` | The platform table and the limits; the measured numbers for macOS. |
| `docs/security/secrets.md` | The sentence that says macOS has not been run; anything learned about prompts and a locked keychain. |
| `docs/devices/launchpad-x.md` | "Hardware status", the auxiliary numbers, the limitations. |
| `examples/plugins/home-assistant/README.md` | "Checked against a stand-in". |
| `src-tauri/src/context/observer/macos.rs` | Its header comment. |
| This file | Delete what is done. |

## 9. After this phase

Candidates, in the order I would take them. None is committed to.

1. **Windows, properly.** The same treatment as section 4, on a real machine.
2. **Hardware animation on the Launchpad X**, if section 4.5 shows stutter: the encoders for the device's own pulse and flash exist and are byte-tested, and are unused only because the palette could not be verified without a unit.
3. **Signing and notarization** of the macOS bundle. It would also settle the Keychain question of section 4.3, since a signed program keeps its identity across builds.
4. **Wayland**, per compositor, starting with whichever one you actually use.
5. **A second capability or a second context source.** Only after the first of each has been used for real for a while.

## 10. Quick reference

| | |
| --- | --- |
| Run the app with the example plugins | `LAUNCHDECK_PLUGINS="$PWD/examples/plugins" npm run tauri dev` |
| Interface only, in a browser, simulated runtime | `npm run dev` |
| Frontend checks | `npm run typecheck && npm run lint && npm test` |
| Rust checks | `cd src-tauri && cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test` |
| One Rust test target | `cargo test --test context_engine` (the files in `src-tauri/tests/`), `cargo test --lib context::` |
| Regenerate a contract fixture | `cd src-tauri && cargo run --example dump_context > ../examples/context-status.json` (also `dump_workspace`, `dump_agents`, `dump_plugins`, `dump_catalog`, `dump_observables`) |
| Production bundle | `npm run tauri build` |
| Data folder on macOS | `~/Library/Application Support/dev.launchdeck.app/` (`workspace.json`, `plugins.json`, `permissions.json`, `agents.json`, `audit.jsonl`, `plugins/`) |
| Ports | 47821: local WebSocket and Agent Bridge (`127.0.0.1` only). 1420: the dev server. |
| Where the macOS-only code is | `src-tauri/src/context/observer/macos.rs`; the `cfg(target_os = "macos")` section of `src-tauri/Cargo.toml` |

### Handing this to an assistant

If you continue with a coding assistant, this is enough to start it:

> Read `NEXT_PHASE.md` and `README.md`. We are on macOS. Work through sections 1 to 3 of `NEXT_PHASE.md`: build, run the checks, and fix what fails, starting with the four tests it predicts. Follow the fixes it proposes unless the code shows a better one, and say so if it does. Keep to section 7. Do not regenerate fixtures to make a test pass, do not describe anything as verified that you did not run, and stop to ask before changing anything outside `src-tauri/src/context/`, `src-tauri/src/surfaces/`, the tests and `Cargo.toml`. Then prepare the CI workflow of section 5. Section 4 needs me, the Launchpad and Home Assistant: give me the list when you get there.
