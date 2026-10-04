//! Guards the IPC contract between the Rust catalog and the TypeScript
//! schemas. The TypeScript tests parse `examples/action-catalog.json`; this
//! test makes sure that file still has every field the runtime emits, and
//! still shows every kind of provider status and availability.

#[path = "support/catalog_scenario.rs"]
mod catalog_scenario;

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
async fn the_catalog_fixture_has_every_field_the_runtime_emits() {
    let fixture_path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../examples/action-catalog.json"
    );
    let text = std::fs::read_to_string(fixture_path).expect("examples/action-catalog.json exists");
    let fixture: Value = serde_json::from_str(&text).expect("fixture is JSON");

    let fresh = catalog_scenario::capture().await;
    let (mut have, mut want) = (BTreeSet::new(), BTreeSet::new());
    paths(&fixture, "", &mut have);
    paths(&fresh, "", &mut want);
    let missing: Vec<_> = want.difference(&have).collect();
    assert!(
        missing.is_empty(),
        "examples/action-catalog.json is out of date, missing {missing:?}.\n\
         Refresh it: cargo run --example dump_catalog > ../examples/action-catalog.json"
    );

    // The fixture is only useful to the UI tests if it shows each state.
    let statuses: BTreeSet<_> = fixture["providers"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["status"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(
        statuses,
        BTreeSet::from(["disabled".into(), "error".into(), "ready".into()])
    );
    let causes: BTreeSet<_> = fixture["actions"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|a| a["availability"]["cause"].as_str().map(String::from))
        .collect();
    assert_eq!(causes, BTreeSet::from(["provider".into(), "state".into()]));
}
