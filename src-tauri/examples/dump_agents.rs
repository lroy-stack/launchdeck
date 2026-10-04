//! Prints a real agents snapshot as JSON. Used to refresh
//! `examples/agents-snapshot.json`, the contract fixture the TypeScript schemas
//! are tested against.
//!
//! `cargo run --example dump_agents > ../examples/agents-snapshot.json`

#[path = "../tests/support/agents_scenario.rs"]
mod agents_scenario;

#[tokio::main]
async fn main() {
    let snap = agents_scenario::capture().await;
    println!(
        "{}",
        serde_json::to_string_pretty(&snap).expect("serialise")
    );
}
