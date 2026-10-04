# @launchdeck/plugin-sdk (AssemblyScript)

Write a LaunchDeck plugin as a few TypeScript-like functions. Implements protocol (ABI) 1, SDK version 0.2.1.

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

To report how something is *now*, rather than that something happened, declare a state in `plugin.json` (schema 3) and call `setState("level", "high")`, or `setState("status", "up", Json.object().setString("channel", "alpha"))` for a state scoped by an argument. See [States](../../docs/plugin-sdk/states.md).

To talk to a service, declare `capabilities.network` in `plugin.json` (schema 4) and call `http.request(new HttpRequest("GET", url).bearer("token"))`. LaunchDeck sends the request if the person granted the address, adds the secret itself, and returns the answer. `onRefresh` lets a plugin keep states current by being asked every few seconds. See [Network](../../docs/plugin-sdk/network.md) and [Secrets](../../docs/plugin-sdk/secrets.md).

Build with `asc --target release` (AssemblyScript 0.28), declare the action and the event in `plugin.json`, and copy the folder into LaunchDeck's plugins folder. The compiled module needs no Node or Rust on the machine that runs it.

Full guide: [docs/plugin-sdk](../../docs/plugin-sdk/README.md). Start with [getting started](../../docs/plugin-sdk/getting-started.md) and the [SDK reference](../../docs/plugin-sdk/assemblyscript.md). Working examples: [examples/plugins](../../examples/plugins).

This package is not published to npm yet; depend on it by path, as the examples do.

## Layout

| File | Contents |
| --- | --- |
| `assembly/index.ts` | The public API: `action`, `onInit`, `onRefresh`, `ok`, `fail`, `publish`, `setState`, `clearState`, logging, `http`, `Json`, `Call`, `Reply`. |
| `assembly/http.ts` | `http.request`, `HttpRequest`, `HttpResponse`, `HttpError`. |
| `assembly/abi.ts` | The three exports the host calls (`ld_abi`, `ld_alloc`, `ld_handle`). Re-export it from your entry file. |
| `assembly/host.ts` | The imports the host provides: `launchdeck.log`, `publish`, `respond`, and `http_request` / `http_response` for a plugin with the network capability. States ride on `publish`. |
| `assembly/json.ts` | A dependency-free JSON value, parser and writer. |
| `assembly/registry.ts` | Where actions and the init and refresh handlers are remembered. Internal. |
