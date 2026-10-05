//! Prints what the Plugins screen is told about the example plugins the app
//! ships, as JSON. Used to refresh `examples/plugin-examples.json`, the
//! contract fixture the TypeScript schema is tested against.
//!
//! `cargo run --example dump_plugin_examples > ../examples/plugin-examples.json`

#[path = "../tests/support/plugin_examples_scenario.rs"]
mod plugin_examples_scenario;

fn main() {
    println!(
        "{}",
        serde_json::to_string_pretty(&plugin_examples_scenario::capture()).expect("serialise")
    );
}
