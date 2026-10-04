// The three functions LaunchDeck gives a plugin. This is the whole host
// surface: no files, no network, no clock, no hardware.

@external("launchdeck", "log")
declare function ld_log(level: i32, ptr: usize, len: i32): void;

@external("launchdeck", "publish")
declare function ld_publish(ptr: usize, len: i32): i32;

@external("launchdeck", "respond")
declare function ld_respond(ptr: usize, len: i32): void;

export function hostLog(level: i32, message: string): void {
  const bytes = String.UTF8.encode(message);
  ld_log(level, changetype<usize>(bytes), bytes.byteLength);
}

// 0 means accepted. Anything else is a rejection the host also logs.
export function hostPublish(json: string): i32 {
  const bytes = String.UTF8.encode(json);
  return ld_publish(changetype<usize>(bytes), bytes.byteLength);
}

export function hostRespond(json: string): void {
  const bytes = String.UTF8.encode(json);
  ld_respond(changetype<usize>(bytes), bytes.byteLength);
}
