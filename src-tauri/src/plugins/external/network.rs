//! The network broker: the only way a plugin's bytes reach a socket.
//!
//! A plugin never opens a connection. It hands LaunchDeck a description of one
//! HTTP request; this module decides whether it may go out, sends it with a
//! client the plugin cannot configure, and hands back a bounded answer.
//!
//! ```text
//!   plugin ─► http_request (JSON) ─► parse and validate
//!                                 ─► requested by the manifest?  ─ no ─► host_not_allowed
//!                                 ─► granted by the person?      ─ no ─► permission_denied
//!                                 ─► resolve, check every address against the origin's reach
//!                                 ─► send (no proxy, no automatic redirects, limits)
//!                                 ─► each redirect goes through the same checks
//!                                 ─► bounded response (JSON) ─► plugin
//! ```
//!
//! There is one broker per running plugin. Nothing here knows any plugin by name.

use std::collections::{BTreeMap, HashMap, VecDeque};
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::json;
use ureq::http::Uri;
use ureq::unversioned::resolver::{DefaultResolver, ResolvedSocketAddrs, Resolver};
use ureq::unversioned::transport::{DefaultConnector, NextTimeout};
use url::Url;

use super::capabilities::{
    reach_allows, reach_of_ip, Decision, Grants, NetworkRequest, Origin, OriginError, Reach,
};
use super::log::{clean_line, Level, PluginLog};
use super::secrets::{SecretRef, SecretStore};
use crate::events::now_ms;

pub const MAX_REQUEST_BODY: usize = 256 * 1024;
/// The whole request message: the body, escaped as JSON, and everything around it.
pub const MAX_REQUEST_MESSAGE: usize = 3 * MAX_REQUEST_BODY;
pub const MAX_RESPONSE_BODY: usize = 1024 * 1024;
pub const MAX_REQUEST_HEADERS: usize = 32;
pub const MAX_REQUEST_HEADER_BYTES: usize = 8 * 1024;
/// What the HTTP client reads of a response's headers before giving up.
pub const MAX_RESPONSE_HEADER_READ: usize = 64 * 1024;
/// What of them is handed to the plugin.
pub const MAX_RESPONSE_HEADER_BYTES: usize = 16 * 1024;
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(10);
pub const MIN_TIMEOUT: Duration = Duration::from_secs(1);
pub const MAX_TIMEOUT: Duration = Duration::from_secs(30);
pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
pub const MAX_REDIRECTS: usize = 5;
/// Requests one call of a plugin (one action, one refresh) may make: one for
/// each instance a refresh is asked about, and room for an action on top.
pub const MAX_REQUESTS_PER_CALL: u32 = 24;
/// Time one call of a plugin may spend waiting on the network.
pub const NETWORK_BUDGET_PER_CALL: Duration = Duration::from_secs(30);
/// Requests one plugin may make in a rolling minute: ten a second on average,
/// which a plugin that polls sixteen things every two seconds stays under.
pub const MAX_REQUESTS_PER_MINUTE: usize = 600;

const METHODS: &[&str] = &["GET", "POST", "PUT", "PATCH", "DELETE"];
const WITH_BODY: &[&str] = &["POST", "PUT", "PATCH"];
/// Headers the host owns: they frame the message or say where it goes.
const HOST_HEADERS: &[&str] = &[
    "host",
    "content-length",
    "connection",
    "transfer-encoding",
    "upgrade",
    "te",
    "trailer",
    "keep-alive",
    "expect",
    "accept-encoding",
    "user-agent",
];
/// Headers that carry credentials and so never follow a redirect to another origin.
const CREDENTIAL_HEADERS: &[&str] = &["authorization", "cookie"];

/// Why a request did not produce a response. `code` is for the plugin's code,
/// `message` for a person.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NetError {
    pub code: &'static str,
    pub message: String,
}

impl NetError {
    fn new(code: &'static str, message: impl Into<String>) -> NetError {
        NetError {
            code,
            message: message.into(),
        }
    }
}

pub mod code {
    pub const PERMISSION_DENIED: &str = "permission_denied";
    pub const HOST_NOT_ALLOWED: &str = "host_not_allowed";
    pub const INVALID_REQUEST: &str = "invalid_request";
    pub const REQUEST_TOO_LARGE: &str = "request_too_large";
    pub const RESPONSE_TOO_LARGE: &str = "response_too_large";
    pub const TIMEOUT: &str = "timeout";
    pub const NETWORK_ERROR: &str = "network_error";
    pub const REDIRECT_NOT_ALLOWED: &str = "redirect_not_allowed";
    pub const TOO_MANY_REDIRECTS: &str = "too_many_redirects";
    pub const RATE_LIMITED: &str = "rate_limited";
    pub const SECRET_MISSING: &str = "secret_missing";
    pub const INVALID_RESPONSE: &str = "invalid_response";
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct AuthSpec {
    /// Key of a secret the manifest declares.
    secret: String,
    /// How it is presented. Only `bearer` exists.
    #[serde(default = "bearer")]
    scheme: String,
}

fn bearer() -> String {
    "bearer".into()
}

/// One request as a plugin describes it.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct WireRequest {
    method: String,
    url: String,
    #[serde(default)]
    headers: BTreeMap<String, String>,
    #[serde(default)]
    body: Option<String>,
    #[serde(default)]
    auth: Option<AuthSpec>,
    #[serde(default)]
    timeout_ms: Option<u64>,
}

/// What a plugin gets back from a request that reached a server.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpResponse {
    pub status: u16,
    /// Lower-case names. Repeated headers are joined with `, `.
    pub headers: BTreeMap<String, String>,
    pub body: String,
}

/// The last thing that went wrong, for the Plugins screen.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NetFailure {
    pub code: String,
    pub message: String,
    /// The origin it was about, when the request got far enough to have one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub origin: Option<String>,
    pub at: u64,
}

/// What the sandbox needs from whoever performs requests: bytes in, bytes out.
pub trait NetGate: Send + Sync {
    /// Perform the request described by `message` within `budget` and return
    /// the answer as JSON. Never fails: a refusal is an answer.
    fn request(&self, message: &[u8], budget: Duration) -> Vec<u8>;
}

/// What a secret of the plugin is bound to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SecretBinding {
    /// The only origin the secret is sent to. `None` while the setting that
    /// names it gives no origin.
    pub origin: Option<Origin>,
}

pub struct Broker {
    plugin: String,
    request: NetworkRequest,
    secrets: HashMap<String, SecretBinding>,
    grants: Arc<Grants>,
    store: Arc<dyn SecretStore>,
    log: Arc<PluginLog>,
    /// When recent requests were made, for the per-minute limit.
    recent: Mutex<VecDeque<Instant>>,
    last_failure: Mutex<Option<NetFailure>>,
}

