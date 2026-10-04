# Testing and debugging

## While developing

1. Point LaunchDeck at your working folder with `LAUNCHDECK_PLUGINS`, rebuild your module, and press **Rescan**. Unchanged plugins are left alone, and a changed module restarts.
2. Open the plugin's card on **Plugins**. The status pill says whether it loaded. If it did not, the card shows the reason in plain words. Typical ones:
   * ``the module imports `env::abort`, which LaunchDeck does not provide``: build with `--use abort=` (see [AssemblyScript](assemblyscript.md)).
   * ``the module does not export `ld_handle(ptr, len) -> i32` ``: the entry file is missing `export * from "@launchdeck/plugin-sdk/assembly/abi"`.
   * `the manifest says protocol 1 but the module reports protocol 2`: `abi` in `plugin.json` and `ld_abi()` disagree.
   * ``capability `network` is not available in this LaunchDeck``: remove it from `capabilities`.
3. **Show log** displays the plugin's log: your `info(...)` lines and the host's own notes (start, failure, rejected events). The log is a ring of the last 100 lines per plugin and is cleared when LaunchDeck exits.
4. In Edit mode, select a pad that calls the plugin and press **Test run** to invoke it without a device. A failure appears as a message with the plugin's own text, and the activity feed shows `action.completed` or `action.failed` and the events the plugin published.

## Automated tests

The Rust workspace tests the SDK on three levels, and plugin authors can borrow the same ideas.

* **Hostile modules in WAT** (`src-tauri/tests/support/wat_plugins.rs`). A small builder turns WebAssembly text into modules that loop forever, trap, grow memory, import WASI, return garbage, return megabytes, publish undeclared events or lie about their protocol. Each case proves the host reports a clear error and keeps running. Add a case when you find a new way a module can misbehave.
* **Real plugins, full chain** (`src-tauri/tests/plugin_sdk.rs`). The example plugins are discovered from `examples/plugins`, loaded, and driven from pads on the virtual Launchpad X. The strongest test presses a pad, waits for the plugin's event to travel the Event Bus and a real event rule, and asserts the LED frame changed. It also covers the Counter's memory between presses, the C module, disabling and enabling, a crashing plugin next to a healthy one, settings, and duplicates.
* **States, full chain** (`src-tauri/tests/plugin_states.rs`). The real Counter and Channels modules run against the virtual Launchpad X with no mocked plugin boundary: load, declare, an action reports a state, the hub holds it, the catalog lists it, a feedback rule changes the pad, **Disable** makes it unavailable and the pad follows, **Enable** reads unknown, and the next report is known again. It also covers scoped instances, two plugins that cannot write each other's states, and refused writes. The cases a module can get wrong (undeclared state, wrong type, unlisted enum value, a call that traps) are unit tests next to the host (`src-tauri/src/plugins/external/state_tests.rs`).
* **Contract fixtures.** `examples/observables.json` is the Observable Catalog, the messages the interface hears and three explanations of a control, captured from the real runtime (`cargo run --example dump_observables`); a Rust test fails if it goes stale and the Vitest suite parses it with the Zod schemas. `examples/plugins-overview.json` is what the Rust host sends to the Plugins screen, captured from real plugins (one healthy, one with a secret setting, one that fails to load, one rejected folder). A Rust test fails if it goes stale and the Vitest suite parses it with the Zod schemas.

Run them:

```sh
cd src-tauri && cargo test            # all Rust tests, including the SDK ones
npm test                              # TypeScript, including the plugins contract
npm run plugins:build                 # rebuild the example modules
```

A minimal check for your own plugin without LaunchDeck: load the `.wasm` in Node, provide the three `launchdeck` imports, call `ld_alloc`, write a request, and call `ld_handle`. The protocol is small enough that this is about fifty lines, and it works for any language.

## Performance

Measured on a debug build in the development container (a release build is faster): loading a small module takes about 30 ms, an invocation with an event costs 20 to 40 microseconds on top of the action engine's own overhead, a pad press reaches the LED frame in about 35 ms (dominated by the 25 Hz render tick, not by the plugin), forty installed plugins start in about one second in total, and a rescan with nothing changed takes about a millisecond.
