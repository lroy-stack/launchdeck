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
import { action, onInit, ok, fail, publish, Json, Call, Reply } from "@launchdeck/plugin-sdk/assembly";
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
* **Publish what changed, once.** One event per meaningful change, with the data a binding needs in the payload (`$payload.level`).
* **Fail with a sentence.** `fail("the counter is at its maximum", "at_limit")` appears on the pad's failure and in the log. Do not trap on expected conditions.
* **Keep state in globals.** The module lives as long as the plugin is running, so a counter survives between presses. Restarting the plugin (disable and enable, changing a setting, **Reload**) starts again from zero. Nothing is saved for you; if a value matters across restarts, make it a setting.
* **Stay short.** A call has 2 seconds and 100 million instructions. Long work does not belong in a plugin yet (there are no timers and no network).
* **Do not name globals like builtins.** A global called `max` hides AssemblyScript's `max()`.

## Memory

The `incremental` runtime frees garbage between calls, but linear memory is capped at 16 MiB. If your plugin builds big structures, release references when you are done with them.

## Debugging

`info("...")` is the printf of this SDK. Lines appear under **Show log** on the plugin's card. See [testing](testing.md).
