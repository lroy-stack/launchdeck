//! Values shaped like a credential (security finding SF-2): whoever receives
//! a file would receive them. Export refuses one in an argument unless the
//! argument is asked of the importer, and in an input's default. A plain
//! address, host or path is fine.
//!
//! An address is read as the runtime's HTTP client reads it and as a
//! browser does (SF-2b), and checked in each reading; one that the two read
//! apart is refused on its own, since what it carries cannot be told.

use serde_json::Value;

use crate::actions::builtin::as_the_client_reads;

/// What in a value is shaped like a credential.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Found {
    /// A credential, said for a person without repeating it.
    Credential(String),
    /// An address that LaunchDeck's HTTP client and a browser read apart:
    /// only one of them reads it as an address, or they find different
    /// hosts.
    ReadApart,
}

impl Found {
    /// What was found `place` (` in "url"`, ` in a feedback rule`, or
    /// nothing), as the rest of a sentence about whatever holds it.
    pub(super) fn said(&self, place: &str) -> String {
        match self {
            Found::Credential(what) => format!("carries a password or token{place}: {what}"),
            Found::ReadApart => format!(
                "holds{place} an address that LaunchDeck's HTTP client and a browser read \
                 differently, so a password or token in it cannot be ruled out"
            ),
        }
    }
}

/// Names that carry a secret only as an address parameter, compared without
/// case, `-` and `_`: as an argument or a member they name other things too
/// (a key on a keyboard).
const SECRET_PARAMETERS: [&str; 3] = ["key", "auth", "sig"];

/// Names that carry a secret wherever they are, compared the same way.
const SECRET_NAMES: [&str; 2] = ["pass", "pwd"];

/// Endings of such names (`client_secret`, `X-Amz-Signature`,
/// `refresh_token`, `api-key`), compared the same way.
const SECRET_ENDINGS: [&str; 8] = [
    "token",
    "secret",
    "password",
    "passwd",
    "apikey",
    "signature",
    "credential",
    "credentials",
];

/// Headers that carry a secret, compared without case.
const SECRET_HEADERS: [&str; 4] = [
    "authorization",
    "proxy-authorization",
    "cookie",
    "x-api-key",
];

/// The path of a Home Assistant webhook: the id after it is the secret.
const WEBHOOK_PATH: &str = "/api/webhook/";

/// Services whose addresses hold the secret in the path, after the second
/// part, on the first part's host or a name under it: a webhook's id or
/// token, a bot's token.
const SECRET_PATHS: [(&str, &str); 6] = [
    ("hooks.slack.com", "/services/"),
    ("discord.com", "/api/webhooks/"),
    ("discordapp.com", "/api/webhooks/"),
    ("maker.ifttt.com", "/with/key/"),
    ("hooks.zapier.com", "/hooks/"),
    ("api.telegram.org", "/bot"),
];

/// What in `value`, at any depth, is shaped like a credential; `None` when
/// nothing is.
pub(super) fn credential_in(value: &Value) -> Option<Found> {
    match value {
        Value::String(text) => credential_in_text(text),
        Value::Array(items) => items.iter().find_map(credential_in),
        Value::Object(map) => header_pair(map).or_else(|| {
            map.iter()
                .find_map(|(name, v)| credential_named(name, v).or_else(|| credential_in(v)))
        }),
        _ => None,
    }
}

/// A header written as a name and a value:
/// `{ "name": "Authorization", "value": "…" }`.
fn header_pair(map: &serde_json::Map<String, Value>) -> Option<Found> {
    match (map.get("name"), map.get("value")) {
        (Some(Value::String(name)), Some(value)) => credential_named(name, value),
        _ => None,
    }
}

/// A member or an argument whose name says it holds a secret, when it holds
/// something: an authorization, cookie or API-key header, or a name such as
/// `password`, `X-Auth-Token` or `accessKey`.
pub(super) fn credential_named(name: &str, value: &Value) -> Option<Found> {
    if !has_text(value) {
        return None;
    }
    if SECRET_HEADERS.iter().any(|h| name.eq_ignore_ascii_case(h)) {
        return Some(Found::Credential(format!("the header \"{name}\"")));
    }
    secret_name(name).then(|| Found::Credential(format!("the value of \"{name}\"")))
}

fn has_text(value: &Value) -> bool {
    match value {
        Value::String(s) => !s.trim().is_empty(),
        Value::Null => false,
        _ => true,
    }
}

/// `name` without case, `-` and `_`.
fn folded(name: &str) -> String {
    name.chars()
        .filter(|c| !matches!(c, '-' | '_'))
        .flat_map(char::to_lowercase)
        .collect()
}

