# Home Assistant (example plugin)

Turns a light or a switch on and off from a pad, and shows on the pad whether it is on. It talks to **your own** Home Assistant over its REST API and to nothing else.

It exists to prove the network capability with something real, in both directions: a press reaches Home Assistant, and what Home Assistant says reaches the pad. It uses only the public SDK. Nothing in LaunchDeck knows this plugin exists; a test fails if its id or its API paths appear in the core.

| | |
| --- | --- |
| Id | `example.home-assistant` |
| Actions | `toggle`, `turn_on`, `turn_off`, each with one argument: **Entity** (`light.kitchen`) |
| State | **Entity state**, one instance per entity: `on`, `off`, `unavailable` |
| Setting | **Home Assistant address** (`http://homeassistant.local:8123` until you change it) |
| Secret | **Long-lived access token** |
| Network | One address: the origin of the setting above |
| Refresh | Every 5 seconds |

Entities it switches: `light.…`, `switch.…` and `input_boolean.…`.

## Set it up

1. Start LaunchDeck with the example plugins, or copy this folder (`plugin.json` and `home-assistant.wasm`) into your plugins folder and press **Rescan**:

   ```bash
   LAUNCHDECK_PLUGINS="$PWD/examples/plugins" npm run tauri dev
   ```

2. Open **Plugins** and find **Home Assistant**. Under **Settings**, type the address as you would in a browser, for example `http://192.168.1.20:8123`, and press Enter.
3. Under **Network access**, read the address and press **Allow**. Until you do, nothing is sent.
4. In Home Assistant, open your profile, **Security**, **Long-lived access tokens**, and create one. Paste it under **Secrets** and press **Save**. LaunchDeck keeps it; the plugin never sees it.
5. Open a surface, switch to **Edit**, pick a pad, and assign **Toggle** from the Action Library. Type the entity id (`light.kitchen`).
6. Under **Feedback**, add feedback from **Entity state** for the same entity: one rule for *On*, one for *Off*, each with a look. Optionally add looks named **Pending** and **Failed**; the pad shows them while a press is on its way and when it fails.
7. Save, switch to **Live**, and press the pad.

Until the pad is saved, the editor reads the entity as *Unknown*: Home Assistant is only asked about entities that a saved pad shows.

## What happens on a press

```text
press
  └─ LaunchDeck marks the action pending               (the pad shows its Pending look)
       └─ POST /api/services/light/toggle  {"entity_id": "light.kitchen"}
       └─ GET  /api/states/light.kitchen   → {"state": "on", …}
       └─ the plugin reports  entity{light.kitchen} = on
  └─ the action completes                              (the Pending look ends)
  └─ the pad's state rule for On applies               (the pad shows its On look)
```

The pad turns to *On* because Home Assistant said the light is on, not because the request returned `200`. If the command works and the state cannot be read, the action fails and says so.

Independently of presses, every 5 seconds LaunchDeck asks the plugin for the entities that saved pads show, and the plugin reads each one. A light switched at the wall therefore shows on the pad within about 5 seconds.

## When something is wrong

| What you see | Why |
| --- | --- |
| The pad shows its resting look, not On or Off | Home Assistant has not answered: the address is not granted, the token is missing, or the server cannot be reached. The plugin clears the state instead of showing the last value. |
| A press shows the Failed look; the card says `(permission_denied)` | The address is waiting for **Allow**, or was denied or revoked. |
| `(secret_missing)` | No token has been entered. |
| *Home Assistant refused the access token* | The token is wrong or was deleted in Home Assistant (`401`). |
| *Home Assistant does not know that entity or service* | A typo in the entity id (`404`). |
| `(network_error)` or `(timeout)` | Nothing answers at that address. |
| `(host_not_allowed)` with a name that "resolved to" another kind of address | For example `homeassistant.local` resolving to a public address. Use the IP address. |

**Show log** on the plugin's card lists each request (method, address and path only) and each refusal.

## Limits

* **Polling, not live.** A change made elsewhere takes up to 5 seconds to show. Home Assistant's WebSocket API would be immediate; it needs a connection that stays open, which the sandbox does not offer.
* **On and off only.** No brightness, colour, scenes, covers or climate; no list of your entities to pick from (you type the id).
* **16 entities per refresh.** With more than 16 shown, each refresh reads the next 16, so each one is read less often.
* **One Home Assistant.** One address and one token.
* **Checked against a stand-in.** The automated tests and the desktop run use a small server that implements the three endpoints above as the Home Assistant documentation describes them (REST API page dated 19 Aug 2026). The plugin has not been run against a real Home Assistant installation in this repository.

## The API it uses

From the [Home Assistant REST API](https://developers.home-assistant.io/docs/api/rest):

| Request | Used for |
| --- | --- |
| `POST /api/services/<domain>/<toggle\|turn_on\|turn_off>` with `{"entity_id": "…"}` | The three actions |
| `GET /api/states/<entity_id>` | Reading the state after an action and on every refresh |

Every request carries `Authorization: Bearer <token>`, added by LaunchDeck.

## Building

```bash
sh scripts/build-example-plugins.sh home-assistant
```

The source is `assembly/index.ts` (about 130 lines). The built `home-assistant.wasm` is committed.
