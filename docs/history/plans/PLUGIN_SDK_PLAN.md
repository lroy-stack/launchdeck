# LaunchDeck Plugin SDK: research and implementation plan

Status: research and implementation done on 2026-10-04. Everything marked "verified" was checked on that date against the sources listed at the end of section 3. Section 10 records what was built and measured. The guide for plugin authors is in [docs/plugin-sdk](../../plugin-sdk/README.md).

> **Update (2026-10-05).** Three things this plan describes as absent were built later and are documented elsewhere: plugin-declared states (manifest schema 3, [docs/plugin-sdk/states.md](../../plugin-sdk/states.md)), one host-mediated capability, network access through a broker with per-address grants (manifest schema 4, [PLUGIN_CAPABILITIES_PLAN.md](PLUGIN_CAPABILITIES_PLAN.md)), and secrets kept in the system credential store ([docs/security/secrets.md](../../architecture/secrets.md)). Where the text below says `capabilities` must be an empty array, that is still how schema 1 to 3 manifests are read; schema 4 declares it as an object. The sandbox itself (no WASI, fuel, memory and time limits, ABI 1) is unchanged.

Goal: turn LaunchDeck from an application into a platform by letting someone who is not on the LaunchDeck team ship a plugin that is discovered, validated, loaded, sandboxed and invoked, without recompiling LaunchDeck, without Rust or Node on the end user's machine, and without ever touching hardware.

The chain this milestone must prove:

```
external plugin on disk
  -> discovered -> validated -> loaded (sandboxed)
  -> registers semantic action -> LaunchDeck invokes it -> plugin executes
  -> returns a normalized result -> publishes a semantic event
  -> existing Event Bus -> existing Surface binding -> virtual or physical device updates
```

## 1. Current architecture relevant to plugins

The seam already exists and is small. Reusing it is the main decision of this plan.

* `plugins::Plugin` is a trait with `manifest()` and `invoke(ctx, action, args) -> Result<Value, ActionError>`. `PluginContext` carries the plugin id and the `EventBus`, and `publish` stamps the plugin id as the event `source`.
* `PluginRegistry` maps an id to `Arc<dyn Plugin>`. The runtime registers System, Demo and Agent Control at startup and hands a clone to the `ActionEngine`.
* A control holds `Action::Plugin { plugin, action, args }`. The `ActionEngine` runs it on `spawn_blocking`, publishes `action.started` and then `action.completed` or `action.failed`.
* Events are `Event { source, type, payload, ts }` on a Tokio broadcast bus. The runtime subscribes once and evaluates every control's bindings (`source`, `event`, `when`, `setState`). Bindings are the only way plugin output reaches a pad.
* The renderer and the `SurfaceDevice` trait sit behind bindings. Nothing above `devices/launchpad_x.rs` knows MIDI.
* `persistence` already offers atomic, owner-only writes (`write_private`) used by Agent Control's `agents.json`.

## 2. Problems with the current internal Plugin abstraction

It is a good in-process seam and a poor public contract. Concretely:

1. A `Plugin` is arbitrary Rust linked into the binary, with the full privilege of LaunchDeck. There is nothing to sandbox and nothing to ship separately.
2. `PluginManifest` is built in code. It has a plugin version but no schema version, no protocol compatibility, no argument types, no declared events and no configuration.
3. The registry is immutable after startup (`register(&mut self)`, then cloned into the engine). There is no runtime registration, no removal, no enable or disable, no health.
4. `PluginContext` exposes a Tokio `Handle` (`spawn_after`) and an unrestricted `publish(kind, payload)`. A third party could publish any event type with any payload size.
5. `Action::Plugin` validation only checks that plugin and action are non-empty. A typo is discovered when a pad is pressed.
6. `permissions` is a list of free strings used for display. Nothing reads it.
7. There is no plugin configuration channel and no plugin-scoped diagnostics.
8. The WebSocket spoofing guard `RESERVED_SOURCES` is a static list. Once plugin ids exist, an outside client could publish events claiming to come from a plugin.

Decision: keep the trait as the single internal seam, make the registry dynamic, and add one new implementation of the trait that runs untrusted WebAssembly. System, Demo and Agent Control stay native and first-party. The public contract (sections 6 and 7) is a new, much smaller surface, not the trait.

## 3. Researched approaches

