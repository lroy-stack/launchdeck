//! The artifact service (plan 7.10, 7.12, 7.17): export and import as the
//! app runs them, over the runtime. The only part of `artifacts` that holds
//! state and touches the disk: one pending import, and the last file saved.
//!
//! What it knows about the installation it reads from the runtime's own
//! getters (the snapshot: workspace, catalog, observables, context, devices)
//! and checks through the runtime's own gates (`validate_assignment`, the
//! state hub's definitions). An import is written only through
//! `Runtime::commit_workspace`, with the plan made again against the
//! workspace as it is under the lock; whatever is refused or fails leaves
//! the workspace in memory and `workspace.json` as they were. Export writes
//! one new file in the exports folder and nothing else. Nothing here runs
//! an action, installs, rescans or grants anything.

use std::fs::{self, OpenOptions};
use std::io::{self, ErrorKind};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};

use serde::Serialize;

use super::apply::{self, ApplyError, ImportReport};
use super::env::{Environment, KnownDevice, PlanHost};
use super::export::{export, ExportPreview, ExportRequest};
use super::format::Rejection;
use super::inspect::{inspect, ParsedArtifact};
use super::limits::ArtifactLimits;
use super::plan::{plan, ImportDecisions, ImportPlan};
use crate::actions::definition::{Assignment, Issue};
use crate::core::runtime::{CommitError, Runtime, Snapshot};
use crate::persistence::write_atomic;
use crate::platform::Platform;
use crate::state::definition::ObservableDefinition;

/// Other names tried for an export before giving up: `-2` to `-999`.
const MAX_NAME_TRIES: u32 = 999;

/// What a file to import turned into: the plan, made with the decisions the
/// import view proposes, or why the file was refused.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "status", rename_all = "camelCase")]
pub enum InspectOutcome {
    Plan { plan: Box<ImportPlan> },
    Rejected { rejection: Rejection },
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "status", rename_all = "camelCase")]
pub enum PreviewOutcome {
    Plan {
        plan: Box<ImportPlan>,
    },
    /// No pending import has this id: another file was opened since, the
    /// import was applied or discarded.
    Expired,
}

/// What became of an import. Anything but `applied` changed nothing.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "status", rename_all = "camelCase")]
pub enum ApplyOutcome {
    Applied {
        report: ImportReport,
    },
    /// Something changed since the preview (the workspace, the decisions,
    /// what is installed): the plan as it is now, to check again.
    Stale {
        plan: Box<ImportPlan>,
    },
    /// The plan still waits for a decision or an answer.
    Blocked {
        plan: Box<ImportPlan>,
    },
    /// The import could not be written. `message` is a sentence for a
    /// person, `detail` what the system said, when it said anything.
    Failed {
        message: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        detail: Option<String>,
    },
    Expired,
}

/// A file saved by export.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportSaved {
    pub file_name: String,
    /// The folder it was saved in, for display.
    pub folder: String,
    pub size_bytes: usize,
    pub content_digest: String,
}

/// Why an export was not saved or not shown: a sentence for a person, and
/// what the system said.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileError {
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

impl std::fmt::Display for FileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.detail {
            Some(detail) => write!(f, "{} ({detail})", self.message),
            None => f.write_str(&self.message),
        }
    }
}

/// The one import waiting for the person's decisions.
struct Pending {
    plan_id: String,
    parsed: ParsedArtifact,
}

/// The pending import, and how many files were opened: a file is stored as
/// pending only if no other was opened after it.
#[derive(Default)]
struct Waiting {
    opened: u64,
    import: Option<Pending>,
}

pub struct ArtifactService {
    runtime: Arc<Runtime>,
    exports_dir: PathBuf,
    app_version: String,
    pending: Mutex<Waiting>,
    last_export: Mutex<Option<PathBuf>>,
}

/// The runtime's own checks, for the plan.
struct RuntimeChecks(Arc<Runtime>);

