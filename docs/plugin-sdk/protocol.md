# Protocol reference (ABI 1)

The contract between LaunchDeck and a module is a handful of WebAssembly exports and imports that exchange JSON through linear memory. It is language independent: the AssemblyScript SDK is one implementation of it and `examples/plugins/c-ping/ping.c` is another, in plain C with no libc and no allocator.

The module is a core WebAssembly module (not a component). The usual core features work (multi-value, bulk memory, reference types, SIMD, sign extension, saturating conversions). Tail calls, multiple memories, 64-bit memories, GC, threads and exceptions are not available.

## Exports the module must provide

| Export | Signature | Meaning |
| --- | --- | --- |
| `memory` | linear memory | The module's one memory. Capped at 16 MiB by the host. |
| `ld_abi` | `() -> i32` | The protocol version the module speaks. Must equal `abi` in the manifest. |
| `ld_alloc` | `(len: i32) -> i32` | Return a pointer to `len` writable bytes, or `0` if it cannot (that call fails). The host writes the request there. |
| `ld_handle` | `(ptr: i32, len: i32) -> i32` | Handle the request at `ptr`. Return `0` after calling `respond`. A non-zero return, or returning without a reply, fails that call and the plugin keeps running. |

Ownership: the buffer from `ld_alloc` becomes the module's. The host never touches it again, so free it (or reuse it) inside `ld_handle`. A module with one static buffer, like the C example, is fine because it finishes each request before the next arrives. Calls to one plugin are serialized.

## Imports the host provides

All live in the module `launchdeck`. Any other import makes the module fail to load, with the name in the error message. There is no WASI.

