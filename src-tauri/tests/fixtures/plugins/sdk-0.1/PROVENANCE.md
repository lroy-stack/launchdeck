# Plugins built with SDK 0.1

Three example plugins exactly as they were built with the AssemblyScript SDK 0.1.x, before the network capability (SDK 0.2.0) existed. They are a compatibility fixture: `modules_built_with_sdk_0_1_still_load_and_run` in `src-tauri/tests/plugin_sdk.rs` loads each one and runs an action. If a change to the host makes that test fail, plugins that people already have stop working.

The examples in `examples/plugins` are rebuilt with the current SDK; these files are not. Never rebuild or edit them. A later SDK that needs the same proof gets its own folder next to this one (`sdk-0.2`, ...).

| Folder | Plugin | Module built in | SDK | Manifest last changed in |
| --- | --- | --- | --- | --- |
| `hello` | `example.hello` 0.1.0 | `9d1f718` (2026-10-04) | 0.1.0 | `9d1f718` |
| `counter` | `example.counter` 0.2.0 | `11da720` (2026-10-04) | 0.1.1 | `11da720` |
| `channels` | `example.channels` 0.1.0 | `11da720` (2026-10-04) | 0.1.1 | `11da720` |

The commits are the ones that last changed `examples/plugins/<name>/<name>.wasm` and `plugin.json` (`git log --format=%h -- examples/plugins/<name>/<name>.wasm`); the files were copied from the tree at `02a95a3`, where they were still unchanged. The SDK version is the one `sdk/assemblyscript/package.json` had in the building commit and the one the manifest's `sdk` field states. All three were compiled with AssemblyScript 0.28.20 and binaryen 131.0.0-nightly.20260721 (their lockfiles at those commits), target `release` (`optimizeLevel` 3, `shrinkLevel` 2, `noAssert`, incremental runtime). Each module imports only `launchdeck.respond`, `launchdeck.publish` and, for Hello, `launchdeck.log`; SDK 0.1 sent states through `publish`.

SHA-256 of every file, checkable with `shasum -a 256 -c` (or `sha256sum -c`) from this folder:

```
0794fe70cb9f8309dd5414acbef86568f2a66e2bf752a88a930fe54a397557a0  channels/channels.wasm
44580ac28db5762b2cda8cac2e370fa8a62a3c0399d2fb8bfb7596da00649d42  channels/plugin.json
f5fdf6b3773bc743b9992ae91c3efda726636472954699333ed2e810dcdddfd6  counter/counter.wasm
b5bf2affdec9cdbdc7922a65ad489fd9ecf716fb23476d9a5157a4d309ee8fbf  counter/plugin.json
69a0956b92df3e14c33a80375cd8fcb4209c52d59a800552bf4edc6ed9578acb  hello/hello.wasm
9e18787f442d4198e38a18cb372527cf8ec4e7cf9cc0de27e35903d880f91b6a  hello/plugin.json
```
