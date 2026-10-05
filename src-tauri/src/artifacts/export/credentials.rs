//! Values shaped like a credential (security finding SF-2): whoever receives
//! a file would receive them. Export refuses one in an argument unless the
//! argument is asked of the importer, and in an input's default. A plain
//! address, host or path is fine.

use serde_json::Value;

/// Query parameters that carry a secret, compared without case.
const SECRET_PARAMETERS: [&str; 10] = [
    "token",
    "key",
    "apikey",
    "api_key",
    "access_token",
    "auth",
    "secret",
    "password",
    "sig",
    "signature",
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

/// What in `value`, at any depth, is shaped like a credential, said for a
/// person without repeating it; `None` when nothing is.
pub(super) fn credential_in(value: &Value) -> Option<String> {
    match value {
        Value::String(text) => credential_in_text(text),
        Value::Array(items) => items.iter().find_map(credential_in),
        Value::Object(map) => map.iter().find_map(|(name, v)| {
            let header = SECRET_HEADERS
                .iter()
                .find(|h| name.eq_ignore_ascii_case(h))
                .filter(|_| has_text(v));
            match header {
                Some(_) => Some(format!("the header \"{name}\"")),
                None => credential_in(v),
            }
        }),
        _ => None,
    }
}

fn has_text(value: &Value) -> bool {
    match value {
        Value::String(s) => !s.trim().is_empty(),
        Value::Null => false,
        _ => true,
    }
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
    let secret = url.query_pairs().find(|(name, value)| {
        !value.is_empty()
            && SECRET_PARAMETERS
                .iter()
                .any(|p| name.eq_ignore_ascii_case(p))
    });
    if let Some((name, _)) = secret {
        return Some(format!("the address parameter \"{name}\""));
    }
    webhook_id(&url.path().to_ascii_lowercase()).then(|| "the id of a webhook".to_string())
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
    lower.match_indices(WEBHOOK_PATH).any(|(i, _)| {
        lower[i + WEBHOOK_PATH.len()..]
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
        ] {
            assert_eq!(credential_in(&plain), None, "{plain}");
        }
    }
}
