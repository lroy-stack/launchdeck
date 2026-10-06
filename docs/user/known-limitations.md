# Known limitations

What LaunchDeck 0.3.0 does not do, or has not been seen to do. It is an alpha. Every item is here because it is true of the code in this release or because nobody has checked it yet; where a check is missing, the reason is given. Where things were run is in [the verification records](../verification/README.md).

## What has not been checked

* **Nobody has used 0.3.0 in the app window.** The window, the tray, Auto Context in the app, the macOS Keychain and keys pressed for you (Accessibility) were built and tested automatically on macOS 15.4.1 (Apple M2), but the checks a person does on the Mac ([manual checks](../contributing/manual-checks-macos.md)) have not been done. The same holds for the Plugins screen's Install button, export to Downloads and the file picker.
* **No Launchpad has been attached.** The Launchpad X and the Launchpad Mini MK3 follow Novation's manuals and are tested against simulated units. The port names each system gives a unit come from documents, not from a real system. See [Launchpad X](launchpad-x.md) and [Launchpad Mini MK3](launchpad-mini-mk3.md).
* **No real Home Assistant.** The Home Assistant example plugin was tested against a stand-in server only.
* **Continuous integration has never run.** The workflows are written; GitHub Actions does not start because of the account's billing ([CI](../contributing/ci.md)).

## Systems

* **macOS first.** 0.3.0 is built and tested on macOS on Apple silicon only. There is no Intel or universal build.
* **Windows has never been built**, on any machine. Its Auto Context observer has never been compiled.
* **Linux:** nothing in 0.3.0 has been built on Linux. An earlier milestone was built and tested there, including the window through the desktop suites.
* **Wayland is not supported** for Auto Context: Wayland does not tell applications which one has focus. LaunchDeck says so and works by hand.

## Installing

* **There is no installer for 0.3.0 yet**; you [build it from source](../getting-started/build-from-source.md). No automatic updates.
* **The macOS build is not signed with a Developer certificate and not notarized.** A downloaded copy is refused by macOS until you choose **Open Anyway** in System Settings, Privacy & Security ([install](../getting-started/install.md#opening-a-build-on-macos)). Not tried with a download.
* **Permission prompts on macOS** have not been seen in the app: Accessibility (for pads that press keys) and access to the Downloads folder (for export). If you said no to Downloads, export says so and where to change it.

## Devices

* After a **crash** or a forced quit, a Launchpad stays in Programmer Mode until LaunchDeck starts again or the unit is plugged in again.
* One program at a time can use a Launchpad. A DAW that takes it over wins after three times within 30 seconds.
* The Mini MK3's pulsing and flashing pads use the nearest colour of its fixed palette, at the unit's own pace.
* The simulator is a Launchpad X; there is no simulated Mini MK3 in the app.
* Settings scrolls a fixed test text; there is no field for your own text.
* Other controllers and other Launchpad models are not supported.

## Plugins and secrets

* **Plugins are not signed.** A plugin's grants, settings and secrets belong to its id, not to its files: a replaced folder with the same id gets them. Install plugins only from people you trust.
* A plugin's one capability is HTTP to addresses you allowed, one by one. No files, no shell, no sockets, no MIDI. Outside state is polled (every few seconds), not pushed.
* **Secrets need a system credential store** (Keychain, Credential Manager, a Secret Service keyring). Without one, secrets are unavailable; they are never written to a file.
* **Secrets that older plugins kept in plain text** (manifest schemas 1 to 3) are no longer used. They stay where they were until you **Migrate** them into the credential store or **Remove** them on the plugin's card ([plugins](plugins.md#secrets-an-older-version-kept-in-plain-text)); a migrated value is used only by a plugin version that declares it as a secret.
* There is no Uninstall button: a plugin is removed by moving its folder out and pressing Rescan.

## Agent Control

* Hook adapters (Claude Code, Codex) can only approve and reject permission requests; they cannot pause, resume, retry or stop a session.
* **Codex has never been run.** Claude Code was verified live in an earlier milestone.
* **The Agent Bridge is not authenticated.** Any program on this computer can register an agent on it; it stays read-only until you allow control.
* Eight agent pads; agent state is not kept across a restart. More in [Agent Control](agent-control.md#known-limitations).

## Auto Context

* Rules match applications only: no window titles, projects, times or conditions.
* On Windows, Store applications share one identity, and an elevated application cannot be identified.

## Sharing surfaces

* **The author of a file is not verified.** LaunchDeck checks that a Surface file or Pack was not damaged or changed since it was written, not who wrote it. Import files from people you trust; the preview shows everything a file would run before you import it.
* LaunchDeck never checks for newer versions of a file you imported, and never merges changes.

## Not built

Accounts, cloud sync, a marketplace, plugin signing, telemetry, an MCP server, mobile apps.
