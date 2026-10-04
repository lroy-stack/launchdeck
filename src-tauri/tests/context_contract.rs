//! Guards the IPC contract between the Rust context status and the TypeScript
//! schemas. The TypeScript tests parse `examples/context-status.json`; this
//! test makes sure that file still has every field the runtime emits.

#[path = "support/context_scenario.rs"]
mod context_scenario;

use std::collections::BTreeSet;

use serde_json::Value;

fn paths(v: &Value, prefix: &str, out: &mut BTreeSet<String>) {
    match v {
        Value::Object(map) => {
            for (key, value) in map {
                let path = if prefix.is_empty() {
                    key.clone()
                } else {
                    format!("{prefix}.{key}")
                };
                out.insert(path.clone());
                paths(value, &path, out);
            }
        }
        Value::Array(items) => {
            for item in items {
                paths(item, &format!("{prefix}[]"), out);
            }
        }
        _ => {}
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn the_context_fixture_has_every_field_the_runtime_emits() {
    let fixture_path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../examples/context-status.json"
    );
    let text = std::fs::read_to_string(fixture_path).expect("examples/context-status.json exists");
    let fixture: Value = serde_json::from_str(&text).expect("fixture is JSON");

    let fresh = context_scenario::capture().await;
    let (mut have, mut want) = (BTreeSet::new(), BTreeSet::new());
    paths(&fixture, "", &mut have);
    paths(&fresh, "", &mut want);
    let missing: Vec<_> = want.difference(&have).collect();
    assert!(
        missing.is_empty(),
        "examples/context-status.json is out of date, missing {missing:?}.\n\
         Refresh it: cargo run --example dump_context > ../examples/context-status.json"
    );
    // It is deterministic: no times, no machine names.
    assert_eq!(fixture, fresh);
}
