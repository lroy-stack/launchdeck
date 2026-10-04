//! Prints a real Plugins screen payload as JSON. Used to refresh
//! `examples/plugins-overview.json`, the contract fixture the TypeScript
//! schemas are tested against.
//!
//! `cargo run --example dump_plugins > ../examples/plugins-overview.json`

#[path = "../tests/support/plugins_scenario.rs"]
mod plugins_scenario;

fn main() {
    println!(
        "{}",
        serde_json::to_string_pretty(&plugins_scenario::capture()).expect("serialise")
    );
}
