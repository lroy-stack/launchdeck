# Capabilities

A capability is something the sandbox does not have and LaunchDeck can do on a plugin's behalf. There is one in this release: **network**. Nothing else exists (no files, no shell, no sockets), and a manifest that asks for anything else is refused.

A plugin never gets the thing itself. It describes what it wants done, LaunchDeck checks that against what the person allowed, and LaunchDeck does it:

```text
plugin  ─►  capability API  ─►  permission check  ─►  LaunchDeck's own HTTP client  ─►  an approved address
```

## Request, grant, use

Three things that are kept apart on purpose:

| | Where it lives | Who writes it | What it means |
| --- | --- | --- | --- |
| **Request** | `plugin.json`, under `capabilities` | The plugin's author | "This plugin would like to reach these addresses, for this reason." |
| **Grant** | `permissions.json` in the app data folder | The person, on the Plugins screen | "I allow this plugin to reach this one address." |
| **Use** | A host function, checked on every call | The plugin, while it runs | One request, sent only if it is both requested and granted. |

A request is not a grant. Installing or enabling a plugin allows nothing; every address starts as *Needs approval*.

## Declaring it

Capabilities need manifest schema 4, where `capabilities` is an object:

```json
{
  "schemaVersion": 4,
  "capabilities": {
    "network": {
      "purpose": "Connect to your Home Assistant instance.",
      "hosts": ["https://api.example.com", { "setting": "baseUrl" }]
    }
  }
}
```

| Field | Rule |
| --- | --- |
| `purpose` | Required, 1 to 280 characters. Shown to the person next to the address, in your words. Say what the plugin does with the access. |
| `hosts` | 1 to 16 destinations, none listed twice. Each is an **origin** written out, or `{ "setting": key }`. |

An origin is a scheme, a host and an optional port, and nothing more: `https://api.example.com`, `http://192.168.1.20:8123`. The scheme must be `http` or `https` and must be written. A path, a query, a fragment, a user name, a wildcard or a trailing dot is an error, so what the manifest asks for is exactly what the person is shown. `http://ha.local` and `https://ha.local` are different origins, and so are two ports.

`{ "setting": "baseUrl" }` means "the origin of the URL the person typed in the setting `baseUrl`". It is for a server only the person knows the address of. The setting must be a `string` in `config`; the request is then for the one origin it currently holds, and changing the setting asks for the new one (the old grant stays on file and covers nothing).

Schemas 1 to 3 keep their `capabilities` array, which must stay empty.

## How a grant behaves

* **Denied by default.** A request goes out only when the current manifest asks for that origin **and** a grant for exactly that origin exists.
* **Never widened.** An update that asks for another address gets *Needs approval* for that address only. What was granted stays granted; nothing new is granted by updating.
* **Per origin, per plugin.** A grant for one plugin means nothing to another, and a grant for one port means nothing for the next.
* **Revocable at any time**, on the Plugins screen. The next request fails with `permission_denied`; nothing has to be restarted.
* **Stays on this computer.** `permissions.json` is next to the workspace, not in it, and not in the plugin's folder. Exporting a surface or copying a plugin carries no grant.
* **A plugin runs whatever its grants are.** Its actions appear and can be pressed; the ones that need the network fail with a sentence that says access is waiting for approval.

The person's side of this is described in [Plugin permissions](../user/plugins.md).

## What else schema 4 adds

| Field | For | Page |
| --- | --- | --- |
| `capabilities.network` | Reaching approved addresses | [Network](network.md) |
| `secrets` | A token LaunchDeck keeps and applies, which the plugin never sees | [Secrets](secrets.md) |
| `refresh` | Being asked, every few seconds, for the states pads are showing | [Network](network.md#keeping-states-current-refresh) |
| `config[].label` | A readable name for a setting on the Plugins screen | [Manifest](manifest.md#config) |

## Not built

Files, shell commands, raw sockets, TCP or UDP, MQTT, WebSockets, OAuth, and any way for a plugin to draw its own permission or settings screen. A second capability would be a second key under `capabilities` with its own grants; nothing is prepared for one that does not exist.
