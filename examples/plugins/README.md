# Example plugins

Five plugins that live outside the LaunchDeck core and are loaded exactly like a third party's would be. They are also the fixtures of the SDK tests (`src-tauri/tests/plugin_sdk.rs`, `plugin_states.rs`, `home_assistant.rs`).

## The reference set

The plugins to copy from. They ship with the app, and the Plugins screen installs each with one click.

| Folder | Id | Shows |
| --- | --- | --- |
| `hello` | `example.hello` | **Minimal.** The smallest complete plugin: one typed action, one event, one setting, a log line. AssemblyScript through the SDK. Start here. |
| `counter` | `example.counter` | **Stateful.** Memory between presses, settings with defaults, failures with a code, and a `level` **state** (`zero`, `low`, `high`, `max`) reported from every action instead of a number, so the surface owns the look. It still publishes `counter.changed` as an event: "it changed" and "it is high" are different facts. |
| `home-assistant` | `example.home-assistant` | **Network and a secret.** Three actions that call your Home Assistant, a state per entity read back from it, a setting for the address, a **secret** for the token that the plugin never sees, pending actions and `refresh`. Asks for one address and reaches nothing until you allow it. [Guide](home-assistant/README.md) |

## SDK conformance samples

Kept to prove one part of the contract. They are not shipped with the app.

| Folder | Id | Shows |
| --- | --- | --- |
| `channels` | `example.channels` | A state **scoped** by an argument: one `status` state with an instance per channel (`channel.up`, `channel.down`, `channel.forget`). |
| `c-ping` | `example.ping` | The same contract from plain C with no SDK and no libc, about thirty lines. Proves the contract is the ABI, not a language. |

## Trying them

Each folder is installable as it is: `plugin.json` and the `.wasm` file are all LaunchDeck reads. In the app, **Plugins** offers Hello, Counter and Home Assistant under **Example plugins**. To load all five from a checkout without installing anything:

```sh
LAUNCHDECK_PLUGINS="$PWD/examples/plugins" npm run tauri dev
```

and open **Plugins**. To write your own, follow [getting started](../../docs/plugin-sdk/getting-started.md).

## Building

The built modules are committed; rebuild only after changing a plugin, the SDK or a lockfile:

```sh
npm run plugins:build                              # hello, counter, channels, home-assistant
sh scripts/build-example-plugins.sh hello counter  # some of them
sh scripts/build-example-plugins.sh --check        # rebuild aside, compare with SHA256SUMS
```

The four AssemblyScript examples need Node 20+ and npm 10+, and install their dependencies with `npm ci` from the committed lockfiles, which a build never rewrites. They are built with the current SDK (0.2.1) and their manifests say `"sdk": "0.2.1"`. `SHA256SUMS` holds the SHA-256 of the four committed modules. `--check` builds each in a temporary copy of the repository, leaves the committed files alone, and exits with 1 if a built or a committed module differs from it. A rebuilt module is committed together with its new line in `SHA256SUMS`.

On macOS 15.4.1 (Apple M2) with Node 22.23.3 and npm 10.9.9, `--check` passed twice in a row: the same sources, lockfiles and SDK gave the same bytes. On Linux, on Windows and with other Node or npm versions this is **NOT VERIFIED**: no such machine was used, and the CI that would run it does not start (account billing).

C Ping is built only when named (`sh scripts/build-example-plugins.sh c-ping`). It needs clang with the `wasm32` target and `wasm-ld`; Apple's clang has neither, so use LLVM from Homebrew or a Linux clang. Its committed `ping.wasm` was not rebuilt here and has no line in `SHA256SUMS`.

The `hello`, `counter` and `channels` modules that SDK 0.1.x built are kept, unchanged, in `src-tauri/tests/fixtures/plugins/sdk-0.1`, where a test loads and runs them: they are the proof that plugins built before the network capability still work ([versioning](../../docs/plugin-sdk/versioning.md#the-frozen-sdk-01-modules)).

Guide: [docs/plugin-sdk](../../docs/plugin-sdk/README.md).
