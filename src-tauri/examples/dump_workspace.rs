//! Prints the default workspace as JSON. Used to refresh `examples/default-workspace.json`,
//! the contract fixture the TypeScript schemas are tested against.
//!
//! `cargo run --example dump_workspace > ../examples/default-workspace.json`

fn main() {
    let ws = launchdeck_lib::surfaces::default_workspace();
    println!(
        "{}",
        serde_json::to_string_pretty(&ws).expect("serialise workspace")
    );
}
