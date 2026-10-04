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

export type ActionHandler = (call: Call) => Reply;
export type InitHandler = (config: Json) => Reply;

export const actionIds: string[] = [];
export const actionHandlers: ActionHandler[] = [];
export let initHandler: InitHandler | null = null;

export function setInitHandler(handler: InitHandler): void {
  initHandler = handler;
}
