//! Guards the IPC contract between the artifact commands and the
//! TypeScript schemas. The TypeScript tests parse the files of
//! `examples/artifacts/`; this test makes sure each is still exactly what
//! the runtime and the artifact service emit (they are deterministic: a
//! fixed app version and plan id, no times, no machine names), and that the
//! folder holds nothing else.

#[path = "support/artifacts_scenario.rs"]
#[allow(dead_code)]
mod artifacts_scenario;

use std::collections::BTreeSet;
use std::path::PathBuf;

use launchdeck_lib::artifacts::inspect;

fn folder() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../examples/artifacts")
}

const REFRESH: &str = "Refresh them: cargo run --example dump_artifacts";

#[tokio::test(flavor = "multi_thread")]
async fn the_artifact_fixtures_are_what_rust_emits() {
    let fresh = artifacts_scenario::capture().await;
    for (name, bytes) in &fresh {
        let path = folder().join(name);
        let fixture = std::fs::read(&path)
            .unwrap_or_else(|e| panic!("examples/artifacts/{name}: {e}. {REFRESH}"));
        assert!(
            fixture == *bytes,
            "examples/artifacts/{name} is out of date. {REFRESH}\n--- emitted now:\n{}",
            String::from_utf8_lossy(bytes)
        );
    }
    let expected: BTreeSet<&str> = fresh.iter().map(|(name, _)| *name).collect();
    let present: BTreeSet<String> = std::fs::read_dir(folder())
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .collect();
    let present: BTreeSet<&str> = present.iter().map(String::as_str).collect();
    assert_eq!(present, expected, "examples/artifacts holds other files");
}

#[test]
fn the_fixture_pack_is_a_valid_file() {
    let bytes = std::fs::read(folder().join("development.ldpack")).unwrap();
    let parsed = inspect(&bytes, "development.ldpack").unwrap_or_else(|r| panic!("{r:?}"));
    assert_eq!(
        parsed.manifest.created_with.version,
        artifacts_scenario::APP_VERSION
    );
    assert_eq!(parsed.content.surfaces.len(), 1);
    assert_eq!(parsed.content.context_rules.len(), 1);
    assert!(parsed.warnings.is_empty(), "{:?}", parsed.warnings);
}
