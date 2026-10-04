// The module side of LaunchDeck's plugin ABI 1. Re-export this from your entry
// file (`export * from "@launchdeck/plugin-sdk/assembly/abi";`) and nothing else is
// exposed to the host.
//
//   ld_abi() -> i32                    the protocol version this module speaks
//   ld_alloc(len) -> ptr               the host writes a request there
//   ld_handle(ptr, len) -> i32         handle it; 0 once a reply was sent
//
// Requests and replies are JSON. See docs/plugin-sdk/protocol.md.

import { Json } from "./json";
import { hostRespond } from "./host";
import {
  Reply,
  Call,
  Watched,
  actionIds,
  actionHandlers,
  initHandler,
  refreshHandler,
} from "./registry";

export function ld_abi(): i32 {
  return 1;
}

export function ld_alloc(len: i32): usize {
  return heap.alloc(<usize>len);
}

function failed(message: string): string {
  return new Reply(false, null, message, "").toString();
}

function handle(text: string): string {
  const request = Json.parse(text);
  if (request == null) return failed("request is not valid JSON");
  const op = request.getString("op");

  if (op == "init") {
    const handler = initHandler;
    if (handler == null) return new Reply(true, null, "", "").toString();
    const config = request.get("config");
    return handler(config != null ? config : Json.object()).toString();
  }

  if (op == "invoke") {
    const id = request.getString("action");
    const at = actionIds.indexOf(id);
    if (at < 0) return failed("this plugin has no action `" + id + "`");
    const args = request.get("args");
    return actionHandlers[at](new Call(id, args != null ? args : Json.object())).toString();
  }

  if (op == "refresh") {
    const handler = refreshHandler;
    if (handler == null) return new Reply(true, null, "", "").toString();
    const watching: Watched[] = [];
    const list = request.get("watching");
    if (list != null) {
      for (let i = 0; i < list.length; i++) {
        const item = list.at(i);
        if (item == null) continue;
        const args = item.get("args");
        watching.push(new Watched(item.getString("state"), args != null ? args : Json.object()));
      }
    }
    return handler(watching).toString();
  }

  return failed("unknown request `" + op + "`");
}

export function ld_handle(ptr: usize, len: i32): i32 {
  const text = String.UTF8.decodeUnsafe(ptr, <usize>len);
  heap.free(ptr); // the host hands the request buffer over to the plugin
  hostRespond(handle(text));
  return 0;
}
