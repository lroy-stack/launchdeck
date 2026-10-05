# Codebase map: Rust

Who owns what in `src-tauri/src`, what each module may import, and where new code goes. The frontend has its own map, [FRONTEND_MAP.md](FRONTEND_MAP.md), so that a Rust package and a TypeScript package can work at the same time without editing the same file. The rules these maps serve are in [ENGINEERING_RULES.md](ENGINEERING_RULES.md); the runtime itself is described in [overview.md](overview.md).

Rust paths are relative to `src-tauri/src/`. A module is written `a::b`; `a::{b, c}` means both. Rows marked **planned (package X)** do not exist yet: package X of `PRODUCTIZATION_PLAN.md` creates them. Section 1 says where such code lives until then.

When a change moves ownership (a new top-level module, a concept changing owner, a new allowed edge), this file changes in the same commit (R1, R21).

## 1. Where does it belong?

| You are adding | It goes in | Today, until the package lands |
| --- | --- | --- |
| A new device model | `devices/<model>.rs`, behind the `SurfaceDevice` trait; its outside crate stays in that one file | same |
| A new action provider | `plugins` (a built-in provider registered in the `PluginRegistry`) or an installed plugin (WebAssembly, `plugins::external`) | same |
| A new state or event | the provider that owns the value: it declares an `ObservableDefinition` and writes the hub (states) or publishes on the bus (events) | same |
| A new IPC command | `commands/<feature>.rs`, a Zod schema in `src/lib/schema/<domain>.ts`, and a contract fixture written by a `dump_*` example (section 6) | the command in `lib.rs` until A6; the schema in `src/lib/schema.ts` until A4 |
| Starter content | `surfaces/<starter>.rs` | same |
| A workspace schema change | the types in `core::model`, plus one new persistence step file (as `persistence/legacy.rs` and `persistence/bindings.rs` are) | same |
| Platform behaviour | `platform.rs`, or the adapter file of the outside system it talks to | same |
| A portable format (Surface or Pack files) | `artifacts/` | planned (C1) |
| UI | `src/features/<area>` (see [FRONTEND_MAP.md](FRONTEND_MAP.md)) | same |

If nothing in the table fits, find the owner of the nearest concept in section 4 before creating anything. A new top-level module needs a row in section 3 in the same commit.

## 2. Rings

Dependencies point inward: domain ← services ← adapters ← UI boundary. The rings give the direction; the exact lists the check enforces are in section 5. One adapter-ring module is imported from inner rings today, and section 5 permits it: `events` (for `Event`, `EventBus`, `now_ms` and the identifier rule).

```text
 UI boundary   lib.rs, commands/ (A6), tray.rs (A6)  ──►  React, only through src/lib/ipc.ts
      │
      ▼
 Adapters      devices::*, renderer, context::observer::*, events::{mod, external},
               plugins::external::{sandbox, network, secrets}, plugins::keyboard,
               actions::builtin, agents::{adapter, adapters::*, bridge}
      │
      ▼
 Services      core::runtime (composition root), core::{review, explain, observables},
               state::hub, actions::{registry, ActionEngine in actions/mod.rs},
               plugins::{PluginRegistry in plugins/mod.rs, system, demo},
               plugins::external::{host, manifest, capabilities, discovery, wasm_plugin, log,
                                   install (D2), legacy_secrets (D1)},
               agents::{service, settings, plugin, demo, notify, view}, audit,
               persistence, artifacts::service (C5)
      │
      ▼
 Domain        core::{model, validate},
               state::{value, address, definition, feedback, eval, lint, index, catalog types},
               actions::{params, definition}, context::{identity, rules, engine, status, portable (B1)},
               agents::{model, visual, observables}, surfaces (content), platform,
               artifacts except service (C1-C5)
```

`agents::pack` (a dead Agent Pack manifest that nothing loads) is outside the rings; C1 deletes it.

## 3. Rust modules