All dates are 2026 unless stated.

**Wasmtime (core modules).** Bytecode Alliance runtime. A new major version ships on the 20th of every month. Every 12th version is an LTS (24, 36, 48) with 24 months of support, and security fixes are backported to every supported release. The latest is 49.0.2; 48.0.5 is the current LTS patch (MSRV 1.95). The security record matters because this is the component that holds untrusted code: on 9 April, 12 advisories included two critical sandbox escapes (Winch, and Cranelift on aarch64), patched in 43.0.1, 42.0.2, 36.0.7 and 24.0.7. RUSTSEC-2026-0269 (filesystem sandbox escape in WASI) is fixed from 46.0.3. RUSTSEC-2026-0315 (fuel accounting skipped by `call_ref` and exception `catch`) is fixed in 48.0.3 and 49.0.1. RUSTSEC-2026-0327 (critical, buffer overflow in component async-lifted callbacks, 2 October) is fixed in 48.0.4 and 49.0.2. Lessons: pin to a supported line, never ship a version below those floors, turn off every feature the plugins do not need (WASI, component model, GC, exceptions, threads), and expect to update.

**Extism.** A plugin framework on top of Wasmtime with host SDKs and plugin development kits (PDKs) for many languages, an `alloc`/bytes-in-bytes-out ABI, host functions, key-value and HTTP helpers. Its Rust SDK 1.30.0 (latest, verified from its manifest) depends on `wasmtime = "43"` and `wasi-common = "43"`. Version 43 is outside the two-month window of non-LTS lines, so the SDK does not carry the fixes above unless Extism ships again. The PDKs are stale on npm (`@extism/as-pdk` 1.0.0, 2024-02; `@extism/js-pdk` 1.1.1, 2024-09) and the JS PDK downloads its compiler binary from a GitHub release, which many corporate networks block. It also enables WASI and extra host features by default that we want off. Good ideas, wrong dependency.

**WASI.** WASI preview 1 gives plugins a POSIX-like surface (files, clocks, random, stdio) that must be capped by preopens and per-call policy. WASI 0.3.0 launched on 11 June and is declared stable, with native async. Either way WASI is a capability catalogue we do not need: the plugin contract has no files, no sockets and no clocks. Decision: link no WASI at all. A module that imports anything outside the `launchdeck` namespace fails to instantiate.

**Component Model and WIT.** The right long-term typed, language-neutral contract. Wasmtime 46 and later enable component async by default, which is exactly the code path hit by RUSTSEC-2026-0327 on 2 October. Guest toolchains for the languages we can verify here (AssemblyScript, JavaScript) are still moving (`jco` 1.35.0, `componentize-js` 0.23.0). Zed's extension system (Wasmtime plus WIT with a versioned API stored in the extension manifest, and a deliberately small API) is the best real-world precedent and validates the manifest-carries-API-version idea. Decision: do not adopt it in this milestone. Compile it out, and design the core ABI so each function maps one to one to a WIT world (sketch in section 7). Revisit when a Rust and a JavaScript toolchain can both produce components that Wasmtime LTS runs without async.

**wasmi.** Pure Rust interpreter. Version 2.0.0 (1 September) has stable fuel metering. No component model, slower, and no stated guarantees for hostile code. Kept as a documented fallback behind the same internal sandbox boundary, for platforms where a JIT is not allowed.

**Native dynamic libraries.** Fast and easy to write, but no isolation (a plugin can read the process memory and call anything), a Rust ABI that is not stable, one binary per OS and CPU, and code signing issues on macOS and Windows. Rejected for third parties.

**Process-isolated plugins.** A child process speaking JSON over stdio or a socket. Strong isolation if the OS sandbox is correct, which is a per-OS project (seccomp, App Sandbox, AppContainer). The plugin must ship a runtime or be a native binary per platform. Possible later for plugins that really need the network, but heavy for "a pad that increments a counter".

**Extension hosts and embedded scripting (VS Code style Node host, Lua, QuickJS).** A script engine inside the process shares memory with LaunchDeck and gives the plugin whatever the engine exposes. A separate Node host needs Node on every machine. Rejected for the same reasons as native libraries and process isolation respectively.

