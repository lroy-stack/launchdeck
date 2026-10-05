//! Import planning (plan 7.12-7.16): what importing a file would do to the
//! workspace, worked out before anything changes. Pure: it writes nothing,
//! runs nothing and asks the runtime only for its checks (`env.host`); the
//! import view renders the plan, and apply (C5) writes exactly its effects.
//!
//! * `decisions` what the person decides: resolution, pads that run
//!   things, answers, context rules, overwriting edits
//! * `types`     the plan and every part of it
//! * `build`     [`plan`]: the parts, the effects and their digest
//! * `blockers`  what stops the import until the person decides or answers
//! * `instances` earlier imports, the instance, the id table (7.13, 7.14)
//! * `inputs`    the answers checked, an assignment as written with them
//! * `rules`     the context rules, opt-in, checked as they would be saved
//! * `checks`    dependencies, devices, providers, what installed
//!   providers say (7.6, 7.7, 7.16)
//! * `walk`      every control and assignment of the content, with its place

mod blockers;
mod build;
mod checks;
mod decisions;
mod inputs;
mod instances;
mod rules;
mod types;
mod walk;

#[cfg(test)]
mod tests;

pub use build::plan;
pub use decisions::{ImportDecisions, Resolution, Runs};
pub use types::{
    AddedRule, AddedSurface, ArtifactSummary, Blocker, BlockerCode, Compatibility, DependencyCheck,
    DependencyStatus, DeviceFit, ExistingInstance, ExistingSurface, ImportPlan, PlanEffects,
    PlanSource, PlanWarning, PlanWarningCode, PlannedInput, PlannedRule, PlannedSurface, Relation,
    ReplacedSurface, RuleOutcome, SurfaceAction, UsedBy,
};
pub(in crate::artifacts) use {inputs::bound, instances::surface_digest};