"May depend on" lists what the module imports in production code today (test code may import more). "Must not depend on" is the matrix of section 5.

| Module | Owns | May depend on | Must not depend on | Platform code here? | Tests | Public consumers |
| --- | --- | --- | --- | --- | --- | --- |
| `lib.rs` (crate root) | `run()`, `AppState`, the 45 Tauri commands, the tray menu, `generate_handler!`, the module list | anything | — | no (`tauri` is confined here) | none of its own | `main.rs` |
| `core::model` | the persisted aggregate: `Workspace` → `Surface` → `Page` → `Control`, `ControlPos`, `Preferences`, `SCHEMA_VERSION`; re-exports `Assignment` and `FeedbackRule` | `actions::definition`, `state::feedback`, `context::rules` (types it embeds) | `core::runtime`, `devices`, `renderer`, `plugins`, `agents`, `persistence`, `events` | no | inline; `examples/default-workspace.json` parsed in Vitest | nearly every module, `lib.rs`, most integration tests |
| `core::validate` | `validate_control`: coordinates, spans, overlap and the logo position of a control on its page | `core::model` | as `core::model` | no | inline | `core::runtime` |
| `core::runtime` | `Runtime`, the composition root: device lifecycle, active surface and page, Live/Edit, long press, action dispatch, control looks, rendering tick, the one activation path `Runtime::activate`, persistence calls, startup of every provider. Split into `core/runtime/` by A7 | anything | — | no | inline (`#[cfg(test)]`, about 600 lines); most files in `src-tauri/tests` | `lib.rs`, integration tests, `examples/headless.rs` |
| `core::{review, explain, observables}` | `review`: what saving a draft control's feedback would refuse and prove (`review_feedback`). `explain`: why a control looks as it does (`explain_control`). `observables`: the native `runtime` and `device` providers' states and events | `core::model`, `state::*`, `actions::definition`, `events::types` | not listed in the matrix; follow the ring direction | no | inline (`review`, `observables`); `tests/feedback_editor.rs`, `tests/plugin_states.rs`, `tests/observables_contract.rs` | `core::runtime`, `lib.rs` |
| `state` | `state::{value, address, definition, feedback, eval, lint, index, catalog, hub}`: scalars, state addresses, observable definitions, `FeedbackRule` and its gate `prepare_rules`, the pure evaluator, lint, the dependency index, the Observable Catalog, the `StateHub` (current values only) | `actions::params`, `actions::definition`, `core::model` (index), `events` (`now_ms`, the identifier rule) | `devices`, `renderer`, `core::runtime`, `plugins`, `agents`, `context`, `surfaces`, `persistence` | no | inline in each file; `tests/{state_hub, state_feedback, state_characterization, plugin_states}.rs` | `core::*`, `actions`, `plugins`, `agents`, `persistence::bindings`, `surfaces::agent_control`, `lib.rs` |
| `actions` | `actions::definition` (`Assignment`, `ActionDefinition`, `Catalog`, `Issue`, provider kinds and status), `actions::params` (`ParamDef`, validation, `valid_param_name`), `actions::registry` (`ActionRegistry`, the single gate `prepare`), `actions::builtin` (navigation, shell and HTTP providers; `ureq`), `ActionEngine` | `core::model`, `plugins`, `plugins::external` (registry), `state`, `events` | `devices`, `renderer`, `core::runtime`, `agents`, `context`, `surfaces`, `persistence` | `actions/builtin.rs` (allowed: shell per system, `cfg(unix)`) | inline; `tests/action_library.rs`, `tests/catalog_contract.rs` | `core::*`, `plugins`, `agents::{plugin, observables}`, `state`, `lib.rs` |
| `plugins` | the `Plugin` trait, `PluginRegistry`, built-in `PluginManifest`, the System plugin (`plugins::system`), the Demo plugin (`plugins::demo`), the keyboard backend (`plugins::keyboard`, the only `enigo` user) | `actions::{definition, params}`, `core::model`, `state`, `events`, `platform` | `devices`, `renderer`, `core::runtime`, `context`, `surfaces`, `agents` | `plugins/keyboard.rs` (allowed); `plugins/system.rs` takes the modifier key from `Platform::current()` | inline; `tests/plugin_sdk.rs` and the other plugin targets | `core::runtime`, `actions`, `agents::plugin`, `persistence::bindings`, `lib.rs` |
| `plugins::external` | installed WebAssembly plugins: `manifest` (schemas 1-4), `discovery`, `sandbox` (the only `wasmtime` user), `wasm_plugin`, `host` (`PluginHost`, `plugins.json`), `capabilities` (grants, `permissions.json`), `network` (the broker), `secrets` (`SecretStore`, the only `keyring` user), `log`; `ABI_VERSION`, `SDK_VERSION` | `plugins`, `actions::{definition, params}`, `state`, `events`, `persistence::write_private`, `audit`, `platform` | as `plugins` | `plugins/external/secrets.rs` (allowed; the store name and the unavailable reason take a `Platform`); `cfg(unix)` file permissions | `plugins/external/{tests, state_tests}.rs`, inline; `tests/{plugin_sdk, plugin_states, plugin_network, home_assistant, secret_store, plugins_contract}.rs` | `core::runtime`, `actions::registry`, integration tests, `examples/headless.rs` |
| `plugins::external::install` | **planned (D2)**: install a bundled example plugin (validate, copy through a temp dir and rename, rescan) | `plugins::external::{discovery, host}` | as `plugins` | no | planned | `commands` |
| `plugins::external::legacy_secrets` | **planned (D1)**: the code for schema 1-3 `config[].secret` values kept in plain text, moved out of `host.rs`; D4 then makes sure such a value never reaches a module and is migrated only on the person's explicit choice | `plugins::external::*` | as `plugins` | no | planned | `plugins::external::host` |
| `agents` | Agent Control: `agents::model` (domain types, statuses, `valid_id`), `visual`, `adapter` (the adapter SDK), `service` (`AgentControl`), `view`, `bridge` (Agent Bridge over the WebSocket), `demo`, `plugin` (the `agent-control` provider), `observables`, `notify`, `settings` (`agents.json`), `pack` (dead, removed in C1); `agents::audit` re-exports `audit` at its old path | `state`, `events`, `actions::{definition, params}`, `plugins`, `core::model` (visual), `persistence::write_private` | `devices`, `renderer`, `core::runtime`, `context`, `surfaces` | no | `agents/tests.rs`, inline; `tests/{agent_bridge, agent_control_flows, agent_control_golden, agents_contract}.rs` | `core::runtime`, `lib.rs`, `surfaces::agent_control`, `persistence::bindings` |
| `agents::adapters` | the only code that names a vendor: `http` (loopback hook endpoint), `hooks`, `claude_code`, `codex`; `HookManager` | `agents::{adapter, model, service}` | as `agents`, and also `core::model`, `agents::visual`, `state::feedback` (same for `agents::bridge`) | no | `agents/adapters/tests.rs` | `core::runtime`, `lib.rs` |
| `audit` | the append-only audit trail (`audit.jsonl`: `Audit`, `AuditEntry`, `Actor`), written by Agent Control and the plugin host; `agents::audit` re-exports it at its old path | `events` (`now_ms`), `agents::model` (`summarize`) | not listed in the matrix; follow the ring direction | `cfg(unix)` file permissions | inline | `agents::{service, view}`, `plugins::external::{capabilities, host}` |
| `context` | the Context Engine: `context::identity` (`ApplicationIdentity`, namespaces), `rules` (`ContextRule`, `ContextSettings`, `checked`), `engine` (Auto / Manual Override state machine), `status` (`ContextStatus`) | `core::model`, `platform` | everything except `context`, `core::model` and `platform` | no: `identity::namespace_of` takes a `Platform`, `native_namespace` passes `Platform::current()` | inline; `tests/context_engine.rs`, `tests/context_contract.rs` | `core::runtime`, `core::model` (`ContextSettings`), integration tests |
| `context::observer` | which application is in front: the `ForegroundObserver` trait and one adapter per system (`macos` with `objc2`/`block2`, `windows` with `windows-sys`, `x11` with `x11rb`), plus `scripted` for tests | `context::identity` | as `context` | yes (allowed) | inline in `x11`; `context::observer::scripted` drives `tests/context_engine.rs` | `core::runtime`, `examples/foreground.rs` |
| `context::portable` | **planned (B1)**: the pure resolver from a Pack's portable application to an `ApplicationIdentity` | `context::identity` | as `context` | no | planned | `artifacts` |
| `events` | `Event`, `EventBus`, the built-in event names (`events::types`), `now_ms`, the identifier rule `valid_ident`; `events::external`: the localhost WebSocket (the only `tokio-tungstenite` user), reserved sources | nothing else in the crate | every other crate module | no | inline in both files | everything that publishes or listens |
| `devices` | the `SurfaceDevice` trait, `DeviceCapabilities`, `DeviceInput`, `SurfaceFrame`, `Led`; `devices::manager`, `devices::virtual_device` (the simulator), `devices::launchpad_x` (the only MIDI code and the only `midir` user) | `core::model` | everything except `devices` and `core::model` | no | inline in `launchpad_x`, `manager`, `virtual_device` | `core::runtime`, `renderer`, `lib.rs`, integration tests |
| `renderer` | pure functions from a control's visual to a capability-adapted `SurfaceFrame` (dropped positions, synthesised pulse and flash, luminance for non-RGB) | `devices`, `core::model` | everything except `renderer`, `devices`, `core::model` | no | inline | `core::runtime` |
| `surfaces` | the starter surfaces, one file each (`surfaces::{development, productivity, smart_home, accessibility}`, and `surfaces::agent_control`, the Agent Control surface), built with `surfaces::build` (positions, controls, actions), `surfaces::navigation` (surface switchers, back control) and `surfaces::palette` (the colours); `surfaces::workspace` (`starter_surfaces`, `default_workspace(_for)`, `blank_surface`); `surfaces::upgrade` (`add_agent_control`, the v1 → v2 content step); `Platform` (re-exported from `platform`). `mod.rs` holds only declarations and re-exports; every child but `agent_control` is private | `core::model`, `agents::{model, observables, plugin, service, visual}`, `state`, `platform` | `devices`, `renderer`, `core::runtime`, `plugins::external`, `persistence`, `context::engine` | no | `surfaces/tests.rs` (incl. the fixture test), inline in `agent_control.rs`; `tests/agent_control_golden.rs` | `core::runtime`, `lib.rs`, `persistence`, `examples/dump_workspace.rs`, integration tests |
| `persistence` | `Store` (`workspace.json`), `migrate()` and one file per schema step (`persistence::legacy` 2 → 3, `persistence::bindings` 3 → 4), `write_atomic`, `write_private` | `core::model`, `state` (the feedback types step 3 → 4 writes), `surfaces::add_agent_control`; in step files only, constants of the features they convert (`agents`, `plugins::demo`) | `core::runtime`, `devices`, `renderer`, `context`, `plugins::external` | `cfg(unix)` file permissions | inline; fixtures `tests/fixtures/workspace-v2.json`, `workspace-v3.json` | `core::runtime`, `lib.rs`, `agents::settings`, `plugins::external::{host, capabilities}` |
| `platform` | `Platform { Linux, MacOs, Windows }` and `Platform::current()`, the only product-logic reader of `cfg!(target_os)` | nothing | — | yes (allowed) | inline | `surfaces` (re-export), `context::identity`, `plugins::system`, `plugins::external::secrets`; every module may import it |
| `commands` | **planned (A6)**: the Tauri commands, one file per feature (`commands/{workspace, context, actions, feedback, devices, agents, plugins}.rs`; later `artifacts.rs` and `app.rs`) | anything | — | no | planned | `lib.rs` |
| `tray` | **planned (A6)**: the tray menu, moved from `lib.rs` | anything | — | no | planned | `lib.rs` |
| `artifacts` | **planned (C1-C5)**: portable Surface and Pack files: `limits`, `json` (pre-pass, digest, canonical writer), `format`, `inspect`, `export`, `env`, `plan`, `apply`, `service` | see section 5 | see section 5 | no | planned: unit tests beside each file, `tests/artifacts_security.rs`, `tests/artifacts_contract.rs` | `lib.rs`, `commands` only |