Sources (all read on 2026-10-04): Wasmtime release and stability policy, docs.wasmtime.dev/stability-release.html; RustSec advisories 2026-0269, 2026-0315 and 2026-0327, rustsec.org; Wasmtime security advisories of 9 April on GitHub; the extism 1.30.0 manifest on crates.io; WASI 0.3.0 announcement of 11 June by the Bytecode Alliance; Zed, "Zed Decoded: Extensions", zed.dev/blog/zed-decoded-extensions; wasmi 2.0.0 release notes, github.com/wasmi-labs/wasmi; npm registry pages for `@extism/as-pdk`, `@extism/js-pdk`, `assemblyscript` 0.28.20 (22 July), `jco` and `componentize-js`.

## 4. Comparison

Scores: high is good for the user of the criterion (so "Safety: high" means safe). Priority order for this milestone: reliability, simplicity, maintainability, portability, DX, scalability.

| Approach | Safety and isolation | Portability | Startup and overhead | DX | Packaging | Versioning | Debugging | Cross-platform | Permissions | Maintainability |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| Wasmtime, core module, own ABI (chosen) | High | High | Low (ms) | Good with an SDK | One `.wasm` | Ours to define | Medium | Same bytes everywhere | Deny by default, per import | High, ~600 lines |
| Extism | High, WASI on | High | Low | Good in theory, stale PDKs | One `.wasm` | Extism's | Medium | Same bytes | Coarse | Lower, pinned to old Wasmtime |
| Wasmtime plus Component Model | High, larger surface | High | Low | Best once toolchains land | One `.wasm` | WIT worlds | Medium | Same bytes | Per interface | Medium now, high later |
| wasmi | Medium (no hostile-code claim) | High | Very low | Same as Wasmtime | One `.wasm` | Ours | Medium | Same bytes | Per import | High |
| Native dylib | None | Low | Lowest | Rust only | One file per target | Fragile | High | Poor | None | Low |
| Child process | Depends on OS sandbox | Low | High | Any language | Binary per target | Ours | High | Poor | OS level | Medium |
| Embedded script or Node host | Low | Medium | Medium | High | Source plus runtime | Ours | High | Medium | Weak | Low |

Verdict: Wasmtime on a supported LTS line with a small hand-written core ABI wins on reliability, simplicity and portability, and it does not close the door on WIT.

## 5. Chosen architecture

```
 <plugins dir>/<any name>/plugin.json + <main>.wasm        (what a third party ships)
              |
   Discovery: scan, read with size caps, parse, validate        (plugins::external::discovery, manifest)
              |
   PluginHost: owns state (plugins.json), status, config, logs  (plugins::external::host)
              |
   WasmPlugin: implements the existing `Plugin` trait           (plugins::external::wasm_plugin)
              |   one Wasmtime Store per plugin, fuel + epoch + memory limits
              v
   PluginRegistry (now dynamic) -> ActionEngine -> EventBus -> bindings -> renderer -> device
                                   (all existing, unchanged semantics)
```

Decisions:

* Runtime: `wasmtime` 48 (LTS, supported to August 2028), `default-features = false`, features `std`, `runtime`, `cranelift` only. No WASI, no component model, no GC, no async, no threads, no pooling allocator, no cache.
* One Store and one instance per plugin, kept for the life of the plugin so plugin state (a counter) persists between invocations, behind a mutex so calls to one plugin are serialized. Different plugins run in parallel.
* The runtime stays hardware-agnostic: a plugin sees `init` and `invoke` messages and three host functions. It cannot name a device, a pad, a note, a frame or a surface.
* Action registration is declarative. The manifest lists the actions and the host registers the plugin in the existing registry. Arguments are validated against the manifest before the module runs. `Action::Plugin` validation at edit time was deliberately left as it was (plugin and action must be non-empty): a surface that names a plugin that is not installed still loads, so sharing a surface never depends on every plugin being present, and pressing the pad fails with a clear message (unknown plugin, plugin disabled, or plugin unavailable with its reason).
* Failure is data. A trap, a limit, a bad response or a bad payload becomes an `ActionError`, the existing `action.failed` event, a log line with target `plugin:<id>`, and (for traps and limits) an `error` status. Nothing propagates as a panic.

## 6. Security model

Threat model: the plugin binary, its manifest and its output are hostile. The user decides to install it; the host must make that decision survivable.

Isolation, enforced by construction:

