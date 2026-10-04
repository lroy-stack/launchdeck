# AssemblyScript SDK

`@launchdeck/plugin-sdk` (in `sdk/assemblyscript`) implements the module side of [protocol 1](protocol.md) so a plugin is just actions. AssemblyScript is a strict subset of TypeScript that compiles to small WebAssembly modules (the Hello plugin is about 20 KB), so anyone who writes TypeScript can write a plugin.

The package is not on npm yet. The examples depend on it by path (`"@launchdeck/plugin-sdk": "file:../../../sdk/assemblyscript"`); copy that line and adjust the path.

## Project setup

```
my-plugin/
  plugin.json
  package.json
  asconfig.json
  assembly/
    index.ts
    tsconfig.json
```

`package.json`:

```json
{
  "name": "my-plugin", "version": "0.1.0", "private": true, "type": "module",
  "scripts": { "build": "asc --target release" },
  "dependencies": { "@launchdeck/plugin-sdk": "file:../../../sdk/assemblyscript" },
  "devDependencies": { "assemblyscript": "0.28.20" }
}
```

`asconfig.json`:

```json
{
  "entries": ["assembly/index.ts"],
  "targets": {
    "release": { "outFile": "my-plugin.wasm", "optimizeLevel": 3, "shrinkLevel": 2, "noAssert": true, "runtime": "incremental" }
  },
  "options": { "use": ["abort="] }
}
```

`"use": ["abort="]` matters: by default the compiler imports an `env.abort` function, which LaunchDeck does not provide and would refuse to load. With it removed, a failed assertion becomes a WebAssembly trap, which the host reports as a plugin error. `assembly/tsconfig.json` (editor support only) is `{ "extends": "assemblyscript/std/assembly.json", "include": ["./**/*.ts"] }`.

## The two lines every plugin needs

```ts
import { action, onInit, ok, fail, publish, setState, Json, Call, Reply } from "@launchdeck/plugin-sdk/assembly";
// with the network capability, also: http, HttpRequest, HttpResponse, HttpError, onRefresh, Watched
export * from "@launchdeck/plugin-sdk/assembly/abi";
```

The second line exports `ld_abi`, `ld_alloc` and `ld_handle`. Without it the module loads with `the module does not export ...`. Import from the `/assembly` subpath; the bare package name does not resolve in AssemblyScript.

## API

| Function | Meaning |
| --- | --- |
| `action(id, handler)` | Register `handler(call: Call): Reply` for an action id declared in `plugin.json`. |
| `onInit(handler)` | Optional. `handler(config: Json): Reply` runs once when the plugin starts. Return `fail(...)` to refuse to start. |
| `ok(value?)` | Success, with an optional `Json` value returned to the caller. |
| `fail(message, code?)` | Failure. `message` is shown to the person; `code` is an optional short identifier. |
| `publish(type, payload?)` | Publish a declared event. Returns `false` if the host refused it (the reason is in the log). Delivered when the call returns normally. |
| `setState(id, value, args?)` | Say how a declared state is now. `value` is a string, a boolean or a number; `args` is a `Json` object naming the instance of a scoped state. Returns `false` if the host refused it. Delivered when the call returns normally. See [States](states.md). |
| `clearState(id, args?)` | Say that an instance has no value now: it reads `unknown` again. |
| `http.request(request)`, `http.get(url)` | Ask LaunchDeck to send an HTTP request to an address the manifest asks for and the person granted. Returns an `HttpResponse`; never traps. Needs `capabilities.network`. See [Network](network.md). |
| `new HttpRequest(method, url)` with `.header()`, `.json()`, `.text()`, `.bearer(secret)`, `.timeout(ms)` | Describe the request. `.bearer("token")` names a [secret](secrets.md); the value is added by LaunchDeck and never reaches the plugin. |
| `onRefresh(handler)` | Optional. `handler(watching: Watched[]): Reply` is called at the manifest's `refresh` interval with the state instances pads are showing (`Watched` has `state: string` and `args: Json`). |
| `debug`, `info`, `warn`, `error`, `log(level, text)` | Write to the plugin's log (see the Plugins screen). |

`Call` has `action: string` and `args: Json`. Arguments were already validated against the action's `params` in the manifest, with defaults filled in and blanks removed, so `getString("name", fallback)` only ever sees a string or nothing.

`Json` is a small JSON value with no dependencies:

| Build | Read |
| --- | --- |
| `Json.object()`, `Json.array()`, `Json.string(s)`, `Json.number(n)`, `Json.boolean(b)` | `get(name)`, `at(index)`, `has(name)`, `length` |
| `setString(name, s)`, `setNumber(name, n)`, `setBool(name, b)`, `set(name, json)` (objects) | `getString(name, fallback)`, `getNumber(name, fallback)`, `getInt(name, fallback)`, `getBool(name, fallback)` |
| `push(json)` (arrays) | `asString()`, `asNumber()`, `asBool()` |
| `toString()`, `Json.parse(text)` | `kind` |

The setters return the object, so they chain: `Json.object().setString("name", n).setNumber("count", 3)`.

## Writing good actions

* **Return a result a surface can use.** Return what happened (`{ "level": "high" }`), not how it should look. Colours and pads belong to the surface.
* **Publish what changed, once.** One event per meaningful change, with the data a feedback rule needs in the payload (`showFromField: "level"`).
* **Report how things are, whenever you learn it.** `setState("level", "high")` costs almost nothing when the value did not change, so report it from every action, including the ones that fail. A pad that shows a level reads the state; a pad that flashes when something happens reads the event. See [States](states.md) for which a fact is.
* **Fail with a sentence.** `fail("the counter is at its maximum", "at_limit")` appears on the pad's failure and in the log. Do not trap on expected conditions.
* **Keep state in globals.** The module lives as long as the plugin is running, so a counter survives between presses. Restarting the plugin (disable and enable, changing a setting, **Reload**) starts again from zero, and every state it declared reads `unknown` until an action reports it again. Nothing is saved for you; if a value matters across restarts, make it a setting.
* **Stay short.** A call has 2 seconds and 100 million instructions of its own. Waiting for a network answer is not counted against them, but a call may spend at most 30 seconds on the network. There are no timers; to do something regularly, declare `refresh`.
* **Say what is true, not what you asked for.** After a request that changes something outside, read the value back and report *that* with `setState`. When the outside system cannot be read, `clearState`, so a pad shows unknown instead of a stale value.
* **Pass network failures on.** `if (!r.ok) return fail(r.message, r.error);` gives the person the reason (`permission_denied`, `secret_missing`, `timeout`) on the pad and on the plugin's card.
* **Do not name globals like builtins.** A global called `max` hides AssemblyScript's `max()`.

## Memory

The `incremental` runtime frees garbage between calls, but linear memory is capped at 16 MiB. If your plugin builds big structures, release references when you are done with them.

## Debugging

`info("...")` is the printf of this SDK. Lines appear under **Show log** on the plugin's card. See [testing](testing.md).
