# LaunchDeck documentation

Organised by who is reading. Every page describes what is in the code today; anything that has not been run on a system, device or service says **NOT VERIFIED** and why. Where things were run is in the [verification records](verification/README.md).

## Using LaunchDeck

Start here:

* [Install](getting-started/install.md): what exists for 0.3.0, where LaunchDeck keeps its data, starting over.
* [The first run](getting-started/first-run.md): the welcome screen and the starter surfaces.
* [Known limitations](user/known-limitations.md): what 0.3.0 does not do, or has not been seen to do.

The guides:

* [Surfaces and the editor](user/surfaces-and-editor.md): surfaces, pages and pads, Live and Edit, the Inspector.
* [The Action Library](user/action-library.md): finding an action and giving it to a pad.
* [The feedback editor](user/feedback-editor.md): pads that follow a value or react to an event.
* [Auto Context](user/context-engine.md): the surface follows the application in front.
* [Agent Control](user/agent-control.md): AI agents on the pads, with approve and reject.
* [Plugins](user/plugins.md): installing plugins, network access, secrets.
* [Sharing surfaces](user/sharing.md): exporting and importing Surface files and Packs.
* Devices: [Launchpad X](user/launchpad-x.md) and [Launchpad Mini MK3](user/launchpad-mini-mk3.md).

## Writing plugins

The [Plugin SDK](plugin-sdk/README.md): [getting started](plugin-sdk/getting-started.md), the [AssemblyScript SDK](plugin-sdk/assemblyscript.md), the [manifest](plugin-sdk/manifest.md), [states](plugin-sdk/states.md), [capabilities](plugin-sdk/capabilities.md) and the [network](plugin-sdk/network.md), [secrets](plugin-sdk/secrets.md), the [protocol](plugin-sdk/protocol.md), [testing](plugin-sdk/testing.md), [packaging](plugin-sdk/packaging.md), [versioning](plugin-sdk/versioning.md) and the [security model](plugin-sdk/security.md). The [example plugins](../examples/plugins/README.md) are working starting points.

## Connecting agents

For programs that report agents to LaunchDeck: the [Agent Bridge protocol](agent-control/bridge-protocol.md), the [adapter SDK](agent-control/adapter-sdk.md), the [domain model](agent-control/domain-model.md), [how agent state becomes light](agent-control/surfaces.md) and the [security model](agent-control/security.md). The [example client](../examples/generic-agent/README.md) has no dependencies.

## Surface files and Packs

[What the files hold](artifacts/README.md), their [format](artifacts/FORMAT.md) (version 1) and [how importing is kept safe](artifacts/security.md).

## How it is built

* [Architecture overview](architecture/overview.md) and [product](architecture/product.md).
* Where code goes: [AGENTS.md](../AGENTS.md), the [Rust codebase map](architecture/CODEBASE_MAP.md), the [frontend map](architecture/FRONTEND_MAP.md) and the [engineering rules](architecture/ENGINEERING_RULES.md).
* Subsystems: [the surface model](architecture/surface-format.md), [state and feedback](architecture/state-and-feedback.md), [observables](architecture/observables.md), [actions](architecture/actions/overview.md) ([definitions](architecture/actions/definitions.md), [parameters](architecture/actions/parameters.md), [providers](architecture/actions/providers.md), [availability](architecture/actions/availability.md)), [plugins](architecture/plugins.md), [secrets](architecture/secrets.md), [the Context Engine](architecture/context-engine.md) and [versions](architecture/versions.md).
* Devices: [what a device says about itself](architecture/devices/capabilities.md), [the shared Novation MK3 layer](architecture/devices/novation-mk3-protocol.md), the [Launchpad X](architecture/devices/launchpad-x.md) and the [Launchpad Mini MK3](architecture/devices/launchpad-mini-mk3.md).

## Contributing

* [Build from source](getting-started/build-from-source.md) and the quality checks.
* [Testing](contributing/testing.md): the suites, ignored tests, contract fixtures.
* [Testing the Launchpads](contributing/hardware-testing.md), with and without a unit.
* [Manual checks on macOS](contributing/manual-checks-macos.md): what a person checks before a release.
* [Continuous integration](contributing/ci.md) and [releasing](contributing/releasing.md).

## Records and history

* [Verification records](verification/README.md): what was run, where, and what was seen.
* [History](history/README.md): the plans of earlier milestones, kept as written.
* Roadmap, not built: [agent-native LaunchDeck](roadmap/agent-native.md) (the study of 2026-10-23: a native MCP server, one-click connection, orchestration from the Launchpad), [marketplace](roadmap/marketplace.md), [MCP](roadmap/mcp.md) and the earlier [MCP bridge](roadmap/mcp-bridge.md) design.
* The current milestone's plans stay at the root: [PRODUCTIZATION_PLAN.md](../PRODUCTIZATION_PLAN.md) and [LAUNCHPAD_MINI_MK3_PLAN.md](../LAUNCHPAD_MINI_MK3_PLAN.md).
