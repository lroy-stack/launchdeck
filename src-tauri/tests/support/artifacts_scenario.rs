//! Real payloads of the artifact commands, from the real runtime and the
//! artifact service. Shared by the `dump_artifacts` example (which writes
//! `examples/artifacts/`) and the contract test (which checks those files
//! are still what Rust emits), so the TypeScript schemas are checked
//! against the real thing:
//!
//! * `development.ldpack`: the Development starter and a context rule for
//!   Visual Studio Code (with its identities on the three systems), exported
//!   as a Pack;
//! * `export-preview.json`: what `artifacts_export_preview` says about it;
//! * `import-plan.json`: what `artifacts_inspect` returns for that file in
//!   the starter workspace: the plan, its rule offered and not selected;
//! * `import-plan-missing.json`: the same for a Pack whose pad uses the
//!   Counter example plugin, in a workspace where it is not installed;
//! * `inspect-rejected.json`: what `artifacts_inspect` returns for
//!   `development.ldpack` with one character of its content changed.
//!
//! Deterministic: the starter workspace as on Linux, X11 identities (a
//! scripted observer), the virtual device only, the app version 9.9.9 and
//! a fixed plan id.

use std::fs;
use std::path::Path;
use std::sync::Arc;

use launchdeck_lib::artifacts::{ArtifactService, ExportRequest, InspectOutcome};
use launchdeck_lib::context::identity::X11;
use launchdeck_lib::context::observer::scripted::ScriptedObserver;
use launchdeck_lib::context::{ApplicationIdentity, ContextRule, ContextSettings, ObserverStatus};
use launchdeck_lib::core::model::{Control, Workspace};
use launchdeck_lib::core::runtime::{NullEmitter, Runtime, RuntimeConfig};
use launchdeck_lib::plugins::keyboard::{Combo, Keyboard};
use launchdeck_lib::plugins::ActionError;
use launchdeck_lib::surfaces::{default_workspace_for, Platform};
use serde::Serialize;
use serde_json::{json, Value};

/// `createdWith.version` in every file here, so a release changes none.
pub const APP_VERSION: &str = "9.9.9";
/// The id of every plan here.
pub const PLAN_ID: &str = "0123456789abcdef0123456789abcdef";

struct NoKeyboard;
impl Keyboard for NoKeyboard {
    fn press(&self, _combo: &Combo) -> Result<(), ActionError> {
        Ok(())
    }
}

/// A runtime and its artifact service: the starter workspace, X11
/// identities, and the plugins in `plugins` (none when it is `None`).
/// Exports go to `exports`.
async fn world(plugins: Option<&Path>, exports: &Path) -> (Arc<Runtime>, ArtifactService) {
    let (observer, _script) =
        ScriptedObserver::new(ObserverStatus::available("the X11 window manager"));
    let rt = Runtime::start(RuntimeConfig {
        workspace: starters(),
        store: None,
        emitter: Arc::new(NullEmitter),
        keyboard: Arc::new(NoKeyboard),
        scan_hardware: false,
        serve_external: false,
        demo_pace: 1.0,
        plugin_dirs: plugins.map(Path::to_path_buf).into_iter().collect(),
        secrets: None,
        observer: Some(Box::new(observer)),
    })
    .await;
    let service =
        ArtifactService::new(rt.clone(), exports.to_path_buf()).with_app_version(APP_VERSION);
    (rt, service)
}

fn starters() -> Workspace {
    default_workspace_for(Platform::Linux)
}

fn request(value: Value) -> ExportRequest {
    serde_json::from_value(value).expect("an export request")
}

/// The Development starter and its rule, as a Pack.
pub fn development_request() -> ExportRequest {
    request(json!({
        "kind": "pack",
        "surfaceIds": ["development"],
        "metadata": {
            "id": "launchdeck-examples.development",
            "name": "Development",
            "version": "1.0.0",
            "description": "The Development starter surface, shown for Visual Studio Code.",
            "author": { "name": "LaunchDeck examples" },
            "license": "MIT"
        },
        "includeContextRules": true,
        "extraIdentities": [
            { "rule": 0, "namespace": "macos", "id": "com.microsoft.VSCode" },
            { "rule": 0, "namespace": "windows", "id": "code.exe" }
        ]
    }))
}

