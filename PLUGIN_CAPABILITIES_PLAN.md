# Plugin Capabilities v1: network

Goal: an installed plugin can talk to destinations a person approved, through LaunchDeck, and to nothing else. Proved with a small Home Assistant plugin that uses only the public SDK.

## 1. What the code is today

| Area | Finding | Consequence |
| --- | --- | --- |
| Sandbox (`plugins/external/sandbox.rs`) | Wasmtime without WASI. Three imports are linked (`log`, `publish`, `respond`); a module that imports anything else does not load. Per call: fuel, a 2 s epoch deadline, 16 MiB of memory. | A plugin has no way to reach the network. Access can only be added as a new import, which is the broker. |
| Manifest (`manifest.rs`) | Schema 3. `capabilities` is an array that must be empty ("fail closed"). | The place for the declaration already exists and is refused today. |
| Settings (`host.rs`) | `config` fields in `plugins.json` (owner-only), typed, edited in the Plugins screen. A field marked `secret` is hidden from the UI but **is handed to the module** at `init` and stored in clear in `plugins.json`. | Settings are reusable as they are. The old `secret` flag is not a safe home for a token. |
| Actions (`actions/mod.rs`, `core/runtime.rs`) | `Plugin::invoke` is synchronous and runs on a blocking thread. The runtime publishes `action.started` and `action.completed` / `action.failed` around it, and shows `running` / `success` / `error` looks, but only on controls without feedback rules. Nothing records which actions are in flight. | The lifecycle exists as events; "pending" is not a state anyone can read, and a control with rules shows nothing while it waits. |
| States | A plugin writes states during an action. Nothing calls a plugin except `init` and `invoke`, and states written in `init` are dropped. | A plugin cannot learn or report anything until a pad is pressed. An external system needs a way to be asked. |
| Secrets | None. `write_private` (0600, atomic) is the only protection in use. No keychain dependency. | New. |
| Audit | `agents/audit.rs`: `audit.jsonl`, one JSON line per entry, rotated, owner-only. | Reused for permission events. |
| HTTP | `ureq` 3.4 (rustls, blocking) is already a dependency. It lets the caller supply the resolver. | The broker is built on it; no new HTTP stack. |

Home Assistant (developers.home-assistant.io/docs/api/rest, page dated 19 Aug 2026): `Authorization: Bearer <long-lived access token>`; `GET /api/states/<entity_id>` returns `{"entity_id", "state", "attributes", ...}` (404 when unknown); `POST /api/services/<domain>/<service>` with `{"entity_id": ...}`; errors are 400, 401, 404, 405. Live changes need the WebSocket API (a persistent connection with an auth handshake and subscriptions).

## 2. Capability model

A capability is something the sandbox does not have and the host can do on a plugin's behalf. Three things are kept apart:

| | Where | Who writes it |
| --- | --- | --- |
| **Request** | `plugin.json` (`capabilities`) | The plugin author |
| **Grant** | `permissions.json` in the app data folder | The person, in the Plugins screen |
| **Use** | A host import, checked on every call | The plugin, at run time |

`capabilities` becomes an object in manifest schema 4. Only `network` exists; any other key is refused, as any non-empty array still is in schemas 1 to 3.

```jsonc
"capabilities": {
  "network": {
    "purpose": "Connect to your Home Assistant instance.",
    "hosts": ["https://api.example.com", { "setting": "baseUrl" }]
  }
}
```

A host is an **origin**: scheme, host and port, nothing else. Either written out, or `{ "setting": key }`, which means "the origin of the URL in this setting", for a server only the person knows. No wildcards, no paths, at most 16.

The code is a `capabilities` module with the request (`NetworkRequest`), the decisions (`Grants`) and one question, `covers(plugin, origin)`. It is generic in the sense that a second capability is a second variant and a second map; nothing is built for capabilities that do not exist.

## 3. Permission lifecycle

```text
installed / enabled / setting changed ─► requested origins (from the manifest and settings)
        each origin:  needs approval ──Allow──► granted ──Revoke──► needs approval
                            └────Deny────► denied  ──Allow──► granted
```

* **Denied by default.** An origin is usable only when the current manifest requests it **and** a grant for exactly that origin exists.
* **Never widened.** A new version that asks for another origin gets `needs approval` for that origin; the grants it had stay. A grant for an origin the plugin no longer requests stays on file, is shown, and covers nothing.
* A plugin runs whatever its grants are. What changes is whether its requests go out.
* `permissions.json` is outside the workspace and outside plugin folders, so grants do not travel with an exported surface or with a plugin.
* Audit (`audit.jsonl`): `permission.requested` (first time an origin is asked for), `permission.granted`, `permission.denied`, `permission.revoked`, with the plugin id and the origin. Nothing else.

