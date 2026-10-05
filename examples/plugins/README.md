# Example plugins

Five plugins that live outside the LaunchDeck core and are loaded exactly like a third party's would be. They are also the fixtures of the SDK tests (`src-tauri/tests/plugin_sdk.rs`, `plugin_states.rs`, `home_assistant.rs`).

| Folder | Id | Shows |
| --- | --- | --- |
| `hello` | `example.hello` | The smallest complete plugin: one typed action, one event, one setting, a log line. AssemblyScript through the SDK. |
| `counter` | `example.counter` | Memory between presses, settings with defaults, failures with a code, and a `level` **state** (`zero`, `low`, `high`, `max`) reported from every action instead of a number, so the surface owns the look. It still publishes `counter.changed` as an event: "it changed" and "it is high" are different facts. |
| `channels` | `example.channels` | A state **scoped** by an argument: one `status` state with an instance per channel (`channel.up`, `channel.down`, `channel.forget`). |
| `home-assistant` | `example.home-assistant` | The **network capability**, both ways: three actions that call your Home Assistant, a state per entity read back from it, a setting for the address, a **secret** for the token that the plugin never sees, pending actions and `refresh`. Asks for one address and reaches nothing until you allow it. [Guide](home-assistant/README.md) |
| `c-ping` | `example.ping` | The same contract from plain C with no SDK and no libc, about thirty lines. Proves the contract is the ABI, not a language. |

Each folder is installable as it is: `plugin.json` and the `.wasm` file are all LaunchDeck reads. Try them without installing:

```sh
LAUNCHDECK_PLUGINS="$PWD/examples/plugins" npm run tauri dev
```

and open **Plugins**. Rebuild the modules with `npm run plugins:build`: Hello, Counter, Channels and Home Assistant, with Node and npm, installed from their lockfiles with `npm ci`. `sh scripts/build-example-plugins.sh --check` rebuilds them in a temporary copy and compares the result, and the committed modules, with `SHA256SUMS`. C Ping is built only when named and needs clang with the wasm32 target and wasm-ld, which Apple's clang does not have.

The committed modules are built with the current SDK (0.2.1). The `hello`, `counter` and `channels` modules that SDK 0.1.x built are kept, unchanged, in `src-tauri/tests/fixtures/plugins/sdk-0.1`: they are the proof that plugins built before the network capability load and run unchanged.

Guide: [docs/plugin-sdk](../../docs/plugin-sdk/README.md).