impl Broker {
    pub fn new(
        plugin: &str,
        request: NetworkRequest,
        secrets: HashMap<String, SecretBinding>,
        grants: Arc<Grants>,
        store: Arc<dyn SecretStore>,
        log: Arc<PluginLog>,
    ) -> Broker {
        Broker {
            plugin: plugin.to_string(),
            request,
            secrets,
            grants,
            store,
            log,
            recent: Mutex::new(VecDeque::new()),
            last_failure: Mutex::new(None),
        }
    }

    /// What the plugin asks for, as resolved when it started.
    pub fn requested(&self) -> &NetworkRequest {
        &self.request
    }

    /// The most recent failure, unless a request has succeeded since.
    pub fn last_failure(&self) -> Option<NetFailure> {
        self.last_failure
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    /// May a request go to this origin now? The manifest must ask for it and
    /// the person must have granted exactly it.
    fn authorize(&self, origin: &Origin) -> Result<(), NetError> {
        if !self.request.requests(origin) {
            return Err(NetError::new(
                code::HOST_NOT_ALLOWED,
                format!("this plugin does not ask for access to {origin}, so it has none"),
            ));
        }
        match self.grants.decision(&self.plugin, origin) {
            Some(Decision::Granted) => Ok(()),
            Some(Decision::Denied) => Err(NetError::new(
                code::PERMISSION_DENIED,
                format!("access to {origin} was denied in Plugins"),
            )),
            None => Err(NetError::new(
                code::PERMISSION_DENIED,
                format!("access to {origin} is waiting for approval in Plugins"),
            )),
        }
    }

    fn within_rate(&self) -> bool {
        let mut recent = self.recent.lock().unwrap_or_else(|e| e.into_inner());
        let now = Instant::now();
        while recent
            .front()
            .is_some_and(|t| now.duration_since(*t) > Duration::from_secs(60))
        {
            recent.pop_front();
        }
        if recent.len() >= MAX_REQUESTS_PER_MINUTE {
            return false;
        }
        recent.push_back(now);
        true
    }

    /// The whole of one request, as a value. `perform` wraps it for the plugin.
    pub fn run(&self, message: &[u8], budget: Duration) -> Result<HttpResponse, NetError> {
        let mut seen_origin = None;
        let started = Instant::now();
        let outcome = self.run_inner(message, budget, &mut seen_origin);
        let ms = started.elapsed().as_millis();
        let mut last = self.last_failure.lock().unwrap_or_else(|e| e.into_inner());
        match &outcome {
            Ok(response) => {
                *last = None;
                self.log.host(
                    Level::Debug,
                    &format!("request answered {} in {ms} ms", response.status),
                );
            }
            Err(e) => {
                *last = Some(NetFailure {
                    code: e.code.to_string(),
                    message: e.message.clone(),
                    origin: seen_origin
                        .as_ref()
                        .map(|o: &Origin| o.as_str().to_string()),
                    at: now_ms(),
                });
                self.log.host(
                    Level::Warn,
                    &format!(
                        "request refused or failed ({}): {}",
                        e.code,
                        clean_line(e.message.as_bytes())
                    ),
                );
            }
        }
        outcome
    }

    fn run_inner(
        &self,
        message: &[u8],
        budget: Duration,
        seen: &mut Option<Origin>,
    ) -> Result<HttpResponse, NetError> {
        if message.len() > MAX_REQUEST_MESSAGE {
            return Err(NetError::new(
                code::REQUEST_TOO_LARGE,
                format!("the request is larger than {MAX_REQUEST_BODY} bytes"),
            ));
        }
        let wire: WireRequest = serde_json::from_slice(message).map_err(|e| {
            NetError::new(
                code::INVALID_REQUEST,
                format!("the request is not understood: {e}"),
            )
        })?;
        let method = wire.method.to_ascii_uppercase();
        if !METHODS.contains(&method.as_str()) {
            return Err(NetError::new(
                code::INVALID_REQUEST,
                format!(
                    "method `{}` is not offered; use one of {}",
                    short(&wire.method),
                    METHODS.join(", ")
                ),
            ));
        }
        let mut url = Url::parse(wire.url.trim()).map_err(|_| {
            NetError::new(
                code::INVALID_REQUEST,
                format!("`{}` is not a URL", short(&wire.url)),
            )
        })?;
        url.set_fragment(None);
        let origin = Origin::of_url(&url).map_err(invalid_origin)?;
        *seen = Some(origin.clone());

        let body = wire.body;
        if body.as_ref().is_some_and(|b| b.len() > MAX_REQUEST_BODY) {
            return Err(NetError::new(
                code::REQUEST_TOO_LARGE,
                format!("the body is larger than {MAX_REQUEST_BODY} bytes"),
            ));
        }
        if body.is_some() && !WITH_BODY.contains(&method.as_str()) {
            return Err(NetError::new(
                code::INVALID_REQUEST,
                format!("a {method} request takes no body"),
            ));
        }
        let mut headers = check_headers(&wire.headers)?;

        // Who may the secret go to, and is it there? Decided before anything is sent.
        let auth = match &wire.auth {
            None => None,
            Some(spec) => {
                if !spec.scheme.eq_ignore_ascii_case("bearer") {
                    return Err(NetError::new(
                        code::INVALID_REQUEST,
                        format!(
                            "auth scheme `{}` is not offered; use `bearer`",
                            short(&spec.scheme)
                        ),
                    ));
                }
                if headers.iter().any(|(n, _)| n == "authorization") {
                    return Err(NetError::new(
                        code::INVALID_REQUEST,
                        "set an Authorization header or use `auth`, not both",
                    ));
                }
                let binding = self.secrets.get(&spec.secret).ok_or_else(|| {
                    NetError::new(
                        code::INVALID_REQUEST,
                        format!(
                            "the manifest declares no secret called `{}`",
                            short(&spec.secret)
                        ),
                    )
                })?;
                if binding.origin.as_ref() != Some(&origin) {
                    return Err(NetError::new(
                        code::HOST_NOT_ALLOWED,
                        match &binding.origin {
                            Some(bound) => format!("the secret `{}` is only ever sent to {bound}, not to {origin}", spec.secret),
                            None => format!("the secret `{}` has no destination yet: the setting that names it is not set", spec.secret),
                        },
                    ));
                }
                Some(spec.secret.clone())
            }
        };

        if !self.within_rate() {
            return Err(NetError::new(
                code::RATE_LIMITED,
                format!("more than {MAX_REQUESTS_PER_MINUTE} requests in a minute"),
            ));
        }
        self.authorize(&origin)?;

        // Read only now that the request is known to be allowed, and held only until it is sent.
        let mut credential = match auth {
            None => None,
            Some(key) => match self.store.get(&SecretRef::new(&self.plugin, &key)) {
                Ok(Some(value)) => Some(value),
                Ok(None) => {
                    return Err(NetError::new(
                        code::SECRET_MISSING,
                        format!("the secret `{key}` is not set; enter it in Plugins"),
                    ))
                }
                Err(e) => {
                    return Err(NetError::new(
                        code::SECRET_MISSING,
                        format!("the secret `{key}` could not be read: {e}"),
                    ))
                }
            },
        };

        let wanted = wire
            .timeout_ms
            .map(Duration::from_millis)
            .unwrap_or(DEFAULT_TIMEOUT)
            .clamp(MIN_TIMEOUT, MAX_TIMEOUT);
        let deadline = Instant::now() + wanted.min(budget);
        let mut method = method;
        let mut body = body;
        let mut current = origin;

        for hop in 0..=MAX_REDIRECTS {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(NetError::new(
                    code::TIMEOUT,
                    "the request did not finish in time",
                ));
            }
            self.log.host(
                Level::Debug,
                &format!("{method} {current}{}", clean_line(url.path().as_bytes())),
            );
            let response = send(
                &method,
                &url,
                &headers,
                credential.as_deref(),
                body.as_deref(),
                remaining,
                &current,
            )?;
            let Some(location) = redirect_target(&response) else {
                return Ok(response);
            };
            if hop == MAX_REDIRECTS {
                return Err(NetError::new(
                    code::TOO_MANY_REDIRECTS,
                    format!("more than {MAX_REDIRECTS} redirects"),
                ));
            }
            let mut next = url.join(&location).map_err(|_| {
                NetError::new(
                    code::INVALID_RESPONSE,
                    "the server redirected to something that is not a URL",
                )
            })?;
            next.set_fragment(None);
            let next_origin = Origin::of_url(&next).map_err(|e| {
                NetError::new(
                    code::REDIRECT_NOT_ALLOWED,
                    format!("the server redirected somewhere that cannot be used: {e}"),
                )
            })?;
            *seen = Some(next_origin.clone());
            // A redirect is a new request: it needs its own permission, and nothing is sent without it.
            self.authorize(&next_origin).map_err(|e| {
                NetError::new(
                    code::REDIRECT_NOT_ALLOWED,
                    format!("{current} redirected to {next_origin}: {}", e.message),
                )
            })?;
            if next_origin != current {
                // Credentials belong to the origin they were meant for.
                credential = None;
                headers.retain(|(n, _)| !CREDENTIAL_HEADERS.contains(&n.as_str()));
            }
            if response.status == 303 || (matches!(response.status, 301 | 302) && method == "POST")
            {
                method = "GET".to_string();
                body = None;
                headers.retain(|(n, _)| n != "content-type");
            }
            url = next;
            current = next_origin;
        }
        Err(NetError::new(
            code::TOO_MANY_REDIRECTS,
            format!("more than {MAX_REDIRECTS} redirects"),
        ))
    }