* No imports are provided except `launchdeck.log`, `launchdeck.publish` and `launchdeck.respond`. No filesystem, shell, network, clock, random, MIDI, process, environment or secrets exist inside the sandbox, so there is nothing to deny at runtime. A module that imports `wasi_snapshot_preview1` or anything else is rejected at load.
* Memory: a single linear memory capped at 16 MiB, one instance, bounded tables. Growth beyond the cap fails inside the guest.
* CPU: a fuel budget per call (deterministic) and an epoch deadline (wall clock, default 2 s) per call. Both turn into a trap that the host catches. Wasm stack depth is capped.
* Data in and out: requests and responses are JSON, capped at 64 KiB. Events are capped at 16 KiB each and 32 per call. Log lines are capped and kept in a ring buffer of 100 entries. `serde_json` recursion limits apply.
* Event integrity: the host stamps `source` with the plugin id, ignores any source the plugin writes, rejects event types the manifest did not declare, and rejects types under host-owned prefixes (`control.`, `action.`, `surface.`, `page.`, `device.`, `plugin.`, `agent.`, `runtime.`, `system.`). Events only feed bindings, which only set a control's visual state. An event cannot press a pad or invoke an action.
* Spoofing from outside: the external WebSocket now rejects installed plugin ids as sources, as it already does for `runtime`, `device` and `agent-control`.
* Manifest and file hygiene: manifest and module sizes are capped, `main` must be a plain file name in the plugin directory (no separators, no `..`, no absolute path, no symlink), ids use a strict grammar, ids need at least two dotted parts, which built-in ids (`system`, `demo`, `agent-control`) do not have, so they cannot collide.
* Configuration: stored in `plugins.json` (atomic write, owner-only on Unix), never in the workspace or in surface JSON. Keys must be declared in the manifest. Values flagged `secret` are redacted in every snapshot, list and log.
* Fail closed on capabilities: the manifest has a `capabilities` array. In v1 it must be empty. A plugin that declares anything is refused, because this host cannot provide it. That keeps a path open to a real permission model (network by host name, for example) without building one now.

What is not claimed: this protects the host from the plugin. It does not protect the user from a plugin that lies in its label, or from a plugin that fills its own pad with misleading state. There is no signing in v1.

## 7. Public contract

Four versions, deliberately decoupled:

| Version | Meaning | Where | v1 value |
| --- | --- | --- | --- |
| Manifest schema | Shape of `plugin.json` | `schemaVersion` | 1 |
| Protocol (ABI) | Functions and messages between host and module | `abi` | 1 |
| SDK | The authoring library a plugin was built with | `sdk` (informational, checked for major compatibility) | 0.1.0 |
| Plugin | The plugin's own release | `version` | author's choice |

### Manifest (`plugin.json`)

```json
{
  "schemaVersion": 1,
  "id": "example.hello",
  "name": "Hello Plugin",
  "version": "0.1.0",
  "description": "Says hello and tells the deck about it.",
  "author": "LaunchDeck examples",
  "abi": 1,
  "sdk": "0.1.0",
  "main": "hello.wasm",
  "actions": [
    { "id": "hello.say", "label": "Say hello", "description": "Greets someone.",
      "args": [ { "name": "name", "type": "string", "required": false } ] }
  ],
  "events": [ { "type": "hello.completed", "description": "A greeting was produced." } ],
  "config": [ { "key": "greeting", "type": "string", "default": "Hello" } ],
  "capabilities": []
}
```

Rules: unknown fields are rejected; `id` is lowercase dotted with at least two segments; `version` and `sdk` are `x.y.z`; argument types are `string`, `number`, `boolean` or `json`; ids, action ids and event types use `[A-Za-z0-9_.:-]{1,64}`. The manifest has no executable content and no marketplace fields.

### Module ABI v1 (JSON over linear memory)

Guest exports: `memory`, `ld_abi() -> i32` (must equal the manifest `abi`), `ld_alloc(len) -> ptr`, `ld_handle(ptr, len) -> i32`. The host writes a request at a pointer from `ld_alloc`; the guest owns and frees that buffer. `ld_handle` returns 0 after calling `respond`, or non-zero to report an internal failure.

Host imports, module `launchdeck`: `log(level, ptr, len)`, `publish(ptr, len) -> i32` (0 accepted, non-zero rejected with a reason in the log), `respond(ptr, len)`. The host copies bytes out of guest memory immediately and bounds every length before reading.

