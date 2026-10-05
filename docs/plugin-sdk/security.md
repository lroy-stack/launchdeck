# Security model

A plugin is third-party code that the person chose to install. The host assumes the plugin binary, its manifest and everything it returns are hostile, and is built so that choice is survivable: a plugin cannot reach anything it was not given, and it cannot take LaunchDeck down.

## Nothing to deny, because nothing is there

The module runs in Wasmtime with **no WASI and no imports except `launchdeck.log`, `launchdeck.publish` and `launchdeck.respond`**. A module that imports anything else does not load. Inside the sandbox there is no filesystem, shell, network, process, clock, random source, environment, MIDI, device or secret store. The only thing a plugin can send out by itself is a log line, a reply, or an event that passed the rules below.

## One door, held by the host: network

A plugin whose manifest declares `capabilities.network` gets two more imports, `http_request` and `http_response`. They are not a socket. The module writes a description of one HTTP request; LaunchDeck decides whether to send it, sends it with its own client, and returns the answer. A module that imports them without declaring the capability does not load.

| Concern | Rule |
| --- | --- |
| Default | Denied. A request goes out only if the current manifest asks for that exact origin (scheme, host, port) **and** the person granted it. Asking is not a grant; an update that asks for more gets nothing new. |
| Parsing | URLs are parsed as a browser would (WHATWG), so decimal, hex and IPv4-mapped spellings of an address are compared as that address. Credentials in a URL, schemes other than `http` and `https`, wildcards and trailing-dot hosts are refused. |
| Local and loopback | Each origin has a reach (this computer, the local network, the internet), shown in the prompt. A name is resolved once by the host, every address it resolved to must be inside that reach, and the connection goes only to those addresses. An internet grant can therefore not be steered to the LAN or to localhost through DNS, and local access is always something the person approved as local. |
| Redirects | Never followed by the HTTP library. Each hop is checked like a first request; a hop to an origin that is not granted ends the request and nothing is sent there. At most 5. Credentials are dropped when the origin changes. |
| Headers | `Host`, `Content-Length`, `Connection`, `Transfer-Encoding`, `Upgrade`, `Accept-Encoding`, `User-Agent` and `Proxy-…` belong to the host. A value with a control character is refused. |
| Size and time | Request body 256 KiB, response 1 MiB after decompression, 10 s per request (30 s at most), 24 requests and 30 s of network per call, 600 requests a minute per plugin, one request in flight per plugin. |
| Proxies | None is used, including one from the environment, because the checks are about where the connection goes. |
| Secrets | Kept by the host in the system's credential store and nowhere else: without one, secrets are unavailable and a request that needs one is not sent. Bound to one origin in the manifest, added by the host as `Authorization: Bearer`, and never copied into the module's memory. See [secrets](secrets.md) and [where secrets are kept](../security/secrets.md). |
| While starting | The network is closed during `init`. |
| Audit | `permission.requested`, `.granted`, `.denied` and `.revoked` are written to `audit.jsonl` with the plugin and the origin. |
| Logs | A request is logged as its method, origin and path. Never the query, the headers, a body or a secret. |

Details and the exact order of the checks: [network](network.md). The person's view: [plugin permissions](../plugins/permissions.md).

## What is enforced

