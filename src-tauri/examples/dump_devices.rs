//! Prints the device list Settings > Devices reads, as JSON: the simulator,
//! a connected Launchpad Mini MK3 with its details, features and a notice,
//! and an unsupported model. Used to refresh `examples/devices.json`, the
//! contract fixture the TypeScript schemas and the device tests read.
//!
//! `cargo run --example dump_devices > ../examples/devices.json`

#[path = "../tests/support/devices_scenario.rs"]
mod devices_scenario;

fn main() {
    println!(
        "{}",
        serde_json::to_string_pretty(&devices_scenario::capture()).expect("serialise")
    );
}
