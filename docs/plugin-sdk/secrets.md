# Secrets

A secret is a value a plugin needs to use and has no reason to hold: an access token, an API key. In LaunchDeck the plugin **names** a secret and LaunchDeck **applies** it. The value never enters the plugin.

```json
"secrets": [
  {
    "key": "token",
    "label": "Long-lived access token",
    "description": "Create one in Home Assistant: your profile, Security, Long-lived access tokens.",
    "host": { "setting": "baseUrl" }
  }
]
```

```ts
http.request(new HttpRequest("GET", base + "/api/states/" + entity).bearer("token"));
```

`.bearer("token")` asks LaunchDeck to add `Authorization: Bearer <the value the person entered>` to this request. The plugin gets the response and nothing else.

## Three kinds of value, kept apart

| | Example | Declared in | Stored in | Does the plugin see it? |
| --- | --- | --- | --- | --- |
| **Setting** | The server's address | `config` | `plugins.json` (private file) | Yes, at `init` |
| **Secret** | The access token | `secrets` | The system's credential store, and nowhere else | **No** |
| **Action argument** | Which light | An action's `params` | The surface, in the workspace | Yes, at each call |

A token is not a setting, and an entity id is not a secret. Put each where it belongs: the surface can then be exported and shared without carrying an address or a token.

## Declaring one

Schema 4. At most 8.

| Field | Rule |
| --- | --- |
| `key` | A plain identifier, unique among the plugin's secrets **and settings**. What `.bearer(key)` names. |
| `label` | Required, 1 to 64 characters. The name of the field on the Plugins screen. |
| `description` | Optional, up to 280 characters. Say where the person gets the value. |
| `host` | Required. One of the entries of `capabilities.network.hosts`, written the same way. The only address this secret is ever sent to. |

A schema 4 manifest may not mark a `config` field `"secret": true`: that older flag hides a value from the screen but hands it to the module and stores it in `plugins.json`. Manifests of schemas 1 to 3 that use it keep working exactly as they did.

## What LaunchDeck does with it

* **Sent to one address only.** A request that names a secret and goes anywhere but the secret's `host` fails with `host_not_allowed`, and nothing is sent. So a plugin cannot post your token to another server, even one it was granted.
* **Sent only with a grant.** The secret is read after the request passed every check (see [Network](network.md#what-is-checked-in-order)) and is held only until the request is sent.
* **Dropped on a redirect to another address**, together with any `Authorization` and `Cookie` header.
* **Only as `Authorization: Bearer`.** There is no other scheme, no way to put a secret in a URL, a body or another header, and no way to read one back.
* **Missing means nothing is sent.** If the person has not entered it, the request fails with `secret_missing` and a sentence that says where to enter it.
* **No credential store means nothing is sent.** On a computer without a secure credential store, or with one that is locked, the request fails with `secret_unavailable`. LaunchDeck never falls back to a file. Pass the message on (`fail(r.message, r.error)`): it tells the person what is wrong.
* **Never written down elsewhere.** Not in the workspace, the manifest, `plugins.json`, `permissions.json`, the audit trail, a snapshot, an event or a log. The plugin's log names the method, the origin and the path of a request, never its query, headers or bodies.

A plugin could still learn a token that the *server* echoes back in a response; that is the server's doing. What LaunchDeck guarantees is that it does not hand the value over and does not send it anywhere but the address the person saw.

## Where the value is kept

In the operating system's credential store, or not at all:

| System | Store |
| --- | --- |
| macOS | Keychain |
| Windows | Credential Manager |
| Linux | The Secret Service (GNOME Keyring, KWallet and others) |

One entry per plugin and key, under the service name `LaunchDeck plugin <plugin id>`. A value is at most 4 KiB.

There is **no file fallback**. On a system where no credential store answers, secrets are *unavailable*: the Plugins screen says so and why, a secret cannot be saved, and a request that names one is not sent. Everything about this, including what happens to the plain `secrets.json` an earlier version could write and to the `secret` settings of older manifests, is in [Where secrets are kept](../security/secrets.md).

## On the Plugins screen

Each secret is a password field with **Save**, and **Remove** once it is set. The screen shows *Set* or *Not set* and never the value, not even masked. Saving a new value replaces the old one. Setting or removing a secret makes the plugin refresh at once, so a pad recovers, or says why it cannot, without a restart.

Without a credential store the section shows **Unavailable** with the reason, and the field is disabled.

## Limits of this version

* Bearer tokens only. No basic auth, no query-string keys, no OAuth flow and no token refresh.
* A secret is not removed when its plugin's folder is deleted. Remove the secret on the Plugins screen first; otherwise the entry stays in the credential store, unused, until you delete it there.
* Whether a credential store answers is checked once, when LaunchDeck starts. Starting a keyring afterwards needs a restart.
