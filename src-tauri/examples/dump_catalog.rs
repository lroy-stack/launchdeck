//! Prints a real Action Library catalog as JSON. Used to refresh
//! `examples/action-catalog.json`, the contract fixture the TypeScript
//! schemas are tested against.
//!
//! `cargo run --example dump_catalog > ../examples/action-catalog.json`

#[path = "../tests/support/catalog_scenario.rs"]
mod catalog_scenario;

#[tokio::main]
async fn main() {
    println!(
        "{}",
        serde_json::to_string_pretty(&catalog_scenario::capture().await).expect("serialise")
    );
}
