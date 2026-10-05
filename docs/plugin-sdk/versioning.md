# Versioning

Four numbers, deliberately independent so a change in one does not force the others.

| Version | What it versions | Where | Today |
| --- | --- | --- | --- |
| Manifest schema | The shape of `plugin.json` | `schemaVersion` | `4` (reads `1` to `4`) |
| Protocol (ABI) | The exports, imports and messages between host and module | `abi` in the manifest, `ld_abi()` in the module | `1` |
| SDK | The authoring library a plugin was built with | `sdk` in the manifest (optional) | `0.2.1` |
| Plugin | Your own release | `version` | yours |

## How they are checked

* **Manifest schema.** Read first. A number outside the supported range fails with `manifest schema version 5 is not supported (this LaunchDeck reads versions 1 to 4)`, so a new shape is reported rather than misread. Schema 2 only *adds* optional fields to actions (`category`, `keywords`, `appearance`, `params`), and schema 1's `args` are converted to `params` when the manifest is read. Schema 3 only adds `states`, the `name` and `fields` of an event and `suggests` on an action. Schema 4 adds `capabilities` as an object (with `network`), `secrets`, `refresh` and a `label` on settings, and refuses the old `secret` flag on a setting. The host reads all four; a plugin never has to move up unless it wants what the newer one adds. None of these changes touches the module: `abi` stays `1`, so no `.wasm` file needs rebuilding. A rebuilt module is needed only to *call* `setState`, `http.request` or to answer `refresh`.
* **Protocol.** The manifest `abi` must equal the host's and the module's `ld_abi()` must equal the manifest. A mismatch refuses to load with both numbers in the message. The protocol changes (to 2) only when the meaning of an existing call changes. State writes ride on `publish` as a new kind of message, so they did not need a new number either. Network access added two imports (`http_request`, `http_response`) that exist only for a plugin whose manifest declares the capability, and one request (`refresh`) that is sent only to a plugin whose manifest asks for it; a module that uses neither sees exactly the protocol it was built for. Additions, such as the reserved `pending` status or new optional request fields, do not need a new number: the host ignores reply fields it does not know, and the SDK ignores request fields it does not know.
* **SDK.** Informational, because the protocol is what actually has to match. The host refuses a plugin built with a newer major version than it knows. Before 1.0, the minor version is the breaking one, so `0.2.x` is refused by a host that reads `0.1`. An older SDK is always accepted: `0.2.0` added `http` and `onRefresh`, `0.2.1` adds one error constant (`HttpError.SecretUnavailable`), and plugins built with `0.1.0`, `0.1.1` and `0.2.0` still load unchanged. The Hello, Counter and Channels modules that 0.1.x built are kept as that proof in `src-tauri/tests/fixtures/plugins/sdk-0.1`, loaded and run by a test; the examples themselves are rebuilt with the current SDK. A plugin that declares `"sdk": "0.2.0"` is refused by a LaunchDeck that only knows 0.1, which is intended: that host has no network broker.
* **Plugin.** Free for you; must look like `1.2.3`. The Plugins screen shows it. Nothing orders or compares plugin versions yet.

## Rules for LaunchDeck

* A host that understands protocol N keeps loading modules built for N until a deprecation is announced with a release's notice.
* A new capability is a new key under the manifest's `capabilities`, and a host refuses a plugin that asks for one it lacks instead of running it half-working. Grants are stored outside the manifest and the workspace, so neither format changes when a grant does.
* The workspace schema did not change for capabilities: looks named `pending` and `failed` are ordinary looks.
* The WebAssembly runtime is pinned to a supported release line and updated for security fixes without changing the protocol.
