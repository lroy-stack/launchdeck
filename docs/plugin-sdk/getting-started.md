# Getting started

One path from an empty folder to a plugin someone else can install: create, build, validate, install, test, read the logs, package. It starts from the Hello example in `examples/plugins/hello`.

You need:

* Node 20 or newer and npm 10 or newer, to build.
* A LaunchDeck checkout. It holds the SDK (`sdk/assemblyscript`), which is not published to the npm registry, and the validator, which needs the Rust toolchain.
* LaunchDeck itself, to try the plugin.

## 1. Create

Copy the Hello example to a folder of your own and leave its build products behind:

```sh
cp -R examples/plugins/hello ~/plugins/acme-notes
cd ~/plugins/acme-notes
rm -rf node_modules hello.wasm
```

Then make it yours:

1. In `plugin.json`, change `id` (lowercase, dotted, at least two parts, for example `acme.notes`; ids starting with `launchdeck.` are reserved), `name`, `version`, `author` and `description`.
2. In `asconfig.json`, change `outFile` (for example `notes.wasm`), and set `main` in `plugin.json` to the same name.
3. Point the SDK dependency at the SDK. The copied `package.json` reaches it by a path that only works inside the checkout (`file:../../../sdk/assemblyscript`). Pick one:

   * **By path**, while you work next to a checkout. The plugin builds with whatever SDK the checkout holds:

     ```sh
     npm install /path/to/launchdeck/sdk/assemblyscript
     ```

   * **By tarball**, to pin one SDK version and keep it with your sources:

     ```sh
     (cd /path/to/launchdeck/sdk/assemblyscript && npm pack --pack-destination ~/plugins)
     npm install ~/plugins/launchdeck-plugin-sdk-0.2.1.tgz
     ```

   Either command rewrites the `@launchdeck/plugin-sdk` entry of `package.json` and the lockfile. Keep `package-lock.json` with your sources.
4. Set `"sdk"` in `plugin.json` to the version you build with (`0.2.1` today). It is informational, and `plugins:validate` warns when it is older than the host's ([versioning](versioning.md)).

The source of Hello, `assembly/index.ts`:

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

and its manifest, `plugin.json` (shortened):

```json
{
  "schemaVersion": 2,
  "id": "example.hello",
  "name": "Hello Plugin",
  "version": "0.1.0",
  "description": "Says hello, tells the deck about it, and shows the smallest complete plugin.",
  "author": "LaunchDeck examples",
  "abi": 1,
  "sdk": "0.2.1",
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

Register one function per action with `action("your.id", fn)` and declare the same ids in `plugin.json`, with a `label`, a `description`, a `category` and the `params` the person fills in: they are what the Action Library shows. Every event the module publishes must be in `events`. The host checks the manifest, not the module. Every field is in the [manifest reference](manifest.md); states, network access and secrets have their own pages.

## 2. Build

```sh
npm ci           # installs exactly what package-lock.json says
npm run build    # writes notes.wasm (about 20 KB)
```

The build is `asc --target release` with the options in `asconfig.json`; keep `"use": ["abort="]` ([AssemblyScript](assemblyscript.md)). `npm ci` never rewrites the lockfile, so the same sources, lockfile and SDK give the same module ([reproducible builds](versioning.md#the-example-plugins-and-reproducible-builds)).

The examples in this repository are built by `npm run plugins:build` at the repository root (or `sh scripts/build-example-plugins.sh hello counter` for some of them); their built modules are committed, so you only rebuild after changing one.

## 3. Validate

From the LaunchDeck checkout:

```sh
npm run plugins:validate -- ~/plugins/acme-notes
```

The folder is checked by the real plugin host, as the app would load it: the manifest is validated, the module is loaded and linked, and `init` runs with the default settings and no network. It prints what the Plugins screen would show and exits with 1 on a problem. Nothing is written next to your folder and nothing is granted. Details and typical messages: [testing](testing.md#validate-a-folder).

## 4. Install locally

A plugin is its folder, and LaunchDeck reads only `plugin.json` and the `.wasm` file the manifest names. Two ways to install it:

* **Into the plugins folder.** Open **Plugins** and press **Show plugins folder**: LaunchDeck creates the folder if needed and shows it. Copy your plugin's folder into it and press **Rescan**. The folder is:

  | System | Folder |
  | --- | --- |
  | macOS | `~/Library/Application Support/dev.launchdeck.app/plugins/` |
  | Windows | `%APPDATA%\dev.launchdeck.app\plugins\` |
  | Linux | `~/.local/share/dev.launchdeck.app/plugins/` |

* **From where you work**, while developing from a LaunchDeck checkout: list the folder that holds your plugin folders in `LAUNCHDECK_PLUGINS`, so a rebuild and **Rescan** pick up the new module without copying.

  ```sh
  LAUNCHDECK_PLUGINS="$HOME/plugins" npm run tauri dev
  ```

  The variable is a list of folders (`:` separated, `;` on Windows), each holding plugin folders. They are searched after the plugins folder. If two folders carry the same id, the first one found wins and the other is listed as skipped.

A plugin found this way is *on*. Nothing is granted to it: a plugin that asks for network access waits for the person's **Allow** ([permissions](../user/plugins.md)).

## 5. Test

On **Plugins**, your plugin's card shows its status, actions, events and settings. If something is wrong it says *error* and what kind of problem stopped it, with the host's own words under **Details**.

To run an action, switch to **Edit**, select a pad, search for your action in the **Action Library** (it is listed as *Plugin: Notes*) and assign it, by pressing **Assign** or dragging the row onto the pad. Fill in the form the manifest's `params` drew and press **Test run**: the action runs at once, without a device, and the result or the plugin's error message is shown. Press **Save** to keep the assignment. You never type an id or any JSON.

To make the pad react to what the plugin publishes, give it a feedback rule: **Feedback, Add feedback, An event**, pick the plugin's event, and choose the look it shows and for how long ([the feedback editor](../user/feedback-editor.md)). In Live mode, pressing the pad runs the plugin, the event reaches the rule, and the pad changes on the simulator and on a Launchpad. A plugin whose fact is true until it changes ("the counter is high", "the light is on") reports a *state* instead, so the pad is right as soon as it appears ([states](states.md)).

## 6. Read the logs

**Show log** on the plugin's card shows the plugin's own lines (`info(...)` in the SDK) and the host's notes about it: start, failures, refused events, and each network request as method, address and path. It keeps the last 100 lines per plugin and is cleared when LaunchDeck exits. [Testing and debugging](testing.md) lists the usual problems and what they mean.

## 7. Package

To give the plugin to someone, zip a folder that holds `plugin.json` and the `.wasm` file:

```sh
mkdir -p dist/acme.notes
cp plugin.json notes.wasm dist/acme.notes/
(cd dist && zip -r acme.notes.zip acme.notes)
```

They unzip it into their plugins folder (**Plugins**, **Show plugins folder**) and press **Rescan**. That is all there is: LaunchDeck does not open zip files, there is no package format beyond the folder, and no checksum or signature is made or checked yet ([packaging](packaging.md)). Settings, grants and secrets belong to the plugin's id, so a new version with the same id keeps them, and so would a different module that took the same id ([security](security.md#what-is-not-claimed)).

## Not using AssemblyScript?

Read the [protocol reference](protocol.md). The C Ping example (`examples/plugins/c-ping`) is about thirty lines of C with no SDK.
