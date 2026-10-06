//! Prints the accepted and rejected arguments of `actions::params::normalize`
//! as JSON. Used to refresh `examples/param-cases.json`, which the
//! TypeScript mirror `normalizeArgs` is tested against.
//!
//! `cargo run --example dump_param_cases > ../examples/param-cases.json`

#[path = "../tests/support/param_cases_scenario.rs"]
mod param_cases_scenario;

fn main() {
    println!(
        "{}",
        serde_json::to_string_pretty(&param_cases_scenario::capture()).expect("serialise")
    );
}