## 4. Network request API

Two imports, linked only for a plugin whose manifest declares `network`. The protocol stays ABI 1: a module that does not import them is unaffected.

```text
launchdeck.http_request(ptr, len) -> i32    run one request; returns the size of the answer
launchdeck.http_response(ptr, cap) -> i32   copy that answer into the module's buffer
```

Request: `{ "method", "url", "headers"?, "body"?, "auth"?: { "secret": key, "scheme": "bearer" }, "timeoutMs"? }`.
Answer: `{ "ok": true, "status", "headers", "body" }` or `{ "ok": false, "error": code, "message" }`.

The call is synchronous for the module (AssemblyScript has no `await`); it runs on the blocking thread every plugin call already uses. Methods: `GET`, `POST`, `PUT`, `PATCH`, `DELETE`. Bodies are text.

Error codes: `permission_denied` (requested, not granted), `host_not_allowed` (not requested, or the name resolved somewhere the grant does not cover), `invalid_request`, `request_too_large`, `response_too_large`, `timeout`, `network_error`, `redirect_not_allowed`, `too_many_redirects`, `rate_limited`, `secret_missing`, `invalid_response`.

## 5. Host allowlist and validation

* URLs are parsed by the `url` crate (WHATWG), so `http://2130706433/`, `http://0x7f.1/` and `http://[::ffff:127.0.0.1]/` are compared as what they are. Refused: any scheme but `http` and `https`, credentials in the URL, an empty host, a trailing-dot host.
* Comparison is on the normalized origin: lower-case host, IDN as punycode, default port left out. `http://ha.local` and `https://ha.local` are different origins, as are different ports.
* Every origin has a **reach**, shown in the prompt and enforced when the name is resolved:

  | Reach | Origin | May resolve to |
  | --- | --- | --- |
  | This computer | `localhost`, `*.localhost`, loopback addresses | loopback only |
  | Your local network | private, link-local, carrier-NAT and unique-local addresses; names ending in `.local`, `.lan`, `.internal`, `.home.arpa`; names without a dot | loopback, private, link-local, unique-local |
  | The internet | every other name, public addresses | public addresses only |

  So a grant for an internet name cannot be turned into access to the LAN or to this computer by changing DNS (rebinding), and local access is always something the person saw and approved.
* The broker resolves the name once, checks **every** address it got, and connects only to those (its own `Resolver`). Unspecified, broadcast and multicast addresses are never allowed.
* No proxy is used, including one set in the environment: the checks are about where the connection goes.

## 6. Redirects

Never followed by the HTTP library. For each `301, 302, 303, 307, 308` the broker resolves `Location`, and the new URL goes through the same checks as a first request (requested, granted, reach). A redirect to an origin that is not granted ends the request with `redirect_not_allowed` and nothing is sent there. At most 5 hops. A hop to another origin drops the secret and the plugin's `Authorization` and `Cookie` headers. `303`, and `301`/`302` after a `POST`, continue as `GET` without a body.

## 7. Limits

| Limit | Value |
| --- | --- |
| Request body | 256 KiB |
| Response body | 1 MiB (after decompression) |
| Request headers | 32, 8 KiB in all |
| Response headers | 64 KiB read, 16 KiB handed to the plugin |
| Timeout | 10 s per request by default, 1 to 30 s on request; 5 s to connect |
| Redirects | 5 |
| Requests in one plugin call | 24, and 30 s of network time |
| Requests per plugin | 600 a minute |
| Concurrency | One call at a time per plugin (one instance), so one request in flight per plugin |

Headers a plugin may not set: `Host`, `Content-Length`, `Connection`, `Transfer-Encoding`, `Upgrade`, `TE`, `Trailer`, `Keep-Alive`, `Expect`, `Proxy-*`, `Accept-Encoding`, `User-Agent`; and `Authorization` when `auth` is used. The time a request takes is not counted against the module's CPU deadline.

## 8. Asynchronous actions

The runtime owns the lifecycle; nothing is simulated in the interface.

```text
invoked ─► pending ─► completed | failed
```

* The runtime keeps the set of actions in flight and publishes it in its state (`pending`: control, action, since). `action.started` / `action.completed` / `action.failed` are unchanged.
* While its action is pending a control shows its look named `pending`, and for 1.5 s after a failure its look named `failed`, when it has them. This also works on a control with feedback rules. Controls without rules keep `running` / `success` / `error`.
* A finished action leaves no look behind. What the pad shows afterwards is what its state rules say, from the state the plugin reports.
* Every pending action ends: the module's CPU budget, the per-request timeout and the per-call network budget bound it, and a plugin that stops mid-call fails the action.

