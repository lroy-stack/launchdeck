# Marketplace (roadmap)

**Not built.** There is no backend, no account, no download. Two pieces underneath it exist and are described where they are marked: the plugin package format and runtime (the [Plugin SDK](../plugin-sdk/README.md)) and the pack manifest type. This document fixes the shape of the remaining manifests so surfaces and plugins written today remain portable.

## Goals

* Share a **surface** (layout, looks, bindings) without sharing code.
* Share a **plugin** (new actions and state) with explicit permissions.
* Share a **pack**: a curated bundle of surfaces and plugins for one job, such as "Video editing" or "AI coding".
* Everything is inspectable before install. Installing never runs code silently.

## Package types

| Type | Contains | Executable code |
| --- | --- | --- |
| `surface` | One or more surfaces as JSON. | No. Declarative only. |
| `plugin` | A plugin manifest and its WebAssembly module. | Yes. Sandboxed (exists today); permissioned (future). |
| `pack` | References to surfaces, plugins and agent adapters, plus declarative data: state mappings and workflow templates. | Only through the plugins and adapters it lists. |

Surfaces contain no code, but they can contain privileged actions (`shell.execute`, `http.request`). Installing a surface must therefore **show every privileged action** and require confirmation, and imported shell commands are disabled until the user enables each one. The catalog already marks them (`privileged: true`), so a future install check can find them without knowing any provider by name.

A pad stores `{provider, action, args}` and nothing else, which is what makes a surface portable: an install flow can list the providers a surface needs by reading those two ids, compare them with the catalog, and report which ones are missing, exactly as the editor does for a surface opened today (see [actions/availability.md](../actions/availability.md#missing)).

## Manifests

All manifests are JSON, carry `schemaVersion`, and use the same ids as the runtime.

### Surface

```json
{
  "schemaVersion": 1,
  "type": "surface",
  "id": "acme.video-editing",
  "name": "Video editing",
  "version": "1.2.0",
  "description": "Cut, mark and export from the pads.",
  "author": { "name": "Acme", "url": "https://example.com" },
  "license": "MIT",
  "requires": {
    "launchdeck": ">=0.2.0",
    "device": { "grid": { "minWidth": 8, "minHeight": 8 }, "rgb": true },
    "plugins": [{ "id": "system", "version": ">=0.1.0" }]
  },
  "privilegedActions": [
    { "control": "export", "action": "shell.execute", "summary": "ffmpeg -i in.mov out.mp4" }
  ],
  "surfaces": ["surface.json"],
  "checksums": { "surface.json": "sha256:..." },
  "signature": "ed25519:..."
}
```

`requires.device` is matched against device capabilities, so an 8x8 surface can be refused or degraded for a smaller device. `privilegedActions` is generated from the surface file and re-checked on install, so a manifest cannot understate what the surface will do.

### Plugin

**This much exists.** A plugin is a folder with `plugin.json` and one `.wasm` module, discovered, validated, sandboxed and loaded by the Plugin SDK host, and installed by copying the folder. The manifest that exists today is documented in [plugin-sdk/manifest.md](../plugin-sdk/manifest.md):

```json
{
  "schemaVersion": 1,
  "id": "acme.homeassistant",
  "name": "Home Assistant",
  "version": "0.4.1",
  "description": "Turn lights on and off.",
  "author": "Acme",
  "abi": 1,
  "sdk": "0.1.0",
  "main": "homeassistant.wasm",
  "actions": [
    { "id": "ha.callService", "label": "Call service",
      "args": [{ "name": "domain", "type": "string", "required": true }, { "name": "service", "type": "string", "required": true }] }
  ],
  "events": [{ "type": "ha.light.changed", "description": "A light changed. Payload: entity, state." }],
  "config": [{ "key": "token", "type": "string", "secret": true }],
  "capabilities": []
}
```

What a marketplace would add around it, none of it built:

```json
{
  "type": "plugin",
  "permissions": [
    { "name": "network", "hosts": ["homeassistant.local:8123"], "reason": "Talk to your Home Assistant" }
  ],
  "checksums": { "homeassistant.wasm": "sha256:..." },
  "signature": "ed25519:..."
}
```

`permissions` would be a host-mediated version of the manifest's `capabilities` array, which is already in the format and must be empty today, so a plugin that needs one is refused by a host that cannot provide it. `checksums` and `signature` would be verified before a package is stored. The runtime contract is the one in [plugin-sdk/protocol.md](../plugin-sdk/protocol.md): actions in, replies and declared events out, no MIDI, no positions. A plugin that needs network access cannot exist until a mediated `network` capability is built; the sandbox gives it none.

### Pack

A pack can carry five kinds of piece. `adapter`, `plugin` and `surface` are references by id and version range. `stateMapping` and `workflowTemplate` are data. An **Agent Pack** is a pack with agent pieces.

```json
{
  "schemaVersion": 1,
  "type": "pack",
  "id": "acme.release-watch",
  "name": "Release watch",
  "version": "1.0.0",
  "includes": [
    { "type": "adapter", "id": "acme.release-script", "version": "^1.0.0", "transport": "bridge" },
    { "type": "plugin", "id": "acme.release-notes", "version": "^0.3.0" },
    { "type": "surface", "id": "acme.release-board", "version": "^1.0.0" },
    { "type": "stateMapping", "adapter": "acme.release-script",
      "map": { "building": "running", "awaiting-review": "waiting", "shipped": "completed" } },
    { "type": "workflowTemplate", "id": "smoke-tests", "label": "Smoke tests",
      "task": "Run the smoke tests against the release candidate" }
  ]
}
```

**This much exists.** The pack manifest type is implemented with validation and tests in `src-tauri/src/agents/pack.rs`, and `examples/agent-pack/pack.json` is a complete example. Nothing loads, installs, signs or runs a pack, and there is no backend. Validation is strict by design: ids are lowercase and dotted, a manifest cannot name a file path or a command, a state mapping cannot target `approval_required` or `offline`, and a piece cannot appear twice. Workflow templates carry text, never commands. Surfaces and plugins stay as described above; the pack only points at them.

`recommendedDevices` is not part of the manifest yet. Device requirements live on the surface manifest (`requires.device`).

An adapter in a pack declares its transport (`bridge`, `hooks` or `builtin`). Third-party adapters would be out of process and speak the Agent Bridge, which is the isolation model in [agent-control/security.md](../agent-control/security.md#future-permission-isolation-for-adapters).

## Install flow (target)

1. User opens a listing. The client fetches the manifest only.
2. A review screen lists: author, signature status, permissions requested, privileged actions, devices supported.
3. Install downloads artifacts, verifies checksums and signature, and stores them in the app data folder. Nothing runs yet.
4. Plugins start only after permissions are granted. Imported shell and HTTP actions stay disabled until each is enabled.
5. Updates that add permissions or privileged actions need a new review.

## Trust and safety

* Signed packages and a revocation list.
* No install scripts, no post-install hooks.
* Plugin code never receives raw MIDI or pad positions, so a malicious plugin cannot read your keystrokes from the surface or fingerprint your layout.
* The marketplace is optional. A package can be installed from a local file.

## Open questions

* Hosting and moderation model.
* Whether surfaces should be able to declare their own icons as images, and how to size-limit them.
* A portable binding syntax for plugins whose states are dynamic.
