import {
  action,
  onInit,
  onRefresh,
  ok,
  fail,
  setState,
  clearState,
  warn,
  http,
  HttpRequest,
  HttpResponse,
  Json,
  Call,
  Reply,
  Watched,
} from "@launchdeck/plugin-sdk/assembly";

export * from "@launchdeck/plugin-sdk/assembly/abi";

// A small Home Assistant client: three actions and one state, over the REST API.
//
//   POST /api/services/<domain>/<service>   {"entity_id": "light.kitchen"}
//   GET  /api/states/<entity_id>            {"state": "on", ...}
//
// The address is a setting. The token is a secret: this code names it
// (`bearer("token")`) and never sees it. LaunchDeck adds it to requests that go
// to the Home Assistant address, and sends nothing anywhere else.

// What can be switched with turn_on / turn_off / toggle.
const DOMAINS: string[] = ["light", "switch", "input_boolean"];

let base: string = "";

function init(config: Json): Reply {
  base = config.getString("baseUrl");
  // One trailing slash is how people paste it.
  while (base.endsWith("/")) base = base.substring(0, base.length - 1);
  return ok();
}

function scope(entity: string): Json {
  return Json.object().setString("entity", entity);
}

// `light.kitchen` is the domain `light` and an object id; nothing else is an entity.
function domainOf(entity: string): string {
  const dot = entity.indexOf(".");
  if (dot <= 0 || dot == entity.length - 1) return "";
  for (let i = 0; i < entity.length; i++) {
    const c = entity.charCodeAt(i);
    const fine = (c >= 97 && c <= 122) || (c >= 48 && c <= 57) || c == 95 || (c == 46 && i == dot);
    if (!fine) return "";
  }
  return entity.substring(0, dot);
}

// Why a request did not work, in words for the person who pressed the pad.
function explain(r: HttpResponse): string {
  if (!r.ok) return r.message;
  if (r.status == 401) return "Home Assistant refused the access token";
  if (r.status == 404) return "Home Assistant does not know that entity or service";
  return "Home Assistant answered " + r.status.toString();
}

// Read one entity and report what Home Assistant says. When it cannot be read
// the state is cleared: a pad then shows unknown, never the last value.
function report(entity: string): HttpResponse {
  const r = http.request(new HttpRequest("GET", base + "/api/states/" + entity).bearer("token"));
  const body = r.success ? r.json() : null;
  const state = body != null ? body.getString("state") : "";
  if (state == "on" || state == "off" || state == "unavailable") {
    setState("entity", state, scope(entity));
  } else {
    clearState("entity", scope(entity));
  }
  return r;
}

function call(c: Call, service: string): Reply {
  const entity = c.args.getString("entity");
  const domain = domainOf(entity);
  if (domain.length == 0) return fail("`" + entity + "` is not an entity id such as light.kitchen", "bad_entity");
  if (DOMAINS.indexOf(domain) < 0) return fail("this plugin switches lights and switches, not `" + domain + "`", "bad_entity");
  if (base.length == 0) return fail("set the Home Assistant address in Plugins", "not_configured");

  const sent = http.request(
    new HttpRequest("POST", base + "/api/services/" + domain + "/" + service)
      .json(Json.object().setString("entity_id", entity))
      .bearer("token"),
  );
  if (!sent.success) {
    // Whatever went wrong, what the pad shows must be what is true now.
    report(entity);
    return fail(explain(sent), sent.ok ? "http_" + sent.status.toString() : sent.error);
  }
  // The request worked. What the entity is now is for Home Assistant to say.
  const now = report(entity);
  if (!now.success) return fail("the command was sent, but the state could not be read: " + explain(now), now.ok ? "http_" + now.status.toString() : now.error);
  return ok();
}

function toggle(c: Call): Reply {
  return call(c, "toggle");
}

function turnOn(c: Call): Reply {
  return call(c, "turn_on");
}

function turnOff(c: Call): Reply {
  return call(c, "turn_off");
}

// LaunchDeck asks, when the plugin starts and every few seconds, for the
// entities that pads are showing. A change made elsewhere arrives here.
function refresh(watching: Watched[]): Reply {
  if (base.length == 0) return fail("set the Home Assistant address in Plugins", "not_configured");
  let failed = "";
  for (let i = 0; i < watching.length; i++) {
    if (watching[i].state != "entity") continue;
    const entity = watching[i].args.getString("entity");
    if (domainOf(entity).length == 0) continue;
    const r = report(entity);
    if (!r.success && failed.length == 0) failed = explain(r);
    // Without an answer there is no point asking for the rest: say so once.
    if (!r.ok) {
      for (let j = i + 1; j < watching.length; j++) {
        if (watching[j].state == "entity") clearState("entity", scope(watching[j].args.getString("entity")));
      }
      break;
    }
  }
  if (failed.length > 0) {
    warn("refresh: " + failed);
    return fail(failed, "refresh_failed");
  }
  return ok();
}

onInit(init);
onRefresh(refresh);
action("toggle", toggle);
action("turn_on", turnOn);
action("turn_off", turnOff);
