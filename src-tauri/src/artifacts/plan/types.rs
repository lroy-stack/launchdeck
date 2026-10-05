//! What an import plan says (plan 7.12): every type the person's import view
//! renders, and the effects, the one part apply writes and the digest covers.

use std::collections::BTreeMap;

use serde::Serialize;

use super::decisions::ImportDecisions;
use crate::artifacts::format::{
    ArtifactKind, Author, CreatedWith, InputDef, InputValue, Requirements, TestedWith,
};
use crate::artifacts::review::{Location, ReviewItem};
use crate::context::identity::ApplicationIdentity;
use crate::devices::DeviceKind;

/// What importing a file would do to this workspace, computed without
/// changing anything. Lists are in the file's order; warnings by place,
/// then code.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportPlan {
    /// 32 hexadecimal digits, random: which pending import this is.
    pub plan_id: String,
    /// `sha256:` and the digest of the canonical JSON of `effects`: two plans
    /// with the same digest write the same thing.
    pub plan_digest: String,
    pub source: PlanSource,
    pub artifact: ArtifactSummary,
    pub surfaces: Vec<PlannedSurface>,
    pub context_rules: Vec<PlannedRule>,
    pub dependencies: Vec<DependencyCheck>,
    pub inputs: Vec<PlannedInput>,
    /// The pads that run things, with what exactly they run.
    pub review: Vec<ReviewItem>,
    /// Earlier imports of the same file (by format and id).
    pub existing: Vec<ExistingInstance>,
    pub compatibility: Compatibility,
    pub warnings: Vec<PlanWarning>,
    /// The decisions this plan was made with.
    pub decisions: ImportDecisions,
    /// What the import view proposes: a new instance, the pads that run
    /// things removed, the inputs' defaults, and no context rule.
    pub suggested: ImportDecisions,
    /// Anything here stops the import until the person decides or answers.
    pub blockers: Vec<Blocker>,
    /// No blocker.
    pub ready: bool,
    pub effects: PlanEffects,
}

/// The file as it was read.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanSource {
    /// Without any character a person cannot see, at most 128 characters.
    pub file_name: String,
    pub size_bytes: usize,
    pub content_digest: String,
    pub format_version: u32,
}

/// What the manifest says about the file, for display.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ArtifactSummary {
    pub kind: ArtifactKind,
    pub id: String,
    pub name: String,
    pub version: String,
    pub description: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub author: Option<Author>,
    pub license: String,
    pub created_with: CreatedWith,
    pub tested_with: Vec<TestedWith>,
}

/// Exactly what apply writes, and the only part of a plan its digest covers.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanEffects {
    /// The instance the surfaces are recorded under (`importedFrom.instance`).
    pub instance: u32,
    pub add_surfaces: Vec<AddedSurface>,
    pub replace_surfaces: Vec<ReplacedSurface>,
    /// Appended after every local rule, in this order.
    pub add_rules: Vec<AddedRule>,
    /// Slots emptied because the person chose to remove the pads that run
    /// things; their labels, looks and feedback stay.
    pub removed_slots: Vec<Location>,
    /// The answers given to the file's inputs that are valid.
    pub input_values: BTreeMap<String, InputValue>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AddedSurface {
    pub source_id: String,
    pub local_id: String,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReplacedSurface {
    pub source_id: String,
    pub local_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AddedRule {
    /// The rule's position in the file.
    pub index: usize,
    /// The identity on this system, as the rule is saved.
    pub application: ApplicationIdentity,
    pub surface_local_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub page_id: Option<String>,
}

/// A surface of the file and what happens to it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlannedSurface {
    /// Its id in the file.
    pub source_id: String,
    /// Its id here: the file's id when free, else `<id>-2`, `-3`..., or the
    /// id of the surface it replaces.
    pub local_id: String,
    pub name: String,
    /// As it is imported: an unknown category becomes `custom`.
    pub category: String,
    pub pages: usize,
    pub controls: usize,
    pub action: SurfaceAction,
    /// The local id is not the file's (another surface has that id).
    pub id_changed: bool,
    /// A surface here already has this name: the local id of the first one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub same_name_as: Option<String>,
    /// Providers its pads and state rules use that are not installed here.
    pub missing_providers: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum SurfaceAction {
    Add,
    /// It replaces a surface of an earlier import, in place.
    #[serde(rename_all = "camelCase")]
    Replace {
        /// The version that surface was imported from.
        previous_version: String,
        /// That surface was changed here since it was imported.
        locally_edited: bool,
    },
}

/// A context rule of the file and what happens to it. Rules are only added
/// when the person selects them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlannedRule {
    pub index: usize,
    /// The application's name for people: the file's, or the answer's.
    pub application_name: String,
    /// The application on this system, when the file names it here.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub application: Option<ApplicationIdentity>,
    pub surface_local_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub page_id: Option<String>,
    pub outcome: RuleOutcome,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum RuleOutcome {
    /// Added at this position of the rules (from 0).
    Add { position: usize },
    /// The person did not select it.
    NotSelected,
    /// A local rule names the same application, and the local rule wins.
    #[serde(rename_all = "camelCase")]
    SkipConflict { local_rule: usize },
    /// An earlier rule of the file, added, names the same application.
    SkipShadowed { by: usize },
    /// The file names no application of this system's kind.
    SkipNoIdentity,
    /// The workspace already has as many rules as it can hold.
    SkipCapacity,
    /// It waits for the answer to this application input.
    NeedsInput { input: String },
}

/// A plugin the file uses, and whether it is here.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DependencyCheck {
    pub plugin_id: String,
    /// For display: the file's name for it, the installed plugin's, or its id.
    pub name: String,
    pub requirement: String,
    /// A pad's action needs it; otherwise only feedback reads it.
    pub required: bool,
    pub status: DependencyStatus,
    /// The controls that use it.
    pub used_by: Vec<UsedBy>,
}

/// Never a reason to refuse the import: a pad of a missing plugin shows as
/// unavailable until the plugin is installed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum DependencyStatus {
    Compatible {
        installed: String,
    },
    /// Installed and turned off, or failed to load.
    Disabled {
        installed: String,
    },
    /// Installed in a version outside the requirement.
    Incompatible {
        installed: String,
    },
    Missing,
}

