//! Prints real Auto Context statuses as JSON. Used to refresh
//! `examples/context-status.json`, the contract fixture the TypeScript schemas
//! and the interface's tests are run against.
//!
//! `cargo run --example dump_context > ../examples/context-status.json`

#[path = "../tests/support/context_scenario.rs"]
mod context_scenario;

#[tokio::main]
async fn main() {
    println!(
        "{}",
        serde_json::to_string_pretty(&context_scenario::capture().await).expect("serialise")
    );
}
