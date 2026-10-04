# Plugin permissions

An installed plugin runs in a sandbox with no network, no files and no shell. One thing can be allowed, by you, one address at a time: **reaching the network through LaunchDeck**. This page is about what you see and decide. The plugin author's side is in [Capabilities](../plugin-sdk/capabilities.md).

## What a plugin can ask for

A plugin's manifest lists the addresses it would like to reach and says why. That is a request. It allows nothing: every address starts as **Needs approval**, and stays that way until you answer.

## The prompt

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

## The three states

| Status | Meaning | What you can do |
| --- | --- | --- |
| **Needs approval** | Asked for, not decided. Nothing is sent. | Allow, Deny |
| **Granted** | Requests to this one address go out. | Revoke |
| **Denied** | You said no. Nothing is sent, and the request is not counted as waiting. | Allow |

**Revoke** takes effect on the next request; nothing needs restarting. A pad that follows a state from that plugin goes back to its resting look, because what can no longer be read is not shown; pressing it fails, and the card says why: *Last request did not go through. Access to … is waiting for approval in Plugins. (permission_denied)*. **Allow** again and it recovers by itself within a few seconds.

## Rules LaunchDeck keeps

* **Nothing is granted by installing, enabling or updating a plugin.** Only your click on **Allow**.
* **An update never widens access.** If a new version asks for another address, that address needs its own approval. The old grants stay; no new one appears.
* **A grant is for one plugin and one address.** It does not cover another port, another scheme (`http` versus `https`), a subdomain, or another plugin.
* **A redirect cannot lead out.** If a server redirects to an address you did not grant, the request ends there and nothing is sent to it.
* **Decisions about addresses a plugin no longer asks for** are listed with **Forget**. They allow nothing.
* **Turning a plugin off** stops everything it does, whatever it was granted. The grants are kept for when you turn it on again.

## Where decisions are kept

In `permissions.json`, in LaunchDeck's data folder next to `workspace.json`, readable by your account only. It is **not** part of the workspace and not inside the plugin's folder: exporting a surface, sharing a workspace file or copying a plugin carries no grant. On another computer every address is *Needs approval* again.

Each decision is also written to the audit trail (`audit.jsonl`, same folder): `permission.requested` the first time a plugin asks for an address, then `permission.granted`, `permission.denied` or `permission.revoked`, with the plugin and the address. No tokens, headers or response bodies are ever written there.

## Secrets

A plugin that needs a token declares a **secret**. You type it once on the plugin's card. LaunchDeck keeps it in your system's credential store (Keychain, Credential Manager, or the Secret Service keyring on Linux), adds it to the plugin's requests to the one address it belongs to, and never hands it to the plugin or shows it again. See [Secrets](../plugin-sdk/secrets.md).

If your system has no credential store that answers, the card says **Unavailable** and why. LaunchDeck does not keep a secret in a plain file instead, so what needs the secret does not work until a store is available. See [where secrets are kept](../security/secrets.md).

A sensible order for a new plugin: set its address, **Allow** the address, then enter the token.

## What this does not do

* It does not judge the plugin. A plugin you allow to reach a server can send that server anything it knows: its own settings and the arguments of the actions you press. It knows nothing about your pads, other plugins or your files.
* It does not use a system proxy. Requests go directly; on a network that only allows traffic through a proxy, internet addresses will not be reachable.
* It covers HTTP and HTTPS requests only. There is no permission for files, commands or other kinds of connection, because plugins cannot do those at all.
* Plugins are not signed. Install plugins from people you trust; see the [security model](../plugin-sdk/security.md).