    /// The answer as the plugin receives it.
    pub fn perform(&self, message: &[u8], budget: Duration) -> Vec<u8> {
        encode(self.run(message, budget))
    }
}

impl NetGate for Broker {
    fn request(&self, message: &[u8], budget: Duration) -> Vec<u8> {
        self.perform(message, budget)
    }
}

/// The JSON a plugin reads: a response, or why there is none.
pub fn encode(outcome: Result<HttpResponse, NetError>) -> Vec<u8> {
    let value = match outcome {
        Ok(r) => json!({ "ok": true, "status": r.status, "headers": r.headers, "body": r.body }),
        Err(e) => json!({ "ok": false, "error": e.code, "message": e.message }),
    };
    serde_json::to_vec(&value).unwrap_or_else(|_| {
        br#"{"ok":false,"error":"network_error","message":"the answer could not be written"}"#
            .to_vec()
    })
}

/// A refusal the sandbox itself gives (too many requests in one call, for example).
pub fn refusal(code: &'static str, message: &str) -> Vec<u8> {
    encode(Err(NetError::new(code, message)))
}

fn short(text: &str) -> String {
    text.chars().filter(|c| !c.is_control()).take(80).collect()
}

fn invalid_origin(e: OriginError) -> NetError {
    NetError::new(code::INVALID_REQUEST, e.to_string())
}

fn is_token(name: &str) -> bool {
    !name.is_empty()
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&b))
}

/// The plugin's headers, lower-cased, with everything the host owns refused.
fn check_headers(headers: &BTreeMap<String, String>) -> Result<Vec<(String, String)>, NetError> {
    if headers.len() > MAX_REQUEST_HEADERS {
        return Err(NetError::new(
            code::INVALID_REQUEST,
            format!("more than {MAX_REQUEST_HEADERS} headers"),
        ));
    }
    let mut out = Vec::with_capacity(headers.len());
    let mut total = 0;
    for (name, value) in headers {
        let lower = name.to_ascii_lowercase();
        if !is_token(&lower) {
            return Err(NetError::new(
                code::INVALID_REQUEST,
                format!("`{}` is not a header name", short(name)),
            ));
        }
        if HOST_HEADERS.contains(&lower.as_str()) || lower.starts_with("proxy-") {
            return Err(NetError::new(
                code::INVALID_REQUEST,
                format!("the header `{lower}` is set by LaunchDeck and cannot be set by a plugin"),
            ));
        }
        // A line break in a value would start a header of the plugin's own choosing.
        if value.chars().any(char::is_control) {
            return Err(NetError::new(
                code::INVALID_REQUEST,
                format!("the value of `{lower}` holds a control character"),
            ));
        }
        total += lower.len() + value.len();
        if out.iter().any(|(n, _): &(String, String)| n == &lower) {
            return Err(NetError::new(
                code::INVALID_REQUEST,
                format!("the header `{lower}` is given twice"),
            ));
        }
        out.push((lower, value.clone()));
    }
    if total > MAX_REQUEST_HEADER_BYTES {
        return Err(NetError::new(
            code::REQUEST_TOO_LARGE,
            format!("the headers are larger than {MAX_REQUEST_HEADER_BYTES} bytes"),
        ));
    }
    Ok(out)
}

fn redirect_target(response: &HttpResponse) -> Option<String> {
    if !matches!(response.status, 301 | 302 | 303 | 307 | 308) {
        return None;
    }
    response
        .headers
        .get("location")
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty())
}

// ---------------------------------------------------------------------------
// Resolving: every address must be within the origin's reach
// ---------------------------------------------------------------------------

/// A name resolved to somewhere its origin's permission does not cover.
#[derive(Debug)]
struct OutOfReach(String);

impl std::fmt::Display for OutOfReach {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for OutOfReach {}

fn reach_words(reach: Reach) -> &'static str {
    match reach {
        Reach::Loopback => "this computer",
        Reach::Local => "your local network",
        Reach::Internet => "the internet",
    }
}

/// Checks the addresses a name resolved to. Pure, so it can be tested without DNS.
pub fn check_addresses(origin: &Origin, addresses: &[SocketAddr]) -> Result<(), String> {
    for address in addresses {
        let ip = address.ip();
        match reach_of_ip(ip) {
            None => return Err(format!("{origin} resolved to {ip}, which nothing may connect to")),
            Some(found) if !reach_allows(origin.reach(), found) => {
                return Err(format!(
                    "{origin} resolved to {ip}, which is on {}; the permission for {origin} covers {} only",
                    reach_words(found),
                    reach_words(origin.reach())
                ))
            }
            Some(_) => {}
        }
    }
    Ok(())
}

