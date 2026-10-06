//! The service on a headless runtime: one pending import from inspect to
//! apply, and exported files that never replace one another. The hostile
//! cases, the apply stage on a real store and the round trip are in
//! `tests/artifacts_security.rs` and `tests/artifacts_roundtrip.rs`.

use std::fs;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use serde_json::json;

use super::*;
use crate::artifacts::testkit;
use crate::core::model::Workspace;

fn request(surface: &str) -> ExportRequest {
    serde_json::from_value(json!({
        "kind": "pack",
        "surfaceIds": [surface],
        "metadata": { "name": "Studio", "version": "1.0.0" }
    }))
    .unwrap()
}

async fn service(dir: &Path) -> ArtifactService {
    ArtifactService::new(testkit::headless().await, dir.to_path_buf()).with_app_version("9.9.9")
}

fn planned(outcome: InspectOutcome) -> ImportPlan {
    match outcome {
        InspectOutcome::Plan { plan } => *plan,
        InspectOutcome::Rejected { rejection } => panic!("{rejection:?}"),
    }
}

#[tokio::test]
async fn an_import_goes_from_the_file_to_the_workspace_once() {
    let dir = tempfile::tempdir().unwrap();
    let artifacts = service(dir.path()).await;
    let saved = artifacts.export_save(&request("development")).unwrap();
    let bytes = fs::read(dir.path().join(&saved.file_name)).unwrap();

    let plan = planned(artifacts.inspect(&bytes, &saved.file_name));
    assert_eq!(
        plan.decisions, plan.suggested,
        "planned as the view proposes"
    );
    assert!(plan.ready, "{:?}", plan.blockers);
    assert_eq!(plan.source.content_digest, saved.content_digest);

    let PreviewOutcome::Plan { plan: again } = artifacts.preview(&plan.plan_id, &plan.decisions)
    else {
        panic!("the pending import is there");
    };
    assert_eq!(
        (&again.plan_id, &again.plan_digest),
        (&plan.plan_id, &plan.plan_digest)
    );

    let outcome = artifacts.apply(&plan.plan_id, &plan.decisions, &plan.plan_digest);
    let ApplyOutcome::Applied { report } = outcome else {
        panic!("{outcome:?}");
    };
    assert_eq!(report.surfaces_added, ["development-2"]);
    let ws = artifacts.runtime.snapshot().workspace;
    let origin = ws.surface("development-2").unwrap().imported_from.clone();
    assert_eq!(origin.unwrap().content_digest, saved.content_digest);

    let again = artifacts.apply(&plan.plan_id, &plan.decisions, &plan.plan_digest);
    assert_eq!(again, ApplyOutcome::Expired, "applied once, then gone");
}

#[tokio::test]
async fn a_new_file_replaces_the_pending_import_and_discard_forgets_it() {
    let dir = tempfile::tempdir().unwrap();
    let artifacts = service(dir.path()).await;
    let saved = artifacts.export_save(&request("development")).unwrap();
    let bytes = fs::read(dir.path().join(&saved.file_name)).unwrap();
    let decisions = ImportDecisions::default();

    let first = planned(artifacts.inspect(&bytes, "a.ldpack"));
    let refused = artifacts.inspect(b"PK\x03\x04", "b.ldpack");
    assert!(matches!(refused, InspectOutcome::Rejected { .. }));
    assert_eq!(
        artifacts.preview(&first.plan_id, &decisions),
        PreviewOutcome::Expired,
        "even a refused file replaces the pending import"
    );

    let second = planned(artifacts.inspect(&bytes, "a.ldpack"));
    assert_ne!(second.plan_id, first.plan_id);
    artifacts.discard(&first.plan_id);
    assert!(matches!(
        artifacts.preview(&second.plan_id, &decisions),
        PreviewOutcome::Plan { .. }
    ));
    artifacts.discard(&second.plan_id);
    assert_eq!(
        artifacts.preview(&second.plan_id, &decisions),
        PreviewOutcome::Expired
    );
}

/// A long inspect holds no lock another command waits for: the pending
/// import is taken only to forget it, and again to store what was planned.
/// Here the inspect waits for the runtime, which someone else holds.
#[tokio::test(flavor = "multi_thread")]
async fn a_long_inspect_blocks_no_other_command() {
    let dir = tempfile::tempdir().unwrap();
    let artifacts = Arc::new(service(dir.path()).await);
    let saved = artifacts.export_save(&request("development")).unwrap();
    let bytes = fs::read(dir.path().join(&saved.file_name)).unwrap();

    let (held, holding) = mpsc::channel();
    let (release, released) = mpsc::channel::<()>();
    let runtime = artifacts.runtime.clone();
    let holder = thread::spawn(move || {
        let _ = runtime.commit_workspace(|_| {
            held.send(()).unwrap();
            released.recv().ok();
            Err::<Workspace, ()>(())
        });
    });
    holding.recv().unwrap();
    let reading = {
        let artifacts = artifacts.clone();
        thread::spawn(move || artifacts.inspect(&bytes, "a.ldpack"))
    };
    thread::sleep(Duration::from_millis(200));
    let (done, answered) = mpsc::channel();
    let other = {
        let artifacts = artifacts.clone();
        thread::spawn(move || {
            artifacts.discard("another");
            done.send(()).unwrap();
        })
    };
    let answered = answered.recv_timeout(Duration::from_secs(2)).is_ok();
    release.send(()).unwrap();
    holder.join().unwrap();
    let plan = planned(reading.join().unwrap());
    other.join().unwrap();
    assert!(answered, "another command waited for the inspect");
    assert!(
        matches!(
            artifacts.preview(&plan.plan_id, &plan.decisions),
            PreviewOutcome::Plan { .. }
        ),
        "the inspect stored what it planned"
    );
}

