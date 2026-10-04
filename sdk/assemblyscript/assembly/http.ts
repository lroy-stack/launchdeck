// Network requests, through LaunchDeck.
//
//   const r = http.request(
//     new HttpRequest("POST", base + "/api/services/light/toggle")
//       .json(Json.object().setString("entity_id", "light.kitchen"))
//       .bearer("token"));
//   if (!r.ok) return fail(r.message, r.error);       // nothing came back
//   if (!r.success) return fail("HTTP " + r.status.toString());
//
// A plugin never opens a connection. It describes a request and LaunchDeck
// sends it, if the manifest asks for the destination (`capabilities.network`)
// and the person has granted it. The call returns when the answer is there;
// meanwhile LaunchDeck shows the action as pending.
//
// A secret is named, never held: `.bearer("token")` asks the host to add
// `Authorization: Bearer <the value the person stored>`. The value does not
// enter the plugin.

import { Json } from "./json";
import { hostHttp } from "./host";

// Why there is no response. Compare `HttpResponse.error` with these.
export namespace HttpError {
  // The destination is asked for in the manifest and the person has not granted it.
  export const PermissionDenied: string = "permission_denied";
  // The destination is not in the manifest, or resolved somewhere the grant does not cover.
  export const HostNotAllowed: string = "host_not_allowed";
  export const InvalidRequest: string = "invalid_request";
  export const RequestTooLarge: string = "request_too_large";
  export const ResponseTooLarge: string = "response_too_large";
  export const Timeout: string = "timeout";
  export const NetworkError: string = "network_error";
  export const RedirectNotAllowed: string = "redirect_not_allowed";
  export const TooManyRedirects: string = "too_many_redirects";
  export const RateLimited: string = "rate_limited";
  // The secret the request names has no value: the person has not entered it.
  export const SecretMissing: string = "secret_missing";
  // The secret cannot be read: this computer has no secure credential store, or
  // it is locked. Nothing was sent. The person is told why in Plugins.
  export const SecretUnavailable: string = "secret_unavailable";
  export const InvalidResponse: string = "invalid_response";
}

export class HttpRequest {
  private headerNames: string[] = [];
  private headerValues: string[] = [];
  private body: string | null = null;
  private secret: string = "";
  private timeoutMs: i32 = 0;

  // `method` is GET, POST, PUT, PATCH or DELETE. `url` is absolute.
  constructor(
    public method: string,
    public url: string,
  ) {}

  // Set a header. The host sets Host, Content-Length, User-Agent and the other
  // headers that frame a request; trying to set one of those is refused.
  header(name: string, value: string): HttpRequest {
    const at = this.headerNames.indexOf(name);
    if (at >= 0) {
      this.headerValues[at] = value;
    } else {
      this.headerNames.push(name);
      this.headerValues.push(value);
    }
    return this;
  }

  // A JSON body (POST, PUT, PATCH), with its content type.
  json(body: Json): HttpRequest {
    this.body = body.toString();
    return this.header("Content-Type", "application/json");
  }

  // A text body (POST, PUT, PATCH).
  text(body: string, contentType: string = "text/plain; charset=utf-8"): HttpRequest {
    this.body = body;
    return this.header("Content-Type", contentType);
  }

  // Ask the host to authenticate with a secret declared in the manifest. The
  // secret is only ever sent to the host it is declared for.
  bearer(secret: string): HttpRequest {
    this.secret = secret;
    return this;
  }

  // How long to wait, 1 to 30 seconds. 10 seconds when not set.
  timeout(ms: i32): HttpRequest {
    this.timeoutMs = ms;
    return this;
  }

  toJson(): Json {
    const out = Json.object().setString("method", this.method).setString("url", this.url);
    if (this.headerNames.length > 0) {
      const headers = Json.object();
      for (let i = 0; i < this.headerNames.length; i++) headers.setString(this.headerNames[i], this.headerValues[i]);
      out.set("headers", headers);
    }
    const body = this.body;
    if (body != null) out.setString("body", body);
    if (this.secret.length > 0) out.set("auth", Json.object().setString("secret", this.secret).setString("scheme", "bearer"));
    if (this.timeoutMs > 0) out.setNumber("timeoutMs", this.timeoutMs);
    return out;
  }
}

export class HttpResponse {
  constructor(
    // A response arrived, whatever its status.
    public ok: bool,
    public status: i32,
    public body: string,
    private headers: Json,
    // When `ok` is false: one of `HttpError`, and a sentence for a person.
    public error: string,
    public message: string,
  ) {}

  // A response arrived and its status is 200 to 299.
  get success(): bool {
    return this.ok && this.status >= 200 && this.status < 300;
  }

  // A response header by its lower-case name, or "".
  header(name: string): string {
    return this.headers.getString(name.toLowerCase());
  }

  // The body as JSON, or null when it is not JSON.
  json(): Json | null {
    return Json.parse(this.body);
  }
}

function noAnswer(code: string, message: string): HttpResponse {
  return new HttpResponse(false, 0, "", Json.object(), code, message);
}

export namespace http {
  // Send one request and wait for the answer. Never traps: when nothing came
  // back, `ok` is false and `error` says why.
  export function request(r: HttpRequest): HttpResponse {
    const text = hostHttp(r.toJson().toString());
    if (text.length == 0) return noAnswer(HttpError.NetworkError, "the host gave no answer");
    const answer = Json.parse(text);
    if (answer == null) return noAnswer(HttpError.InvalidResponse, "the host's answer could not be read");
    if (!answer.getBool("ok")) {
      return noAnswer(answer.getString("error", HttpError.NetworkError), answer.getString("message", "the request failed"));
    }
    const headers = answer.get("headers");
    return new HttpResponse(true, answer.getInt("status"), answer.getString("body"), headers != null ? headers : Json.object(), "", "");
  }

  export function get(url: string): HttpResponse {
    return request(new HttpRequest("GET", url));
  }
}