/// A name that says its value is a secret: one of [`SECRET_NAMES`], one with
/// an ending of [`SECRET_ENDINGS`], or one ending in a separate word "key"
/// (`api_key`, `Subscription-Key`, `accessKey`, never `hotkey`).
fn secret_name(name: &str) -> bool {
    let plain = folded(name);
    let split = name.len().saturating_sub(3);
    let key_word = match (name.get(..split), name.get(split..)) {
        (Some(before), Some(end)) if !before.is_empty() && end.eq_ignore_ascii_case("key") => {
            before.ends_with(['-', '_'])
                || (end.starts_with('K') && before.ends_with(|c: char| c.is_ascii_lowercase()))
        }
        _ => false,
    };
    SECRET_NAMES.contains(&plain.as_str())
        || SECRET_ENDINGS.iter().any(|e| plain.ends_with(e))
        || key_word
}

/// An address parameter that carries a secret: a [`secret_name`], or one of
/// [`SECRET_PARAMETERS`].
fn secret_parameter(name: &str) -> bool {
    secret_name(name) || SECRET_PARAMETERS.contains(&folded(name).as_str())
}

fn credential_in_text(text: &str) -> Option<Found> {
    if let Some(found) = as_json(text).as_ref().and_then(credential_in) {
        return Some(found);
    }
    let lower = text.to_ascii_lowercase();
    if let Some(found) = addresses(text).find_map(credential_in_address) {
        return Some(found);
    }
    let what = if let Some(header) = header_in(&lower) {
        format!("the header \"{header}\"")
    } else if let Some(name) = assignment_in(text) {
        format!("the value of \"{name}\"")
    } else if webhook_id(&lower) {
        "the id of a webhook".to_string()
    } else {
        return None;
    };
    Some(Found::Credential(what))
}

/// A text that is a JSON object or array, such as a request body, read as
/// JSON.
fn as_json(text: &str) -> Option<Value> {
    let text = text.trim();
    if text.starts_with('{') || text.starts_with('[') {
        serde_json::from_str(text).ok()
    } else {
        None
    }
}

/// Where the run of characters that `part_of` accepts and that ends at
/// byte `end` of `text` starts.
fn run_start(text: &str, end: usize, part_of: impl Fn(char) -> bool) -> usize {
    text[..end]
        .char_indices()
        .rev()
        .find(|&(_, c)| !part_of(c))
        .map_or(0, |(i, c)| i + c.len_utf8())
}

/// The addresses in a text: the whole text, and every address of any
/// scheme inside it (`https://`, `postgresql://`), up to a space or a
/// quote.
fn addresses(text: &str) -> impl Iterator<Item = &str> + '_ {
    let scheme_char = |c: char| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.');
    let inside = text.match_indices("://").map(move |(i, _)| {
        let rest = &text[run_start(text, i, scheme_char)..];
        let end = rest
            .find(|c: char| c.is_whitespace() || matches!(c, '"' | '\'' | '`' | '<' | '>'))
            .unwrap_or(rest.len());
        &rest[..end]
    });
    std::iter::once(text.trim()).chain(inside)
}

const WITH_USER: &str = "an address with a user name or password";

/// What an address carries, in the browser's reading and in the HTTP
/// client's; then whether the two read it apart; then a user name or a
/// password as written, which neither reader may take for one.
fn credential_in_address(address: &str) -> Option<Found> {
    let browser = Reading::browser(address);
    let client = Reading::client(address);
    if let Some(what) = browser.iter().chain(&client).find_map(Reading::credential) {
        return Some(Found::Credential(what));
    }
    let seen = browser.as_ref().and_then(Reading::host);
    if seen != client.as_ref().and_then(Reading::host) {
        return Some(Found::ReadApart);
    }
    user_as_written(address).then(|| Found::Credential(WITH_USER.into()))
}

/// An `@` between `://` and the first `/`, `?` or `#` after it, with no
/// blank in between.
fn user_as_written(address: &str) -> bool {
    address.split_once("://").is_some_and(|(_, rest)| {
        let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
        authority.contains('@') && !authority.contains(char::is_whitespace)
    })
}

/// An address as one reader reads it: a browser (WHATWG, the `url` crate)
/// or the HTTP client that sends `http.request` (`actions::builtin`).
struct Reading {
    with_user: bool,
    /// Lowercase; empty when there is none.
    host: String,
    path: String,
    /// The query, and the fragment when the reader keeps one.
    parts: Vec<String>,
}