impl PlanHost for RuntimeChecks {
    fn validate_assignment(&self, assignment: &Assignment) -> Result<Assignment, Vec<Issue>> {
        self.0.validate_assignment(assignment)
    }

    fn state_definition(&self, provider: &str, id: &str) -> Option<ObservableDefinition> {
        self.0.state_hub().definition(provider, id)
    }
}

/// Why the commit's builder refused the candidate.
enum Refused {
    Stale(Box<ImportPlan>),
    Blocked(Box<ImportPlan>),
    Failed(ApplyError),
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

impl ArtifactService {
    /// Exports are saved in `exports_dir` (the app passes Downloads).
    pub fn new(runtime: Arc<Runtime>, exports_dir: PathBuf) -> Self {
        Self {
            runtime,
            exports_dir,
            app_version: env!("CARGO_PKG_VERSION").to_string(),
            pending: Mutex::new(Waiting::default()),
            last_export: Mutex::new(None),
        }
    }

    /// The same service, writing `version` as the app's version
    /// (`createdWith.version`): for fixtures that must not change with
    /// each release.
    pub fn with_app_version(self, version: &str) -> Self {
        Self {
            app_version: version.to_string(),
            ..self
        }
    }

    pub fn limits(&self) -> ArtifactLimits {
        ArtifactLimits::new()
    }

    /// What exporting `request` would write, without writing it.
    pub fn export_preview(&self, request: &ExportRequest) -> ExportPreview {
        let snapshot = self.runtime.snapshot();
        export(&snapshot.workspace, &self.environment(&snapshot), request).preview
    }

    /// Export `request` into a new file of the exports folder: the proposed
    /// name, or `-2`, `-3`... before the extension when it is taken. Never
    /// replaces a file.
    pub fn export_save(&self, request: &ExportRequest) -> Result<ExportSaved, FileError> {
        let snapshot = self.runtime.snapshot();
        let result = export(&snapshot.workspace, &self.environment(&snapshot), request);
        let preview = result.preview;
        let Some(bytes) = result.bytes else {
            let message = preview.errors.first().map_or_else(
                || "The file cannot be written.".to_string(),
                |e| e.message.clone(),
            );
            return Err(FileError {
                message,
                detail: None,
            });
        };
        let path = self.write_new(&preview.file_name, &bytes)?;
        let file_name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        *lock(&self.last_export) = Some(path);
        Ok(ExportSaved {
            file_name,
            folder: self.exports_dir.display().to_string(),
            size_bytes: bytes.len(),
            content_digest: preview.content_digest,
        })
    }

    /// Show the folder of the last file export saved, in the system's file
    /// manager. The webview never names a folder.
    pub fn show_export(&self) -> Result<(), FileError> {
        let folder = lock(&self.last_export)
            .as_deref()
            .and_then(Path::parent)
            .map(Path::to_path_buf)
            .ok_or_else(|| FileError {
                message: "No file has been exported yet.".into(),
                detail: None,
            })?;
        open::that_detached(&folder).map_err(|e| FileError {
            message: "LaunchDeck could not show the folder of the exported file.".into(),
            detail: Some(e.to_string()),
        })
    }

    /// Read a file to import, as hostile input, and plan it with the
    /// decisions the import view proposes (`suggested`). Replaces any
    /// pending import, also when the file is refused. The pending import is
    /// not held while the file is read and planned, so no other command
    /// waits for that.
    pub fn inspect(&self, bytes: &[u8], file_name: &str) -> InspectOutcome {
        self.inspect_opened(self.opening(), bytes, file_name)
    }

    /// A file is being opened: the pending import is forgotten, and the
    /// file's turn is returned.
    fn opening(&self) -> u64 {
        let mut waiting = lock(&self.pending);
        waiting.opened += 1;
        waiting.import = None;
        waiting.opened
    }

