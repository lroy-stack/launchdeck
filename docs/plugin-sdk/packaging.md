# Packaging and installing

## The package is a folder

```
acme.notes/            any folder name; the manifest id is what counts
  plugin.json
  notes.wasm
```

That folder is the whole artifact. Nothing is compiled, downloaded or executed at install time, and the format is the same on every system: the same bytes have been run on Linux and macOS (by the test suites), and have not been run on Windows. To ship it, zip the folder; to install it, unzip into the plugins folder.

There is no package format beyond the folder yet. A zip is only a way to carry it: LaunchDeck does not open zip files and has no "install from a file". No checksum or signature is made or checked, and grants are not tied to a module's contents. The manifest does not block adding them later (see the [marketplace roadmap](../roadmap/marketplace.md)). There is no marketplace. The **Install** buttons on the Plugins screen install only the example plugins that ship with the app (Hello, Counter and Home Assistant), by copying their folders into the plugins folder.

## Where plugins are searched

1. The `plugins` folder inside LaunchDeck's app data folder (create it if it does not exist):

   | System | Folder |
   | --- | --- |
   | macOS | `~/Library/Application Support/dev.launchdeck.app/plugins/` |
   | Windows | `%APPDATA%\dev.launchdeck.app\plugins\` |
   | Linux | `~/.local/share/dev.launchdeck.app/plugins/` |

2. Every folder listed in the `LAUNCHDECK_PLUGINS` environment variable (a path list: `:` on macOS and Linux, `;` on Windows), for development.

**Show plugins folder** on the Plugins screen creates the first one if needed and shows it. Each of those folders contains plugin folders. Only direct children are read. If two folders provide the same plugin id, the earlier one in this order wins and the other is listed under **Installed plugins** as skipped.

## When plugins load

LaunchDeck scans at startup and when you press **Rescan** on the Plugins screen. A rescan keeps unchanged plugins running (so their in-memory state survives), restarts plugins whose files changed, removes plugins whose folder is gone, and gives plugins that stopped on an error another try. A plugin is "changed" when the bytes of its manifest or module differ from the ones loaded.

## Status

| Status | Meaning |
| --- | --- |
| *on* (enabled) | Loaded and healthy. |
| *disabled* | The person turned it off. Never loaded and never invoked. A pad that calls it fails with a message saying the plugin is disabled. |
| *error* | It failed validation, failed to load or start, or was stopped after a trap or limit. The reason is on the card. Pads that call it fail with a message saying the plugin is unavailable, followed by the reason. **Reload** tries again. |

Enabled or disabled is remembered across restarts. A failure never changes that choice, so a plugin that crashed stays "wanted" and recovers on the next reload or rescan.

## Settings

Declared in the manifest (`config`) and edited on the plugin's card. Changing one restarts the plugin so it reads the new value. Values are stored in `plugins.json` in the app data folder, next to `workspace.json`. The file is written atomically and owner-only on Unix, and it is never part of a workspace or a shared surface, so a surface can be exported without exposing a token. If `plugins.json` is damaged or written by a newer LaunchDeck, it is copied to `plugins.json.bak` and the plugins start with their defaults.

```json
{ "schemaVersion": 1,
  "plugins": { "acme.notes": { "enabled": true, "config": { "limit": 100 } } } }
```

Do not edit it by hand while LaunchDeck runs.

Two more things belong to an installed plugin and are kept apart from its settings, from the workspace and from the plugin's folder:

| | Kept in | Travels with a surface or a plugin folder? |
| --- | --- | --- |
| Network grants | `permissions.json` in the app data folder (owner-only) | No. See [plugin permissions](../plugins/permissions.md). |
| Secrets | The system's credential store. Never a file: without a store they are unavailable | No. See [secrets](secrets.md). |

Changing a setting that a network address comes from asks for the new address; the grant for the old one stays on file and allows nothing.

## Updating a plugin

Replace the files in the plugin's folder and press **Rescan**: the plugin restarts with the new module. Its settings, its network grants and its secrets stay, because they belong to the plugin's **id**, not to its files. A new version that asks for another address gets nothing for it until the person allows it. The same holds for any module that declares the id: LaunchDeck does not compare a new module with the one the person installed (module hashing is not implemented), so a different plugin put in the folder under the same id inherits the grants and the use of the secrets. See [security](security.md#what-is-not-claimed).

## Using a plugin from a surface

A plugin's actions appear in the **Action Library** in the Surface editor, listed as *Plugin: Notes*, as soon as the plugin is found. A person searches for one, assigns it to a pad and fills in its form; there is nothing to configure in the plugin or the host. The pad stores `{ "provider": "acme.notes", "action": "notes.add", "args": { "text": "hi" } }` and follows what the plugin publishes with an event rule on `provider: "acme.notes"`.

While a plugin is disabled or failed to load, its actions stay in the library as *Unavailable* with the reason, and pads that use them keep their assignment and show a flag. A surface that names a plugin which is not installed at all still loads and can be edited; the assignment is shown as missing with its saved settings, and the pad reports the failure when pressed. That is deliberate, so sharing a surface never depends on every plugin being present. See [availability](../actions/availability.md).
