# Where secrets are kept

A plugin's secret (an access token, an API key) is kept in **the operating system's credential store, or not at all**. LaunchDeck does not write a secret to a file of its own, on any system, for any reason.

| System | Store | Shown as |
| --- | --- | --- |
| macOS | Keychain Services (the login keychain) | Keychain |
| Windows | Credential Manager | Credential Manager |
| Linux and other Unix | The Secret Service over D-Bus (GNOME Keyring, KWallet, KeePassXC and others that provide it) | Secret Service keyring |

Entries are filed under the service name `LaunchDeck plugin <plugin id>`, with the secret's key as the account, so they can be seen and removed with the system's own tool (Keychain Access, Credential Manager, Seahorse or `secret-tool`).

The stores are reached through the [`keyring`](https://crates.io/crates/keyring) crate (4.2.0, published 29 August 2026), which uses each system's native API: Security.framework, the Windows credential API, and the Secret Service through a pure-Rust D-Bus client with an encrypted session. Nothing was changed there; it was already what LaunchDeck used when a store was present.

## When there is no store

Some systems have none that answers: a Linux session without a keyring daemon or without a session bus, a locked-down account. LaunchDeck checks once, when it starts.

```text
a credential store answers        →  secrets can be saved and used
no credential store answers       →  secrets are unavailable, and the screen says why
```

What "unavailable" means, exactly:

* On **Plugins**, the *Secrets* section of a plugin shows **Unavailable**, the reason, and what would change it. The field and its Save button are disabled.
* Saving a secret through the API is refused with *no secure credential store is available*.
* A request that names a secret is **not sent**. It fails with the code `secret_unavailable` and a sentence saying why, which the pad's failure and the plugin's card show. A request that names no secret is not affected, and neither is anything else the plugin does.
* **Nothing is written instead.** There is no `secrets.json`, no setting, no environment variable that turns a file into a store. A test fails if a store that is not the system's, none, or the in-memory one for tests is ever added.

To get out of that state on Linux: install and unlock a Secret Service provider (`gnome-keyring`, KWallet, KeePassXC with its Secret Service integration), make sure a D-Bus session bus exists, and restart LaunchDeck. The check is made once per start.

A store that exists but is locked asks to be unlocked in the system's own dialog. If it stays locked, requests that need a secret fail with `secret_unavailable` and are not sent.

## What a secret never touches

For a secret declared by a manifest of schema 4 (see [plugin secrets](../plugin-sdk/secrets.md)):

| Place | Holds the value? |
| --- | --- |
| The workspace (`workspace.json`), exported surfaces | Never |
| The plugin's manifest and folder | Never |
| `plugins.json` (settings), `permissions.json` (grants) | Never |
| The audit trail, the plugin's log, LaunchDeck's log | Never. A request is logged as method, address and path only. |
| Events, snapshots, anything sent to the interface | Never. The interface learns only whether a value is set. |
| The plugin's WebAssembly memory | Never. The plugin names the secret; LaunchDeck adds `Authorization: Bearer …` to the request after every check has passed, and only for the one address the secret is bound to. |

These are asserted by tests against the real example plugin and a real local server (`src-tauri/tests/secret_store.rs`, `home_assistant.rs`, `plugin_network.rs`), and in the real desktop app with a real Secret Service (`tools/desktop-e2e`, suites `secrets` and `home_assistant`), which search every file of the data folder, the keyring's own files included, for the token.

## The plain file of an earlier version

The first release with secrets fell back to `secrets.json` in the app data folder, readable by the owner only and not encrypted, when no credential store answered. That fallback is gone. An installation that used it may still have the file.

What LaunchDeck does with it now is fixed and the same every time:

1. **It is never used.** No value in it authenticates a request, with or without a credential store. A secret that exists only there reads *Not set* (or *Unavailable*).
2. **It is reported.** *Plugins* shows a notice, *Secrets kept in plain text by an earlier version*, listing each one by plugin and name. Never by value.
3. **Nothing happens to it by itself.** It is not migrated, changed or deleted until you press a button.
4. **Move to *(your store)*.** Offered only when a credential store answers. For each secret, in a fixed order (plugin id, then key):
   * if the credential store already holds a value for it, that value is kept (it is the one entered after the file stopped being used) and the plain one is dropped;
   * otherwise the value is written to the store and **read back**; it counts as moved only if what comes back is what was written.

   Only when **every** secret in the file is safely in the store is the file overwritten with zeros, flushed, and removed. If one could not be moved (a value too long for the store, for example), the file is left whole, the ones that did move are in the store, and the notice stays. Pressing Move again is safe: it finds them already there.
5. **Delete without moving.** Always offered, behind a second confirmation. The file is overwritten and removed.
6. A file this version cannot read (damaged, or written by a newer version) is reported as such and can only be deleted.

Each of these is recorded in the audit trail (`secrets.migrated`, `secrets.legacy_discarded`) with counts and no values.

### What "removed" can and cannot promise

Overwriting a file's bytes before deleting it is the most a program can do, and it is **not a guarantee**. A journaling or copy-on-write filesystem (APFS, Btrfs, ZFS), an SSD's wear levelling, a snapshot or a backup may still hold an earlier copy of those bytes. If the token mattered, **revoke it at its source and create a new one** after moving. LaunchDeck says this next to the button.

## Secrets of older plugins

Manifests before schema 4 could mark a *setting* as `"secret": true`. That was never a credential store: the value is saved with the plugin's settings in `plugins.json` (owner-only, not encrypted) and is handed to the plugin when it starts.

| | Schema 1 to 3, `config[].secret` | Schema 4, `secrets` |
| --- | --- | --- |
| Stored | `plugins.json`, in the clear | The system's credential store |
| Reaches the plugin's code | Yes | No |
| Without a credential store | Works as before | Unavailable |
| Can leave the computer | No: these plugins have no network access | Only to the one address it is bound to |

**They keep working, unchanged.** Changing how an installed plugin receives its configuration would break it, and such a plugin cannot reach the network (the network capability needs schema 4), so the exposure is the plain file on disk. What is new:

* The setting is flagged on the plugin's card: *An older kind of secret … stored on this computer without encryption and is handed to the plugin. It keeps working as it did.*
* The plugin's log says the same when it starts.
* A schema 4 manifest that marks a setting `secret` is **refused**. Nothing new can be stored this way.

**The way out is the plugin's.** Its author moves to schema 4 and declares the value under `secrets`, with the same key. When you install that version, LaunchDeck finds the value the old version left in `plugins.json`, does not hand it to the new version, and lists it in the same notice as above (*left in the plugin's settings by an older version of it*). **Move** puts it in the credential store, reads it back, and removes it from `plugins.json`; **Delete without moving** just removes it.

## Testing against a real store

`cargo test` never touches the credential store of the machine it runs on: the store is injected, and the tests use an in-memory one or one that is unavailable.

```bash
sh scripts/test-secret-service.sh
```

runs the one test that does use a real store, inside a private D-Bus session with a GNOME Keyring started for it, in a temporary folder that is deleted afterwards, and then checks that the keyring's files hold no value in the clear. Linux only (`apt install dbus gnome-keyring`). The desktop suites do the same for the whole app.

On macOS and Windows that test has not been run: this repository was developed on Linux. The code path is the same crate with the native backend for each system.
