// The functions LaunchDeck gives a plugin. This is the whole host surface: no
// files, no sockets, no clock, no hardware. The last two exist only for a
// plugin whose manifest asks for the network; they are imported only by a
// plugin that calls `http.request`, so a plugin that does not is unaffected.

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

@external("launchdeck", "http_request")
declare function ld_http_request(ptr: usize, len: i32): i32;

@external("launchdeck", "http_response")
declare function ld_http_response(ptr: usize, cap: i32): i32;

// Hand one request (JSON) to the host and return its answer (JSON). The host
// decides whether anything is sent; a refusal is an answer too. An empty
// string means the host gave no answer at all.
export function hostHttp(json: string): string {
  const bytes = String.UTF8.encode(json);
  const size = ld_http_request(changetype<usize>(bytes), bytes.byteLength);
  if (size <= 0) return "";
  const buffer = new ArrayBuffer(size);
  const got = ld_http_response(changetype<usize>(buffer), size);
  if (got < 0) return "";
  return String.UTF8.decodeUnsafe(changetype<usize>(buffer), <usize>got);
}