## 4. Owners of cross-cutting concepts

One owner per concept. Before adding a type, a constant or a helper for one of these, use the owner.

| Concept | Owner | Notes |
| --- | --- | --- |
| Workspace aggregate | `core::model::Workspace` | changed only through `Runtime` methods (`core::runtime`), checked by `core::validate`, saved by `persistence::Store` |
| Surface, Page, Control, `ControlPos` | `core::model` | geometry: `core::validate::validate_control`; starters: `surfaces` |
| Assignment and `ParamDef` | `actions::definition::Assignment`, `actions::params::ParamDef` | every save and every press passes `ActionRegistry::prepare` (`actions::registry`); TypeScript mirror `normalizeArgs` in `src/lib/actions.ts` |
| `FeedbackRule` | `state::feedback` | evaluation `state::eval`, lint `state::lint`, review and explanation `core::{review, explain}`; TypeScript mirrors `operatorsFor`, `problemsOf` in `src/lib/feedbackEdit.ts` |
| Hub values | `state::hub::StateHub` | written only by providers: the runtime, Agent Control, built-in plugins through their `PluginContext`, and the host on behalf of installed plugins; never persisted |
| Observable Catalog | `state::catalog::ObservableCatalog` | sources: `core::observables` (runtime, device), `agents::observables`, plugin manifests (`plugins::external::manifest`), the Demo plugin |
| Event names | `events::types` (built-in) | runtime and device event definitions in `core::observables`; agent events in `agents::service`; installed plugins declare theirs in the manifest; reserved sources in `events::external` |
| Identifier rule (`[A-Za-z0-9_.:-]{1,64}`) | `events::valid_ident` | used by `events::external`, `state::definition`, `plugins::external::{manifest, sandbox, wasm_plugin}` and `agents::model::valid_id`; `state::address::valid_ident` is a different, stricter rule for state ids; parameter names (also plugin config and secret keys): `actions::params::valid_param_name` |
| Audit trail (`audit.jsonl`) | `audit` | append-only, so it is R11's exception to `write_atomic`; `agents::audit` is a re-export, not a second owner |
| Platform | `platform::Platform` | logic takes `Platform` as a parameter; `surfaces::Platform` is a re-export, not a second owner |
| File writes | `persistence::{write_atomic, write_private}` | `write_private` is owner-only on Unix |
| Workspace schema version and migrations | `persistence::migrate` with one step file per version | the constant `SCHEMA_VERSION` sits in `core::model` beside the types it versions |
| Secrets | `plugins::external::secrets::SecretStore` (system credential store) | the value is used only by `plugins::external::network`, which adds it to a request for its origin (the host only checks which secrets are set); never given to a plugin, except the legacy schema 1-3 `config[].secret` settings until D4 (section 7) |
| Grants | `plugins::external::capabilities` (`permissions.json`) | Agent Control's per-agent control grants are in `agents::settings` (`agents.json`) |
| Context rules | `context::rules::{ContextRule, ContextSettings}` | stored in `Workspace.context`; checked by `context::rules::checked` |
| Agent model | `agents::model` | the live state is `agents::service::AgentControl` |
| Plugin manifest and SDK version | `plugins::external::manifest` (`MANIFEST_SCHEMA`), `plugins::external::{ABI_VERSION, SDK_VERSION}` | built-in providers describe themselves with `plugins::PluginManifest` |
| Portable artifacts and their versions | `artifacts` (planned, C1) | `formatVersion` 1, independent of every other version number |
| Version requirements | the `semver` crate (planned, C1) | today a hand-rolled `Version` in `plugins::external::manifest` and `valid_version` in `agents::pack`; C1 replaces both |
| IPC payload shapes | the Rust types (serde) | Zod schemas in `src/lib/schema.ts` (split by A4); contract fixtures in section 6 |
| Tauri commands | `lib.rs` today; `commands` after A6 | only `src/lib/ipc.ts` calls them |