    /// [`Self::inspect`] of the file opened at `turn`, which becomes the
    /// pending import unless another file was opened since.
    fn inspect_opened(&self, turn: u64, bytes: &[u8], file_name: &str) -> InspectOutcome {
        let parsed = match inspect(bytes, file_name) {
            Ok(parsed) => parsed,
            Err(rejection) => return InspectOutcome::Rejected { rejection },
        };
        let snapshot = self.runtime.snapshot();
        let env = self.environment(&snapshot);
        let ws = &snapshot.workspace;
        let first = plan(&parsed, ws, &env, &ImportDecisions::default());
        let proposed = if first.suggested == first.decisions {
            first
        } else {
            plan(&parsed, ws, &env, &first.suggested)
        };
        let mut waiting = lock(&self.pending);
        if waiting.opened == turn {
            waiting.import = Some(Pending {
                plan_id: proposed.plan_id.clone(),
                parsed,
            });
        }
        InspectOutcome::Plan {
            plan: Box::new(proposed),
        }
    }

    /// The pending import's plan with `decisions`, against the workspace
    /// and what is installed now.
    pub fn preview(&self, plan_id: &str, decisions: &ImportDecisions) -> PreviewOutcome {
        let waiting = lock(&self.pending);
        let Some(p) = waiting.import.as_ref().filter(|p| p.plan_id == plan_id) else {
            return PreviewOutcome::Expired;
        };
        let snapshot = self.runtime.snapshot();
        let mut planned = plan(
            &p.parsed,
            &snapshot.workspace,
            &self.environment(&snapshot),
            decisions,
        );
        planned.plan_id = p.plan_id.clone();
        PreviewOutcome::Plan {
            plan: Box::new(planned),
        }
    }

    /// Import the pending file as the person decided, in one transaction:
    /// under the runtime's lock the plan is made again against the
    /// workspace as it is; if its digest is not `plan_digest` the import is
    /// stale, if anything still blocks it it is blocked; otherwise the new
    /// workspace is built, checked, saved, and only then shown. The pending
    /// import is dropped once it is applied.
    pub fn apply(
        &self,
        plan_id: &str,
        decisions: &ImportDecisions,
        plan_digest: &str,
    ) -> ApplyOutcome {
        let mut waiting = lock(&self.pending);
        let Some(p) = waiting.import.as_ref().filter(|p| p.plan_id == plan_id) else {
            return ApplyOutcome::Expired;
        };
        let snapshot = self.runtime.snapshot();
        let env = self.environment(&snapshot);
        let mut report = None;
        let committed = self.runtime.commit_workspace(|current| {
            let mut planned = plan(&p.parsed, current, &env, decisions);
            planned.plan_id = p.plan_id.clone();
            if planned.plan_digest != plan_digest {
                return Err(Refused::Stale(Box::new(planned)));
            }
            if !planned.ready {
                return Err(Refused::Blocked(Box::new(planned)));
            }
            let candidate = apply::build(&p.parsed, &planned, current).map_err(Refused::Failed)?;
            apply::verify(&candidate, current).map_err(Refused::Failed)?;
            report = Some(apply::report(&planned, current.context.auto));
            Ok(candidate)
        });
        match committed {
            Ok(()) => {
                waiting.import = None;
                let report = report.expect("the builder reports what it built");
                log::info!(
                    "import applied: {} surfaces added, {} replaced, {} rules",
                    report.surfaces_added.len(),
                    report.surfaces_replaced.len(),
                    report.rules_added
                );
                ApplyOutcome::Applied { report }
            }
            Err(CommitError::Build(Refused::Stale(plan))) => ApplyOutcome::Stale { plan },
            Err(CommitError::Build(Refused::Blocked(plan))) => ApplyOutcome::Blocked { plan },
            Err(CommitError::Build(Refused::Failed(e))) => ApplyOutcome::Failed {
                message: e.to_string(),
                detail: None,
            },
            Err(CommitError::Save(why)) => {
                log::error!("import not saved: {why}");
                ApplyOutcome::Failed {
                    message: "LaunchDeck could not save the workspace, so nothing was imported."
                        .into(),
                    detail: Some(why),
                }
            }
        }
    }