impl Reading {
    fn browser(address: &str) -> Option<Reading> {
        let url = url::Url::parse(address).ok()?;
        Some(Reading {
            with_user: !url.username().is_empty() || url.password().is_some(),
            host: url.host_str().unwrap_or_default().to_string(),
            path: url.path().to_string(),
            parts: [url.query(), url.fragment()]
                .into_iter()
                .flatten()
                .map(str::to_string)
                .collect(),
        })
    }

    fn client(address: &str) -> Option<Reading> {
        let read = as_the_client_reads(address)?;
        Some(Reading {
            with_user: read.user,
            host: read.host,
            path: read.path,
            parts: read.query.into_iter().collect(),
        })
    }

    fn host(&self) -> Option<&str> {
        Some(self.host.as_str()).filter(|h| !h.is_empty())
    }

    fn credential(&self) -> Option<String> {
        if self.with_user {
            return Some(WITH_USER.into());
        }
        let secret = self
            .parts
            .iter()
            .flat_map(|part| parameters(part))
            .find(|(name, value)| !value.is_empty() && secret_parameter(name));
        if let Some((name, _)) = secret {
            return Some(format!("the address parameter \"{name}\""));
        }
        let path = percent_decoded(&self.path).to_ascii_lowercase();
        if webhook_id(&path) {
            return Some("the id of a webhook".to_string());
        }
        let host = self.host.as_str();
        let on = |service: &str| {
            host == service
                || host
                    .strip_suffix(service)
                    .is_some_and(|sub| sub.ends_with('.'))
        };
        SECRET_PATHS
            .iter()
            .any(|(service, part)| on(service) && followed_by_a_value(&path, part))
            .then(|| "the secret part of a webhook or bot address".to_string())
    }
}

/// The `name=value` pairs of a query or a fragment, decoded, separated by
/// `&` or `;`.
fn parameters(part: &str) -> Vec<(String, String)> {
    url::form_urlencoded::parse(part.replace(';', "&").as_bytes())
        .map(|(name, value)| (name.into_owned(), value.into_owned()))
        .collect()
}

