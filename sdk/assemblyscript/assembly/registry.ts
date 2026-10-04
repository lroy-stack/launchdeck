// Where actions and the init handler are remembered. Internal.

import { Json } from "./json";

export class Call {
  constructor(
    public action: string,
    public args: Json,
  ) {}
}

export class Reply {
  constructor(
    public succeeded: bool,
    public value: Json | null,
    public error: string,
    public code: string,
  ) {}

  toString(): string {
    if (this.succeeded) {
      const v = this.value;
      return '{"status":"ok","value":' + (v == null ? "null" : v.toString()) + "}";
    }
    const o = Json.object().setString("status", "failed").setString("error", this.error);
    if (this.code.length > 0) o.setString("code", this.code);
    return o.toString();
  }
}

// One instance of one of the plugin's states that a control is showing.
export class Watched {
  constructor(
    // The state's id, as declared in plugin.json.
    public state: string,
    // What picks the instance (`{"entity": "light.kitchen"}`); empty for a state without arguments.
    public args: Json,
  ) {}
}

export type ActionHandler = (call: Call) => Reply;
export type InitHandler = (config: Json) => Reply;
export type RefreshHandler = (watching: Watched[]) => Reply;

export const actionIds: string[] = [];
export const actionHandlers: ActionHandler[] = [];
export let initHandler: InitHandler | null = null;

export function setInitHandler(handler: InitHandler): void {
  initHandler = handler;
}

export let refreshHandler: RefreshHandler | null = null;

export function setRefreshHandler(handler: RefreshHandler): void {
  refreshHandler = handler;
}
