//! Guards the IPC contract between the Rust Observable Catalog and the
//! TypeScript schemas. The TypeScript tests parse `examples/observables.json`;
//! this test makes sure that file still has every field the runtime emits and
//! still shows every case the wire format has.

#[path = "support/observables_scenario.rs"]
mod observables_scenario;

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
async fn the_observables_fixture_has_every_field_the_runtime_emits() {
    let fixture_path = concat!(env!("CARGO_MANIFEST_DIR"), "/../examples/observables.json");
    let text = std::fs::read_to_string(fixture_path).expect("examples/observables.json exists");
    let fixture: Value = serde_json::from_str(&text).expect("fixture is JSON");

    let fresh = observables_scenario::capture().await;
    let (mut have, mut want) = (BTreeSet::new(), BTreeSet::new());
    paths(&fixture, "", &mut have);
    paths(&fresh, "", &mut want);
    let missing: Vec<_> = want.difference(&have).collect();
    assert!(
        missing.is_empty(),
        "examples/observables.json is out of date, missing {missing:?}.\n\
         Refresh it: cargo run --example dump_observables > ../examples/observables.json"
    );

    // The fixture is only useful to the UI tests if it shows each case.
    let entries = fixture["catalog"]["observables"].as_array().unwrap();
    let statuses: BTreeSet<_> = fixture["catalog"]["providers"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["status"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(
        statuses,
        BTreeSet::from(["disabled".into(), "error".into(), "ready".into()])
    );
    let instance_status: BTreeSet<_> = entries
        .iter()
        .flat_map(|e| e["instances"].as_array().cloned().unwrap_or_default())
        .map(|i| i["status"].as_str().unwrap().to_string())
        .collect();
    assert!(instance_status.contains("known"), "{instance_status:?}");
    assert!(entries
        .iter()
        .any(|e| e["kind"] == "state" && !e["params"].as_array().unwrap_or(&vec![]).is_empty()));
    assert!(entries.iter().any(|e| e["kind"] == "event"));
    for case in ["overlay", "missingLook", "unknown"] {
        assert!(fixture["explain"][case]["rules"].is_array(), "{case}");
    }
    assert!(fixture["explain"]["overlay"]["overlay"].is_object());
    assert_eq!(
        fixture["explain"]["missingLook"]["diagnostics"][0]["kind"],
        "missingLook"
    );
    assert_eq!(fixture["messages"]["catalog"]["catalog"], true);

    // The review of a draft: a saved control has nothing to say and no preview;
    // a changed one has a preview, an issue on each part of a rule that can be
    // wrong in this scenario, and a note of every kind.
    let review = &fixture["review"];
    let clean = &review["clean"];
    assert!(clean["issues"].as_array().unwrap().is_empty());
    assert!(clean["notes"].as_array().unwrap().is_empty());
    assert!(clean["preview"].is_null());
    assert!(clean["limits"]["maxRules"].as_u64().unwrap() > 0);
    let draft = &review["draft"];
    assert!(draft["preview"]["rules"].is_array());
    assert_eq!(draft["preview"]["layer"], "state");
    let issues = draft["issues"].as_array().unwrap();
    let fields: BTreeSet<_> = issues
        .iter()
        .map(|i| i["field"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(
        fields,
        BTreeSet::from(["args", "look", "state", "test"].map(String::from))
    );
    assert!(
        issues
            .iter()
            .any(|i| i["field"] == "args" && i["param"] == "channel"),
        "an argument problem names its argument: {issues:?}"
    );
    let notes: BTreeSet<_> = draft["notes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|n| n["kind"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(
        notes,
        BTreeSet::from(
            [
                "missingLook",
                "shadowed",
                "missingObservable",
                "providerUnavailable"
            ]
            .map(String::from)
        )
    );
}
