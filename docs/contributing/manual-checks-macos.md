# Manual checks on macOS

What a person checks by hand on a Mac before a release, because no automated test reaches it: the app window, the tray, Auto Context in the running app, the Keychain, keys pressed for you, a physical Launchpad and a real Home Assistant. It replaces section 4 of the [October 2026 next-phase plan](../history/next-phase-2026-10.md#4-check-by-hand-on-the-mac), brought up to the 0.3.0 interface, and adds the release acceptance walk-through.

**Status for 0.3.0: none of these checks has been done** (NOT VERIFIED (f)). The headless acceptance test (`src-tauri/tests/release_acceptance.rs`) drives the same scenario through the runtime, without a window.

## Before you start

1. Quit LaunchDeck.
2. **Move** the data folder aside, never delete it: `mv ~/Library/Application\ Support/dev.launchdeck.app ~/Library/Application\ Support/dev.launchdeck.app.before-checks`. Move it back afterwards to get your own surfaces again.
3. Run the build under test: the `.app` from the `.dmg` of the release candidate ([releasing](releasing.md#4-the-bundles)), or `npm run tauri dev` from the release candidate commit. Write down which; some answers differ between them (below).
4. Keep a text file open and note, for each check, **seen**, **not seen** or **not done**, with what you saw. The record goes under [docs/verification/](../verification/README.md).

## 1. The acceptance walk-through

The release acceptance scenario of the productization plan, in the window.

- [ ] The first-run screen shows **Welcome to LaunchDeck**, the simulator under **Your Launchpad**, and five choices. Choose **Development**, press **Start with Development**: the Development surface is on screen in **Live** mode.
- [ ] Click a pad on the virtual Launchpad: it runs (a Development pad echoes a placeholder) and lights as running, then success.
- [ ] **Edit**: select an empty pad, find **Keyboard shortcut** in the Action Library (press `/` to search), **Assign to …**, fill in the keys, **Save**. Back in **Live**, press it.
- [ ] On that pad, **Feedback, Add feedback**: add a rule, save, and see the pad follow it.
- [ ] **Settings, Auto Context**: bring another application (Terminal) to the front once, come back, add a rule Terminal → Productivity, switch Auto Context **On**. Switching to Terminal shows Productivity.
- [ ] **Plugins, Example plugins, Install** next to Counter: it is listed as installed and on. Give a pad a Counter action and a feedback rule on its level; pressing the pad changes the level and the look.
- [ ] **Export…** in the sidebar: choose Development, tick **Include Auto Context rules**, **Save**. The file is in Downloads; **Show** opens the folder. Note whether macOS asked for access to Downloads, and what it said.
- [ ] **Settings, Workspace, Reset to starter surfaces**, then remove the Counter plugin's folder (**Plugins, Show plugins folder**, move the `example.counter` folder out of it) and press **Rescan**.
- [ ] **Import…**, choose the file. The preview says Counter is **Missing**, lists the rule unchecked, and shows the pads that run things. Tick the rule, **Import**. The surface is added and marked **Imported**; its Counter pad reads as needing the plugin; Auto Context is still as it was.
- [ ] Install Counter again from **Plugins**: the imported pad works without importing again.
- [ ] Quit LaunchDeck (tray, **Quit LaunchDeck**) and start it again: surfaces, the rule, Auto Context and the plugin are as you left them, and the first-run screen does not come back.

## 2. The app itself

- [ ] The window opens; **Agents, Run Agent Demo** animates the pads of the Agent Control surface; APPROVE flashes, and pressing it answers the request.
- [ ] The tray icon is in the menu bar. Closing the window leaves LaunchDeck running; the tray menu's **Open LaunchDeck** brings the window back, its **Surface** submenu switches surfaces, **Quit LaunchDeck** quits.
- [ ] **Settings** shows its sections (Devices, Auto Context, Notifications, Plugins & permissions, Security, Workspace, Developer, About). **About** shows 0.3.0 alpha and the six compatibility numbers; **Copy for a bug report** copies them.
- [ ] A surface in **Edit**: rename it in its header, rename and delete a page from its menu, delete a surface (the confirmation names the Auto Context rules that go with it).
- [ ] With LaunchDeck quit, put a damaged `workspace.json` in the data folder (a copy with its last character removed). On start, a banner says the surfaces could not be opened, the file is kept as `workspace.json.bak`, and **Show folder** opens the folder.

## 3. Auto Context

**Settings, Auto Context** should say the observer is available, watching through macOS application notifications.

- [ ] Bring Terminal, a browser and an editor to the front once each. Back in LaunchDeck they are offered under **Add a rule**, identified by bundle id (`com.apple.Terminal`, …), never by a window title.
- [ ] Application A has a rule: switching to it shows surface A. Application B: surface B.
- [ ] An application without a rule: nothing changes with **Keep the current surface**; the fallback surface appears when one is chosen.
- [ ] Pick a surface by hand while A is in front: the sidebar reads **Manual override**, and going to LaunchDeck and back to A does not undo it.
- [ ] **Resume Auto** applies A's rule at once. Switching to B also ends an override.
- [ ] In **Edit** nothing switches and the sidebar reads **Paused while editing**; back in **Live** the current application's rule is applied once.
- [ ] Quit and reopen: the rules and the Auto switch are kept, and `workspace.json` has a `context` entry with rules and nothing about what was in front.

Record, with no expected answer yet: what LaunchDeck's own window reads as (*LaunchDeck itself* in a bundled app; under `tauri dev` the binary has no bundle id and may read as nothing in front); what counts as in front for Spotlight, the Cmd+Tab switcher, Mission Control, a full-screen application and a menu-bar application's popover, and whether any of them makes the surface flicker; how fast the switch feels (on Linux it is about 210 ms, 150 ms of it the deliberate wait).

## 4. Secrets in the Keychain

Install the Home Assistant example from **Plugins**.

- [ ] Its card's **Secrets** section says the secret is kept in your system's credential store (Keychain), and offers a field, not *Unavailable*. **Settings, Plugins & permissions** says the same.
- [ ] Save any value as the token. In **Keychain Access**, login keychain, search `LaunchDeck plugin`: one entry, `LaunchDeck plugin example.home-assistant`, account `token`.
- [ ] `grep -r "<the value>" ~/Library/Application\ Support/dev.launchdeck.app/` finds nothing.
- [ ] **Remove** the secret on the card: the entry is gone from Keychain Access.
- [ ] Legacy plaintext secrets: with a schema 1-3 plugin whose `plugins.json` entry holds a plaintext secret, the card shows **Legacy insecure secret**. **Migrate to secure storage** moves it into the Keychain (entry `LaunchDeck plugin <id>`) and removes it from `plugins.json`; on another copy, **Remove legacy value** removes it without moving.
- [ ] Optional, knowingly: `cd src-tauri && cargo test --test secret_store -- --ignored` writes, reads and removes that same Keychain entry.

Record: whether macOS asks for permission when a rebuilt binary reads an entry an earlier build wrote, what the app shows meanwhile and what happens on *Deny* (acceptable: the secret is used, or nothing is sent and the card says why; a crash, an unexplained hang or a plain copy of the value is a bug); with the login keychain locked and LaunchDeck restarted, whether macOS offers to unlock it or the card says *Unavailable*.

## 5. Keys pressed for you

These need **System Settings, Privacy & Security, Accessibility**: for LaunchDeck when it runs as an `.app`, for the terminal that started it under `tauri dev`.

- [ ] Accessibility surface: volume up, volume down, mute, play/pause. Productivity surface: Copy, Paste, Undo, Redo, Search (Cmd shortcuts).
- [ ] Without the permission: does the pad turn red with a message, or does nothing happen? Silence is a finding.
- [ ] The starter pads that differ on macOS and have never been pressed: Development **Terminal** and **VS Code** (start Terminal and Visual Studio Code); Productivity **Files** (Finder) and **Screenshot** (Cmd+Shift+4).

## 6. A physical Launchpad

Only with a Launchpad X or Launchpad Mini MK3. Follow [Launchpad X](../user/launchpad-x.md#trying-it-on-real-hardware) or [Launchpad Mini MK3](../user/launchpad-mini-mk3.md#trying-it-on-a-unit), then run the acceptance suite, `sh scripts/launchpad-acceptance.sh` ([hardware testing](hardware-testing.md)), and commit its report under `docs/verification/`. In the window:

- [ ] **Settings, Devices** lists the unit as connected within a few seconds (or after **Look again**), with its firmware and **Programmer Mode (confirmed)**.
- [ ] The grid mirrors the virtual Launchpad pad for pad, in the right colours; the top row, side column and logo light and respond.
- [ ] A press runs the pad's action; a long press (500 ms) runs the long-press action.
- [ ] **Brightness**: the slider changes the unit's brightness; **Device's own level** gives it back. **Sleep** turns the LEDs off and **Wake** on. **Scroll a test text** scrolls "LaunchDeck" once; **Stop the text** stops it.
- [ ] Unplug: the unit is listed as unplugged and the app goes on with the simulator. Plug in again: it is taken again within a few seconds.
- [ ] **Disconnect**, and quitting from the tray and with Cmd+Q: the unit goes dark and returns to its own mode and layout.

## 7. A real Home Assistant

Follow [the example's setup](../../examples/plugins/home-assistant/README.md#set-it-up): address, **Allow**, token, a pad with **Toggle** and feedback from **Entity state**.

- [ ] Under **Network access** the request shows the exact address and, for an address on your network, *Your local network*.
- [ ] The pad shows the light's real state, goes pending on a press, and ends on or off as the light does.
- [ ] Change the light from Home Assistant's own interface: the pad follows within about 5 seconds (it polls).
- [ ] **Revoke** the address: the next press sends nothing and the card says why. **Allow** again: it recovers.

Record: whether macOS asks for **Local Network** access the first time (a request that times out with no reason usually means it was denied: System Settings, Privacy & Security, Local Network); the address by IP and by name (`homeassistant.local`) and what the prompt says for each; HTTPS, including a certificate the system does not trust; any difference between the real API and the stand-ins (`tools/desktop-e2e/lib/mock_home_assistant.py`, `src-tauri/tests/support/mock_home_assistant.rs`), which means the stand-in must be fixed first.

## Afterwards

Quit LaunchDeck, move the test data folder aside too (it is the evidence for the record), and move your own folder back. Write the record and update README "Verified where" and the CHANGELOG only from it.