Requests:

```json
{ "op": "init",   "abi": 1, "host": {"name": "launchdeck", "version": "0.2.0", "sdk": "0.1.0"},
  "plugin": {"id": "example.hello", "version": "0.1.0"}, "config": {"greeting": "Hello"} }
{ "op": "invoke", "action": "hello.say", "args": {"name": "Leroy"} }
```

Responses (normalized by the host into `Result<Value, ActionError>`):

```json
{ "status": "ok", "value": { "greeting": "Hello, Leroy!" } }
{ "status": "failed", "error": "name is too long", "code": "invalid_args" }
```

`publish` takes `{ "type": "hello.completed", "payload": {...} }`. The `status` field is a string so the contract can grow. `pending` (an action that finishes later) is reserved and rejected in v1; the compatible design is `{"status":"pending","token":"..."}` plus a future `launchdeck.complete(token, ptr, len)` import, which is additive and needs ABI 2 only if the semantics of existing calls change. Arguments are validated against the manifest (names, types, required) before the module is called, so a plugin only sees well-typed input.

State follows the existing model: a plugin publishes events, bindings turn them into control state. There is no second state store.

WIT sketch for a future component form of the same contract, not implemented:

```wit
package launchdeck:plugin@0.1.0;
world plugin {
  import log: func(level: u8, message: string);
  import publish: func(event-json: string) -> result<_, string>;
  export abi: func() -> u32;
  export handle: func(request-json: string) -> string;
}
```

### Lifecycle and statuses

`enabled` (loaded, healthy), `disabled` (never loaded, never invoked), `error` (failed validation, init, or trapped). Enabled flag and config persist in `plugins.json`. Rescan keeps unchanged plugins loaded, reloads changed ones, removes vanished ones.

## 8. Packaging model

A plugin is a directory holding `plugin.json` and one `.wasm` module. That directory is the whole artifact: no Node, Rust or compiler is needed to run it, and the same bytes run on macOS, Windows and Linux. Install means copying the directory into the user plugins folder (`<app data>/plugins/`), or any folder listed in `LAUNCHDECK_PLUGINS` for development. Compressed bundles, checksums and signatures are future work that the manifest does not block. No marketplace is built.

## 9. Developer workflow