## 9. Keeping states current: `refresh`

A plugin for an outside system has to be asked. Schema 4 adds `"refresh": { "intervalSeconds": n }` (2 to 3600). The host then sends `{"op":"refresh","watching":[{"state","args"}...]}`: the instances of the plugin's states that controls in the workspace read. It is sent when the plugin starts, when what is watched changes, when a grant, a setting or a secret changes, and every interval. One refresh names at most 16 instances; when more are shown, each refresh takes the next 16 in turn. Only plugins that declare it receive it. It runs like an action (same sandbox, same broker), reports to the plugin's log, and never to a pad.

## 10. Secrets

Three scopes, never mixed: a **setting** (the server's URL; `plugins.json`; given to the module), a **secret** (the token; the credential store; never given to the module), an **action argument** (the entity; the surface).

* Schema 4 adds `"secrets": [{ "key", "label", "description", "host" }]`. `host` is one of the plugin's network hosts: the only origin the secret is ever sent to.
* The plugin names a secret (`auth: { secret: "token", scheme: "bearer" }`); the broker adds `Authorization: Bearer …` when the request goes to that origin with a grant. The value never enters WebAssembly memory.
* `SecretStore` (get, set, delete) has three implementations: the operating system's credential store through `keyring` (Keychain, Credential Manager, Secret Service); a private file (`secrets.json`, 0600) when the system has no credential store, said plainly in the UI; and an in-memory one for tests.
* A schema 4 manifest may not mark a `config` field `secret`. Older manifests keep working as before.
* Not written anywhere: the value in logs, events, snapshots, the workspace or the audit trail. Logs name the method, the origin and the path, never the query, the headers or the bodies.

## 11. Home Assistant plugin

`examples/plugins/home-assistant`, AssemblyScript, public SDK only. Setting `baseUrl`; secret `token` bound to it; actions `turn_on`, `turn_off`, `toggle` with an `entity` argument (`light.`, `switch.`, `input_boolean.`); state `entity` per entity with `on`, `off`, `unavailable`.

REST, not WebSocket: an action posts to `/api/services/<domain>/<service>` and then reads `/api/states/<entity>`; `refresh` reads each watched entity. A change made elsewhere shows after at most one interval (5 s). A subscription would be immediate and needs a persistent connection the sandbox does not have; that is a later capability. When Home Assistant cannot be reached the plugin clears the state, so the pad reads `unknown` and not the last value.

Nothing in LaunchDeck knows the plugin's id.

## 12. Versions

| | Change |
| --- | --- |
| Manifest | Schema 4 (`capabilities` object, `secrets`, `refresh`, `label` on settings). Schemas 1 to 3 load unchanged. |
| Protocol | ABI 1, unchanged: two optional imports, one optional request. |
| SDK | 0.2.0: `http`, `onRefresh`. |
| Workspace | Schema 4, unchanged. |
| New files | `permissions.json`, and `secrets.json` only without a credential store. |

## 13. Tests

* **Unit**: origin parsing and normalization, reach, the grant table, header rules, the secret stores.
* **Broker against a local server** (the real broker, a real socket): allowed, denied, undeclared, not granted, wrong scheme, explicit port, redirect to an allowed and to a forbidden origin, timeout, response too large, request too large, malformed response, rate and per-call limits, concurrency.
* **Through the sandbox**: a probe plugin built with the SDK, and the real Home Assistant plugin, against a mock Home Assistant that implements the three endpoints used. The broker is never mocked.
* **End to end** (`tests/home_assistant.rs`): enable, permission requested, grant, URL and secret, state `off`, pad shows Off, press Toggle, pending, the call reaches the mock, state `on`, pad shows On; revoke, the action fails and nothing reaches the mock; grant again, it recovers. Plus permission expansion after an update, and a plugin stopped mid-request.
* **Secrets**: not in the workspace, the manifest, `plugins.json`, logs or snapshots; missing and deleted secrets fail cleanly.
* **Desktop** (`tools/desktop-e2e`, suite `home_assistant`): the prompt, deny and allow, settings, setting and removing the secret, the pad following the mock, revoke.

## 14. Order of work

1. Manifest schema 4 and the capability model with its storage and audit.
2. The broker, alone, with its tests.
3. Secrets.
4. The two imports, `refresh`, the pending set and looks.
5. SDK 0.2.0, the probe plugin, the Home Assistant plugin, the mock.
6. Commands and the Plugins screen.
7. End-to-end tests, desktop suite, documentation.
