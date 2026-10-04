import { action, onInit, ok, publish, info, Json, Call, Reply } from "@launchdeck/plugin-sdk/assembly";

// Required: exposes the plugin ABI to LaunchDeck. Nothing else is exported.
export * from "@launchdeck/plugin-sdk/assembly/abi";

let greeting = "Hello";

// Runs once when LaunchDeck starts the plugin. `config` holds the settings
// declared in plugin.json, with the person's values applied.
function init(config: Json): Reply {
  greeting = config.getString("greeting", "Hello");
  return ok();
}

// hello.say: greet someone, tell the deck about it, and return the message.
function say(call: Call): Reply {
  const name = call.args.getString("name", "world");
  const message = greeting + ", " + name + "!";
  info("greeting " + name);
  publish(
    "hello.completed",
    Json.object().setString("name", name).setString("message", message),
  );
  return ok(Json.object().setString("message", message));
}

onInit(init);
action("hello.say", say);
