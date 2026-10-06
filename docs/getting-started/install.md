# Install LaunchDeck

LaunchDeck 0.3.0 is the first external alpha. This page says what you can install today, where LaunchDeck keeps its data, and how to start over.

## What exists for 0.3.0

| System | What there is | Status |
| --- | --- | --- |
| macOS 11 or later, Apple silicon | Build it from source ([build from source](build-from-source.md)). A `.app` and a `.dmg` can be built with `npx tauri build --bundles app,dmg` | Built from source and tested here (macOS 15.4.1, Apple M2). No bundle has been built or opened here yet: NOT VERIFIED |
| Linux | `.deb` and `.rpm` packages built by the Bundle workflow on GitHub Actions | NOT VERIFIED: the workflow has never run (GitHub Actions does not start, account billing), and nothing in 0.3.0 has been built on Linux |
| Windows | An NSIS installer built by the same workflow | NOT VERIFIED: never built, on any machine |

There is no installer you can download yet; one is added when the release is validated. Not planned for this alpha: MSI, AppImage, Intel or universal macOS builds, and automatic updates.

## Opening a build on macOS

A LaunchDeck build is not signed with an Apple Developer certificate and not notarized. macOS therefore refuses to open a copy that was downloaded (one that carries the quarantine flag) the first time. To open it anyway:

1. Try to open LaunchDeck once and close the warning.
2. Open **System Settings**, **Privacy & Security**, scroll to the message about LaunchDeck and press **Open Anyway**.
3. Confirm with your password.

This is the usual macOS path for unsigned applications. It has not been tried with a LaunchDeck download (NOT VERIFIED): a build made on the same Mac carries no quarantine flag, so macOS does not ask. Only open a build that you made yourself or that comes from someone you trust.

The first time a pad presses keys for you (the Productivity and Accessibility starters do), macOS asks for **Accessibility** permission. Under `npm run tauri dev` it asks for the terminal that started LaunchDeck; for a built `.app`, for LaunchDeck. The first export to Downloads may ask for access to the Downloads folder. Neither prompt has been seen in the app yet (NOT VERIFIED).

## Where LaunchDeck keeps its data

Everything LaunchDeck saves is in one folder, named after its bundle identifier `dev.launchdeck.app`:

| System | Folder |
| --- | --- |
| macOS | `~/Library/Application Support/dev.launchdeck.app/` |
| Linux | `$XDG_DATA_HOME/dev.launchdeck.app/`, usually `~/.local/share/dev.launchdeck.app/` |
| Windows | `%APPDATA%\dev.launchdeck.app\` |

**Settings, Developer, Data folder** shows the exact path on your computer and has a **Show data folder** button. The Linux and Windows paths follow Tauri's rule for the application data folder; for 0.3.0 only the macOS one was seen.

What is in it:

| File or folder | What it holds |
| --- | --- |
| `workspace.json` | Your surfaces, pages, pads, preferences and Auto Context rules. A file that could not be read is kept as `workspace.json.bak`; older formats keep a `.v2.bak` or `.v3.bak` copy when they are converted. |
| `plugins/` | The plugins you installed, one folder each. |
| `plugins.json` | Which plugins are on, and their settings. |
| `permissions.json` | The network addresses you allowed or denied, per plugin. |
| `agents.json`, `audit.jsonl` | Agent Control's settings and grants, and the audit trail. |

Secrets are not in this folder: they are in your system's credential store (Keychain on macOS), or nowhere. Exports go to your Downloads folder.

## Starting over

To start as on a new computer, quit LaunchDeck and **move** the data folder aside (rename it, for example to `dev.launchdeck.app.old`). Do not delete it: it is the only copy of your surfaces. LaunchDeck creates a new folder with the starter surfaces and shows the first-run screen. Secrets that plugins stored stay in the credential store until you remove them on the plugin's card.

To bring back only the starter surfaces and keep everything else, use **Settings, Workspace, Reset to starter surfaces**.

Next: [the first run](first-run.md).