/// The standard resolver, with the check above between the answer and the connection.
/// The connection is made to the addresses that were checked, so a second
/// lookup cannot give a different answer.
#[derive(Debug)]
struct ReachResolver {
    origin: Origin,
    inner: DefaultResolver,
}

impl Resolver for ReachResolver {
    fn resolve(
        &self,
        uri: &Uri,
        config: &ureq::config::Config,
        timeout: NextTimeout,
    ) -> Result<ResolvedSocketAddrs, ureq::Error> {
        let addresses = self.inner.resolve(uri, config, timeout)?;
        let list: Vec<SocketAddr> = addresses.iter().copied().collect();
        check_addresses(&self.origin, &list)
            .map_err(|why| ureq::Error::Other(Box::new(OutOfReach(why))))?;
        Ok(addresses)
    }
}

// ---------------------------------------------------------------------------
// Sending one request (one hop)
// ---------------------------------------------------------------------------

fn send(
    method: &str,
    url: &Url,
    headers: &[(String, String)],
    credential: Option<&str>,
    body: Option<&str>,
    timeout: Duration,
    origin: &Origin,
) -> Result<HttpResponse, NetError> {
    let config = ureq::Agent::config_builder()
        .timeout_global(Some(timeout))
        .timeout_connect(Some(CONNECT_TIMEOUT.min(timeout)))
        // Redirects are followed here, one checked hop at a time, never by the client.
        .max_redirects(0)
        .http_status_as_error(false)
        // Straight to the destination: the checks are about where the connection goes.
        .proxy(None)
        .max_response_header_size(MAX_RESPONSE_HEADER_READ)
        .max_idle_connections(0)
        .user_agent(concat!("LaunchDeck/", env!("CARGO_PKG_VERSION")))
        .build();
    let agent = ureq::Agent::with_parts(
        config,
        DefaultConnector::default(),
        ReachResolver {
            origin: origin.clone(),
            inner: DefaultResolver::default(),
        },
    );
    let mut builder = ureq::http::Request::builder()
        .method(method)
        .uri(url.as_str());
    for (name, value) in headers {
        builder = builder.header(name.as_str(), value.as_str());
    }
    if let Some(secret) = credential {
        builder = builder.header("authorization", format!("Bearer {secret}"));
    }
    let bad = |_| {
        NetError::new(
            code::INVALID_REQUEST,
            "the request could not be built from these headers",
        )
    };
    let result = match body {
        Some(b) => agent.run(builder.body(b.to_string()).map_err(bad)?),
        None => agent.run(builder.body(()).map_err(bad)?),
    };
    let mut response = result.map_err(|e| transport_error(e, origin))?;
    let status = response.status().as_u16();
    let mut out = BTreeMap::new();
    let mut total = 0;
    for (name, value) in response.headers() {
        let Ok(value) = value.to_str() else { continue };
        total += name.as_str().len() + value.len();
        if total > MAX_RESPONSE_HEADER_BYTES {
            break;
        }
        out.entry(name.as_str().to_ascii_lowercase())
            .and_modify(|v: &mut String| {
                v.push_str(", ");
                v.push_str(value);
            })
            .or_insert_with(|| value.to_string());
    }
    let bytes = response
        .body_mut()
        .with_config()
        // One byte more than is allowed, so that "exactly the limit" and "over it" can be told apart.
        .limit(MAX_RESPONSE_BODY as u64 + 1)
        .read_to_vec()
        .map_err(|e| transport_error(e, origin))?;
    if bytes.len() > MAX_RESPONSE_BODY {
        return Err(NetError::new(
            code::RESPONSE_TOO_LARGE,
            format!("the response is larger than {MAX_RESPONSE_BODY} bytes"),
        ));
    }
    let body = String::from_utf8(bytes).map_err(|_| {
        NetError::new(
            code::INVALID_RESPONSE,
            "the response body is not text (UTF-8)",
        )
    })?;
    Ok(HttpResponse {
        status,
        headers: out,
        body,
    })
}

