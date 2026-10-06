# Network

A plugin cannot open a connection. It can ask LaunchDeck to send one HTTP request to an address its manifest asks for and the person has granted (see [Capabilities](capabilities.md)), and it gets the answer back.

```ts
import { action, ok, fail, http, HttpRequest, HttpError, Json, Call, Reply } from "@launchdeck/plugin-sdk/assembly";

function toggle(call: Call): Reply {
  const r = http.request(
    new HttpRequest("POST", base + "/api/services/light/toggle")
      .json(Json.object().setString("entity_id", call.args.getString("entity")))
      .bearer("token"),            // a secret, named and never seen: see Secrets
  );
  if (!r.ok) return fail(r.message, r.error);                  // nothing came back
  if (!r.success) return fail("the server answered " + r.status.toString());
  return ok();
}
```

## The SDK

| | Meaning |
| --- | --- |
| `new HttpRequest(method, url)` | `method` is `GET`, `POST`, `PUT`, `PATCH` or `DELETE`. `url` is absolute, `http` or `https`. |
| `.header(name, value)` | Set a header. Setting one twice keeps the last value. |
| `.json(body)` | A JSON body, with `Content-Type: application/json`. |
| `.text(body, contentType?)` | A text body. `text/plain; charset=utf-8` unless you say otherwise. |
| `.bearer(secret)` | Ask LaunchDeck to add `Authorization: Bearer …` from a [secret](secrets.md) declared in the manifest. |
| `.timeout(ms)` | How long to wait, 1,000 to 30,000. 10,000 when not set. |
| `http.request(r)` | Send it and wait. Returns an `HttpResponse`. **Never traps.** |
| `http.get(url)` | Shorthand for a `GET` with nothing else. |

`HttpResponse`:

| | Meaning |
| --- | --- |
| `ok` | A response arrived, whatever its status. A `404` is `ok`. |
| `success` | A response arrived and its status is 200 to 299. |
| `status`, `body` | The status and the body as text. |
| `header(name)` | A response header by name (any case), or `""`. |
| `json()` | The body as `Json`, or `null` when it is not JSON. |
| `error`, `message` | When `ok` is false: one of the codes below, and a sentence for a person. |

