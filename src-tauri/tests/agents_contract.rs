//! Guards the IPC contract between the Rust agents snapshot and the TypeScript
//! schemas. The TypeScript tests parse `examples/agents-snapshot.json`; this
//! test makes sure that file still has every field the runtime emits.

#[path = "support/agents_scenario.rs"]
mod agents_scenario;

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
async fn the_agents_fixture_has_every_field_the_runtime_emits() {
    let fixture_path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../examples/agents-snapshot.json"
    );
    let text = std::fs::read_to_string(fixture_path).expect("examples/agents-snapshot.json exists");
    let fixture: Value = serde_json::from_str(&text).expect("fixture is JSON");

    let fresh = agents_scenario::capture().await;
    let (mut have, mut want) = (BTreeSet::new(), BTreeSet::new());
    paths(&fixture, "", &mut have);
    paths(&fresh, "", &mut want);
    let missing: Vec<_> = want.difference(&have).collect();
    assert!(
        missing.is_empty(),
        "examples/agents-snapshot.json is out of date, missing {missing:?}.\n\
         Refresh it: cargo run --example dump_agents > ../examples/agents-snapshot.json"
    );
}
