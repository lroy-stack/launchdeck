import { action, ok, fail, publish, setState, clearState, Json, Call, Reply } from "@launchdeck/plugin-sdk/assembly";

export * from "@launchdeck/plugin-sdk/assembly/abi";

// A state with an argument has one instance per value of it. `status` is
// scoped by `channel`, so "channel alpha is up" and "channel beta is down" are
// two instances of the same state, each addressed by its channel.
function scope(call: Call): Json {
  return Json.object().setString("channel", call.args.getString("channel"));
}

function set(call: Call, status: string): Reply {
  const channel = call.args.getString("channel");
  if (channel.length == 0) return fail("a channel is required", "bad_args");
  if (!setState("status", status, scope(call))) {
    return fail("LaunchDeck did not accept the status of `" + channel + "`", "state_refused");
  }
  publish(
    "channel.changed",
    Json.object().setString("channel", channel).setString("status", status),
  );
  return ok();
}

function up(call: Call): Reply {
  return set(call, "up");
}

function down(call: Call): Reply {
  return set(call, "down");
}

// The instance has no value any more: it reads unknown, not "down".
function forget(call: Call): Reply {
  clearState("status", scope(call));
  return ok();
}

action("channel.up", up);
action("channel.down", down);
action("channel.forget", forget);
