import { action, onInit, ok, fail, setState, http, HttpRequest, HttpResponse, Json, Call, Reply } from "@launchdeck/plugin-sdk/assembly";

export * from "@launchdeck/plugin-sdk/assembly/abi";

// What a request came to, as a value a test can read.
function describe(r: HttpResponse): Json {
  const out = Json.object().setBool("ok", r.ok).setNumber("status", r.status).setString("body", r.body);
  out.setString("error", r.error).setString("message", r.message);
  out.setString("contentType", r.header("Content-Type"));
  return out;
}

function build(args: Json): HttpRequest {
  const request = new HttpRequest(args.getString("method", "GET"), args.getString("url"));
  if (args.has("body")) request.text(args.getString("body"), "text/plain");
  // A body of a given size, made here: an action's arguments cannot carry a large one.
  const size = args.getInt("bodySize");
  if (size > 0) request.text("b".repeat(size), "text/plain");
  const secret = args.getString("secret");
  if (secret.length > 0) request.bearer(secret);
  const timeout = args.getInt("timeoutMs");
  if (timeout > 0) request.timeout(timeout);
  return request;
}

// What a request made during `init` came to, for `init_result`.
let duringInit: Json = Json.object();

function init(config: Json): Reply {
  const url = config.getString("initUrl");
  if (url.length > 0) duringInit = describe(http.get(url));
  return ok();
}

// One request, exactly as asked. Always succeeds as an action: the answer is the value.
function fetch(call: Call): Reply {
  const r = http.request(build(call.args));
  if (r.ok) setState("status", r.status.toString());
  return ok(describe(r));
}

// The same request `count` times in one call.
function burst(call: Call): Reply {
  const count = call.args.getInt("count");
  let answered = 0;
  let lastError = "";
  for (let i = 0; i < count; i++) {
    const r = http.request(build(call.args));
    if (r.ok) answered++;
    else lastError = r.error;
  }
  return ok(Json.object().setNumber("answered", answered).setString("lastError", lastError));
}

// A request, and then the module dies before it can answer.
function fetchThenTrap(call: Call): Reply {
  http.request(build(call.args));
  setState("status", "never delivered");
  unreachable();
  return ok();
}

// A request, then a long computation, to show the wait is not the module's time.
function fetchThenWork(call: Call): Reply {
  const r = http.request(build(call.args));
  let x: u32 = 0;
  for (let i = 0; i < 200000; i++) x = (x * 31 + <u32>i) & 0xffff;
  return ok(describe(r).setNumber("x", x));
}

function initResult(call: Call): Reply {
  return ok(duringInit);
}

function failing(call: Call): Reply {
  return fail("asked to fail", "asked");
}

onInit(init);
action("fetch", fetch);
action("burst", burst);
action("fetch_then_trap", fetchThenTrap);
action("fetch_then_work", fetchThenWork);
action("init_result", initResult);
action("fail", failing);
