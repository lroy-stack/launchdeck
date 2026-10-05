# Versioning

Four numbers, deliberately independent so a change in one does not force the others.

| Version | What it versions | Where | Today |
| --- | --- | --- | --- |
| Manifest schema | The shape of `plugin.json` | `schemaVersion` | `4` (reads `1` to `4`) |
| Protocol (ABI) | The exports, imports and messages between host and module | `abi` in the manifest, `ld_abi()` in the module | `1` |
| SDK | The authoring library a plugin was built with | `sdk` in the manifest (optional) | `0.2.1` |
| Plugin | Your own release | `version` | yours |

The app's own version and every other compatibility number LaunchDeck has (workspace schema, Agent Bridge protocol, Surface and Pack format) are listed in [the versions table](../architecture/versions.md), and shown in the app under **Settings, About**.

## How they are checked

* **Manifest schema.** Read first. A number outside the supported range fails with `manifest schema version 5 is not supported (this LaunchDeck reads versions 1 to 4)`, so a new shape is reported rather than misread. Schema 2 only *adds* optional fields to actions (`category`, `keywords`, `appearance`, `params`), and schema 1's `args` are converted to `params` when the manifest is read. Schema 3 only adds `states`, the `name` and `fields` of an event and `suggests` on an action. Schema 4 adds `capabilities` as an object (with `network`), `secrets`, `refresh` and a `label` on settings, and refuses the old `secret` flag on a setting. The host reads all four; a plugin never has to move up unless it wants what the newer one adds. None of these changes touches the module: `abi` stays `1`, so no `.wasm` file needs rebuilding. A rebuilt module is needed only to *call* `setState`, `http.request` or to answer `refresh`.
* **Protocol.** The manifest `abi` must equal the host's and the module's `ld_abi()` must equal the manifest. A mismatch refuses to load with both numbers in the message. The protocol changes (to 2) only when the meaning of an existing call changes. State writes ride on `publish` as a new kind of message, so they did not need a new number either. Network access added two imports (`http_request`, `http_response`) that exist only for a plugin whose manifest declares the capability, and one request (`refresh`) that is sent only to a plugin whose manifest asks for it; a module that uses neither sees exactly the protocol it was built for. Additions, such as the reserved `pending` status or new optional request fields, do not need a new number: the host ignores reply fields it does not know, and the SDK ignores request fields it does not know.
* **SDK.** Informational, because the protocol is what actually has to match. The host refuses a plugin built with a newer major version than it knows. Before 1.0, the minor version is the breaking one, so `0.2.x` is refused by a host that reads `0.1`. An older SDK is always accepted: `0.2.0` added `http` and `onRefresh`, `0.2.1` adds one error constant (`HttpError.SecretUnavailable`), and plugins built with `0.1.0`, `0.1.1` and `0.2.0` still load unchanged. The Hello, Counter and Channels modules that 0.1.x built are kept as that proof in `src-tauri/tests/fixtures/plugins/sdk-0.1`, loaded and run by a test; the examples themselves are rebuilt with the current SDK. A plugin that declares `"sdk": "0.2.0"` is refused by a LaunchDeck that only knows 0.1, which is intended: that host has no network broker.
* **Plugin.** Free for you; must look like `1.2.3`. The Plugins screen shows it. Nothing orders or compares plugin versions yet.

## The example plugins and reproducible builds

The four AssemblyScript examples in `examples/plugins` are built with the current SDK, and their manifests say so (`"sdk": "0.2.1"`). `scripts/build-example-plugins.sh` builds Hello, Counter, Channels and Home Assistant by default. It installs their dependencies with `npm ci`, from the committed lockfiles, which pin AssemblyScript and binaryen and are never rewritten by a build. `examples/plugins/SHA256SUMS` records the SHA-256 of the four committed modules, and

```sh
sh scripts/build-example-plugins.sh --check
```

builds each of them again in a temporary copy of the repository, leaving the committed files alone, and exits with 1 if a built module or a committed one differs from `SHA256SUMS`. A change to a plugin's source, to the SDK or to a lockfile therefore comes with the rebuilt module and its new line in `SHA256SUMS`, in the same commit.

Reproducible here means: the same sources, lockfile and SDK give the same bytes. That was observed on macOS 15.4.1 (Apple M2) with Node 22.23.3 and npm 10.9.9: two `--check` runs in a row matched `SHA256SUMS` for all four, and Hello built outside the repository against a packed SDK gave the same bytes too. Other systems and other Node or npm versions are **NOT VERIFIED**: no Linux or Windows machine was used, and the CI that would run it does not start (account billing). C Ping is not covered: it is built with clang for `wasm32` and `wasm-ld`, which Apple's clang does not have, so its committed `ping.wasm` was not rebuilt and has no line in `SHA256SUMS`.

## The frozen SDK 0.1 modules

Rebuilding the examples would have removed the only proof that plugins built with an older SDK still run. That proof is kept as a test fixture instead: `src-tauri/tests/fixtures/plugins/sdk-0.1` holds the Hello, Counter and Channels modules and manifests exactly as SDK 0.1.0 and 0.1.1 built them, with a `PROVENANCE.md` that names the commits that built them and the SHA-256 of every file. The test `modules_built_with_sdk_0_1_still_load_and_run` (`src-tauri/tests/plugin_sdk.rs`) loads them through the real plugin host and runs one action of each, checking the reply, the event and the state it reports. Those files are never rebuilt or edited. When a later SDK changes what it builds in a way that needs the same proof, the modules it replaces get a folder of their own next to this one.

## Rules for LaunchDeck

* A host that understands protocol N keeps loading modules built for N until a deprecation is announced with a release's notice.
* A new capability is a new key under the manifest's `capabilities`, and a host refuses a plugin that asks for one it lacks instead of running it half-working. Grants are stored outside the manifest and the workspace, so neither format changes when a grant does.
* The workspace schema did not change for capabilities: looks named `pending` and `failed` are ordinary looks.
* The WebAssembly runtime is pinned to a supported release line and updated for security fixes without changing the protocol.
