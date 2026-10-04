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

and open **Plugins**. Rebuild the modules with `npm run plugins:build` (Node for the AssemblyScript ones, clang with the wasm32 target for `c-ping`). The build is reproducible, so rebuilding unchanged sources with the same SDK gives identical bytes.

The committed `hello`, `counter` and `channels` modules were built with SDK 0.1.x and are kept that way on purpose: they are the proof that plugins built before the network capability load and run unchanged. Rebuilding them with SDK 0.2.0 gives different bytes that work the same and still import nothing but `log`, `publish` and `respond`.

Guide: [docs/plugin-sdk](../../docs/plugin-sdk/README.md).