## 5. Import matrix and allowlists

Enforced by `scripts/check-architecture.mjs`, with the rules as data in `scripts/architecture/rust.mjs` (both added by package A2). Change this section and `rust.mjs` in one commit. The lists below are copied from `PRODUCTIZATION_PLAN.md` section 5.1. They apply to non-test Rust code.

Forbidden imports:

* `context/**` imports only `crate::{context, core::model}`.
* `devices/**` only `crate::{devices, core::model}`; `renderer/**` only `crate::{renderer, devices, core::model}`; `events/**` no other crate module.
* `state/**` must not import `devices, renderer, core::runtime, plugins, agents, context, surfaces, persistence`.
* `core/model.rs`, `core/validate.rs` must not import `core::runtime, devices, renderer, plugins, agents, persistence, events`.
* `actions/**` must not import `devices, renderer, core::runtime, agents, context, surfaces, persistence`.
* `plugins/**` must not import `devices, renderer, core::runtime, context, surfaces, agents`.
* `agents/**` must not import `devices, renderer, core::runtime, context, surfaces`; `agents/adapters/**` and `agents/bridge.rs` also not `core::model, agents::visual, state::feedback`.
* `surfaces/**` must not import `devices, renderer, core::runtime, plugins::external, persistence, context::engine`.
* `persistence/**` must not import `core::runtime, devices, renderer, context, plugins::external`; feature constants are allowed only in migration step files.
* `artifacts/**` (except `service.rs`) may import only `crate::{artifacts, core::{model, validate}, state::{feedback, address, value}, actions::{definition, params}, context::{identity, rules, portable}, devices` (capability types), `platform, plugins::external::manifest, plugins::system` (action-id constants)`}`. `artifacts/service.rs` may also import `core::runtime` and `persistence`. Only `lib.rs` and `commands/**` import `artifacts`.
* `core::runtime`, `lib.rs`, `commands/**` may import anything.

