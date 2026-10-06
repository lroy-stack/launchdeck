//! Accepted and rejected arguments for `actions::params::normalize`, as Rust
//! answers them. `src/lib/actions.ts` mirrors `normalize` (`normalizeArgs`)
//! so a form can react while the person types; its test replays every row
//! of `examples/param-cases.json`, and the contract test here makes sure the
//! file is what this table gives. Shared by the `dump_param_cases` example
//! and the contract test.

use launchdeck_lib::actions::params::{normalize, ParamDef, RefTarget, MAX_TEXT_VALUE};
use serde_json::{json, Value};

/// The longest reference id `normalize` takes, in bytes.
const MAX_REF_ID: usize = 128;

fn number(name: &str, label: &str, min: Option<f64>, max: Option<f64>) -> ParamDef {
    let mut d = ParamDef::number(name, label);
    d.min = min;
    d.max = max;
    d
}

fn text(name: &str, label: &str) -> ParamDef {
    ParamDef::string(name, label)
}

fn methods() -> ParamDef {
    ParamDef::choice("method", "Method", &[("GET", "GET"), ("POST", "POST")])
}

/// One row: a name for failure messages, the definitions, the arguments.
fn cases() -> Vec<(&'static str, Vec<ParamDef>, Value)> {
    let long = |n: usize| "x".repeat(n);
    vec![
        // Required and blank values.
        (
            "required text absent",
            vec![text("url", "Link").required()],
            json!({}),
        ),
        (
            "required text null",
            vec![text("url", "Link").required()],
            json!({ "url": null }),
        ),
        (
            "required text blank",
            vec![text("url", "Link").required()],
            json!({ "url": " \t\n " }),
        ),
        (
            "blank is what Rust trims: U+0085 and U+3000",
            vec![text("url", "Link").required()],
            json!({ "url": "\u{85}\u{3000}" }),
        ),
        (
            "U+FEFF is not blank to Rust",
            vec![text("url", "Link").required()],
            json!({ "url": "\u{FEFF}" }),
        ),
        (
            "required text given",
            vec![text("url", "Link").required()],
            json!({ "url": "https://a.b" }),
        ),
        (
            "optional blank text left out",
            vec![text("cwd", "Folder")],
            json!({ "cwd": "  " }),
        ),
        // Defaults.
        (
            "a default fills an absent value",
            vec![methods().default_value(json!("GET")).required()],
            json!({}),
        ),
        (
            "a default fills a blank text",
            vec![text("text", "Text").default_value(json!("continue"))],
            json!({ "text": "" }),
        ),
        (
            "a default number",
            vec![number("slot", "Slot", Some(0.0), Some(7.0)).default_value(json!(3))],
            json!({ "slot": null }),
        ),
        (
            "a given value wins over the default",
            vec![methods().default_value(json!("GET"))],
            json!({ "method": "POST" }),
        ),
        // String.
        (
            "text must be text",
            vec![text("url", "Link")],
            json!({ "url": 5 }),
        ),
        (
            "text at maxLength",
            vec![text("url", "Link").max_length(5)],
            json!({ "url": "abcde" }),
        ),
        (
            "text over maxLength",
            vec![text("url", "Link").max_length(5)],
            json!({ "url": "abcdef" }),
        ),
        (
            "maxLength counts characters",
            vec![text("url", "Link").max_length(5)],
            json!({ "url": "ééééé" }),
        ),
        (
            "text at the 16384 limit",
            vec![text("body", "Body")],
            json!({ "body": long(MAX_TEXT_VALUE) }),
        ),
        (
            "text over the 16384 limit",
            vec![text("body", "Body")],
            json!({ "body": long(MAX_TEXT_VALUE + 1) }),
        ),
        (
            "a maxLength above the limit is capped at 16384",
            vec![text("body", "Body").max_length(MAX_TEXT_VALUE + 100)],
            json!({ "body": long(MAX_TEXT_VALUE + 1) }),
        ),
        // Number.
        (
            "number must be a number",
            vec![number("slot", "Slot", None, None)],
            json!({ "slot": "5" }),
        ),
        (
            "number may be a decimal",
            vec![number("gain", "Gain", Some(-1.5), Some(2.5))],
            json!({ "gain": 0.25 }),
        ),
        (
            "whole number",
            vec![number("slot", "Slot", None, None).integer()],
            json!({ "slot": 2.5 }),
        ),
        (
            "whole number given",
            vec![number("slot", "Slot", None, None).integer()],
            json!({ "slot": 2 }),
        ),
        (
            "between min and max",
            vec![number("slot", "Slot", Some(0.0), Some(7.0))],
            json!({ "slot": 9 }),
        ),
        (
            "between with decimals",
            vec![number("gain", "Gain", Some(-1.5), Some(2.5))],
            json!({ "gain": -2 }),
        ),
        (
            "at the min",
            vec![number("slot", "Slot", Some(0.0), Some(7.0))],
            json!({ "slot": 0 }),
        ),
        (
            "at the max",
            vec![number("slot", "Slot", Some(0.0), Some(7.0))],
            json!({ "slot": 7 }),
        ),
        (
            "min only",
            vec![number("count", "Count", Some(1.0), None)],
            json!({ "count": 0 }),
        ),
        (
            "max only",
            vec![number("count", "Count", None, Some(10.0))],
            json!({ "count": 11 }),
        ),
        // Boolean.
        (
            "boolean must be on or off",
            vec![ParamDef::boolean("loud", "Loud")],
            json!({ "loud": "yes" }),
        ),
        (
            "boolean given",
            vec![ParamDef::boolean("loud", "Loud")],
            json!({ "loud": false }),
        ),
        (
            "required boolean false is given",
            vec![ParamDef::boolean("loud", "Loud").required()],
            json!({ "loud": false }),
        ),
        // Enum.
        (
            "enum must be listed",
            vec![methods()],
            json!({ "method": "TRACE" }),
        ),
        ("enum given", vec![methods()], json!({ "method": "GET" })),
        (
            "enum is not text that is blank",
            vec![methods().required()],
            json!({ "method": "" }),
        ),
        // Reference.
        (
            "reference blank is missing",
            vec![ParamDef::reference("surfaceId", "Surface", RefTarget::Surface).required()],
            json!({ "surfaceId": "  " }),
        ),
        (
            "reference at the 128 limit",
            vec![ParamDef::reference(
                "surfaceId",
                "Surface",
                RefTarget::Surface,
            )],
            json!({ "surfaceId": long(MAX_REF_ID) }),
        ),
        (
            "reference over the 128 limit",
            vec![ParamDef::reference(
                "surfaceId",
                "Surface",
                RefTarget::Surface,
            )],
            json!({ "surfaceId": long(MAX_REF_ID + 1) }),
        ),
        (
            "the 128 limit counts bytes",
            vec![ParamDef::reference(
                "surfaceId",
                "Surface",
                RefTarget::Surface,
            )],
            json!({ "surfaceId": "é".repeat(65) }),
        ),
        (
            "reference must be text",
            vec![ParamDef::reference("pageId", "Page", RefTarget::Page)],
            json!({ "pageId": 4 }),
        ),
        // Json.
        (
            "json null is missing",
            vec![ParamDef::json("payload", "Payload").required()],
            json!({ "payload": null }),
        ),
        (
            "json value given",
            vec![ParamDef::json("payload", "Payload")],
            json!({ "payload": { "a": [1, 2] } }),
        ),
        (
            "json empty text is a value",
            vec![ParamDef::json("payload", "Payload").required()],
            json!({ "payload": "" }),
        ),
        // Unknown keys, and every problem at once.
        (
            "unknown keys",
            vec![text("url", "Link")],
            json!({ "url": "a", "extra": 1, "zzz": null }),
        ),
        ("no definitions, no arguments", vec![], json!({})),
        (
            "every problem, unknown keys first, then in definition order",
            vec![
                text("url", "Link").required().max_length(20),
                methods().default_value(json!("GET")),
                number("slot", "Slot", Some(0.0), Some(7.0)).integer(),
                ParamDef::boolean("loud", "Loud"),
            ],
            json!({ "slot": 9.5, "loud": 1, "other": true }),
        ),
    ]
}

/// The table as `examples/param-cases.json` holds it: for each row, what
/// `normalize` stores (`expectedArgs`, `null` when it refuses) and every
/// issue with its parameter, kind and message.
pub fn capture() -> Value {
    let rows: Vec<Value> = cases()
        .into_iter()
        .map(|(name, params, args)| {
            let (expected_args, issues) = match normalize(&params, &args) {
                Ok(stored) => (stored, vec![]),
                Err(issues) => (Value::Null, issues),
            };
            json!({
                "name": name,
                "params": params,
                "args": args,
                "expectedArgs": expected_args,
                "expectedIssues": issues
                    .iter()
                    .map(|i| json!({ "param": i.param, "kind": i.kind, "message": i.message }))
                    .collect::<Vec<_>>(),
            })
        })
        .collect();
    json!({ "cases": rows })
}
