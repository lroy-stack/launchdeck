# Versions

LaunchDeck carries one release number and six compatibility numbers. They are independent: a change in one does not force a change in any other. All of them are shown in **Settings, About**, read by the `app_info` command (`src-tauri/src/commands/app.rs`) from the constants named below, and copied by its "Copy for a bug report" button.

| Number | Today | What it governs | Source | Who may change it, and when |
| --- | --- | --- | --- | --- |
| App version | `0.3.0`, the first external alpha | Which release this is. Plain numeric, because installers refuse pre-release suffixes; the channel (`alpha`) says what kind of release it is | `version` in `src-tauri/Cargo.toml`, the only source | The person cutting a release (package R4 for 0.3.0). `package.json` keeps an equal copy; `tauri.conf.json` has none, so Tauri reads Cargo.toml. Every provider built into the app (System, Demo, Agent Control, navigation, shell, HTTP) reports this number |
| Plugin SDK | `0.2.1` | The AssemblyScript authoring library a plugin was built with; the host refuses a newer minor version than it knows (before 1.0 the minor is the breaking one) | `SDK_VERSION` in `plugins/external/mod.rs`, equal to `sdk/assemblyscript/package.json` | Whoever releases the SDK, in the same commit as the host change it needs; `tests/plugins_contract.rs` holds the two equal |
| Plugin ABI | `1` | The exports, imports and messages between the host and a plugin's module | `ABI_VERSION` in `plugins/external/mod.rs` | Only when the meaning of an existing call changes. Additions do not need a new number; modules built for 1 keep loading until a deprecation is announced in a release |
| Plugin manifest schemas | `1` to `4` | The shapes of `plugin.json` this build reads; it writes none | `OLDEST_SCHEMA` and `MANIFEST_SCHEMA` in `plugins/external/manifest.rs` | A new shape of `plugin.json` gets the next number, and the older ones stay readable |
| Workspace schema | `4` | The shape of `workspace.json`. An older file is migrated on load; a newer one is refused, never downgraded | `SCHEMA_VERSION` in `core/model.rs` | Only with one new migration step file in `persistence` (rule R11) |
| Agent Bridge protocol | `1` | The `agent.*` messages local programs send over the WebSocket | `PROTOCOL` in `agents/bridge.rs` | Only for a change an existing client would misread; documented in [the bridge protocol](../agent-control/bridge-protocol.md) |
| Surface and Pack format | `1` | `.ldsurface` and `.ldpack` files, independent of the workspace schema | `FORMAT_VERSION` in `artifacts/mod.rs` | Only for a change an existing reader would misread; specified in `PRODUCTIZATION_PLAN.md` section 7 |

The integers are compatibility numbers, not release numbers. A `1` says "the first version of this contract", not that the contract is finished or will never change; none of them is a 1.0 promise, and neither is the app's own `0.x`.

## How they are kept honest

* `npm run versions:check` (`scripts/check-versions.mjs`, part of `npm run check`) fails when `package.json` and `src-tauri/Cargo.toml` differ or when `src-tauri/tauri.conf.json` has a version of its own.
* `tests/catalog_contract.rs` fails when a provider built into the app reports anything but the app version.
* The unit tests in `commands/app.rs` hold `app_info` to the constants above and to the numbers in this table: changing one of them fails a test, and the fix includes this table.
* `examples/app-info.json` is the contract fixture of `app_info` (written by `dump_app_info`, guarded by `tests/app_info_contract.rs`, parsed in `src/features/settings/settings.test.tsx`).

The plugin author's view of the plugin numbers is in [the SDK's versioning page](../plugin-sdk/versioning.md).