/// `text` with each `%` and two hexadecimal digits read as the byte they
/// stand for, so an escaped letter reads as the letter.
fn percent_decoded(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let hex = bytes
            .get(i + 1..i + 3)
            .filter(|h| bytes[i] == b'%' && h.iter().all(u8::is_ascii_hexdigit))
            .and_then(|h| u8::from_str_radix(std::str::from_utf8(h).ok()?, 16).ok());
        match hex {
            Some(byte) => {
                out.push(byte);
                i += 3;
            }
            None => {
                out.push(bytes[i]);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// A header written as `Name: value` on one line, also right after curl's
/// `-H`, whose name is one of [`SECRET_HEADERS`] or a hyphenated
/// [`secret_name`] (`X-Auth-Token`, `Private-Token`): the name, in lower
/// case. A single word before a colon is prose ("the password: …").
fn header_in(lower: &str) -> Option<&str> {
    let name_char = |c: char| c.is_ascii_alphanumeric() || c == '-';
    lower.match_indices(':').find_map(|(i, _)| {
        let run = &lower[run_start(lower, i, name_char)..i];
        let name = match run.strip_prefix("-h") {
            Some(after_option) if !run.starts_with("--") => after_option,
            _ => run.trim_start_matches('-'),
        };
        let value = lower[i + 1..].lines().next().unwrap_or_default();
        let secret = SECRET_HEADERS.contains(&name) || (name.contains('-') && secret_name(name));
        (secret && !value.trim().is_empty()).then_some(name)
    })
}

/// A `name=value` word whose name says it holds a secret, as a form body,
/// an environment variable or an option writes it (`password=…`,
/// `GITHUB_TOKEN=…`, `--api-key=…`): the name.
fn assignment_in(text: &str) -> Option<&str> {
    let separator =
        |c: char| c.is_whitespace() || matches!(c, '&' | ';' | '"' | '\'' | '`' | '?' | '#');
    text.split(separator).find_map(|word| {
        let (name, value) = word.split_once('=')?;
        let name = name.trim_start_matches('-');
        (!value.is_empty() && secret_name(name)).then_some(name)
    })
}

/// `/api/webhook/` followed by an id.
fn webhook_id(lower: &str) -> bool {
    followed_by_a_value(lower, WEBHOOK_PATH)
}

/// `part` followed by something other than a blank or a `/`.
fn followed_by_a_value(lower: &str, part: &str) -> bool {
    lower.match_indices(part).any(|(i, _)| {
        lower[i + part.len()..]
            .chars()
            .next()
            .is_some_and(|c| !c.is_whitespace() && c != '/')
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn each_shape_is_named_and_never_repeated() {
        for (value, said) in [
            (
                json!("https://u:p@e.com"),
                "an address with a user name or password",
            ),
            (
                json!("ftp://u@e.com"),
                "an address with a user name or password",
            ),
            (
                json!("https://e.com/?Token=x"),
                "the address parameter \"Token\"",
            ),
            (
                json!("see https://e.com/?sig=x&a=b now"),
                "the address parameter \"sig\"",
            ),
            (json!("http://ha:8123/api/webhook/x"), "the id of a webhook"),
            (json!("POST /api/webhook/abc"), "the id of a webhook"),
            (
                json!("curl -H \"Authorization: Bearer x\""),
                "the header \"authorization\"",
            ),
            (json!({ "Cookie": "a=b" }), "the header \"Cookie\""),
            (json!([[{ "X-API-KEY": 1 }]]), "the header \"X-API-KEY\""),
            // A member named for a secret, at any depth.
            (json!({ "password": "x" }), "the value of \"password\""),
            (json!({ "token": "x" }), "the value of \"token\""),
            (
                json!({ "auth": { "user": "a", "pass": "x" } }),
                "the value of \"pass\"",
            ),
            (json!([{ "api_key": "x" }]), "the value of \"api_key\""),
            (
                json!({ "X-Auth-Token": "x" }),
                "the value of \"X-Auth-Token\"",
            ),
            (json!({ "Api-Key": "x" }), "the value of \"Api-Key\""),
            (json!({ "accessKey": "x" }), "the value of \"accessKey\""),
            (
                json!({ "client_secret": 7 }),
                "the value of \"client_secret\"",
            ),
            // Parameter names as services spell them, in the query or the
            // fragment, whatever separates the pairs.
            (
                json!("https://e.com/?client_secret=x"),
                "the address parameter \"client_secret\"",
            ),
            (
                json!("https://e.com/?refresh_token=x"),
                "the address parameter \"refresh_token\"",
            ),
            (
                json!("https://e.com/?api-key=x"),
                "the address parameter \"api-key\"",
            ),
            (
                json!("https://e.com/?access-token=x"),
                "the address parameter \"access-token\"",
            ),
            (
                json!("https://gitlab.com/api?private_token=x"),
                "the address parameter \"private_token\"",
            ),
            (
                json!("https://s3.example.com/o?X-Amz-Signature=x"),
                "the address parameter \"X-Amz-Signature\"",
            ),
            (
                json!("https://s3.example.com/o?X-Amz-Credential=x"),
                "the address parameter \"X-Amz-Credential\"",
            ),
            (
                json!("https://e.com/?pass=x"),
                "the address parameter \"pass\"",
            ),
            (
                json!("https://e.com/?a=1;token=x"),
                "the address parameter \"token\"",
            ),
            (
                json!("https://e.com/cb#access_token=x&a=1"),
                "the address parameter \"access_token\"",
            ),
            (
                json!("https://e.com/?%74oken=x"),
                "the address parameter \"token\"",
            ),
            // Services whose address holds the secret in its path.
            (
                json!("https://hooks.slack.com/services/T0/B0/x"),
                "the secret part of a webhook or bot address",
            ),
            (
                json!("https://discord.com/api/webhooks/1/x"),
                "the secret part of a webhook or bot address",
            ),
            (
                json!("https://ptb.discordapp.com/api/webhooks/1/x"),
                "the secret part of a webhook or bot address",
            ),
            (
                json!("https://maker.ifttt.com/trigger/ev/with/key/x"),
                "the secret part of a webhook or bot address",
            ),
            (
                json!("https://api.telegram.org/bot1:x/sendMessage"),
                "the secret part of a webhook or bot address",
            ),
            (
                json!("https://hooks.zapier.com/hooks/catch/1/x/"),
                "the secret part of a webhook or bot address",
            ),
            (
                json!("http://ha.local:8123/api/%77ebhook/x"),
                "the id of a webhook",
            ),
            // In a request body or a command: JSON in a text, form pairs,
            // assignments and options, hyphenated header names, curl's
            // -H without a space, a header as a name and a value, a user
            // name and password in an address of any scheme.
            (json!("{\"api_key\":\"x\"}"), "the value of \"api_key\""),
            (
                json!("{\"Authorization\": \"Bearer x\"}"),
                "the header \"Authorization\"",
            ),
            (json!("token=x&a=1"), "the value of \"token\""),
            (json!("password=x"), "the value of \"password\""),
            (
                json!("GITHUB_TOKEN=x make release"),
                "the value of \"GITHUB_TOKEN\"",
            ),
            (
                json!("wget --password=x https://e.com/f"),
                "the value of \"password\"",
            ),
            (
                json!("curl -H 'X-Auth-Token: x' https://e.com"),
                "the header \"x-auth-token\"",
            ),
            (json!("Private-Token: x"), "the header \"private-token\""),
            (
                json!("curl -HAuthorization:Bearer_x https://e.com"),
                "the header \"authorization\"",
            ),
            (
                json!({ "headers": [{ "name": "Authorization", "value": "Bearer x" }] }),
                "the header \"Authorization\"",
            ),
            (
                json!("psql postgresql://admin:x@db/app"),
                "an address with a user name or password",
            ),
            (
                json!("redis-cli -u redis://:x@host:6379"),
                "an address with a user name or password",
            ),
            (
                json!("wscat -c wss://u:x@e.com/ws"),
                "an address with a user name or password",
            ),
        ] {
            assert_eq!(
                credential_in(&value),
                Some(Found::Credential(said.into())),
                "{value}"
            );
        }
        for plain in [
            json!("https://e.com/path?page=2"),
            json!("https://e.com/api/webhook/"),
            json!("x-authorization: y"),
            json!("Authorization:"),
            json!({ "Cookie": "" }),
            json!({ "cookies": "a=b" }),
            json!("mailto:me@e.com"),
            json!(42),
            // Near misses: a count, a keyboard, a key of a keyboard, an
            // empty value, the services' own pages.
            json!("https://e.com/?tokens=3&keyboard=1&page=2"),
            json!({ "key": "Enter", "hotkey": "a", "monkey": "b", "keys": "x" }),
            json!({ "token": "", "password": null }),
            json!({ "auth": "none" }),
            json!("https://e.com/x#section-2"),
            json!("https://hooks.slack.com/"),
            json!("https://discord.com/api/webhooks/"),
            json!("https://api.telegram.org/"),
            json!("https://e.com/bot1:x/services/a"),
            json!("Check the password: it must be long"),
            json!("echo token: $X; FOO=bar make --tokens=3 a=1&b=2"),
            json!("{\"page\": 2, \"keys\": [\"a\"]}"),
            json!({ "headers": [{ "name": "Accept", "value": "text/plain" }] }),
            json!("see ftp://files.example.com/pub and née://x"),
        ] {
            assert_eq!(credential_in(&plain), None, "{plain}");
        }
    }

    /// SF-2b: every address is read as the runtime's HTTP client reads it
    /// and as a browser does. Each reading is checked; an address only one
    /// of them reads, or that they read with different hosts, is refused on
    /// its own; and a user name before the host counts as written, whatever
    /// either reader makes of it.
    #[test]
    fn an_address_is_read_as_the_client_and_a_browser_read_it() {
        for apart in [
            // Only a browser reads a percent or a letter outside ASCII in
            // a host, or a backslash as a slash.
            "https://ex%61mple.com/hook",
            "https://b\u{fc}cher.example/",
            "https://e.com\\x/?a=1",
            "see https://e.com\\x/ now",
            // A browser reads a number as an IP address.
            "http://0x7f.1/",
            "http://127.1:8123/",
        ] {
            assert_eq!(
                credential_in(&json!(apart)),
                Some(Found::ReadApart),
                "{apart}"
            );
        }
        for (value, said) in [
            // The client keeps the path a browser shortens: the secret part
            // of a webhook address is still in the file.
            (
                "https://hooks.slack.com/services/T0/B0/x/../../../..",
                "the secret part of a webhook or bot address",
            ),
            // Neither reads it, and the user name and password are there.
            (
                "https://user:pw\\@evil.example/",
                "an address with a user name or password",
            ),
            (
                "curl https://a@b@c/",
                "an address with a user name or password",
            ),
        ] {
            assert_eq!(
                credential_in(&json!(value)),
                Some(Found::Credential(said.into())),
                "{value}"
            );
        }
        // Addresses the two read alike, and texts that are no address.
        for plain in [
            "https://E.com./x(1)?page=2#top",
            "[link](https://e.com/a,b;c)",
            "curl https://e.com/a|grep x",
            "https://[::1]:8123/",
            "file:///etc/hosts",
            "protocol:// is not supported",
            "echo https://e.com and mail me@e.org",
            "localhost:8080",
            "Enter",
        ] {
            assert_eq!(credential_in(&json!(plain)), None, "{plain}");
        }
        assert_eq!(
            Found::ReadApart.said(" in \"url\""),
            "holds in \"url\" an address that LaunchDeck's HTTP client and a browser read \
             differently, so a password or token in it cannot be ruled out"
        );
    }
}
