import { action, onInit, ok, fail, publish, setState, Json, Call, Reply } from "@launchdeck/plugin-sdk/assembly";

export * from "@launchdeck/plugin-sdk/assembly/abi";

// The counter lives in the module's memory for as long as the plugin runs, so
// it survives between invocations. Restarting the plugin (disable and enable,
// changing a setting) starts from zero again, and LaunchDeck then treats the
// `level` state as unknown until an action reports it.
let count: i32 = 0;
let step: i32 = 1;
let limit: i32 = 9;

function init(config: Json): Reply {
  step = config.getInt("step", 1);
  limit = config.getInt("max", 9);
  if (step < 1 || limit < 1) return fail("step and max must be at least 1", "bad_config");
  return ok();
}

// A pad shows the level, not the number: the person decides what each level looks like.
function level(): string {
  if (count <= 0) return "zero";
  if (count >= limit) return "max";
  return count * 2 > limit ? "high" : "low";
}

// How the counter is now. This is a state (it is true until it changes), so it
// is reported every time the plugin learns it, including when an action fails
// because the counter was already at its limit.
function report(): void {
  setState("level", level());
}

function state(): Json {
  return Json.object().setNumber("value", count).setString("level", level());
}

// The same change as an event: something that happened, announced once.
function changed(): Reply {
  report();
  publish("counter.changed", state());
  return ok(state());
}

function increment(call: Call): Reply {
  if (count >= limit) {
    report();
    return fail("the counter is at its maximum", "at_limit");
  }
  count = min(count + step, limit);
  return changed();
}

function decrement(call: Call): Reply {
  if (count <= 0) {
    report();
    return fail("the counter is already at zero", "at_limit");
  }
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