    /// Forget the pending import, if it is this one.
    pub fn discard(&self, plan_id: &str) {
        let mut waiting = lock(&self.pending);
        if waiting
            .import
            .as_ref()
            .is_some_and(|p| p.plan_id == plan_id)
        {
            waiting.import = None;
        }
    }

    /// What export and import know about this installation, from the
    /// runtime's snapshot, with the runtime's checks for the plan.
    fn environment(&self, snapshot: &Snapshot) -> Environment {
        Environment {
            app_version: self.app_version.clone(),
            namespace: snapshot.context.namespace.map(str::to_string),
            providers: Environment::providers_from(&snapshot.catalog),
            observable_sources: snapshot
                .observables
                .providers
                .iter()
                .map(|p| p.id.clone())
                .collect(),
            devices: snapshot
                .state
                .devices
                .iter()
                .map(|d| KnownDevice {
                    name: d.identity.name.clone(),
                    model: d.identity.model.clone(),
                    kind: d.identity.kind,
                    connected: d.connected,
                    capabilities: d.capabilities,
                })
                .collect(),
            recent_applications: snapshot.context.recent.clone(),
            host: Some(Arc::new(RuntimeChecks(self.runtime.clone()))),
        }
    }

    /// Write `bytes` under `name`, or the first free `-2`, `-3`... variant,
    /// in the exports folder. The name is taken by creating the file, so a
    /// file that appears meanwhile is never replaced; a name whose
    /// temporary file (`<name>.tmp`, which `write_atomic` uses) exists is
    /// skipped for the same reason, also when it is a link, even to
    /// nothing: `write_atomic` would follow it.
    fn write_new(&self, name: &str, bytes: &[u8]) -> Result<PathBuf, FileError> {
        let dir = &self.exports_dir;
        fs::create_dir_all(dir).map_err(|e| self.not_saved(&e))?;
        for candidate in candidates(name) {
            let path = dir.join(&candidate);
            if fs::symlink_metadata(dir.join(format!("{candidate}.tmp"))).is_ok() {
                continue;
            }
            match OpenOptions::new().write(true).create_new(true).open(&path) {
                Ok(_) => {}
                Err(e) if e.kind() == ErrorKind::AlreadyExists => continue,
                Err(e) => return Err(self.not_saved(&e)),
            }
            return match write_atomic(&path, bytes) {
                Ok(()) => Ok(path),
                Err(e) => {
                    let _ = fs::remove_file(&path);
                    Err(self.not_saved(&e))
                }
            };
        }
        Err(FileError {
            message: format!(
                "Every name LaunchDeck tried for the file is taken in {}.",
                dir.display()
            ),
            detail: None,
        })
    }

    fn not_saved(&self, e: &io::Error) -> FileError {
        let message = if e.kind() == ErrorKind::PermissionDenied {
            match Platform::current() {
                Platform::MacOs => "macOS did not allow LaunchDeck to save in Downloads. Allow \
                                    it in System Settings, Privacy & Security, Files and Folders."
                    .to_string(),
                _ => format!(
                    "LaunchDeck is not allowed to save in {}.",
                    self.exports_dir.display()
                ),
            }
        } else {
            format!(
                "LaunchDeck could not save the file in {}.",
                self.exports_dir.display()
            )
        };
        FileError {
            message,
            detail: Some(e.to_string()),
        }
    }
}

/// `name`, then `<stem>-2.<ext>` to `<stem>-999.<ext>`.
fn candidates(name: &str) -> impl Iterator<Item = String> + '_ {
    let (stem, ext) = name.rsplit_once('.').unwrap_or((name, ""));
    let numbered = (2..=MAX_NAME_TRIES).map(move |n| {
        if ext.is_empty() {
            format!("{stem}-{n}")
        } else {
            format!("{stem}-{n}.{ext}")
        }
    });
    std::iter::once(name.to_string()).chain(numbered)
}

#[cfg(test)]
#[path = "service_tests.rs"]
mod tests;
