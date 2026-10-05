//! Writes the artifact contract fixtures into `examples/artifacts/`: a Pack
//! as export writes it, an export preview, two import plans and a refused
//! file, all from the real runtime and the artifact service. The TypeScript
//! schemas are tested against them, and `tests/artifacts_contract.rs` fails
//! when they are no longer what Rust emits.
//!
//! `cargo run --example dump_artifacts`

use std::fs;
use std::path::Path;

#[path = "../tests/support/artifacts_scenario.rs"]
#[allow(dead_code)]
mod artifacts_scenario;

#[tokio::main]
async fn main() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../examples/artifacts");
    fs::create_dir_all(&dir).expect("examples/artifacts can be created");
    for (name, bytes) in artifacts_scenario::capture().await {
        fs::write(dir.join(name), bytes).expect("the fixture is written");
        println!("examples/artifacts/{name}");
    }
}
