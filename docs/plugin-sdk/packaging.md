# Packaging and installing

## The package is a folder

```
acme.notes/            any folder name; the manifest id is what counts
  plugin.json
  notes.wasm
```

That folder is the whole artifact. Nothing is compiled, downloaded or executed at install time, and the same bytes run on macOS, Windows and Linux. To ship it, zip the folder; to install it, unzip into the plugins folder. Compressed bundles, checksums and signatures are not part of the format yet and the manifest does not block adding them (see the [marketplace roadmap](../roadmap/marketplace.md)). There is no marketplace.

## Where plugins are searched

1. The `plugins` folder inside LaunchDeck's app data folder (create it if it does not exist):

   | System | Folder |
   | --- | --- |
   | macOS | `~/Library/Application Support/dev.launchdeck.app/plugins/` |
   | Windows | `%APPDATA%\dev.launchdeck.app\plugins\` |
   | Linux | `~/.local/share/dev.launchdeck.app/plugins/` |

2. Every folder listed in the `LAUNCHDECK_PLUGINS` environment variable (a path list: `:` on macOS and Linux, `;` on Windows), for development.

Each of those folders contains plugin folders. Only direct children are read. If two folders provide the same plugin id, the earlier one in this order wins and the other is listed under **Installed plugins** as skipped.

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
| Secrets | The system's credential store, or `secrets.json` (owner-only) when there is none | No. See [secrets](secrets.md). |

Changing a setting that a network address comes from asks for the new address; the grant for the old one stays on file and allows nothing.

## Using a plugin from a surface

A plugin's actions appear in the **Action Library** in the Surface editor, listed as *Plugin: Notes*, as soon as the plugin is found. A person searches for one, assigns it to a pad and fills in its form; there is nothing to configure in the plugin or the host. The pad stores `{ "provider": "acme.notes", "action": "notes.add", "args": { "text": "hi" } }` and follows what the plugin publishes with an event rule on `provider: "acme.notes"`.

While a plugin is disabled or failed to load, its actions stay in the library as *Unavailable* with the reason, and pads that use them keep their assignment and show a flag. A surface that names a plugin which is not installed at all still loads and can be edited; the assignment is shown as missing with its saved settings, and the pad reports the failure when pressed. That is deliberate, so sharing a surface never depends on every plugin being present. See [availability](../actions/availability.md).