| Import | Signature | Meaning |
| --- | --- | --- |
| `log` | `(level: i32, ptr: i32, len: i32)` | Add a line to the plugin's log. Levels: 0 debug, 1 info, 2 warn, 3 error. At most 50 lines per call; lines are cut at 1 KiB and stripped of control characters. |
| `publish` | `(ptr: i32, len: i32) -> i32` | Publish an event, or write a state (JSON below). Returns a code. |
| `respond` | `(ptr: i32, len: i32)` | Give the reply to the current request. The host copies the bytes at once. Calling it twice keeps the last reply. |
| `http_request` | `(ptr: i32, len: i32) -> i32` | **Only for a plugin whose manifest declares `capabilities.network`.** Run the HTTP request described by the JSON at `ptr` and return the size of the answer. See [Network](network.md#the-wire-format-for-other-languages). |
| `http_response` | `(ptr: i32, cap: i32) -> i32` | Same condition. Copy that answer into the module's buffer; `-1` when there is none or it does not fit. |

The two network imports are linked only when the manifest declares the capability. A module that imports them without it fails to load like any other unknown import, and a module that does not import them never notices they exist, which is why this did not need a new protocol number.

`publish` returns `0` accepted, `1` malformed, `2` too large, `3` too many in this call, `4` not allowed (an event type or a state the manifest does not declare, or a type under a LaunchDeck prefix), `5` invalid (a declared state, but the arguments or the value do not fit its declaration). The reason is also written to the plugin's log. An accepted event or state write is queued and delivered only if the call ends normally, so a call that traps or exceeds a limit publishes nothing.

### State messages

Events and states share the one `publish` import, and the module tells them apart by the message. Nothing was added to the protocol to carry states: a message with a `state` key is a state write, and a host that predates states never sees one from a module that does not call `setState`.

```json
{"state":"level","value":"high"}
{"state":"status","args":{"channel":"alpha"},"value":"up"}
{"state":"status","args":{"channel":"alpha"},"clear":true}
```

| Key | Meaning |
| --- | --- |
| `state` | The id of a state declared in the manifest. The provider is never part of the message: it is the plugin that sent it, so a module cannot write a state it did not declare. |
| `args` | The arguments that pick the instance, for a state declared with `params`. Omit it for a state without. |
| `value` | One scalar: a boolean, a number or a string (at most 256 bytes). It must have the declared type and, for an enum, be one of the listed values. |
| `clear` | `true` instead of `value`: this instance has no value now and reads `unknown` again. |

A state message is at most 2 KiB, and one call may write at most 64 instances. Writing the value an instance already has is not a change. Writes are queued during the call, in order, and applied when it returns normally.

Every pointer and length the module passes is checked against its memory before the host reads. A bad range is a trap-level failure of that plugin, never a read outside it.

## Requests

JSON objects. `op` is written first, so a minimal module can tell requests apart from a fixed offset (the C example looks at byte 9, which is `i` for `init` and `v` for `invoke`). Requests are at most 64 KiB.

```json
{"op":"init","abi":1,"host":{"name":"launchdeck","version":"0.2.0","sdk":"0.1.0"},"plugin":{"id":"example.hello","version":"0.1.0"},"config":{"greeting":"Hello"}}
{"op":"invoke","action":"hello.say","args":{"name":"Leroy"}}
```

`init` is sent once, when the plugin starts or restarts, with its settings: the person's stored values where they still fit the declared type, the declared defaults otherwise. Settings marked `secret` in a schema 1 to 3 manifest are included; values declared under `secrets` in schema 4 never are. Events published and states written during `init` are dropped, with a warning in the log. If `init` replies `failed`, traps or returns no usable reply, the plugin does not start and shows status *error* with the reason.

`invoke` carries an action id declared in the manifest and arguments that already passed validation against it.

```json
{"op":"refresh","watching":[{"state":"entity","args":{"entity":"light.kitchen"}}]}
```

`refresh` is sent only to a plugin whose manifest declares `refresh`. `watching` lists up to 16 instances of the plugin's own states that saved controls read. It is answered like `invoke` (`ok` or `failed`); the reply goes to the plugin's log and never to a pad. States written during it are applied when it returns normally. A module built before `refresh` existed is never sent one. See [Network](network.md#keeping-states-current-refresh).

The network is closed during `init`: `http_request` answers `invalid_request` there.

## Replies

```json
{"status":"ok","value":{"message":"Hello, Leroy!"}}
{"status":"failed","error":"name is too long","code":"too_long"}
```

`value` is any JSON (or absent). `error` is a message a person can read; it is shown on the pad's failure and in the log. `code` is optional and must match `[A-Za-z0-9_.:-]{1,64}`, otherwise it is dropped; when valid it is appended to the message as `(code)`. Replies are at most 64 KiB.

`status` is a string so the protocol can grow. `pending` (an action that finishes later, after the call returned) is reserved and rejected in protocol 1. That is a different thing from the *pending* the runtime shows while a call is still running, for example while it waits for a network answer: that one needs nothing from the module.

A reply the host cannot use (not JSON, not an object, unknown status, too large) fails only that call with a message naming the problem. The plugin keeps running.

## Limits per call

| Limit | Value | When exceeded |
| --- | --- | --- |
| CPU (fuel) | 100 million units, roughly one per executed instruction | Trap. The plugin stops with status *error*. |
| Wall clock | 2 seconds of the module's own time. Time spent waiting inside `http_request` is not counted; it has its own limits (10 s per request by default, 30 s per call). | Trap, same. |
| Memory | 16 MiB | `memory.grow` fails inside the module; a module that cannot cope traps. |
| Wasm stack | 512 KiB | Trap. |
| Request, reply | 64 KiB each | Reply too large fails the call only. |
| Event | 16 KiB; 32 per call | `publish` returns 2 or 3. |
| State write | 2 KiB; 64 per call | `publish` returns 2 or 3. |
| States held | 64 declared, 1,024 instances | Declaration is refused at load; a write past the instance limit is dropped with a warning. |
| Network | 24 requests per call, 600 a minute, request body 256 KiB, response 1 MiB | The request is answered with an error code; the call goes on. |
| Module file | 8 MiB | Refused at discovery. |

A trap, a limit, a broken export or a bad pointer drops the instance (the module's memory is lost) and marks the plugin *error*. The log has the reason, other plugins keep running, and **Reload** on the Plugins screen starts it again.

## Language notes

Built and tested in this repository: AssemblyScript (through the SDK) and C (clang with `--target=wasm32 -nostdlib`). Any toolchain that produces a core module with these exports and no other imports should work, for example Rust on `wasm32-unknown-unknown` with `#[no_mangle]` exports. Those were not built here, because the environment this milestone was developed in had no extra Rust targets, so treat them as expected to work rather than proven. A toolchain that insists on importing WASI will fail to load with the import named in the error.

## Relation to the Component Model

Protocol 1 is a core-module ABI on purpose: it needs nothing beyond Wasmtime's stable core runtime. Each function maps one to one to a WIT world, which is how a component form can be added later without changing what plugins do:

```wit
package launchdeck:plugin@0.1.0;
world plugin {
  import log: func(level: u8, message: string);
  import publish: func(event-json: string) -> result<_, string>;
  // with the network capability: import http-request: func(request-json: string) -> string;
  export abi: func() -> u32;
  export handle: func(request-json: string) -> string;
}
```

This is design only. Nothing in the host loads components.
