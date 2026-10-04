# Security model

A plugin is third-party code that the person chose to install. The host assumes the plugin binary, its manifest and everything it returns are hostile, and is built so that choice is survivable: a plugin cannot reach anything it was not given, and it cannot take LaunchDeck down.

## Nothing to deny, because nothing is there

The module runs in Wasmtime with **no WASI and no imports except `launchdeck.log`, `launchdeck.publish` and `launchdeck.respond`**. A module that imports anything else does not load. Inside the sandbox there is no filesystem, shell, network, process, clock, random source, environment, MIDI, device or secret store, so there is no permission to ask for and no check to bypass. The only thing a plugin can send out is a log line, a reply, or an event that passed the rules below.

## What is enforced

| Concern | Rule |
| --- | --- |
| CPU | Fuel budget per call (100 million) and a 2 second wall-clock deadline. Exceeding either traps; the plugin stops with status *error*. |
| Memory | One linear memory, 16 MiB. Wasm stack 512 KiB. |
| Size | Manifest 64 KiB, module 8 MiB, request and reply 64 KiB, event 16 KiB, 32 events per call, log lines 1 KiB with a ring of 100. |
| Memory safety at the boundary | Every pointer and length from the module is bounds-checked before the host reads. |
| Events | `source` is always the plugin id (anything the plugin writes is ignored). Types must be declared in the manifest. LaunchDeck prefixes (`control.`, `action.`, `surface.`, `page.`, `device.`, `plugin.`, `agent.`, `runtime.`, `system.`) are forbidden. An event can only feed bindings, which only set a control's visual state; it cannot press a pad or invoke an action. |
| Arguments | Validated against the manifest's `params` before the module sees them. The same declaration draws the form, so a plugin ships no UI code and no HTML. |
| Failure | Traps and limit violations are contained. A failing call becomes `action.failed` with the plugin's own message; a trap or limit takes only that plugin to *error* status. A malformed or oversized reply fails only that call. Events from a call that did not end normally are never delivered. |
| Files and paths | `main` must be a plain file name in the plugin folder. Symlinks are refused. Plugin ids use a strict grammar and cannot collide with built-in ids. |
| Spoofing from outside | The localhost WebSocket refuses events that claim to come from an installed plugin, as it does for `runtime`, `device` and `agent-control`. |
| Settings | Stored in `plugins.json` (atomic write, owner-only on Unix), never in a workspace or surface file. Only keys declared in the manifest are accepted. Secret values are never part of the plugin list, snapshots or the UI, and the host never logs them. A plugin that writes its own secret into its log would show it there, so it should not. |
| Capabilities | The manifest field must be empty. A plugin that asks for one is refused, so a future permission model (for example network access by host name) cannot be silently granted by an old host. |

Shell commands and HTTP requests are still **core actions** that only a person can attach to a pad. A plugin cannot run a command or start a request, and there is no API that takes a string to execute.

## What is not claimed

* This protects LaunchDeck and the person's machine from the plugin. It does not protect the person from a plugin that lies in its name or description, or that shows misleading state on a pad it owns. Install plugins from people you trust.
* There is no signing, checksum verification or revocation yet (see the [marketplace roadmap](../roadmap/marketplace.md)). Anyone who can write to the plugins folder can install code, as with any other file in the app data folder.
* The sandbox is only as strong as Wasmtime. The host pins the LTS line (48) and the patch floor that fixes the advisories known on 2026-10-04 (RUSTSEC-2026-0315 and -0327 need 48.0.4 or newer); keep it updated. `Cargo.toml` documents the rule.
* A plugin cannot read your pads, but it can be slow within its limits: it can use up to 2 seconds of one thread per call. Calls to different plugins run in parallel, and calls to one plugin are serialized.

## Reporting a problem

If you find a way for a module to reach something it should not, treat it as a security bug in the host, not in the plugin.
