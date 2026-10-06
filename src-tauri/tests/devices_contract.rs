//! Guards the IPC contract of the device list (`RuntimeState.devices`). The
//! TypeScript tests parse `examples/devices.json`; this test makes sure that
//! file is exactly what Rust emits for the scenario's devices, with their
//! details, features and notice, so a new field fails here until the fixture
//! is refreshed.

#[path = "support/devices_scenario.rs"]
mod devices_scenario;

use serde_json::Value;

fn fixture() -> Value {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../examples/devices.json");
    let text = std::fs::read_to_string(path).expect("examples/devices.json exists");
    serde_json::from_str(&text).expect("fixture is JSON")
}

#[test]
fn the_devices_fixture_is_what_the_scenario_lists() {
    assert_eq!(
        fixture(),
        devices_scenario::capture(),
        "examples/devices.json differs from the scenario.\n\
         Refresh it: cargo run --example dump_devices > ../examples/devices.json"
    );
}

#[test]
fn the_fixture_holds_a_simulator_a_connected_mini_with_a_notice_and_an_unsupported_model() {
    let fixture = fixture();
    let devices = fixture.as_array().expect("a list");
    let status: Vec<&str> = devices
        .iter()
        .map(|d| d["status"].as_str().unwrap())
        .collect();
    assert_eq!(status, ["connected", "connected", "unsupported"]);
    assert_eq!(devices[0]["identity"]["kind"], "virtual");
    assert_eq!(devices[1]["notice"], devices_scenario::NOTICE);
    assert_eq!(devices[1]["details"]["firmware"], "0531");
    assert_eq!(devices[1]["features"]["scrollingText"], true);
    assert!(devices[2].get("notice").is_none(), "skipped when absent");
}
