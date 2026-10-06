//! Values shaped like a credential (security finding SF-2): whoever receives
//! a file would receive them. Export refuses one in an argument unless the
//! argument is asked of the importer, and in an input's default. A plain
//! address, host or path is fine.

use serde_json::Value;

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

/// What in `value`, at any depth, is shaped like a credential, said for a
/// person without repeating it; `None` when nothing is.
pub(super) fn credential_in(value: &Value) -> Option<String> {
    match value {
        Value::String(text) => credential_in_text(text),
        Value::Array(items) => items.iter().find_map(credential_in),
        Value::Object(map) => map
            .iter()
            .find_map(|(name, v)| credential_named(name, v).or_else(|| credential_in(v))),
        _ => None,
    }
}

/// A member or an argument whose name says it holds a secret, when it holds
/// something: an authorization, cookie or API-key header, or a name such as
/// `password`, `X-Auth-Token` or `accessKey`.
pub(super) fn credential_named(name: &str, value: &Value) -> Option<String> {
    if !has_text(value) {
        return None;
    }
    if SECRET_HEADERS.iter().any(|h| name.eq_ignore_ascii_case(h)) {
        return Some(format!("the header \"{name}\""));
    }
    secret_name(name).then(|| format!("the value of \"{name}\""))
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

fn credential_in_text(text: &str) -> Option<String> {
    let lower = text.to_ascii_lowercase();
    if let Some(found) = addresses(text).find_map(|url| credential_in_address(&url)) {
        return Some(found);
    }
    if let Some(header) = SECRET_HEADERS.iter().find(|h| header_line(&lower, h)) {
        return Some(format!("the header \"{header}\""));
    }
    webhook_id(&lower).then(|| "the id of a webhook".to_string())
}

/// The addresses in a text: the whole text when it is one (any scheme), and
/// every http or https address inside it, up to a space or a quote.
fn addresses(text: &str) -> impl Iterator<Item = url::Url> + '_ {
    let lower = text.to_ascii_lowercase();
    let starts: Vec<usize> = ["http://", "https://"]
        .iter()
        .flat_map(|scheme| lower.match_indices(scheme).map(|(i, _)| i))
        .collect();
    let whole = url::Url::parse(text.trim()).ok();
    let inside = starts.into_iter().filter_map(move |start| {
        let rest = &text[start..];
        let end = rest
            .find(|c: char| c.is_whitespace() || matches!(c, '"' | '\'' | '`' | '<' | '>'))
            .unwrap_or(rest.len());
        url::Url::parse(&rest[..end]).ok()
    });
    whole.into_iter().chain(inside)
}

fn credential_in_address(url: &url::Url) -> Option<String> {
    if !url.username().is_empty() || url.password().is_some() {
        return Some("an address with a user name or password".into());
    }
    let pairs = [url.query(), url.fragment()].into_iter().flatten();
    let secret = pairs
        .flat_map(parameters)
        .find(|(name, value)| !value.is_empty() && secret_parameter(name));
    if let Some((name, _)) = secret {
        return Some(format!("the address parameter \"{name}\""));
    }
    let path = percent_decoded(url.path()).to_ascii_lowercase();
    if webhook_id(&path) {
        return Some("the id of a webhook".to_string());
    }
    let host = url.host_str().unwrap_or_default();
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

/// `name:` followed by a value on the same line, as a header is written.
fn header_line(lower: &str, name: &str) -> bool {
    let pattern = format!("{name}:");
    lower.match_indices(&pattern).any(|(i, _)| {
        let starts_word = lower[..i]
            .chars()
            .next_back()
            .is_none_or(|c| !(c.is_ascii_alphanumeric() || c == '-'));
        let value = lower[i + pattern.len()..]
            .lines()
            .next()
            .unwrap_or_default();
        starts_word && !value.trim().is_empty()
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
        ] {
            assert_eq!(credential_in(&value).as_deref(), Some(said), "{value}");
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
        ] {
            assert_eq!(credential_in(&plain), None, "{plain}");
        }
    }
}