A request is synchronous for the plugin: `http.request` returns when the answer is there. LaunchDeck shows the action as *pending* in the meantime (see [the action lifecycle](#what-the-pad-shows-while-a-request-is-out)).

### Error codes

Compare `r.error` with `HttpError.…`.

| Code | When |
| --- | --- |
| `permission_denied` | The manifest asks for this address and the person has not granted it (or denied it, or revoked it). |
| `host_not_allowed` | The manifest does not ask for this address; or the name resolved to somewhere the grant does not cover (see [reach](#where-an-address-may-lead)). |
| `invalid_request` | Not a URL, a scheme other than `http`/`https`, credentials in the URL, a method that is not allowed, a header LaunchDeck owns, a body on a `GET`. Also: a request made while the plugin starts. |
| `request_too_large` | The body or the headers are over the limit. |
| `response_too_large` | The answer is over the limit. Nothing of it is handed over. |
| `timeout` | No answer in time, or the call used up its time on the network. |
| `network_error` | The connection failed: nothing listening, the name does not resolve, TLS failed. |
| `redirect_not_allowed` | The server redirected to an address that is not granted. Nothing was sent there. |
| `too_many_redirects` | More than 5 redirects. |
| `rate_limited` | Too many requests in one call, or in one minute. |
| `secret_missing` | `.bearer("token")` names a secret the person has not entered. Nothing was sent. |
| `secret_unavailable` | The secret cannot be read: this computer has no secure credential store, or it is locked. Nothing was sent. See [where secrets are kept](../architecture/secrets.md). |
| `invalid_response` | What came back is not HTTP, or not text. |

Pass the code on when you fail an action, `fail(r.message, r.error)`: the person then reads the reason on the pad's failure and on the plugin's card.

## What is checked, in order

Every request, and every redirect of it, goes through the same steps. A failure at any step sends nothing.

1. **The URL is parsed** the way a browser parses it, so `http://2130706433/`, `http://0x7f.1/` and `http://[::ffff:127.0.0.1]/` are compared as the addresses they are. Credentials in the URL (`http://user:pass@host`) are refused.
2. **Its origin is compared** (scheme, host, port; lower-cased, default port left out) with the origins the manifest asks for. Not there: `host_not_allowed`.
3. **The grant is looked up** for exactly that origin. Missing or denied: `permission_denied`.
4. **The name is resolved once, by LaunchDeck**, and every address it resolved to must be inside the origin's reach. The connection is made to those addresses and no others, so the name cannot be pointed somewhere else between the check and the connection.
5. **The request is sent** by LaunchDeck's own client, with the headers it owns, without any proxy.

### Where an address may lead

Each origin has a *reach*, shown in the permission prompt and enforced when the name is resolved:

| Reach | Origins | May resolve to |
| --- | --- | --- |
| This computer | `localhost`, `*.localhost`, `127.0.0.0/8`, `::1` | Loopback addresses only |
| Your local network | Private, link-local and carrier-NAT IPv4, unique-local and link-local IPv6; names ending in `.local`, `.lan`, `.internal`, `.home.arpa`; names without a dot | Loopback and local addresses, never a public one |
| The internet | Every other name, and public addresses | Public addresses only |

So a grant for `https://api.example.com` cannot be turned into access to your router or to a service on this computer by changing what the name resolves to, and reaching something local is always a request the person saw as local. Unspecified, broadcast and multicast addresses are never connected to.

### Redirects

LaunchDeck's client never follows a redirect by itself. For `301`, `302`, `303`, `307` and `308` the new URL goes through steps 1 to 4 again. A redirect to an origin that is not requested and granted ends the request with `redirect_not_allowed`. At most 5 are followed. A redirect to **another** origin, even a granted one, drops the secret and the plugin's `Authorization` and `Cookie` headers. `303`, and `301` or `302` after a `POST`, continue as a `GET` without a body.

### Headers

LaunchDeck sets `Host`, `Content-Length`, `Connection`, `Transfer-Encoding`, `Accept-Encoding`, `User-Agent` and the other headers that frame a request; a plugin that sets one of them, or any `Proxy-…` header, gets `invalid_request`. `Authorization` cannot be set by hand in the same request that uses `.bearer(…)`. A header value with a line break is refused.

## Limits

| Limit | Value |
| --- | --- |
| Request body | 256 KiB, text |
| Response body | 1 MiB after decompression, text (UTF-8) |
| Request headers | 32, and 8 KiB in all |
| Response headers | 16 KiB handed to the plugin |
| Timeout | 10 s per request by default, 1 to 30 s with `.timeout()`; 5 s to connect |
| Redirects | 5 |
| Requests in one call | 24, and 30 s on the network in all |
| Requests per plugin | 600 a minute |
| At the same time | One. Calls to one plugin are serialized, so a plugin has one request out at a time. |

The time a request waits is not counted against the plugin's own 2 seconds of CPU time per call. Binary bodies are not supported in either direction.

## What the pad shows while a request is out

LaunchDeck, not the plugin, owns the life of an action:

```text
pressed ─► pending ─► completed | failed
```

* While its action is pending, a control shows its look named **`pending`**, if it has one. For 1.5 seconds after a failure it shows its look named **`failed`**, if it has one. Both end by themselves.
* The runtime also publishes which actions are pending, so the desktop app marks the pad as busy whether or not it has a `pending` look.
* A finished action leaves nothing on the pad. What the pad shows afterwards comes from its state rules, which read what the plugin **reports**. A `200` does not turn a pad green; `setState("entity", "on", …)` after reading the real value does.
* Every pending action ends: by the answer, the timeout, the call's network budget, or the plugin stopping.

So a plugin for an outside system does two separate things: it performs the action and returns `ok()` or `fail(…)`, and it reports the state it read back.

## Keeping states current: `refresh`

A plugin runs only when it is called. To show a state that changes elsewhere (a light switched at the wall), declare:

```json
"refresh": { "intervalSeconds": 5 }
```

and register a handler:

```ts
import { onRefresh, Watched, Reply, ok } from "@launchdeck/plugin-sdk/assembly";

onRefresh((watching: Watched[]): Reply => {
  for (let i = 0; i < watching.length; i++) {
    // watching[i].state is a state id of this plugin, watching[i].args its arguments
    report(watching[i].args.getString("entity"));
  }
  return ok();
});
```

`watching` lists the instances of this plugin's states that **saved controls read**: nothing is asked for that no pad shows. LaunchDeck calls the handler when the plugin starts, when what is watched changes, when a grant, a setting or a secret changes, and every `intervalSeconds` (2 to 3,600). A refresh names at most 16 instances; when more are shown, each refresh takes the next 16 in turn. With nothing watched the plugin is not called.

A refresh runs like an action, in the same sandbox and through the same checks. Its result goes to the plugin's log and never to a pad. If the outside system cannot be reached, clear the state (`clearState`) so the pad reads *unknown* and not the last value it happened to hold.

This is polling. A change made elsewhere shows after at most one interval. A subscription (WebSocket, server-sent events) needs a connection that stays open, which the sandbox does not offer.

## Not available

* **During `init`.** The network is closed while a plugin starts. Make requests from an action or from `refresh`.
* **Without the capability.** The two host functions exist only for a plugin whose manifest declares `capabilities.network`. Any other module that imports them does not load.
* A system or environment proxy. Requests always go directly, because the checks are about where the connection goes. Behind a proxy-only network, requests to the internet fail with `network_error`.
* A name that resolves to both a public and a private address (split-horizon DNS): it is refused, because one of the two is outside the grant.

## The wire format (for other languages)

Two imports in the module `launchdeck`. The protocol is still ABI 1.

| Import | Signature | Meaning |
| --- | --- | --- |
| `http_request` | `(ptr: i32, len: i32) -> i32` | Run the request written as JSON at `ptr`. Returns the size in bytes of the answer, which the host keeps. |
| `http_response` | `(ptr: i32, cap: i32) -> i32` | Copy that answer to `ptr`. Returns its size, or `-1` when there is none or `cap` is too small. |

```json
{"method":"POST","url":"https://api.example.com/v1/things","headers":{"content-type":"application/json"},"body":"{}","auth":{"secret":"token","scheme":"bearer"},"timeoutMs":5000}
```

```json
{"ok":true,"status":200,"headers":{"content-type":"application/json"},"body":"{\"done\":true}"}
{"ok":false,"error":"permission_denied","message":"access to https://api.example.com is waiting for approval in Plugins"}
```

Only `method` and `url` are required. Response header names are lower-case. The `refresh` request is `{"op":"refresh","watching":[{"state":"entity","args":{"entity":"light.kitchen"}}]}` and is answered like `invoke`.