Every module may import `platform`.

Crate confinement: midir → `devices/launchpad_x.rs`; wasmtime → `plugins/external/sandbox.rs`; keyring → `plugins/external/secrets.rs`; enigo → `plugins/keyboard.rs`; objc2, block2, objc2_*, windows_sys, x11rb → `context/observer/**`; tokio_tungstenite → `events/external.rs` (tests anywhere); ureq → `actions/builtin.rs`, `plugins/external/network.rs`; tauri → `lib.rs`, `commands/**`, `tray.rs`; semver, sha2 → `artifacts/**` and `plugins/external/manifest.rs`. A new outside system gets exactly one adapter file and a row in this list.

Platform isolation: `cfg!(target_os…)`, `#[cfg(target_os…)]`, `cfg(windows)` appear only in `platform.rs`, `context/observer/**`, `plugins/external/secrets.rs`, `plugins/keyboard.rs`, `actions/builtin.rs`, and file-permission code under `cfg(unix)`. Logic takes `Platform` as a parameter.

## 6. Contract fixtures

The contract fixtures today. Each `dump_*` fixture is written from the real runtime by an example in `src-tauri/examples/`, guarded by a Rust test (exact equality for the workspace; for the others, the test fails when the runtime emits a field the fixture lacks), and parsed with the Zod schemas in Vitest. R10 asks that every Rust-emitted shape has one: a new shape gets its fixture, generator and guard in the same change. Regenerate a fixture only for an intended change, and say so in the commit (see [ENGINEERING_RULES.md](ENGINEERING_RULES.md#regenerating-a-contract-fixture)).

| Fixture (`examples/`) | Generator | Rust guard | TypeScript tests |
| --- | --- | --- | --- |
| `default-workspace.json` | `dump_workspace` (`surfaces::default_workspace_for(Platform::Linux)`) | `surfaces::tests::contract_fixture_matches_the_default_workspace` in `surfaces/tests.rs` (exact equality) | `src/lib/schema.test.ts`, `src/lib/actions.test.ts`, `src/lib/feedback.test.ts`, `src/stores/runtime.test.ts` |
| `agents-snapshot.json` | `dump_agents` (`tests/support/agents_scenario.rs`) | `tests/agents_contract.rs` | `src/lib/agents.test.ts` |
| `plugins-overview.json` | `dump_plugins` (`tests/support/plugins_scenario.rs`) | `tests/plugins_contract.rs` | `src/lib/plugins.test.ts`, `src/features/plugins/access.test.tsx` |
| `action-catalog.json` | `dump_catalog` (`tests/support/catalog_scenario.rs`) | `tests/catalog_contract.rs` | `src/lib/actions.test.ts` |
| `observables.json` | `dump_observables` (`tests/support/observables_scenario.rs`) | `tests/observables_contract.rs` | `src/lib/schema.test.ts`, `src/lib/observables.test.ts`, `src/lib/feedback.test.ts`, `src/lib/feedbackEdit.test.ts`, and through `src/features/feedback/testkit.tsx` the feedback feature tests |
| `context-status.json` | `dump_context` (`tests/support/context_scenario.rs`) | `tests/context_contract.rs` | `src/features/context/context.test.tsx` |
| `feedback-editor-counter.json`, `feedback-editor-event.json` | none: they are the controls the feedback editor produces, and `src/lib/feedbackEdit.test.ts` proves its functions produce them | `tests/feedback_editor.rs` runs them on the real runtime | `src/lib/feedbackEdit.test.ts`, `src/features/feedback/rules.test.tsx` |

`src/lib/preview.ts` (the browser-only stand-in for the runtime) also reads `default-workspace.json`, `agents-snapshot.json`, `action-catalog.json` and `observables.json`. Rust-only goldens, not shared with TypeScript: `src-tauri/tests/fixtures/agent-control-golden.json` (`tests/agent_control_golden.rs`) and the migration inputs `src-tauri/tests/fixtures/workspace-v2.json`, `workspace-v3.json`.

Planned: `examples/artifacts/*` from `dump_artifacts`, guarded by `tests/artifacts_contract.rs` (C5); `examples/param-cases.json` from `dump_param_cases`, guarding the `normalizeArgs` mirror (E1).

## 7. Known exceptions and their planned fix

| Exception | Where | Fix |
| --- | --- | --- |
| `editor` ↔ `feedback` import cycle; shared widgets inside features; the sidebar importing a feature | `src/features/{editor, feedback, surfaces, agents, devices}`, `src/components/Sidebar.tsx` | A4; details in [FRONTEND_MAP.md](FRONTEND_MAP.md) |
| Migration step files read feature constants | `persistence::bindings` (`agents`, `plugins::demo`), `persistence::migrate` (`surfaces::add_agent_control`) | permanent and allowed: a step must know the shapes it converts |
| Modules with many responsibilities | `core/runtime.rs`, `lib.rs` | A7, A6 (structural moves, behaviour unchanged) |
| Dead Agent Pack and a second version parser | `agents::pack` | C1 deletes it and uses `semver` |
| A schema 1-3 `config[].secret` setting is kept in plain text in `plugins.json` and handed to the plugin's code | `plugins::external::host` | D4: never handed to a module again; Migrate (write, read back, then remove) or Remove, on the person's choice only |
