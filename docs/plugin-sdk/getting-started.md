# Getting started

You will build the Hello plugin that ships in `examples/plugins/hello`, install it, and press a pad that triggers it. You need Node 20 or newer to build. Nothing else.

## 1. Look at the source

```ts
import { action, onInit, ok, publish, info, Json, Call, Reply } from "@launchdeck/plugin-sdk/assembly";
export * from "@launchdeck/plugin-sdk/assembly/abi";   // required: exposes the ABI

let greeting = "Hello";

function init(config: Json): Reply {
  greeting = config.getString("greeting", "Hello");
  return ok();
}

function say(call: Call): Reply {
  const name = call.args.getString("name", "world");
  const message = greeting + ", " + name + "!";
  info("greeting " + name);
  publish("hello.completed", Json.object().setString("name", name).setString("message", message));
  return ok(Json.object().setString("message", message));
}

onInit(init);
action("hello.say", say);
```

and its manifest, `plugin.json`:

```json
{
  "schemaVersion": 2,
  "id": "example.hello",
  "name": "Hello Plugin",
  "version": "0.1.0",
  "description": "Says hello, tells the deck about it, and shows the smallest complete plugin.",
  "author": "LaunchDeck examples",
  "abi": 1,
  "sdk": "0.1.0",
  "main": "hello.wasm",
  "actions": [
    { "id": "hello.say", "label": "Say hello",
      "description": "Greets someone, publishes hello.completed and returns the greeting.",
      "category": "Greetings", "keywords": ["greet", "welcome", "hi"],
      "params": [{ "name": "name", "label": "Who to greet", "type": "string", "placeholder": "World" }] }
  ],
  "events": [{ "type": "hello.completed", "description": "A greeting was produced. Payload: name, message." }],
  "config": [{ "key": "greeting", "type": "string", "default": "Hello", "description": "The word that starts every greeting." }],
  "capabilities": []
}
```

Every action the module registers must be in `actions`, and every event it publishes must be in `events`. The host checks the manifest, not the module.

## 2. Build

```sh
cd examples/plugins/hello
npm install
npm run build        # writes hello.wasm (about 20 KB)
```

The build is `asc --target release` with the options in `asconfig.json`. Prebuilt `.wasm` files are committed for the examples so you can skip this step. `npm run plugins:build` at the repository root rebuilds all three.

## 3. Install

A plugin is its folder. Copy it into the user plugins folder, one folder per plugin:

| System | Folder |
| --- | --- |
| macOS | `~/Library/Application Support/dev.launchdeck.app/plugins/` |
| Windows | `%APPDATA%\dev.launchdeck.app\plugins\` |
| Linux | `~/.local/share/dev.launchdeck.app/plugins/` |

Create `plugins` if it does not exist. Only `plugin.json` and the `.wasm` file are needed, so `node_modules` and the sources can stay behind.

While developing, point LaunchDeck at your working folders instead of copying:

```sh
LAUNCHDECK_PLUGINS=/path/to/examples/plugins npm run tauri dev
```

The variable is a list of folders (`:` separated, `;` on Windows). A folder in it holds plugin folders, the same layout as the user plugins folder. If two folders carry the same plugin id, the first one wins and the other is listed as skipped.

## 4. Run

Open **Plugins** in the sidebar. Hello shows as *installed* and *on*, with its action, its event and its `greeting` setting. If something is wrong the card says *error* with the reason in plain words, and a skipped folder shows under **Installed plugins**. Press **Rescan** after copying a plugin in.

To use it, switch to **Edit** mode, select a pad, type `hello` in the Action Library's search and assign **Say hello** (listed as *Plugin: Hello Plugin*), by pressing **Assign** or dragging the row onto the pad. Type a name into the **Who to greet** field the manifest declared, and press **Test run** to run it right away and see the result. Press **Save** to keep it. You never type an id or any JSON: the library and the form come from `plugin.json`.

To make the pad react to what the plugin publishes, add a feedback rule to that control in the surface definition (`workspace.json` in the app data folder, the same place the built-in surfaces keep theirs; the inspector shows rules read-only):

```json
"feedback": [
  { "when": { "event": { "provider": "example.hello", "id": "hello.completed" } },
    "show": "greeted", "hold": { "ms": 3000 } }
],
"states": { "greeted": { "color": "#1edc5a", "animation": "none" } }
```

In Live mode, pressing the pad runs the plugin, the plugin publishes `hello.completed`, the rule shows the look `greeted` for three seconds, and the pad changes on the simulator and on a Launchpad, then returns to its resting look. To take the look from the event itself, name a payload field: `"showFromField": "level"` with `"hold": "latch"` (the Counter example does this: it publishes a level and the surface owns one look per level, kept until the next event). A pad with no feedback rules still gets the automatic running, success or error look from the action result. Installed plugins publish events for now; a plugin that reports a *state* (so that a pad created later is right at once) arrives with the next manifest schema.

## 5. Make your own

1. Copy `examples/plugins/hello` and change `id` (lowercase, dotted, at least two parts, for example `acme.notes`), `name`, `author` and the files' contents.
2. In `asconfig.json` change `outFile`, and `main` in `plugin.json` to match.
3. Register one function per action with `action("your.id", fn)` and declare the same ids in `plugin.json`, with a `label`, a `description`, a `category` and the `params` the person should fill in. They are what the Action Library shows.
4. Build, copy the folder, press **Rescan**.

Not using AssemblyScript? Read the [protocol reference](protocol.md). The C example is thirty lines.
