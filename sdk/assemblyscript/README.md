# @launchdeck/plugin-sdk (AssemblyScript)

Write a LaunchDeck plugin as a few TypeScript-like functions. Implements protocol (ABI) 1, SDK version 0.1.0.

```ts
import { action, ok, publish, Json, Call, Reply } from "@launchdeck/plugin-sdk/assembly";
export * from "@launchdeck/plugin-sdk/assembly/abi";   // required

function say(call: Call): Reply {
  const name = call.args.getString("name", "world");
  publish("hello.completed", Json.object().setString("name", name));
  return ok(Json.object().setString("message", "Hello, " + name + "!"));
}
action("hello.say", say);
```

Build with `asc --target release` (AssemblyScript 0.28), declare the action and the event in `plugin.json`, and copy the folder into LaunchDeck's plugins folder. The compiled module needs no Node or Rust on the machine that runs it.

Full guide: [docs/plugin-sdk](../../docs/plugin-sdk/README.md). Start with [getting started](../../docs/plugin-sdk/getting-started.md) and the [SDK reference](../../docs/plugin-sdk/assemblyscript.md). Working examples: [examples/plugins](../../examples/plugins).

This package is not published to npm yet; depend on it by path, as the examples do.

## Layout

| File | Contents |
| --- | --- |
| `assembly/index.ts` | The public API: `action`, `onInit`, `ok`, `fail`, `publish`, logging, `Json`, `Call`, `Reply`. |
| `assembly/abi.ts` | The three exports the host calls (`ld_abi`, `ld_alloc`, `ld_handle`). Re-export it from your entry file. |
| `assembly/host.ts` | The three imports the host provides (`launchdeck.log`, `publish`, `respond`). Nothing else exists. |
| `assembly/json.ts` | A dependency-free JSON value, parser and writer. |
| `assembly/registry.ts` | Where actions and the init handler are remembered. Internal. |
