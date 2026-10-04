// LaunchDeck plugin SDK for AssemblyScript (protocol / ABI 1).
//
//   import { action, onInit, ok, fail, publish, info, Json, Call, Reply }
//     from "@launchdeck/plugin-sdk/assembly";
//   export * from "@launchdeck/plugin-sdk/assembly/abi";   // exposes the ABI to the host
//
//   function say(call: Call): Reply {
//     publish("hello.completed", Json.object().setString("name", "world"));
//     return ok();
//   }
//   action("hello.say", say);
//
// A plugin only describes what happened (actions, events). It never learns
// about pads, colours, MIDI or devices: surfaces decide what an event looks like.

import { Json } from "./json";
import { hostLog, hostPublish } from "./host";
import {
  Call,
  Reply,
  ActionHandler,
  InitHandler,
  actionIds,
  actionHandlers,
  setInitHandler,
} from "./registry";

export { Json, Kind } from "./json";
export { Call, Reply, ActionHandler, InitHandler } from "./registry";

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
// fail(...) to refuse to start.
export function onInit(handler: InitHandler): void {
  setInitHandler(handler);
}
