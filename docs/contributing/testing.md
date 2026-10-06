# Testing

What the test suites cover, how to run each, and the contract fixtures that hold Rust and TypeScript together. The quality gate that runs most of them is `npm run check` ([build from source](../getting-started/build-from-source.md#check-a-change)).

## The suites

| Suite | Where | Run with | What it proves |
| --- | --- | --- | --- |
| Vitest | `src/**/*.test.{ts,tsx}`, `scripts/**/*.test.mjs` | `npm test` | The interface against the contract fixtures and a mocked IPC; the check scripts themselves |
| Rust unit tests | `#[cfg(test)]` modules and `*_tests.rs` files under `src-tauri/src/` | `node scripts/cargo.mjs test --lib` | Each module on its own: the MK3 codec byte for byte, the session against a simulated unit, the Context Engine as a state machine, the artifact format, the plugin sandbox |
| Rust integration tests | `src-tauri/tests/*.rs`, helpers in `tests/support/` | `node scripts/cargo.mjs test --tests` (`--test <name>` for one) | Scenarios through the real `Runtime`, `PluginHost` and `ArtifactService`, without a window |
| Desktop suites | `tools/desktop-e2e/` | `python3 tools/desktop-e2e/run.py [suite]` | The real window through WebDriver, on Linux with X11 only ([its README](../../tools/desktop-e2e/README.md)) |
| Hardware acceptance | `src-tauri/src/devices/novation/hardware_tests.rs` | `sh scripts/launchpad-acceptance.sh` | A physical Launchpad, with a person answering ([hardware testing](hardware-testing.md)) |
| Plugin tests | the plugin's own folder | see [testing a plugin](../plugin-sdk/testing.md) | A plugin against the host's contract |

The integration tests most worth knowing:

| Test | Covers |
| --- | --- |
| `release_acceptance.rs` | The release acceptance scenario, headless, from a clean data folder to a restart (first run, Action Library, feedback rule, Auto Context rule, Counter installed, Pack exported, reset, imported without the plugin, plugin installed again, restart). |
| `artifacts_security.rs`, `artifacts_roundtrip.rs` | Every hostile Surface or Pack file is refused with the workspace unchanged; export then import gives back what was exported. |
| `context_engine.rs` | Auto Context with a scripted observer and the virtual Launchpad. |
| `state_feedback.rs`, `state_hub.rs`, `state_characterization.rs`, `feedback_editor.rs` | The State Hub, feedback rules and the feedback editor's controls on the real runtime. |
| `plugin_sdk.rs`, `plugin_states.rs`, `plugin_network.rs`, `plugin_install.rs`, `home_assistant.rs` | Installed plugins: the sandbox, states, the network broker, installing the examples, the Home Assistant example against a stand-in server. |
| `secret_store.rs`, `legacy_plaintext_secrets.rs` | Secrets with an in-memory store, a refusing store and a garbling store; legacy plaintext values never reach a module. |
| `agent_bridge.rs`, `agent_control_flows.rs`, `agent_control_golden.rs` | The Agent Bridge and the five Agent Control flows. |
| `*_contract.rs` | The contract fixtures (below). |

## Tests that do not run by default

They are `#[ignore]`d and run only by name, with `-- --ignored`:

* `secret_store::the_real_credential_store_keeps_a_secret_out_of_every_plain_file` uses the machine's real credential store. On Linux run it through `sh scripts/test-secret-service.sh`, which starts a private Secret Service and removes it afterwards. On macOS it writes, reads and removes a real Keychain entry: run it knowingly.
* `novation::hardware_tests::launchpad_acceptance` needs a Launchpad and a person; `scripts/launchpad-acceptance.sh` runs it.
* Measurements that check nothing and print timings: `the_save_on_context_activation_is_measured`, the three in `novation/measure_tests.rs`, `p9_routing_cost_does_not_grow_with_the_workspace`. Add `--nocapture` to see what they print.
* `regenerate_export_goldens` rewrites `src-tauri/tests/fixtures/artifacts/valid/`; run it only for an intended change of what export writes, and say so in the commit.

## Contract fixtures

Every payload Rust sends the interface has a fixture in `examples/`: written by a `dump_*` example in `src-tauri/examples/` from the real runtime, guarded by a Rust test that fails when Rust emits something the fixture lacks, and parsed by the Zod schemas in Vitest. The table of fixtures, generators and guards is section 6 of the [codebase map](../architecture/CODEBASE_MAP.md#6-contract-fixtures).

Regenerate a fixture only when what Rust emits changed on purpose, never to make a test pass, and say so in the commit:

```bash
cd src-tauri
cargo run --example dump_catalog > ../examples/action-catalog.json
```

A refactor never changes a fixture ([engineering rules](../architecture/ENGINEERING_RULES.md#regenerating-a-contract-fixture), R18). `default-workspace.json` and `context-status.json` are always written as on Linux, whatever system writes them.

## Running cargo on a small machine

Every npm script runs cargo through `scripts/cargo.mjs`, and `LAUNCHDECK_CARGO` names the cargo binary or a wrapper script it runs instead of `cargo`. A wrapper is the place for what a given machine needs: a fixed toolchain, a shared target directory for several checkouts, or fewer parallel jobs on a machine with little memory. For example:

```bash
LAUNCHDECK_CARGO="$HOME/bin/cargo-small.sh" npm run check
```

where the script exports `CARGO_TARGET_DIR` and `CARGO_BUILD_JOBS=4` and runs `exec cargo +1.97.0 "$@"`. The wrapper must take cargo's arguments unchanged; it is one executable path, not a command line.

Two habits save disk and time:

* `cargo test` with no target also builds every example in `src-tauri/examples/`, and each of them links all of Tauri and Wasmtime. While working, run `cargo test --lib --tests` (with `--test <name>` filters) instead; `npm run check:full` runs everything.
* The `dump_*` examples are needed only to regenerate a fixture. Their binaries under `src-tauri/target/debug/examples/` can be deleted afterwards.

## Known sensitivities

* A few Testing Library tests wait for the interface to settle, with a 5 s limit. On a heavily loaded machine one of them (`src/features/feedback/rules.test.tsx`, "building the Counter pad with the editor alone") has timed out once and passed on the next run. If a Vitest failure does not repeat, run the suite again before looking for a bug, and name the test if it recurs.
* Two cargo processes that share a target directory wait for each other's lock; running heavy cargo commands one at a time is faster on a small machine.