/// A control of the file, by the ids the file gives it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UsedBy {
    pub surface_source_id: String,
    pub page_id: String,
    pub control_id: String,
    pub label: String,
}

/// An input of the file, the answer given and what is wrong with it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlannedInput {
    pub def: InputDef,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<InputValue>,
    /// Sentences for a person; empty when the answer is fine.
    pub issues: Vec<String>,
    /// The pads whose argument it replaces (not those being removed).
    pub used_by: Vec<Location>,
}

/// An earlier import of the same file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExistingInstance {
    pub instance: u32,
    /// The newest version its surfaces were imported from.
    pub version: String,
    /// How the file's version compares with it: `newer` is an upgrade,
    /// `older` a downgrade.
    pub relation: Relation,
    /// It was imported from exactly this content.
    pub identical_content: bool,
    pub surfaces: Vec<ExistingSurface>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Relation {
    Same,
    Newer,
    Older,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExistingSurface {
    pub local_id: String,
    pub name: String,
    pub source_id: String,
    /// Changed here since it was imported.
    pub locally_edited: bool,
}

/// How the file fits this system and its devices. Never blocks.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Compatibility {
    /// The kind of application identity this system has.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub namespace: Option<String>,
    /// As the file declares them.
    pub requirements: Requirements,
    pub devices: Vec<DeviceFit>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceFit {
    pub name: String,
    pub model: String,
    pub kind: DeviceKind,
    pub connected: bool,
    /// Every control of the file has its pads on this device.
    pub fits: bool,
    /// How many controls do not.
    pub outside: usize,
    /// The device shows colour, or the file needs none.
    pub rgb_ok: bool,
}

/// Something a person should know that does not stop the import.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanWarning {
    pub code: PlanWarningCode,
    pub message: String,
    /// A JSON pointer into the file, when the warning has a place there.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub at: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum PlanWarningCode {
    /// The file's name ends in the other kind's extension, or in none.
    ExtensionMismatch,
    /// A category this version does not know: imported as `custom`.
    UnknownCategory,
    /// A surface of the instance being replaced that the file no longer has:
    /// kept as it is.
    KeptFromPreviousVersion,
    /// A pad opens a surface by an id the file does not rename.
    ExternalSurfaceReference,
    /// What the installed provider says of a pad or a state rule.
    InvalidForInstalledVersion,
    /// A provider of one segment (part of LaunchDeck) this version does not have.
    UnknownHostProvider,
    /// An event rule listens to a source nothing here offers: an outside program.
    ExternalEventSource,
    /// A dependency the content does not use.
    UnusedDependency,
    MissingDependency,
    DisabledDependency,
    IncompatibleDependency,
}

/// What stops the import, and the input it is about.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Blocker {
    pub code: BlockerCode,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum BlockerCode {
    /// The file was imported before: import it as new, or replace one import.
    ResolutionNeeded,
    /// A surface it would replace was changed here; confirm to overwrite it.
    OverwriteEditsNotConfirmed,
    /// Pads run things: keep them or remove them.
    RunsDecisionNeeded,
    InputRequired,
    InputInvalid,
    /// The workspace would hold more surfaces than it can.
    WorkspaceFull,
}