| Concern | Rule |
| --- | --- |
| CPU | Fuel budget per call (100 million) and a 2 second wall-clock deadline. Exceeding either traps; the plugin stops with status *error*. |
| Memory | One linear memory, 16 MiB. Wasm stack 512 KiB. |
| Size | Manifest 64 KiB, module 8 MiB, request and reply 64 KiB, event 16 KiB, 32 events per call, log lines 1 KiB with a ring of 100. |
| Memory safety at the boundary | Every pointer and length from the module is bounds-checked before the host reads. |
| Events | `source` is always the plugin id (anything the plugin writes is ignored). Types must be declared in the manifest. LaunchDeck prefixes (`control.`, `action.`, `surface.`, `page.`, `device.`, `plugin.`, `agent.`, `runtime.`, `system.`) are forbidden. An event can only feed feedback rules, which only choose which of a control's looks to show; it cannot press a pad or invoke an action. |
| States | A module writes only the states its own manifest declares. The provider is never part of the message, so there is no way to write as another plugin or as Agent Control. The state, its arguments and its value are checked against the declaration (type, listed enum values, argument names) before they reach the hub, a state message is at most 2 KiB, one call writes at most 64 instances, and a plugin holds at most 64 states and 1,024 instances. States from a call that did not end normally are never delivered, and nothing is delivered after the plugin stopped. A state is data: it only feeds feedback rules and the catalog, and cannot press a pad or invoke an action. |
| Arguments | Validated against the manifest's `params` before the module sees them. The same declaration draws the form, so a plugin ships no UI code and no HTML. |
| Failure | Traps and limit violations are contained. A failing call becomes `action.failed` with the plugin's own message; a trap or limit takes only that plugin to *error* status. A malformed or oversized reply fails only that call. Events from a call that did not end normally are never delivered. |
| Files and paths | `main` must be a plain file name in the plugin folder. Symlinks are refused. Plugin ids use a strict grammar and cannot collide with built-in ids. |
| Spoofing from outside | The localhost WebSocket refuses events that claim to come from an installed plugin, as it does for `runtime`, `device` and `agent-control`. |
| Settings | Stored in `plugins.json` (atomic write, owner-only on Unix), never in a workspace or surface file. Only keys declared in the manifest are accepted. |
| The old `secret` setting flag | In manifests of schemas 1 to 3, a setting marked `"secret": true` is **never handed to the module**: it reads as not set, and it cannot be given a value. A value an earlier version kept in clear in `plugins.json` is never used, is listed by name on the plugin's card as a *Legacy insecure secret*, and is moved into the credential store or removed only on the person's choice. Schema 4 refuses the flag; use [`secrets`](secrets.md), which the module never receives. See [secrets of older plugins](../security/secrets.md#secrets-of-older-plugins). |
| Capabilities | Only `network` exists. A manifest that asks for anything else, in the array of schemas 1 to 3 or as an unknown key in schema 4, is refused, so a host that cannot provide a capability never runs a plugin that needs it. Grants live in `permissions.json`, outside the workspace and outside plugin folders. |

Shell commands are still a **core action** that only a person can attach to a pad, and so is the built-in *HTTP request* action. A plugin cannot run a command, and the only requests it can cause are the ones described above. There is no API that takes a string to execute.

## What is not claimed

* This protects LaunchDeck and the person's machine from the plugin. It does not protect the person from a plugin that lies in its name or description, or that shows misleading state on a pad it owns. Install plugins from people you trust.
* There is no signing, checksum verification or revocation yet (see the [marketplace roadmap](../roadmap/marketplace.md)). Anyone who can write to the plugins folder can install code, as with any other file in the app data folder.
* The sandbox is only as strong as Wasmtime. The host pins the LTS line (48) and the patch floor that fixes the advisories known on 2026-10-04 (RUSTSEC-2026-0315 and -0327 need 48.0.4 or newer); keep it updated. `Cargo.toml` documents the rule.
* A plugin cannot read your pads, but it can be slow within its limits: it can use up to 2 seconds of one thread per call, plus up to 30 seconds waiting on the network if it has that capability. Calls to different plugins run in parallel, and calls to one plugin are serialized.
* A granted address is trusted with what the plugin knows: its settings and the arguments of the actions you press. The grant limits *where* data can go, not *what* the plugin sends there.
* A secret is safe from the plugin, not from the server it is sent to. It is as safe at rest as the system's credential store makes it: another program running as you can usually ask that store too.
* DNS answers are checked, not authenticated. For an `http` origin, someone who controls the network path can still read or change the traffic; prefer `https` where the server offers it.

## Reporting a problem

If you find a way for a module to reach something it should not, treat it as a security bug in the host, not in the plugin.
