# Plugin SDK

A LaunchDeck plugin is a folder with two files, `plugin.json` and one `.wasm` module. You can write it in any language that compiles to WebAssembly, the people who install it need no Node, Rust or compiler, and the same files run on macOS, Windows and Linux.

A plugin does three things: it **offers actions** (things a pad can do, which appear in the Surface editor's [Action Library](../actions/action-library.md) automatically, with a form drawn from the manifest), it **returns a result** for each call, and it **publishes events** (things that happened). It never learns about pads, colours, MIDI or devices. A surface decides what an event looks like on a pad, which is why one plugin works on every controller.

```
pad press
  -> Action Engine calls  example.hello / hello.say
  -> your module runs in a sandbox and returns { "message": "Hello, Leroy!" }
  -> your module also publishes  hello.completed { name, message }
  -> Event Bus
  -> the pad's binding  (source example.hello, event hello.completed -> state "greeted")
  -> the pad turns green on every device
```

Nothing after "your module" is plugin code. That is the whole contract.

## What a plugin can and cannot do

| Can | Cannot |
| --- | --- |
| Declare typed actions and return a result or a readable error | Read or write files |
| Publish events that were declared in its manifest | Open sockets, run commands, start processes |
| Keep state in its own memory between calls | See other plugins, surfaces, pads, the workspace or the device |
| Read its own settings (secrets included) | Read the clock, the environment or random numbers from the host |
| Write log lines to its own log | Publish events as someone else or press a pad |

If a feature needs something in the right column, the answer is a future host-mediated capability, never ambient access. See [security](security.md).

## Read in this order

1. [Getting started](getting-started.md): build the Hello plugin, install it, press a pad.
2. [Manifest reference](manifest.md): every field of `plugin.json`, including the action metadata and parameters the library uses.
3. [AssemblyScript SDK](assemblyscript.md): the library used by the examples.
4. [Protocol reference](protocol.md): the module ABI, for other languages and for debugging.
5. [Security model](security.md): what the sandbox enforces and what it does not claim.
6. [Packaging and installing](packaging.md): where plugins live, enabling, settings, the state file.
7. [Testing and debugging](testing.md): logs, the Plugins screen, writing tests.
8. [Versioning](versioning.md): the four version numbers and how they are checked.

Working examples live in `examples/plugins`: `hello` (the smallest complete plugin), `counter` (state between calls, settings, levels) and `c-ping` (about thirty lines of C with no SDK, to show the contract is the ABI and not a language).

How a plugin's actions become library entries, pads and form fields is described in [docs/actions](../actions/overview.md). The research and the decisions behind this design are in [PLUGIN_SDK_PLAN.md](../../PLUGIN_SDK_PLAN.md) and [ACTION_LIBRARY_PLAN.md](../../ACTION_LIBRARY_PLAN.md).
