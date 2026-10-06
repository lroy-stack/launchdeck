# Plugins

A plugin adds actions a pad can run and values a pad can show: the Counter example counts, the Home Assistant example switches lights and shows whether they are on. An installed plugin runs in a sandbox with no network, no files, no shell and no access to your devices. One thing can be allowed, by you, one address at a time: **reaching the network through LaunchDeck**.

Status: installing, permissions and secrets are built and tested with the Rust tests and Vitest on macOS, with an in-memory credential store. The **Plugins** screen in the app window, the files inside a built bundle and the real macOS Keychain have not been checked by a person yet (NOT VERIFIED: the manual checks on the Mac have not been done).

## Installing a plugin

**Plugins, Example plugins** lists the examples that come with LaunchDeck: **Hello**, **Counter** and **Home Assistant**. **Install** next to one copies it into your plugins folder and turns it on. An example that asks for network access is marked so; it reaches nothing until you allow an address (below).

Any other plugin is a folder with a `plugin.json` and a `.wasm` file:

1. **Plugins, Show plugins folder** opens the folder LaunchDeck installs into (`plugins` in the [data folder](../getting-started/install.md#where-launchdeck-keeps-its-data)).
2. Put the plugin's folder in it.
3. Press **Rescan**. LaunchDeck says how many plugins were added, reloaded, removed, and how many folders have problems.

A folder that cannot be loaded is listed as **skipped**, with one sentence saying what is wrong and the details under **Details**. Nothing in a folder runs until LaunchDeck has checked its manifest and put it in the sandbox. LaunchDeck never downloads or installs a plugin by itself, and a shared surface never installs one ([sharing](sharing.md#missing-plugins)).

**Plugins are not signed.** LaunchDeck cannot tell who made a plugin or whether its files were changed. Install plugins only from people you trust.

To remove a plugin, turn it off, move its folder out of the plugins folder, and press **Rescan**. Its network decisions stay in `permissions.json` and its secrets in the credential store until you remove them; pads that use it read as needing it.

Writing a plugin: the [Plugin SDK](../plugin-sdk/README.md).

## The plugin's card

Each installed plugin has a card on **Plugins** with its name, version, id, author and folder, and:

* **Disable** and **Enable.** A plugin that is off does nothing at all. A plugin that stopped after a problem says what kind of problem in a sentence (details under **Details**) and offers **Reload**.
* **Show log**: what the plugin and LaunchDeck wrote about it.
* **Network access**, **Settings** and **Secrets**, described below.
* The actions it offers, the states it publishes (the values pads can follow) and the events it sends.

The plugin's actions appear in the [Action Library](action-library.md) labelled *Plugin: its name*; its states and events appear in the [feedback editor](feedback-editor.md).

**Settings, Plugins & permissions** says where secrets are kept on this computer, how many addresses wait for your decision, and has **Show plugins folder** and **Open Plugins**.

## Network access

This part is about what you see and decide. The plugin author's side is in [Capabilities](../plugin-sdk/capabilities.md).

### What a plugin can ask for

A plugin's manifest lists the addresses it would like to reach and says why. That is a request. It allows nothing: every address starts as **Needs approval**, and stays that way until you answer.

### The prompt

On **Plugins**, the plugin's card has a **Network access** section:

```text
Home Assistant requests network access to:
  http://192.168.1.20:8123     Your local network
  from the setting “Home Assistant address”
PURPOSE  Connect to your Home Assistant instance.
It will be able to send requests to this address and read the answers, and to no other.
You can take this back at any time.
                                                            [ Deny ]  [ Allow ]
```

* **The address is exact**: the scheme (`http` or `https`), the host and the port. Never "network access" in general, never a wildcard.
* **How far it reaches** is said in words:

  | Shown as | Means |
  | --- | --- |
  | This computer | A service running on this machine (`localhost`, `127.0.0.1`). |
  | Your local network | A device in your home or office network. |
  | The internet | A public server. |

  LaunchDeck enforces this when it connects: a name approved as an internet address is refused if it turns out to point into your network or at this computer.
* **The purpose** is the plugin author's sentence. Check that it fits what the plugin says it does.
* When the address comes from one of the plugin's own settings (your server's address, which only you know), the prompt says which setting. The request is for what the setting holds **now**; changing the setting asks again, for the new address.

The sidebar shows a count next to **Plugins** while a request is waiting.

### The three states

| Status | Meaning | What you can do |
| --- | --- | --- |
| **Needs approval** | Asked for, not decided. Nothing is sent. | Allow, Deny |
| **Granted** | Requests to this one address go out. | Revoke |
| **Denied** | You said no. Nothing is sent, and the request is not counted as waiting. | Allow |

**Revoke** takes effect on the next request; nothing needs restarting. A pad that follows a state from that plugin goes back to its resting look, because what can no longer be read is not shown; pressing it fails, and the card says why: *Last request did not go through. Access to … is waiting for approval in Plugins. (permission_denied)*. **Allow** again and it recovers by itself within a few seconds.

### Rules LaunchDeck keeps

* **Nothing is granted by installing, enabling or updating a plugin.** Only your click on **Allow**.
* **An update never widens access.** If a new version asks for another address, that address needs its own approval. The old grants stay; no new one appears.
* **A grant is for one plugin and one address.** It does not cover another port, another scheme (`http` versus `https`), a subdomain, or another plugin.
* **A redirect cannot lead out.** If a server redirects to an address you did not grant, the request ends there and nothing is sent to it.
* **Decisions about addresses a plugin no longer asks for** are listed with **Forget**. They allow nothing.
* **Turning a plugin off** stops everything it does, whatever it was granted. The grants are kept for when you turn it on again.
* **A grant belongs to the plugin's id, not to its files.** So do its secrets and settings. When the plugin's folder is replaced, by an update or by a different plugin that uses the same id, the new files get the same grants and their requests carry the same secrets. LaunchDeck does not check that the module is the one you allowed (module hashing is not implemented). Take updates only from the plugin's author.

### Where decisions are kept

In `permissions.json`, in LaunchDeck's data folder next to `workspace.json`, readable by your account only. It is **not** part of the workspace and not inside the plugin's folder: exporting a surface, sharing a workspace file or copying a plugin carries no grant. On another computer every address is *Needs approval* again.

Each decision is also written to the audit trail (`audit.jsonl`, same folder): `permission.requested` the first time a plugin asks for an address, then `permission.granted`, `permission.denied` or `permission.revoked`, with the plugin and the address. No tokens, headers or response bodies are ever written there.

## Secrets

A plugin that needs a token declares a **secret**. You type it once on the plugin's card. LaunchDeck keeps it in your system's credential store (Keychain, Credential Manager, or the Secret Service keyring on Linux), adds it to the plugin's requests to the one address it belongs to, and never hands it to the plugin or shows it again. See [Secrets](../plugin-sdk/secrets.md).

If your system has no credential store that answers, the card says **Unavailable** and why. LaunchDeck does not keep a secret in a plain file instead, so what needs the secret does not work until a store is available. See [where secrets are kept](../architecture/secrets.md).

A sensible order for a new plugin: set its address, **Allow** the address, then enter the token.

## Secrets an older version kept in plain text

Plugins written for manifest schemas 1 to 3 could mark a setting as secret, and earlier versions of LaunchDeck kept such values unencrypted, in `plugins.json` (or in a separate `secrets.json`). **LaunchDeck no longer uses those values**: a secret is read from the system's credential store only, so the plugin does not get it until you decide.

* On the plugin's card, **Legacy insecure secret** names each value. **Migrate to secure storage** writes it to the credential store, reads it back, and only then removes the plain copy. **Remove legacy value** deletes it without moving, after a confirmation. If this computer has no credential store, the value cannot be moved and the card says why.
* At the top of **Plugins**, **Secrets kept in plain text by an earlier version** lists every such value, also those of plugins no longer installed, with **Move to** your credential store for all of them at once, or **Delete without moving**.
* A value that was moved can be used only by a plugin version that declares it as a secret (manifest schema 4). Until such a version is installed, the card says so.

Overwriting a file is the most a program can do: a backup or the disk itself may still hold an older copy. Replace a token that matters.

Tested with an in-memory store; migrating into the real macOS Keychain is NOT VERIFIED (the manual checks on the Mac have not been done). How it works: [where secrets are kept](../architecture/secrets.md).

## What this does not do

* It does not judge the plugin. A plugin you allow to reach a server can send that server anything it knows: its own settings and the arguments of the actions you press. It knows nothing about your pads, other plugins or your files.
* It does not use a system proxy. Requests go directly; on a network that only allows traffic through a proxy, internet addresses will not be reachable.
* It covers HTTP and HTTPS requests only. There is no permission for files, commands or other kinds of connection, because plugins cannot do those at all.
* Plugins are not signed. Install plugins from people you trust; see the [security model](../plugin-sdk/security.md).
