# Where secrets are kept

A plugin's secret (an access token, an API key) is kept in **the operating system's credential store, or not at all**. LaunchDeck does not write a secret to a file of its own, on any system, for any reason.

**LaunchDeck never hands a plaintext secret stored in `plugins.json` to a plugin, whatever the version of its manifest.** A credential is used from the credential store, or it is unavailable. A plain value an earlier version left behind stays where it is, unused, until the person moves it into the credential store or removes it ([below](#secrets-of-older-plugins)).

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
| `plugins.json` (settings), `permissions.json` (grants) | Never written there. A plain value an earlier version left in `plugins.json` is never read to be used ([below](#secrets-of-older-plugins)). |
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

Manifests before schema 4 could mark a *setting* as `"secret": true`. That was never a credential store: earlier versions saved the value with the plugin's settings in `plugins.json` (owner-only, not encrypted) and handed it to the plugin when it started. That is over. A value an installation still has is kept, untouched, and **never used**.

| | Schema 1 to 3, `config[].secret` | Schema 4, `secrets` |
| --- | --- | --- |
| Where a value is kept | Nowhere new: such a setting can no longer be given a value. One an earlier version left in `plugins.json` stays there until you act | The system's credential store |
| Reaches the plugin's code | **No**. The module reads the setting as not set, like an optional setting nobody filled in | No |
| Used to authenticate | No | Yes, by the broker, for the one address it is bound to |
| Without a credential store | Unavailable | Unavailable |

What LaunchDeck does, exactly:

1. **It is never used.** A setting marked `secret` is left out of the configuration a module receives, and the host drops every credential key at the module boundary (`init`) whatever it is given. The plugin loads and runs; only what needs that value does not work. LaunchDeck starts as usual.
2. **It is never changed by itself.** Starting, reloading, rescanning, saving another setting, disabling or enabling the plugin leave the plain value exactly as it is. Typing a new value for such a setting is refused: it could only be kept in plain text.
3. **It is reported, by name.** The plugin's card shows **Legacy insecure secret**: *This plugin stores a credential using an obsolete plaintext format. LaunchDeck will no longer use this value. Move it to secure storage to continue using this plugin.* It names the key, never the value, and offers two buttons, **Migrate to secure storage** and **Remove legacy value**. There is no option to keep using the value. The plugin's log says the same when it starts. The notice above (*Secrets kept in plain text by an earlier version*) lists it too, *in the plugin's settings*.
4. **Migrate to secure storage.** Offered only when a credential store answers; without one the button is disabled and the store's reason is shown. For each plain value of the plugin: it is written to the credential store under the plugin id and the key, **read back and compared**, and only when every one of them is there are they removed from `plugins.json`, which is saved atomically. If any step fails (the store refuses, gives back something else, or already holds a *different* value for that key), `plugins.json` is not touched and the card says why in a sentence. A value the store already holds unchanged counts as moved, so trying again is safe.
5. **After migrating**, the value is in the credential store. A module still never receives it: a plugin whose manifest is older than schema 4 cannot use it, and its card says that it needs a version that declares this credential under `secrets` (manifest schema 4). When such a version is installed, it declares the same key under `secrets` and LaunchDeck applies the value through the network broker, as for any schema 4 secret.
6. **Remove legacy value.** Behind a second confirmation. The plain value is removed from `plugins.json`; nothing else changes.

Each step is recorded in the audit trail (`secrets.migrated`, `secrets.legacy_discarded`, with the plugin id and counts), and in the plugin's log by key. No value reaches an error, the audit trail, a log, the interface or a contract fixture.

A schema 4 manifest that marks a setting `secret` is **refused**. The same rules apply to a value an older version of a plugin kept as a setting under a key its current schema 4 manifest declares under `secrets`: listed, never handed to the module, moved or removed on request; once moved, the plugin uses it.

**Move** in the notice above does the same for every plugin at once, value by value: a value in `plugins.json` is removed only when the credential store holds exactly that value afterwards.

These rules are tested with an in-memory store, a store that refuses every write and one that gives back something else, against a module that answers with the configuration it was handed, for manifests of schemas 1 to 4 (`src-tauri/tests/legacy_plaintext_secrets.rs`, `secret_store.rs`), and the card in Vitest (`src/features/plugins/legacySecret.test.tsx`). Migrating into the real macOS Keychain, and the card in the running app, are **NOT VERIFIED**: the macOS checks by hand have not been done (reason (f) of the productization plan).

## Testing against a real store

`cargo test` never touches the credential store of the machine it runs on: the store is injected, and the tests use an in-memory one or one that is unavailable.

```bash
sh scripts/test-secret-service.sh
```

runs the one test that does use a real store, inside a private D-Bus session with a GNOME Keyring started for it, in a temporary folder that is deleted afterwards, and then checks that the keyring's files hold no value in the clear. Linux only (`apt install dbus gnome-keyring`). The desktop suites do the same for the whole app.

That test has been run on Linux, against a real Secret Service. On macOS the Keychain path compiles, and the tests pass there with the in-memory store; the real Keychain is **NOT VERIFIED**: the check by hand ([4.3 in NEXT_PHASE.md](../../NEXT_PHASE.md#43-secrets-in-the-keychain)) has not been done. On Windows nothing has been built or run. The code path is the same crate with the native backend for each system.
