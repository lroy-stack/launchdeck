//! Workspace schema 2 to 3: actions become assignments.
//!
//! Version 2 stored a control's `press` and `longPress` as a tagged union of
//! eight shapes. Version 3 stores one shape, `{provider, action, args}`. The
//! conversion runs on the JSON, before typed parsing, so it can accept shapes
//! the current types no longer know. It is structural and lossless: nothing is
//! dropped, and an action it does not recognise is kept under the `legacy`
//! provider, where the editor shows it as missing instead of deleting it.

use serde_json::{json, Map, Value};

/// Convert every control's actions in a workspace document, in place. An
/// action that already is an assignment is left alone, so the step is idempotent.
pub fn upgrade_actions(doc: &mut Value) {
    let Some(surfaces) = doc.get_mut("surfaces").and_then(Value::as_array_mut) else {
        return;
    };
    for surface in surfaces {
        let Some(pages) = surface.get_mut("pages").and_then(Value::as_array_mut) else {
            continue;
        };
        for page in pages {
            let Some(controls) = page.get_mut("controls").and_then(Value::as_array_mut) else {
                continue;
            };
            for control in controls {
                for slot in ["press", "longPress"] {
                    if let Some(action) = control.get_mut(slot) {
                        if action.is_object() && action.get("provider").is_none() {
                            *action = assignment_from_legacy(action);
                        }
                    }
                }
            }
        }
    }
}

fn assignment(provider: &str, action: &str, args: Map<String, Value>) -> Value {
    json!({ "provider": provider, "action": action, "args": Value::Object(args) })
}

fn text(v: &Value, key: &str) -> Option<String> {
    v.get(key)
        .and_then(Value::as_str)
        .map(str::to_string)
        .filter(|s| !s.is_empty())
}

fn put(args: &mut Map<String, Value>, key: &str, value: Option<String>) {
    if let Some(v) = value {
        args.insert(key.to_string(), Value::String(v));
    }
}