/// One surface with one pad of the Counter example plugin, as a Pack.
pub fn counter_request() -> ExportRequest {
    request(json!({
        "kind": "pack",
        "surfaceIds": ["counter"],
        "metadata": {
            "id": "launchdeck-examples.counter",
            "name": "Counter",
            "version": "1.0.0",
            "author": { "name": "LaunchDeck examples" },
            "license": "MIT"
        }
    }))
}

fn copy_dir(from: &Path, to: &Path) {
    fs::create_dir_all(to).unwrap();
    for entry in fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        if entry.file_type().unwrap().is_file() {
            fs::copy(entry.path(), to.join(entry.file_name())).unwrap();
        }
    }
}

/// The Counter example plugin, copied into `plugins`.
pub fn install_counter(plugins: &Path) {
    let examples = Path::new(env!("CARGO_MANIFEST_DIR")).join("../examples/plugins");
    copy_dir(&examples.join("counter"), &plugins.join("counter"));
}

/// The pad the feedback editor makes for the Counter: press to add one,
/// and a look for each level.
pub fn counter_pad() -> Control {
    let path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../examples/feedback-editor-counter.json");
    serde_json::from_str(&fs::read_to_string(path).unwrap()).unwrap()
}

/// The one file `service` saved in `exports`.
fn saved(service: &ArtifactService, request: &ExportRequest, exports: &Path) -> Vec<u8> {
    let saved = service.export_save(request).expect("the export is saved");
    fs::read(exports.join(saved.file_name)).unwrap()
}

/// What inspecting `bytes` returns, with the fixed plan id.
fn inspected(service: &ArtifactService, bytes: &[u8], file_name: &str) -> InspectOutcome {
    let mut outcome = service.inspect(bytes, file_name);
    if let InspectOutcome::Plan { plan } = &mut outcome {
        plan.plan_id = PLAN_ID.into();
    }
    outcome
}

fn json_file(value: &impl Serialize) -> Vec<u8> {
    let mut text = serde_json::to_string_pretty(value).expect("serialise");
    text.push('\n');
    text.into_bytes()
}

/// Every fixture of `examples/artifacts/`, by file name.
pub async fn capture() -> Vec<(&'static str, Vec<u8>)> {
    let tmp = tempfile::tempdir().unwrap();
    let exports = tmp.path().join("exports");

    // Development and a rule for Visual Studio Code, exported.
    let (rt, service) = world(None, &exports).await;
    let code = ApplicationIdentity::new(X11, "Code", "Visual Studio Code");
    rt.update_context(ContextSettings {
        rules: vec![ContextRule {
            application: code,
            surface_id: "development".into(),
            page_id: None,
        }],
        ..ContextSettings::default()
    })
    .unwrap();
    let development = development_request();
    let preview = service.export_preview(&development);
    let pack = saved(&service, &development, &exports);

    // Imported into the starter workspace.
    let (_, fresh) = world(None, &exports).await;
    let plan = inspected(&fresh, &pack, "development.ldpack");

    // A Pack that uses the Counter, imported where it is not installed.
    let plugins = tmp.path().join("plugins");
    install_counter(&plugins);
    let (rt, with_counter) = world(Some(&plugins), &exports).await;
    let id = rt.create_surface("Counter").unwrap();
    rt.update_control(&id, "home", counter_pad()).unwrap();
    let counter = saved(&with_counter, &counter_request(), &exports);
    let missing = inspected(&fresh, &counter, "counter.ldpack");

    // One character of the content changed.
    let text = String::from_utf8(pack.clone()).unwrap();
    let tampered = text.replacen("\"label\": \"Terminal\"", "\"label\": \"Terminai\"", 1);
    assert_ne!(tampered, text, "the content holds the Terminal pad");
    let rejected = inspected(&fresh, tampered.as_bytes(), "development.ldpack");
    assert!(matches!(rejected, InspectOutcome::Rejected { .. }));

    vec![
        ("development.ldpack", pack),
        ("export-preview.json", json_file(&preview)),
        ("import-plan.json", json_file(&plan)),
        ("import-plan-missing.json", json_file(&missing)),
        ("inspect-rejected.json", json_file(&rejected)),
    ]
}
