//! Prints a real Observable Catalog, the messages the UI hears about it and
//! three explanations of a control, as JSON. Used to refresh
//! `examples/observables.json`, the contract fixture the TypeScript schemas
//! are tested against.
//!
//! `cargo run --example dump_observables > ../examples/observables.json`

#[path = "../tests/support/observables_scenario.rs"]
mod observables_scenario;

#[tokio::main]
async fn main() {
    println!(
        "{}",
        serde_json::to_string_pretty(&observables_scenario::capture().await).expect("serialise")
    );
}