fn strings(v: &Value) -> Vec<String> {
    v.as_array()
        .map(|a| {
            a.iter()
                .filter_map(|x| x.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default()
}

/// Shortcut keys as one piece of text: `["mod","c"]` becomes `mod+c`.
fn keys_text(keys: &Value) -> Option<String> {
    match keys {
        Value::String(s) => Some(s.clone()),
        Value::Array(_) => Some(strings(keys).join("+")),
        _ => None,
    }
}

/// Launch arguments, one per line.
fn lines_text(args: &Value) -> Option<String> {
    let text = match args {
        Value::String(s) => s.clone(),
        Value::Array(_) => strings(args).join("\n"),
        _ => return None,
    };
    (!text.trim().is_empty()).then_some(text)
}

fn assignment_from_legacy(old: &Value) -> Value {
    let mut args = Map::new();
    match old.get("type").and_then(Value::as_str) {
        Some("keyboardShortcut") => {
            put(&mut args, "keys", old.get("keys").and_then(keys_text));
            assignment("system", "keyboardShortcut", args)
        }
        Some("openUrl") => {
            put(&mut args, "url", text(old, "url"));
            assignment("system", "openUrl", args)
        }
        Some("launchApplication") => {
            put(&mut args, "app", text(old, "app"));
            put(&mut args, "args", old.get("args").and_then(lines_text));
            assignment("system", "launchApplication", args)
        }
        Some("shellCommand") => {
            put(&mut args, "command", text(old, "command"));
            assignment("shell", "execute", args)
        }
        Some("httpRequest") => {
            put(
                &mut args,
                "method",
                text(old, "method").map(|m| m.to_ascii_uppercase()),
            );
            put(&mut args, "url", text(old, "url"));
            put(&mut args, "body", text(old, "body"));
            assignment("http", "request", args)
        }
        Some("openSurface") => {
            put(&mut args, "surfaceId", text(old, "surfaceId"));
            put(&mut args, "pageId", text(old, "pageId"));
            assignment("navigation", "openSurface", args)
        }
        Some("openPage") => {
            put(&mut args, "pageId", text(old, "pageId"));
            assignment("navigation", "openPage", args)
        }
        Some("plugin") => {
            let provider = text(old, "plugin").unwrap_or_default();
            let action = text(old, "action").unwrap_or_default();
            let mut args = old
                .get("args")
                .and_then(Value::as_object)
                .cloned()
                .unwrap_or_default();
            // The System plugin used to be called with lists; its settings are text now.
            if provider == "system" {
                if let Some(keys) = args.get("keys").and_then(keys_text) {
                    args.insert("keys".into(), Value::String(keys));
                }
                if args.contains_key("args") {
                    match args.get("args").and_then(lines_text) {
                        Some(lines) => args.insert("args".into(), Value::String(lines)),
                        None => args.remove("args"),
                    };
                }
            }
            assignment(&provider, &action, args)
        }
        other => {
            // Unknown: keep everything it said, where the editor can show it.
            let mut kept = old.as_object().cloned().unwrap_or_default();
            kept.remove("type");
            assignment("legacy", other.unwrap_or("unknown"), kept)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn convert(old: Value) -> Value {
        assignment_from_legacy(&old)
    }

    #[test]
    fn each_legacy_shape_maps_to_its_provider_action() {
        assert_eq!(
            convert(json!({"type":"keyboardShortcut","keys":["mod","shift","p"]})),
            json!({"provider":"system","action":"keyboardShortcut","args":{"keys":"mod+shift+p"}})
        );
        assert_eq!(
            convert(json!({"type":"openUrl","url":"https://github.com"})),
            json!({"provider":"system","action":"openUrl","args":{"url":"https://github.com"}})
        );
        assert_eq!(
            convert(json!({"type":"launchApplication","app":"code","args":["--new-window","."]})),
            json!({"provider":"system","action":"launchApplication",
                   "args":{"app":"code","args":"--new-window\n."}})
        );
        assert_eq!(
            convert(json!({"type":"launchApplication","app":"code","args":[]})),
            json!({"provider":"system","action":"launchApplication","args":{"app":"code"}})
        );
        assert_eq!(
            convert(json!({"type":"shellCommand","command":"git pull"})),
            json!({"provider":"shell","action":"execute","args":{"command":"git pull"}})
        );
        assert_eq!(
            convert(
                json!({"type":"httpRequest","method":"post","url":"https://a.b/x","body":null})
            ),
            json!({"provider":"http","action":"request","args":{"method":"POST","url":"https://a.b/x"}})
        );
        assert_eq!(
            convert(json!({"type":"openSurface","surfaceId":"dev","pageId":null})),
            json!({"provider":"navigation","action":"openSurface","args":{"surfaceId":"dev"}})
        );
        assert_eq!(
            convert(json!({"type":"openSurface","surfaceId":"dev","pageId":"git"})),
            json!({"provider":"navigation","action":"openSurface","args":{"surfaceId":"dev","pageId":"git"}})
        );
        assert_eq!(
            convert(json!({"type":"openPage","pageId":"home"})),
            json!({"provider":"navigation","action":"openPage","args":{"pageId":"home"}})
        );
    }

    #[test]
    fn plugin_actions_keep_their_arguments() {
        assert_eq!(
            convert(
                json!({"type":"plugin","plugin":"demo","action":"startTask","args":{"agent":"alpha"}})
            ),
            json!({"provider":"demo","action":"startTask","args":{"agent":"alpha"}})
        );
        for none in [json!(null), json!([1]), json!("x")] {
            assert_eq!(
                convert(json!({"type":"plugin","plugin":"acme.x","action":"go","args":none})),
                json!({"provider":"acme.x","action":"go","args":{}})
            );
        }
        // A system call written with lists gets the text form the schema expects.
        assert_eq!(
            convert(
                json!({"type":"plugin","plugin":"system","action":"keyboardShortcut",
                           "args":{"keys":["mod","c"]}})
            ),
            json!({"provider":"system","action":"keyboardShortcut","args":{"keys":"mod+c"}})
        );
    }

    #[test]
    fn an_unknown_action_is_kept_not_dropped() {
        let out = convert(json!({"type":"teleport","where":"moon"}));
        assert_eq!(
            out,
            json!({"provider":"legacy","action":"teleport","args":{"where":"moon"}})
        );
        let out = convert(json!({"nonsense": true}));
        assert_eq!(out["provider"], "legacy");
        assert_eq!(out["args"]["nonsense"], true);
    }

    #[test]
    fn upgrading_a_document_is_idempotent_and_touches_only_actions() {
        let mut doc = json!({
            "schemaVersion": 2,
            "surfaces": [{ "id": "s", "pages": [{ "id": "p", "controls": [
                { "id": "a", "label": "A",
                  "press": {"type":"openUrl","url":"https://a.b"},
                  "longPress": {"type":"openPage","pageId":"p"} },
                { "id": "b", "label": "B", "press": null }
            ]}]}]
        });
        upgrade_actions(&mut doc);
        let controls = &doc["surfaces"][0]["pages"][0]["controls"];
        assert_eq!(controls[0]["press"]["provider"], "system");
        assert_eq!(controls[0]["longPress"]["provider"], "navigation");
        assert!(controls[1]["press"].is_null());
        assert_eq!(controls[0]["label"], "A");
        let once = doc.clone();
        upgrade_actions(&mut doc);
        assert_eq!(doc, once);
    }
}
