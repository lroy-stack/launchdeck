// LaunchDeck plugin SDK for AssemblyScript (protocol / ABI 1).
//
//   import { action, onInit, ok, fail, publish, info, Json, Call, Reply }
//     from "@launchdeck/plugin-sdk/assembly";
//   export * from "@launchdeck/plugin-sdk/assembly/abi";   // exposes the ABI to the host
//
//   function say(call: Call): Reply {
//     publish("hello.completed", Json.object().setString("name", "world"));
//     setState("greeted", true);
//     return ok();
//   }
//   action("hello.say", say);
//
// A plugin only describes what happened (actions, events) and how things are
// now (states). It never learns about pads, colours, MIDI or devices: surfaces
// decide what an event or a state looks like.

import { Json } from "./json";
import { hostLog, hostPublish } from "./host";
import {
  Call,
  Reply,
  ActionHandler,
  InitHandler,
  RefreshHandler,
  actionIds,
  actionHandlers,
  setInitHandler,
  setRefreshHandler,
} from "./registry";

export { Json, Kind } from "./json";
export { Call, Reply, Watched, ActionHandler, InitHandler, RefreshHandler } from "./registry";
export { http, HttpRequest, HttpResponse, HttpError } from "./http";

// ---- results ---------------------------------------------------------------

// Success, with an optional value returned to whoever invoked the action.
export function ok(value: Json | null = null): Reply {
  return new Reply(true, value, "", "");
}

// Failure with a message a person can read. `code` is an optional short
// identifier (letters, digits, `_ . : -`) for programs.
export function fail(message: string, code: string = ""): Reply {
  return new Reply(false, null, message, code);
}

// ---- events ----------------------------------------------------------------

// Publish a semantic event onto LaunchDeck's event bus. The type must be
// declared in plugin.json. The source is always this plugin's id. Events are
// delivered when the current call returns successfully. Returns false when the
// host refused it (the reason is in the plugin log).
export function publish(type: string, payload: Json | null = null): bool {
  const event = Json.object().setString("type", type);
  if (payload != null) event.set("payload", payload);
  return hostPublish(event.toString()) == 0;
}

// ---- states ----------------------------------------------------------------

// Say how something is now. `id` is a state declared in the manifest's
// `states`; the value must have its declared type, and for an enum it must be
// one of the listed values. A state with arguments (`session`, `channel`) is
// addressed by `args`, which must name every required argument:
//
//   setState("level", "high");
//   setState("status", "up", Json.object().setString("channel", "alpha"));
//
// Writing the value a state already has is not a change. States are delivered
// when the current call returns, and are dropped if the call traps. LaunchDeck
// does not remember them: whenever the plugin starts (also after it was
// disabled and enabled again) every state is unknown until an action reports
// it, and a write made inside `onInit` is ignored. Returns false when the host
// refused the write (the reason is in the plugin log).
export function setState<T>(id: string, value: T, args: Json | null = null): bool {
  const message = Json.object().setString("state", id);
  if (args != null) message.set("args", args);
  if (isString<T>()) {
    message.setString("value", changetype<string>(value));
  } else if (isBoolean<T>()) {
    message.setBool("value", <bool>value);
  } else if (isInteger<T>() || isFloat<T>()) {
    message.setNumber("value", <f64>value);
  } else {
    ERROR("setState takes a string, a boolean or a number");
  }
  return hostPublish(message.toString()) == 0;
}

// Say that an instance of a state has no value now (a session ended): it reads
// unknown again. For a state without arguments this is rarely what you want.
export function clearState(id: string, args: Json | null = null): bool {
  const message = Json.object().setString("state", id).setBool("clear", true);
  if (args != null) message.set("args", args);
  return hostPublish(message.toString()) == 0;
}

// ---- logging ---------------------------------------------------------------

export enum Level {
  Debug = 0,
  Info = 1,
  Warn = 2,
  Error = 3,
}

export function log(level: Level, message: string): void {
  hostLog(level, message);
}
export function debug(message: string): void {
  hostLog(Level.Debug, message);
}
export function info(message: string): void {
  hostLog(Level.Info, message);
}
export function warn(message: string): void {
  hostLog(Level.Warn, message);
}
export function error(message: string): void {
  hostLog(Level.Error, message);
}

// ---- registration ----------------------------------------------------------

// Register the function that runs when `id` (declared in plugin.json) is invoked.
export function action(id: string, handler: ActionHandler): void {
  actionIds.push(id);
  actionHandlers.push(handler);
}

// Optional. Runs once when the plugin starts, with its configuration. Return
// fail(...) to refuse to start. The network is not available here.
export function onInit(handler: InitHandler): void {
  setInitHandler(handler);
}

// Optional, for a plugin that declares `refresh` in plugin.json. LaunchDeck
// calls it when the plugin starts and then every interval, with the instances
// of the plugin's states that controls are showing. Report each one with
// setState (or clearState when it cannot be known), so a pad follows the real
// thing and not the last action.
export function onRefresh(handler: RefreshHandler): void {
  setRefreshHandler(handler);
}