/// What went wrong on the wire, in words that never include a header or a body.
fn transport_error(e: ureq::Error, origin: &Origin) -> NetError {
    use ureq::Error as E;
    match e {
        E::Other(inner) => match inner.downcast_ref::<OutOfReach>() {
            Some(out) => NetError::new(code::HOST_NOT_ALLOWED, out.0.clone()),
            None => NetError::new(
                code::NETWORK_ERROR,
                format!("the request to {origin} failed"),
            ),
        },
        E::Timeout(_) => NetError::new(code::TIMEOUT, format!("{origin} did not answer in time")),
        E::BodyExceedsLimit(_) => NetError::new(
            code::RESPONSE_TOO_LARGE,
            format!("the response is larger than {MAX_RESPONSE_BODY} bytes"),
        ),
        E::LargeResponseHeader(_, _) => NetError::new(
            code::RESPONSE_TOO_LARGE,
            format!("the response headers are larger than {MAX_RESPONSE_HEADER_READ} bytes"),
        ),
        E::HostNotFound => {
            NetError::new(code::NETWORK_ERROR, format!("{origin} could not be found"))
        }
        E::ConnectionFailed => NetError::new(
            code::NETWORK_ERROR,
            format!("could not connect to {origin}"),
        ),
        E::Io(io) => NetError::new(
            code::NETWORK_ERROR,
            format!("could not reach {origin}: {}", io.kind()),
        ),
        E::Protocol(_) | E::Http(_) | E::Decompress(_, _) | E::BodyStalled => NetError::new(
            code::INVALID_RESPONSE,
            format!("{origin} sent something that is not valid HTTP"),
        ),
        E::Tls(_) | E::Rustls(_) | E::Pem(_) => NetError::new(
            code::NETWORK_ERROR,
            format!("a secure connection to {origin} could not be made"),
        ),
        E::BadUri(_) => NetError::new(code::INVALID_REQUEST, "the URL cannot be used"),
        _ => NetError::new(
            code::NETWORK_ERROR,
            format!("the request to {origin} failed"),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugins::external::capabilities::{HostSource, RequestedHost};
    use crate::plugins::external::secrets::MemoryStore;

    include!("../../../tests/support/http_server.rs");

    const PLUGIN: &str = "test.net";
    const ROOMY: Duration = Duration::from_secs(20);

    struct Rig {
        broker: Broker,
        grants: Arc<Grants>,
        store: Arc<MemoryStore>,
        log: Arc<PluginLog>,
    }

    /// A broker for a plugin that asks for `requested`, of which `granted` are granted.
    /// The secret `token` is bound to the first requested origin.
    fn rig(requested: &[&str], granted: &[&str]) -> Rig {
        let hosts: Vec<RequestedHost> = requested
            .iter()
            .map(|o| RequestedHost {
                origin: Origin::of(o).unwrap(),
                source: HostSource::Declared,
            })
            .collect();
        let grants = Arc::new(Grants::new(None, None));
        for o in granted {
            grants
                .decide(PLUGIN, &Origin::of(o).unwrap(), Some(Decision::Granted))
                .unwrap();
        }
        let store = Arc::new(MemoryStore::new());
        let log = Arc::new(PluginLog::new(PLUGIN));
        let mut secrets = HashMap::new();
        secrets.insert(
            "token".to_string(),
            SecretBinding {
                origin: hosts.first().map(|h| h.origin.clone()),
            },
        );
        let broker = Broker::new(
            PLUGIN,
            NetworkRequest {
                purpose: "Testing.".into(),
                hosts,
                unresolved: vec![],
            },
            secrets,
            grants.clone(),
            store.clone(),
            log.clone(),
        );
        Rig {
            broker,
            grants,
            store,
            log,
        }
    }

    fn get(rig: &Rig, url: &str) -> Result<HttpResponse, NetError> {
        rig.broker.run(
            json!({ "method": "GET", "url": url })
                .to_string()
                .as_bytes(),
            ROOMY,
        )
    }

    fn ask(rig: &Rig, request: serde_json::Value) -> Result<HttpResponse, NetError> {
        rig.broker.run(request.to_string().as_bytes(), ROOMY)
    }

    fn refused(r: Result<HttpResponse, NetError>) -> NetError {
        match r {
            Err(e) => e,
            Ok(r) => panic!("the request went through with status {}", r.status),
        }
    }

    fn hello() -> TestServer {
        TestServer::start(|_| Reply::text(200, "hello"))
    }

    // -- permission -----------------------------------------------------------

    #[test]
    fn a_granted_origin_is_reached_and_the_answer_comes_back_bounded() {
        let server = TestServer::start(|r| {
            assert_eq!(r.target, "/api/thing?x=1");
            Reply::Response {
                status: 201,
                headers: vec![
                    ("x-one".into(), "a".into()),
                    ("X-One".into(), "b".into()),
                    ("content-type".into(), "application/json".into()),
                ],
                body: br#"{"ok":true}"#.to_vec(),
                delay: StdDuration::ZERO,
            }
        });
        let rig = rig(&[&server.origin()], &[&server.origin()]);
        let r = ask(
            &rig,
            json!({ "method": "post", "url": server.url("/api/thing?x=1#frag"), "headers": { "Content-Type": "application/json", "X-Custom": "1" }, "body": "{\"a\":1}" }),
        )
        .unwrap();
        assert_eq!((r.status, r.body.as_str()), (201, r#"{"ok":true}"#));
        assert_eq!(
            r.headers.get("x-one").map(String::as_str),
            Some("a, b"),
            "names are lower-case, repeats are joined"
        );
        let seen = &server.received()[0];
        assert_eq!(
            (seen.method.as_str(), seen.body.as_str()),
            ("POST", "{\"a\":1}")
        );
        assert_eq!(seen.header("x-custom"), Some("1"));
        assert_eq!(seen.header("content-type"), Some("application/json"));
        assert!(
            seen.header("user-agent")
                .unwrap()
                .starts_with("LaunchDeck/"),
            "the host names itself"
        );
        assert_eq!(seen.header("authorization"), None);
        assert!(rig.broker.last_failure().is_none());
    }

    #[test]
    fn nothing_goes_out_without_a_grant() {
        let server = hello();
        // Asked for and never decided.
        let rig = rig(&[&server.origin()], &[]);
        let e = refused(get(&rig, &server.url("/")));
        assert_eq!(e.code, code::PERMISSION_DENIED);
        assert!(e.message.contains("waiting for approval"), "{}", e.message);
        // Denied.
        rig.grants
            .decide(
                PLUGIN,
                &Origin::of(&server.origin()).unwrap(),
                Some(Decision::Denied),
            )
            .unwrap();
        let e = refused(get(&rig, &server.url("/")));
        assert_eq!(e.code, code::PERMISSION_DENIED);
        assert!(e.message.contains("was denied"), "{}", e.message);
        assert_eq!(server.hits(), 0, "no byte reached the server");
        let failure = rig.broker.last_failure().unwrap();
        assert_eq!(
            (failure.code.as_str(), failure.origin.as_deref()),
            ("permission_denied", Some(server.origin().as_str()))
        );

        // Granted: it works. Revoked: it stops, at once.
        rig.grants
            .decide(
                PLUGIN,
                &Origin::of(&server.origin()).unwrap(),
                Some(Decision::Granted),
            )
            .unwrap();
        assert_eq!(get(&rig, &server.url("/")).unwrap().body, "hello");
        assert!(
            rig.broker.last_failure().is_none(),
            "a success clears the last failure"
        );
        rig.grants
            .decide(PLUGIN, &Origin::of(&server.origin()).unwrap(), None)
            .unwrap();
        assert_eq!(
            refused(get(&rig, &server.url("/"))).code,
            code::PERMISSION_DENIED
        );
        assert_eq!(server.hits(), 1);
    }

    #[test]
    fn an_origin_the_manifest_does_not_ask_for_is_never_reached_even_with_a_grant_on_file() {
        let (asked, other) = (hello(), hello());
        let rig = rig(&[&asked.origin()], &[&asked.origin(), &other.origin()]);
        let e = refused(get(&rig, &other.url("/")));
        assert_eq!(e.code, code::HOST_NOT_ALLOWED);
        assert!(e.message.contains("does not ask for"), "{}", e.message);
        assert_eq!(other.hits(), 0);
    }

    #[test]
    fn a_grant_is_for_one_scheme_one_host_and_one_port() {
        let (a, b) = (hello(), hello());
        let rig = rig(&[&a.origin()], &[&a.origin()]);
        // Another port on the same host.
        assert_eq!(refused(get(&rig, &b.url("/"))).code, code::HOST_NOT_ALLOWED);
        // The same port under https.
        assert_eq!(
            refused(get(&rig, &a.url("/").replace("http://", "https://"))).code,
            code::HOST_NOT_ALLOWED
        );
        // The same machine by another name is another origin.
        let by_name = format!("http://localhost:{}/", a.addr.port());
        assert_eq!(refused(get(&rig, &by_name)).code, code::HOST_NOT_ALLOWED);
        assert_eq!((a.hits(), b.hits()), (0, 0));
        // Another spelling of the same address is the same origin, and is covered.
        let spelled = format!("http://2130706433:{}/", a.addr.port());
        assert_eq!(get(&rig, &spelled).unwrap().body, "hello");
        assert_eq!(a.hits(), 1);
    }

    #[test]
    fn requests_that_are_not_plain_http_are_refused_before_any_check() {
        let server = hello();
        let rig = rig(&[&server.origin()], &[&server.origin()]);
        for (url, says) in [
            ("not a url", "not a URL"),
            ("//no-scheme/", "not a URL"),
            ("ftp://127.0.0.1/", "only http and https"),
            ("file:///etc/passwd", "only http and https"),
            ("ws://127.0.0.1/", "only http and https"),
            (
                &format!("http://user:pw@127.0.0.1:{}/", server.addr.port()),
                "user name or password",
            ),
            ("http://0.0.0.0/", "nothing may connect"),
        ] {
            let e = refused(get(&rig, url));
            assert_eq!(e.code, code::INVALID_REQUEST, "{url}");
            assert!(e.message.contains(says), "{url}: {}", e.message);
        }
        for method in ["TRACE", "CONNECT", "OPTIONS", "HEAD", "PROPFIND", ""] {
            let e = refused(ask(
                &rig,
                json!({ "method": method, "url": server.url("/") }),
            ));
            assert_eq!(e.code, code::INVALID_REQUEST, "{method}");
        }
        let e = refused(ask(
            &rig,
            json!({ "method": "GET", "url": server.url("/"), "body": "x" }),
        ));
        assert!(e.message.contains("takes no body"), "{}", e.message);
        let e = refused(ask(
            &rig,
            json!({ "method": "GET", "url": server.url("/"), "socket": true }),
        ));
        assert_eq!(
            e.code,
            code::INVALID_REQUEST,
            "unknown fields are refused, not ignored"
        );
        assert_eq!(
            refused(rig.broker.run(b"not json", ROOMY)).code,
            code::INVALID_REQUEST
        );
        assert_eq!(server.hits(), 0);
    }

    // -- headers ----------------------------------------------------------------

    #[test]
    fn a_plugin_cannot_set_the_headers_that_frame_or_route_the_request() {
        let server = hello();
        let rig = rig(&[&server.origin()], &[&server.origin()]);
        for name in [
            "Host",
            "content-length",
            "Transfer-Encoding",
            "Connection",
            "Upgrade",
            "TE",
            "Trailer",
            "Expect",
            "Proxy-Authorization",
            "Accept-Encoding",
            "User-Agent",
        ] {
            let e = refused(ask(
                &rig,
                json!({ "method": "GET", "url": server.url("/"), "headers": { name: "x" } }),
            ));
            assert_eq!(e.code, code::INVALID_REQUEST, "{name}");
            assert!(
                e.message.contains("set by LaunchDeck"),
                "{name}: {}",
                e.message
            );
        }
        // A line break would smuggle a second header, or a second request.
        for value in ["a\r\nHost: evil.example", "a\nX: y", "a\0b"] {
            let e = refused(ask(
                &rig,
                json!({ "method": "GET", "url": server.url("/"), "headers": { "X-A": value } }),
            ));
            assert_eq!(e.code, code::INVALID_REQUEST, "{value:?}");
        }
        let e = refused(ask(
            &rig,
            json!({ "method": "GET", "url": server.url("/"), "headers": { "bad name": "x" } }),
        ));
        assert_eq!(e.code, code::INVALID_REQUEST);
        let many: serde_json::Map<String, serde_json::Value> =
            (0..33).map(|i| (format!("x-{i}"), json!("v"))).collect();
        assert_eq!(
            refused(ask(
                &rig,
                json!({ "method": "GET", "url": server.url("/"), "headers": many })
            ))
            .code,
            code::INVALID_REQUEST
        );
        let e = refused(ask(
            &rig,
            json!({ "method": "GET", "url": server.url("/"), "headers": { "x-big": "v".repeat(9000) } }),
        ));
        assert_eq!(e.code, code::REQUEST_TOO_LARGE);
        assert_eq!(server.hits(), 0);
        // The Host header is the one of the URL, always.
        get(&rig, &server.url("/")).unwrap();
        assert_eq!(
            server.received()[0].header("host"),
            Some(server.addr.to_string().as_str())
        );
    }

    // -- redirects --------------------------------------------------------------

    #[test]
    fn a_redirect_within_what_is_granted_is_followed() {
        let target = hello();
        let target_url = target.url("/landed");
        let first = TestServer::start(move |r| match r.target.as_str() {
            "/same" => Reply::redirect(302, "/here"),
            "/here" => Reply::text(200, "same origin"),
            _ => Reply::redirect(307, &target_url),
        });
        let rig = rig(
            &[&first.origin(), &target.origin()],
            &[&first.origin(), &target.origin()],
        );
        assert_eq!(get(&rig, &first.url("/same")).unwrap().body, "same origin");
        assert_eq!(get(&rig, &first.url("/other")).unwrap().body, "hello");
        assert_eq!(target.received()[0].target, "/landed");
    }

    #[test]
    fn a_redirect_to_an_origin_that_is_not_granted_is_refused_and_nothing_is_sent_there() {
        let forbidden = hello();
        let to = forbidden.url("/steal");
        let allowed = TestServer::start(move |_| Reply::redirect(302, &to));
        // Not requested at all.
        let rig1 = rig(&[&allowed.origin()], &[&allowed.origin()]);
        let e = refused(get(&rig1, &allowed.url("/")));
        assert_eq!(e.code, code::REDIRECT_NOT_ALLOWED);
        assert!(e.message.contains(&forbidden.origin()), "{}", e.message);
        // Requested, and not granted.
        let rig2 = rig(
            &[&allowed.origin(), &forbidden.origin()],
            &[&allowed.origin()],
        );
        assert_eq!(
            refused(get(&rig2, &allowed.url("/"))).code,
            code::REDIRECT_NOT_ALLOWED
        );
        assert_eq!(
            forbidden.hits(),
            0,
            "the forbidden origin never saw a request"
        );
        assert_eq!(
            rig2.broker.last_failure().unwrap().origin.as_deref(),
            Some(forbidden.origin().as_str())
        );

        // A redirect to a scheme that is not http is refused the same way.
        let odd = TestServer::start(|_| Reply::redirect(302, "file:///etc/passwd"));
        let rig3 = rig(&[&odd.origin()], &[&odd.origin()]);
        assert_eq!(
            refused(get(&rig3, &odd.url("/"))).code,
            code::REDIRECT_NOT_ALLOWED
        );
    }

    #[test]
    fn redirects_end_after_a_few_hops() {
        let server = TestServer::start(|r| {
            let n: u32 = r.target.trim_start_matches("/").parse().unwrap_or(0);
            Reply::redirect(302, &format!("/{}", n + 1))
        });
        let rig = rig(&[&server.origin()], &[&server.origin()]);
        assert_eq!(
            refused(get(&rig, &server.url("/0"))).code,
            code::TOO_MANY_REDIRECTS
        );
        assert_eq!(server.hits(), MAX_REDIRECTS + 1);
    }

    #[test]
    fn a_post_that_is_redirected_follows_what_the_status_says() {
        let server = TestServer::start(|r| match r.target.as_str() {
            "/see-other" => Reply::redirect(303, "/done"),
            "/keep" => Reply::redirect(307, "/done"),
            _ => Reply::text(200, &format!("{} {}", r.method, r.body)),
        });
        let rig = rig(&[&server.origin()], &[&server.origin()]);
        let post = |path: &str| {
            ask(
                &rig,
                json!({ "method": "POST", "url": server.url(path), "body": "payload" }),
            )
            .unwrap()
            .body
        };
        assert_eq!(
            post("/see-other"),
            "GET ",
            "303 continues as a GET without the body"
        );
        assert_eq!(post("/keep"), "POST payload", "307 repeats the request");
    }

    // -- limits -----------------------------------------------------------------

    #[test]
    fn a_server_that_does_not_answer_ends_in_a_timeout() {
        let server =
            TestServer::start(|_| Reply::text(200, "late").after(StdDuration::from_secs(4)));
        let rig = rig(&[&server.origin()], &[&server.origin()]);
        let started = Instant::now();
        let e = refused(ask(
            &rig,
            json!({ "method": "GET", "url": server.url("/"), "timeoutMs": 1000 }),
        ));
        assert_eq!(e.code, code::TIMEOUT);
        assert!(
            started.elapsed() < Duration::from_millis(2500),
            "took {:?}",
            started.elapsed()
        );
        // The call's remaining budget is a timeout of its own, whatever the request asked for.
        let started = Instant::now();
        let e = refused(
            rig.broker.run(
                json!({ "method": "GET", "url": server.url("/"), "timeoutMs": 30000 })
                    .to_string()
                    .as_bytes(),
                Duration::from_millis(700),
            ),
        );
        assert_eq!(e.code, code::TIMEOUT);
        assert!(
            started.elapsed() < Duration::from_millis(2500),
            "took {:?}",
            started.elapsed()
        );
        // And nothing is left to wait for when the budget is spent.
        assert_eq!(
            refused(
                rig.broker.run(
                    json!({ "method": "GET", "url": server.url("/") })
                        .to_string()
                        .as_bytes(),
                    Duration::ZERO
                )
            )
            .code,
            code::TIMEOUT
        );
    }

    #[test]
    fn responses_and_requests_are_bounded() {
        let server = TestServer::start(|r| match r.target.as_str() {
            "/big" => Reply::Response {
                status: 200,
                headers: vec![],
                body: vec![b'x'; MAX_RESPONSE_BODY + 1],
                delay: StdDuration::ZERO,
            },
            "/fits" => Reply::Response {
                status: 200,
                headers: vec![],
                body: vec![b'x'; MAX_RESPONSE_BODY],
                delay: StdDuration::ZERO,
            },
            _ => Reply::text(200, &r.body.len().to_string()),
        });
        let rig = rig(&[&server.origin()], &[&server.origin()]);
        assert_eq!(
            refused(get(&rig, &server.url("/big"))).code,
            code::RESPONSE_TOO_LARGE
        );
        assert_eq!(
            get(&rig, &server.url("/fits")).unwrap().body.len(),
            MAX_RESPONSE_BODY
        );

        let post = |n: usize| {
            ask(
                &rig,
                json!({ "method": "POST", "url": server.url("/echo"), "body": "b".repeat(n) }),
            )
        };
        assert_eq!(
            post(MAX_REQUEST_BODY).unwrap().body,
            MAX_REQUEST_BODY.to_string()
        );
        let before = server.hits();
        assert_eq!(
            refused(post(MAX_REQUEST_BODY + 1)).code,
            code::REQUEST_TOO_LARGE
        );
        assert_eq!(
            refused(rig.broker.run(&vec![b' '; MAX_REQUEST_MESSAGE + 1], ROOMY)).code,
            code::REQUEST_TOO_LARGE
        );
        assert_eq!(server.hits(), before, "an oversized request is never sent");
    }

    #[test]
    fn an_answer_that_is_not_http_or_not_text_is_an_error_not_a_crash() {
        let server = TestServer::start(|r| match r.target.as_str() {
            "/garbage" => Reply::Raw(b"\x00\x01\x02 this is not http\r\n\r\n".to_vec()),
            "/closed" => Reply::Raw(Vec::new()),
            "/short" => {
                Reply::Raw(b"HTTP/1.1 200 OK\r\ncontent-length: 50\r\n\r\nonly this".to_vec())
            }
            _ => Reply::Response {
                status: 200,
                headers: vec![],
                body: vec![0xff, 0xfe, 0x00, 0x80],
                delay: StdDuration::ZERO,
            },
        });
        let rig = rig(&[&server.origin()], &[&server.origin()]);
        for path in ["/garbage", "/closed", "/short"] {
            let e = refused(get(&rig, &server.url(path)));
            assert!(
                [code::INVALID_RESPONSE, code::NETWORK_ERROR].contains(&e.code),
                "{path}: {} {}",
                e.code,
                e.message
            );
        }
        let e = refused(get(&rig, &server.url("/binary")));
        assert_eq!(e.code, code::INVALID_RESPONSE);
        assert!(e.message.contains("not text"), "{}", e.message);
    }

    #[test]
    fn nothing_listening_is_a_network_error() {
        let port = {
            let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            l.local_addr().unwrap().port()
        };
        let origin = format!("http://127.0.0.1:{port}");
        let rig = rig(&[&origin], &[&origin]);
        let e = refused(get(&rig, &format!("{origin}/")));
        assert_eq!(e.code, code::NETWORK_ERROR, "{}", e.message);
    }

    #[test]
    fn a_plugin_cannot_send_more_than_its_share_in_a_minute() {
        let server = hello();
        let rig = rig(&[&server.origin()], &[&server.origin()]);
        for i in 0..MAX_REQUESTS_PER_MINUTE {
            assert!(get(&rig, &server.url("/")).is_ok(), "request {i}");
        }
        assert_eq!(
            refused(get(&rig, &server.url("/"))).code,
            code::RATE_LIMITED
        );
        assert_eq!(server.hits(), MAX_REQUESTS_PER_MINUTE);
    }

    #[test]
    fn requests_of_different_plugins_run_at_the_same_time() {
        let server =
            TestServer::start(|_| Reply::text(200, "ok").after(StdDuration::from_millis(400)));
        let origin = server.origin();
        let started = Instant::now();
        let threads: Vec<_> = (0..4)
            .map(|_| {
                let (origin, url) = (origin.clone(), server.url("/"));
                std::thread::spawn(move || {
                    let rig = rig(&[&origin], &[&origin]);
                    get(&rig, &url).map(|r| r.status)
                })
            })
            .collect();
        for t in threads {
            assert_eq!(t.join().unwrap().unwrap(), 200);
        }
        assert!(
            started.elapsed() < Duration::from_millis(1200),
            "four 400 ms requests took {:?}",
            started.elapsed()
        );
    }

    // -- where a name resolves ---------------------------------------------------

    #[test]
    fn a_name_that_resolves_outside_its_reach_is_refused() {
        let at = |ip: &str| -> Vec<SocketAddr> { vec![format!("{ip}:80").parse().unwrap()] };
        let internet = Origin::of("https://api.example.com").unwrap();
        assert!(check_addresses(&internet, &at("93.184.216.34")).is_ok());
        // DNS rebinding: a public name that now points inside.
        for inside in [
            "127.0.0.1",
            "10.0.0.5",
            "192.168.1.10",
            "169.254.169.254",
            "[::1]",
            "[fd00::1]",
            "[::ffff:10.0.0.1]",
        ] {
            let why = check_addresses(&internet, &at(inside)).unwrap_err();
            assert!(why.contains("covers the internet only"), "{inside}: {why}");
        }
        // One bad address among good ones is enough.
        let mixed: Vec<SocketAddr> = vec![
            "93.184.216.34:80".parse().unwrap(),
            "192.168.1.1:80".parse().unwrap(),
        ];
        assert!(check_addresses(&internet, &mixed).is_err());

        let local = Origin::of("http://homeassistant.local:8123").unwrap();
        assert!(check_addresses(&local, &at("192.168.1.20")).is_ok());
        assert!(
            check_addresses(&local, &at("127.0.0.1")).is_ok(),
            "a local name may be this computer"
        );
        assert!(check_addresses(&local, &at("93.184.216.34"))
            .unwrap_err()
            .contains("covers your local network only"));

        let this = Origin::of("http://localhost:8080").unwrap();
        assert!(check_addresses(&this, &at("127.0.0.1")).is_ok());
        assert!(check_addresses(&this, &at("192.168.1.20"))
            .unwrap_err()
            .contains("covers this computer only"));
        assert!(check_addresses(&this, &at("0.0.0.0"))
            .unwrap_err()
            .contains("nothing may connect"));
    }

    #[test]
    fn localhost_is_resolved_and_reached_when_granted() {
        let server = hello();
        let origin = format!("http://localhost:{}", server.addr.port());
        let rig = rig(&[&origin], &[&origin]);
        assert_eq!(get(&rig, &format!("{origin}/")).unwrap().body, "hello");
    }

    // -- secrets ------------------------------------------------------------------

    #[test]
    fn the_host_adds_the_secret_and_the_plugin_never_holds_it() {
        let server = TestServer::start(|r| {
            Reply::json(
                200,
                &format!(r#"{{"auth":"{}"}}"#, r.header("authorization").is_some()),
            )
        });
        let rig = rig(&[&server.origin()], &[&server.origin()]);
        let with_auth = json!({ "method": "GET", "url": server.url("/v1/things/42?secret=no"), "auth": { "secret": "token" } });

        // Not set: a clear failure, and nothing is sent.
        let e = refused(ask(&rig, with_auth.clone()));
        assert_eq!(e.code, code::SECRET_MISSING);
        assert!(e.message.contains("enter it in Plugins"), "{}", e.message);
        assert_eq!(server.hits(), 0);

        rig.store
            .set(&SecretRef::new(PLUGIN, "token"), "s3cr3t-token-value")
            .unwrap();
        let r = ask(&rig, with_auth.clone()).unwrap();
        assert_eq!(
            server.received()[0].header("authorization"),
            Some("Bearer s3cr3t-token-value")
        );
        // What the plugin receives, and what LaunchDeck wrote down, hold no trace of it.
        let answer = String::from_utf8(encode(Ok(r))).unwrap();
        assert!(!answer.contains("s3cr3t"), "{answer}");
        let lines: Vec<String> = rig.log.entries().into_iter().map(|l| l.message).collect();
        assert!(!lines.is_empty());
        for line in &lines {
            assert!(
                !line.contains("s3cr3t") && !line.contains("Bearer") && !line.contains("secret=no"),
                "{line}"
            );
        }
        assert!(
            lines.iter().any(|l| l.contains("/v1/things/42")),
            "the path is logged, the query is not: {lines:?}"
        );

        // Deleted: authentication stops cleanly.
        rig.store.delete(&SecretRef::new(PLUGIN, "token")).unwrap();
        assert_eq!(refused(ask(&rig, with_auth)).code, code::SECRET_MISSING);
        assert_eq!(server.hits(), 1);
    }

    #[test]
    fn a_secret_goes_to_its_own_origin_and_nowhere_else() {
        let (home, other) = (hello(), hello());
        // `token` is bound to the first origin; both are granted.
        let rig = rig(
            &[&home.origin(), &other.origin()],
            &[&home.origin(), &other.origin()],
        );
        rig.store
            .set(&SecretRef::new(PLUGIN, "token"), "s3cr3t")
            .unwrap();
        let e = refused(ask(
            &rig,
            json!({ "method": "GET", "url": other.url("/"), "auth": { "secret": "token" } }),
        ));
        assert_eq!(e.code, code::HOST_NOT_ALLOWED);
        assert!(e.message.contains("only ever sent to"), "{}", e.message);
        assert_eq!(other.hits(), 0);

        let e = refused(ask(
            &rig,
            json!({ "method": "GET", "url": home.url("/"), "auth": { "secret": "nope" } }),
        ));
        assert!(e.message.contains("declares no secret"), "{}", e.message);
        let e = refused(ask(
            &rig,
            json!({ "method": "GET", "url": home.url("/"), "auth": { "secret": "token", "scheme": "basic" } }),
        ));
        assert!(e.message.contains("use `bearer`"), "{}", e.message);
        let e = refused(ask(
            &rig,
            json!({ "method": "GET", "url": home.url("/"), "auth": { "secret": "token" }, "headers": { "Authorization": "Bearer mine" } }),
        ));
        assert!(e.message.contains("not both"), "{}", e.message);
        assert_eq!(home.hits(), 0);
    }

    #[test]
    fn credentials_do_not_follow_a_redirect_to_another_origin() {
        let elsewhere = TestServer::start(|r| {
            Reply::text(
                200,
                &format!("{:?} {:?}", r.header("authorization"), r.header("cookie")),
            )
        });
        let to = elsewhere.url("/");
        let home = TestServer::start(move |r| match r.target.as_str() {
            "/stay" => Reply::redirect(302, "/landed"),
            "/landed" => Reply::text(200, &format!("{:?}", r.header("authorization"))),
            _ => Reply::redirect(302, &to),
        });
        let rig = rig(
            &[&home.origin(), &elsewhere.origin()],
            &[&home.origin(), &elsewhere.origin()],
        );
        rig.store
            .set(&SecretRef::new(PLUGIN, "token"), "s3cr3t")
            .unwrap();
        let go = |path: &str| {
            ask(&rig, json!({ "method": "GET", "url": home.url(path), "auth": { "secret": "token" }, "headers": { "Cookie": "session=1" } })).unwrap().body
        };
        assert_eq!(
            go("/stay"),
            r#"Some("Bearer s3cr3t")"#,
            "the same origin keeps it"
        );
        assert_eq!(
            go("/leave"),
            "None None",
            "another origin gets neither the secret nor the cookie"
        );
    }
}