/// Of two files opened one after the other, the later one is pending,
/// whichever is planned first.
#[tokio::test]
async fn the_file_opened_last_is_the_pending_one() {
    let dir = tempfile::tempdir().unwrap();
    let artifacts = service(dir.path()).await;
    let saved = artifacts.export_save(&request("development")).unwrap();
    let bytes = fs::read(dir.path().join(&saved.file_name)).unwrap();
    let first = artifacts.opening();
    let second = planned(artifacts.inspect(&bytes, "b.ldpack"));
    let late = planned(artifacts.inspect_opened(first, &bytes, "a.ldpack"));
    let decisions = ImportDecisions::default();
    assert_eq!(
        artifacts.preview(&late.plan_id, &decisions),
        PreviewOutcome::Expired,
        "a file opened earlier never replaces a later one"
    );
    assert!(matches!(
        artifacts.preview(&second.plan_id, &decisions),
        PreviewOutcome::Plan { .. }
    ));
}

#[tokio::test]
async fn an_export_never_replaces_a_file() {
    let dir = tempfile::tempdir().unwrap();
    let artifacts = service(dir.path()).await;
    let preview = artifacts.export_preview(&request("development"));
    assert!(preview.errors.is_empty(), "{:?}", preview.errors);
    assert_eq!(preview.file_name, "studio-1.0.0.ldpack");

    let first = artifacts.export_save(&request("development")).unwrap();
    assert_eq!(first.file_name, "studio-1.0.0.ldpack");
    assert_eq!(first.folder, dir.path().display().to_string());
    assert_eq!(first.content_digest, preview.content_digest);
    assert_eq!(first.size_bytes, preview.size_bytes);
    let written = fs::read(dir.path().join(&first.file_name)).unwrap();
    assert_eq!(written.len(), first.size_bytes);

    let second = artifacts.export_save(&request("development")).unwrap();
    assert_eq!(second.file_name, "studio-1.0.0-2.ldpack");
    // Someone else's temporary file, which an atomic write would clobber.
    fs::write(dir.path().join("studio-1.0.0-3.ldpack.tmp"), b"theirs").unwrap();
    let third = artifacts.export_save(&request("development")).unwrap();
    assert_eq!(third.file_name, "studio-1.0.0-4.ldpack");
    assert_eq!(
        fs::read(dir.path().join("studio-1.0.0-3.ldpack.tmp")).unwrap(),
        b"theirs"
    );
    assert_eq!(
        fs::read(dir.path().join(&first.file_name)).unwrap(),
        written
    );
    let last = lock(&artifacts.last_export).clone();
    assert_eq!(last, Some(dir.path().join("studio-1.0.0-4.ldpack")));
}

/// A link where the temporary file would go (an archive unpacked in
/// Downloads can leave one) is a taken name too, even when it points at
/// nothing: `write_atomic` would follow it and write elsewhere.
#[cfg(unix)]
#[tokio::test]
async fn an_export_never_writes_through_a_link_left_at_its_temporary_file() {
    let dir = tempfile::tempdir().unwrap();
    let elsewhere = tempfile::tempdir().unwrap();
    let target = elsewhere.path().join("planted");
    let link = dir.path().join("studio-1.0.0.ldpack.tmp");
    std::os::unix::fs::symlink(&target, &link).unwrap();
    let artifacts = service(dir.path()).await;

    let saved = artifacts.export_save(&request("development")).unwrap();
    assert_eq!(saved.file_name, "studio-1.0.0-2.ldpack");
    assert!(!target.exists(), "the export was written through the link");
    assert!(fs::symlink_metadata(&link)
        .unwrap()
        .file_type()
        .is_symlink());
    assert!(!dir.path().join("studio-1.0.0.ldpack").exists());
}

#[tokio::test]
async fn an_export_that_is_refused_writes_nothing_and_there_is_nothing_to_show() {
    let dir = tempfile::tempdir().unwrap();
    let artifacts = service(dir.path()).await;
    let error = artifacts.export_save(&request("nowhere")).unwrap_err();
    assert!(error.message.ends_with('.'), "{error:?}");
    assert_eq!(error.detail, None);
    assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 0);
    let nothing = artifacts.show_export().unwrap_err();
    assert_eq!(nothing.message, "No file has been exported yet.");
}

#[test]
fn other_names_go_before_the_extension() {
    let names: Vec<String> = candidates("studio-1.0.0.ldpack").take(3).collect();
    assert_eq!(
        names,
        [
            "studio-1.0.0.ldpack",
            "studio-1.0.0-2.ldpack",
            "studio-1.0.0-3.ldpack"
        ]
    );
    assert_eq!(candidates("x").nth(1).unwrap(), "x-2");
    assert_eq!(candidates("x").count(), 999);
}

#[test]
fn a_file_error_reads_as_a_sentence_and_what_the_system_said() {
    let e = FileError {
        message: "Not saved.".into(),
        detail: Some("Permission denied (os error 13)".into()),
    };
    assert_eq!(
        e.to_string(),
        "Not saved. (Permission denied (os error 13))"
    );
}
