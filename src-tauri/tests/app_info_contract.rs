//! Guards the IPC contract of `app_info` (Settings > About). The TypeScript
//! tests parse `examples/app-info.json`; this test makes sure that file still
//! has every field the command emits, and that it holds no real data folder.

#[path = "support/app_info_scenario.rs"]
mod app_info_scenario;

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

#[test]
fn the_app_info_fixture_has_every_field_the_command_emits() {
    let fixture_path = concat!(env!("CARGO_MANIFEST_DIR"), "/../examples/app-info.json");
    let text = std::fs::read_to_string(fixture_path).expect("examples/app-info.json exists");
    let fixture: Value = serde_json::from_str(&text).expect("fixture is JSON");

    let fresh = app_info_scenario::capture();
    let (mut have, mut want) = (BTreeSet::new(), BTreeSet::new());
    paths(&fixture, "", &mut have);
    paths(&fresh, "", &mut want);
    let missing: Vec<_> = want.difference(&have).collect();
    assert!(
        missing.is_empty(),
        "examples/app-info.json is out of date, missing {missing:?}.\n\
         Refresh it: cargo run --example dump_app_info > ../examples/app-info.json"
    );
    assert_eq!(fixture["dataFolder"], app_info_scenario::DATA_FOLDER);
    assert_eq!(fixture["channel"], "alpha");
}