1. Copy the `examples/plugins/hello` layout. The SDK (`sdk/assemblyscript`, package `@launchdeck/plugin-sdk`) is not published to npm yet, so the project depends on it by path.
2. Write actions in AssemblyScript (a TypeScript subset): register a function per action id, return `ok(value)` or `fail(message)`, call `publish(type, payload)`.
3. `npm run build` produces the `.wasm`. The SDK's `asc` flags are in `asconfig.json`.
4. Put the directory in a folder on `LAUNCHDECK_PLUGINS` and start LaunchDeck, or press rescan.
5. Read plugin diagnostics (the log on the plugin's card, backed by `plugins_logs`) and the status on the Plugins screen.
6. Any language that can emit a wasm module with the ABI above works: the runtime contract does not depend on AssemblyScript. A C module built with clang is part of the test suite as proof.

Why AssemblyScript first: it is the only toolchain I can verify end to end in a locked-down environment (npm only, no GitHub downloads, no extra Rust targets), it has a small learning curve for anyone who knows TypeScript, produces small modules (about 20 KB for Hello), and gives a real local debugging loop through Node's WebAssembly. A Rust SDK is the next language to add when the `wasm32-unknown-unknown` target can be installed and tested; its crate would be a thin wrapper over the same six functions.

## 10. Implementation phases

1. Contract and validation: manifest types, grammar, version rules, discovery with size and path hygiene, error entries instead of crashes. Done.
2. Sandbox: Wasmtime engine, limits, epoch ticker started only when a plugin loads, ABI glue, response and event bounds. Done.
3. Integration: dynamic registry, `WasmPlugin`, `PluginHost` with persistence (`plugins.json`), enable, disable, reload, rescan, config, logs, runtime wiring, six Tauri commands (`plugins_list`, `plugins_set_enabled`, `plugins_set_config`, `plugins_reload`, `plugins_rescan`, `plugins_logs`), dynamic spoofing guard, and a Plugins screen with the management controls. Done.
4. SDK and examples: AssemblyScript SDK, `hello` (required), `counter` (stateful), a C module that proves language independence. Done, with reproducible builds (`npm run plugins:build`).
5. Tests: the extension boundary and the full chain through the virtual Launchpad. Done (see the end of this section).
6. Documentation: this plan, the SDK guide in `docs/plugin-sdk`, and the architecture, plugins, marketplace and README updates. Done.

### What was built and measured

Tests (all passing): 68 unit tests in `plugins::external` (manifest grammar and versions, discovery and hygiene, sandbox limits, hostile modules written in WAT, host lifecycle, settings and secrets, log bounds); 13 end-to-end tests in `src-tauri/tests/plugin_sdk.rs` that load the real example plugins and drive them from pads on the virtual Launchpad, including the strongest one (pad press, plugin runs, plugin event, Event Bus, real binding, LED frame changes) which was mutation-checked (removing the binding makes it fail); a Rust test that keeps `examples/plugins-overview.json` complete; and 8 new TypeScript tests that parse that fixture with the Zod schemas. Agent Control and the earlier suites are unchanged and pass (290 library tests in total, 3 + 11 + 1 integration tests for the bridge, the flows and the agents contract). The production build was also run in a virtual desktop with the three example plugins installed in the app data folder: pads turned green and amber from plugin events, disabling a plugin from the Plugins screen made its pad fail with "plugin `example.counter` is disabled", and the log viewer showed the plugin's own lines.

Performance reasoning. A press costs one `spawn_blocking` hop (already there for every action), a mutex held for the call, two small JSON messages, and the module's own work. The module is compiled once at load and instantiated once; nothing is compiled or instantiated per call. Measured on a debug build in the development container: loading a small module 30 to 40 ms, an invocation with an event 20 to 40 microseconds, a press reaching the LED frame in about 35 ms (set by the 25 Hz render tick, not by the plugin), 40 installed plugins starting in about one second in total, and a rescan with nothing changed in about 1 ms. Release builds are faster. A runaway module is bounded by 100 million fuel units or 2 seconds, whichever comes first, and only blocks its own plugin because other plugins have their own instance and lock. The cost is binary size: the production installer grew from 4.1 MiB to 7.6 MiB (20 MB executable) because Wasmtime brings Cranelift. That is the price of running untrusted code with a JIT and the main reason wasmi stays documented as the fallback.

## 11. Migration and reuse strategy

Reused as is: the `Plugin` trait, `PluginRegistry` API shape, `ActionEngine`, `Action::Plugin`, `EventBus`, bindings and renderer, `persistence::write_private`, the id grammar from Agent Control, tracing logs, the `PluginManifest` snapshot field (extended with optional fields so existing fixtures stay valid).

Changed minimally: the registry becomes shareable and mutable at runtime; `RuntimeConfig` gains plugin directories; the WebSocket reserved-source list consults installed plugin ids.

Not migrated: System, Demo and Agent Control stay native. They are trusted first-party code, Agent Control needs deep host access, and rewriting them as sandboxed plugins would add risk for no user benefit. Demo remains the reference for built-in authors; `examples/plugins/hello` is the reference for third parties.

## 12. Risks

* Wasmtime security cadence. Mitigation: LTS 48, exact floors recorded (48.0.4 or later), no unnecessary features, a documented upgrade rule, wasmi as an escape hatch behind the same boundary.
* Binary size and compile time from Cranelift. Measured: installer 4.1 MiB to 7.6 MiB, and the first release build is slow. Mitigation: minimal features; module compile cache is future work; wasmi is the smaller fallback.
* One misbehaving plugin blocking a thread. Mitigation: epoch deadline and fuel, `spawn_blocking`, per-plugin mutex so other plugins are unaffected.
* Plugins that need I/O (network, files) cannot exist yet. Mitigation: the `capabilities` array fails closed today and a host-mediated capability is the planned route, not ambient WASI.
* AssemblyScript is a niche toolchain. Mitigation: the ABI is language independent and proven with a C module; Rust SDK is the next step.
* Component Model timing. Mitigation: one-to-one WIT mapping documented; decision to be revisited on evidence.
* No signing means a user can install a lying plugin. Mitigation: deny-by-default sandbox limits damage to the plugin's own pad state; signing is part of the marketplace roadmap.
