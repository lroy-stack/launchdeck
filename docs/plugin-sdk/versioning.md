# Versioning

Four numbers, deliberately independent so a change in one does not force the others.

| Version | What it versions | Where | Today |
| --- | --- | --- | --- |
| Manifest schema | The shape of `plugin.json` | `schemaVersion` | `3` (reads `1`, `2` and `3`) |
| Protocol (ABI) | The exports, imports and messages between host and module | `abi` in the manifest, `ld_abi()` in the module | `1` |
| SDK | The authoring library a plugin was built with | `sdk` in the manifest (optional) | `0.1.1` |
| Plugin | Your own release | `version` | yours |

## How they are checked

* **Manifest schema.** Read first. A number outside the supported range fails with `manifest schema version 4 is not supported (this LaunchDeck reads versions 1 to 3)`, so a new shape is reported rather than misread. Schema 2 only *adds* optional fields to actions (`category`, `keywords`, `appearance`, `params`), and schema 1's `args` are converted to `params` when the manifest is read. Schema 3 only adds `states`, the `name` and `fields` of an event and `suggests` on an action. The host reads all three; a plugin never has to move up unless it wants what the newer one adds. Neither change touches the module: `abi` stays `1`, so no `.wasm` file needs rebuilding. A rebuilt module is needed only to *call* `setState`.
* **Protocol.** The manifest `abi` must equal the host's and the module's `ld_abi()` must equal the manifest. A mismatch refuses to load with both numbers in the message. The protocol changes (to 2) only when the meaning of an existing call changes. State writes ride on `publish` as a new kind of message, so they did not need a new number either. Additions, such as the reserved `pending` status or new optional request fields, do not need a new number: the host ignores reply fields it does not know, and the SDK ignores request fields it does not know.
* **SDK.** Informational, because the protocol is what actually has to match. The host refuses a plugin built with a newer major version than it knows. Before 1.0, the minor version is the breaking one, so `0.2.x` is refused by a host that reads `0.1`. An older SDK is always accepted: this release moved the SDK from `0.1.0` to `0.1.1` (it adds `setState` and `clearState`), and plugins built with `0.1.0` still load.
* **Plugin.** Free for you; must look like `1.2.3`. The Plugins screen shows it. Nothing orders or compares plugin versions yet.

## Rules for LaunchDeck

* A host that understands protocol N keeps loading modules built for N until a deprecation is announced with a release's notice.
* New capabilities are added to the manifest's `capabilities` array with the host version that introduced them, and an old host refuses a plugin that needs one it lacks, instead of running it half-working.
* The WebAssembly runtime is pinned to a supported release line and updated for security fixes without changing the protocol.
