import { action, onInit, ok, fail, publish, Json, Call, Reply } from "@launchdeck/plugin-sdk/assembly";

export * from "@launchdeck/plugin-sdk/assembly/abi";

// State lives in the module's memory for as long as the plugin runs, so it
// survives between invocations. Restarting the plugin (disable and enable,
// changing a setting) starts from zero again.
let count: i32 = 0;
let step: i32 = 1;
let limit: i32 = 9;

function init(config: Json): Reply {
  step = config.getInt("step", 1);
  limit = config.getInt("max", 9);
  if (step < 1 || limit < 1) return fail("step and max must be at least 1", "bad_config");
  return ok();
}

// The pad shows the level, not the number: the surface binds each level to a look.
function level(): string {
  if (count <= 0) return "zero";
  if (count >= limit) return "max";
  return count * 2 > limit ? "high" : "low";
}

function state(): Json {
  return Json.object().setNumber("value", count).setString("level", level());
}

function changed(): Reply {
  publish("counter.changed", state());
  return ok(state());
}

function increment(call: Call): Reply {
  if (count >= limit) return fail("the counter is at its maximum", "at_limit");
  count = min(count + step, limit);
  return changed();
}

function decrement(call: Call): Reply {
  if (count <= 0) return fail("the counter is already at zero", "at_limit");
  count = max(count - step, 0);
  return changed();
}

function reset(call: Call): Reply {
  count = 0;
  return changed();
}

onInit(init);
action("counter.increment", increment);
action("counter.decrement", decrement);
action("counter.reset", reset);
