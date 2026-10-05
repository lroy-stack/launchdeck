//! Prints the default workspace as JSON. Used to refresh `examples/default-workspace.json`,
//! the contract fixture the TypeScript schemas are tested against.
//!
//! `cargo run --example dump_workspace > ../examples/default-workspace.json`
//!
//! The fixture is the Linux workspace on every system, so that refreshing it
//! on a Mac or on Windows writes the same file.

use launchdeck_lib::surfaces::{default_workspace_for, Platform};

fn main() {
    let ws = default_workspace_for(Platform::Linux);
    println!(
        "{}",
        serde_json::to_string_pretty(&ws).expect("serialise workspace")
    );
}
