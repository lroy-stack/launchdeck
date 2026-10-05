//! Prints what `app_info` answers (Settings > About) as JSON, with a
//! placeholder for the data folder. Used to refresh `examples/app-info.json`,
//! the contract fixture the TypeScript schemas and the About tests read.
//!
//! `cargo run --example dump_app_info > ../examples/app-info.json`

#[path = "../tests/support/app_info_scenario.rs"]
mod app_info_scenario;

fn main() {
    println!(
        "{}",
        serde_json::to_string_pretty(&app_info_scenario::capture()).expect("serialise")
    );
}
