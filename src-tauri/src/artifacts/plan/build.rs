//! The plan as a whole (plan 7.12): every part computed from the file, the
//! workspace, the environment and the person's decisions, then what stops
//! the import, the effects and their digest.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;

use serde_json::Value;

use super::blockers::{blockers, Stops};
use super::checks;
use super::decisions::{ImportDecisions, Resolution, Runs};
use super::inputs::{self, Answers};
use super::instances::{self, Mapping, Target};
use super::rules;
use super::types::*;
use super::walk::{self, Pressed};
use crate::actions::definition::Assignment;
use crate::artifacts::env::Environment;
use crate::artifacts::format::{echo, InputDef, InputValue};
use crate::artifacts::inspect::{InspectWarningCode, ParsedArtifact};
use crate::artifacts::json::{content_digest, to_canonical};
use crate::artifacts::review::{classify, Location, ReviewItem, ReviewKind};
use crate::core::model::Workspace;
use crate::platform::Platform;

/// What importing `parsed` into `workspace` would do with `decisions`, on
/// the installation `env` describes. Changes nothing, runs nothing, reads
/// nothing but its arguments (and, through `env.host`, the runtime's
/// checks) and the system it runs on, whose rules decide what a full path
/// and a launch are. The same file, workspace, decisions and installed
/// providers give the same effects and digest; `plan_id` is new each time
/// (the service keeps the id of the pending import).
pub fn plan(
    parsed: &ParsedArtifact,
    workspace: &Workspace,
    env: &Environment,
    decisions: &ImportDecisions,
) -> ImportPlan {
    let namespace = env.namespace.as_deref();
    let existing = instances::existing(parsed, workspace);
    let target = instances::target(parsed, workspace, &existing, decisions);
    let table = instances::id_table(parsed, workspace, &target);
    let local_ids: BTreeMap<String, String> = table
        .iter()
        .map(|m| (m.source_id.clone(), m.local_id.clone()))
        .collect();
    let surfaces = instances::planned_surfaces(parsed, workspace, env, &table);
    let mut answers = inputs::answers(
        &parsed.manifest.inputs,
        &decisions.inputs,
        namespace,
        Platform::current(),
    );
    let pads = pads(parsed, env, &answers, &local_ids, decisions.runs);
    let (warnings, dependencies) =
        looked_over(parsed, env, decisions, &pads, &target, &table, &mut answers);
    let would_be = would_be(workspace, parsed, &table);
    let rules = rules::plan_rules(
        parsed,
        workspace,
        &would_be,
        namespace,
        &decisions.context_rules,
        &answers,
        &local_ids,
    );
    let inputs = planned_inputs(parsed, decisions, &answers, &pads);
    let effects = effects(parsed, &target, &table, rules.added, &pads, &answers);
    let blockers = blockers(&Stops {
        target: &target,
        surfaces: &surfaces,
        decisions,
        runs_things: pads.runs_things,
        inputs: &inputs,
        parsed,
        workspace,
        effects: &effects,
    });
    ImportPlan {
        plan_id: new_plan_id(),
        plan_digest: digest(&effects),
        source: PlanSource {
            file_name: parsed.file_name.clone(),
            size_bytes: parsed.size_bytes,
            content_digest: parsed.content_digest.clone(),
            format_version: parsed.format_version,
        },
        artifact: summary(parsed),
        surfaces,
        context_rules: rules.rules,
        dependencies,
        inputs,
        suggested: suggested(parsed, namespace, !existing.is_empty(), pads.runs_things),
        review: pads.review,
        existing,
        compatibility: checks::compatibility(parsed, env),
        warnings,
        decisions: decisions.clone(),
        ready: blockers.is_empty(),
        blockers,
        effects,
    }
}

/// The file's pads as they would be written, what they run, and the slots
/// the person chose to empty.
struct Pads<'a> {
    pads: Vec<Pressed<'a>>,
    /// In the same order: each assignment with the answers and local ids.
    written: Vec<Assignment>,
    /// In the same order: whether the pad stays (its slot is not emptied).
    stays: Vec<bool>,
    review: Vec<ReviewItem>,
    /// A pad runs a command, sends a request or starts a program.
    runs_things: bool,
    removed: Vec<Location>,
}

impl Pads<'_> {
    /// The pads that stay, with their assignment as written.
    fn kept(&self) -> impl Iterator<Item = (&Pressed<'_>, &Assignment)> {
        self.pads
            .iter()
            .zip(&self.written)
            .zip(&self.stays)
            .filter(|(_, stays)| **stays)
            .map(|(pad, _)| pad)
    }
}

fn pads<'a>(
    parsed: &'a ParsedArtifact,
    env: &Environment,
    answers: &Answers,
    local_ids: &BTreeMap<String, String>,
    runs: Option<Runs>,
) -> Pads<'a> {
    let pads = walk::assignments(&parsed.content);
    let written: Vec<Assignment> = pads
        .iter()
        .map(|p| {
            inputs::bound(
                p.assignment,
                &parsed.manifest.inputs,
                &answers.values,
                local_ids,
            )
        })
        .collect();
    let kinds: Vec<Option<ReviewKind>> = written.iter().map(|w| classify(w, env)).collect();
    let review: Vec<ReviewItem> = pads
        .iter()
        .zip(&written)
        .zip(&kinds)
        .filter_map(|((p, w), kind)| kind.map(|k| ReviewItem::new(p.at.clone(), p.label, k, w)))
        .collect();
    let decided: Vec<bool> = kinds
        .iter()
        .map(|k| k.is_some_and(needs_decision))
        .collect();
    let removing = runs == Some(Runs::Remove);
    let removed: Vec<Location> = pads
        .iter()
        .zip(&decided)
        .filter(|(_, decided)| removing && **decided)
        .map(|(p, _)| p.at.clone())
        .collect();
    Pads {
        runs_things: decided.contains(&true),
        stays: decided.iter().map(|d| !(removing && *d)).collect(),
        removed,
        pads,
        written,
        review,
    }
}

/// Everything a person should know that does not stop the import, in
/// order, and the dependencies; what installed providers say of an answer
/// goes to its input.
fn looked_over(
    parsed: &ParsedArtifact,
    env: &Environment,
    decisions: &ImportDecisions,
    pads: &Pads<'_>,
    target: &Target<'_>,
    table: &[Mapping<'_>],
    answers: &mut Answers,
) -> (Vec<PlanWarning>, Vec<DependencyCheck>) {
    let mut warnings = inspected(parsed);
    for (p, written) in pads.kept() {
        let given = &decisions.inputs;
        let defs = &parsed.manifest.inputs;
        checks::check_pad(p, written, env, defs, given, answers, &mut warnings);
    }
    answers.drop_refused();
    checks::provider_warnings(&parsed.content, env, &mut warnings);
    checks::state_rules(&parsed.content, env, &mut warnings);
    let dependencies = checks::dependencies(parsed, env, &mut warnings);
    kept_from_previous_version(target, table, &mut warnings);
    sort_warnings(&mut warnings);
    (warnings, dependencies)
}

fn effects(
    parsed: &ParsedArtifact,
    target: &Target<'_>,
    table: &[Mapping<'_>],
    add_rules: Vec<AddedRule>,
    pads: &Pads<'_>,
    answers: &Answers,
) -> PlanEffects {
    PlanEffects {
        instance: target.instance,
        add_surfaces: table
            .iter()
            .zip(&parsed.content.surfaces)
            .filter(|(m, _)| m.replaces.is_none())
            .map(|(m, s)| AddedSurface {
                source_id: m.source_id.clone(),
                local_id: m.local_id.clone(),
                name: s.name.clone(),
            })
            .collect(),
        replace_surfaces: table
            .iter()
            .filter(|m| m.replaces.is_some())
            .map(|m| ReplacedSurface {
                source_id: m.source_id.clone(),
                local_id: m.local_id.clone(),
            })
            .collect(),
        add_rules,
        removed_slots: pads.removed.clone(),
        input_values: answers.values.clone(),
    }
}

/// A new instance when there is a choice to make, the pads that run things
/// removed when there are any, the inputs' defaults that are valid here,
/// and no context rule.
fn suggested(
    parsed: &ParsedArtifact,
    namespace: Option<&str>,
    imported_before: bool,
    runs_things: bool,
) -> ImportDecisions {
    ImportDecisions {
        resolution: imported_before.then_some(Resolution::ImportAsNew),
        runs: runs_things.then_some(Runs::Remove),
        inputs: defaults(&parsed.manifest.inputs, namespace),
        context_rules: Vec::new(),
        confirm_overwrite_edits: false,
    }
}

/// A command, a request or a launch, which the person keeps or removes; an
/// instruction to an agent is shown and always kept.
fn needs_decision(kind: ReviewKind) -> bool {
    kind != ReviewKind::InstructsAgent
}

/// What inspecting the file found that a person should know.
fn inspected(parsed: &ParsedArtifact) -> Vec<PlanWarning> {
    parsed
        .warnings
        .iter()
        .map(|w| PlanWarning {
            code: match w.code {
                InspectWarningCode::ExtensionMismatch => PlanWarningCode::ExtensionMismatch,
                InspectWarningCode::UnknownCategory => PlanWarningCode::UnknownCategory,
            },
            message: w.message.clone(),
            at: w.at.clone(),
        })
        .collect()
}

/// Surfaces of the instance being replaced that the file no longer has.
fn kept_from_previous_version(
    target: &Target<'_>,
    table: &[Mapping<'_>],
    warnings: &mut Vec<PlanWarning>,
) {
    for surface in &target.instance_surfaces {
        let replaced = table
            .iter()
            .any(|m| m.replaces.is_some_and(|r| r.id == surface.id));
        if !replaced {
            warnings.push(PlanWarning {
                code: PlanWarningCode::KeptFromPreviousVersion,
                message: format!(
                    "The surface \"{}\" is not in this version of the file; it is kept as it is.",
                    echo(&surface.name)
                ),
                at: None,
            });
        }
    }
}

/// One part of a JSON pointer: positions in order of number, before names.
#[derive(PartialEq, Eq, PartialOrd, Ord)]
enum Segment {
    Index(usize),
    Name(String),
}

fn pointer_key(pointer: &str) -> Vec<Segment> {
    pointer
        .split('/')
        .map(|s| {
            s.parse()
                .map_or_else(|_| Segment::Name(s.into()), Segment::Index)
        })
        .collect()
}

/// By place in the file (those without a place first), then by code.
fn sort_warnings(warnings: &mut [PlanWarning]) {
    warnings.sort_by_cached_key(|w| (w.at.as_deref().map(pointer_key), w.code));
}

/// The workspace as it would be after the import, enough to check context
/// rules against: replaced surfaces replaced, new ones added.
fn would_be(workspace: &Workspace, parsed: &ParsedArtifact, table: &[Mapping<'_>]) -> Workspace {
    let mut ws = workspace.clone();
    for (m, incoming) in table.iter().zip(&parsed.content.surfaces) {
        let mut surface = incoming.to_model();
        surface.id = m.local_id.clone();
        match ws.surface_mut(&m.local_id) {
            Some(slot) if m.replaces.is_some() => *slot = surface,
            _ => ws.surfaces.push(surface),
        }
    }
    ws
}

/// Each input with the answer given, what is wrong with it, and the pads
/// whose argument it replaces.
fn planned_inputs(
    parsed: &ParsedArtifact,
    decisions: &ImportDecisions,
    answers: &Answers,
    pads: &Pads<'_>,
) -> Vec<PlannedInput> {
    // The pads that stay, by the inputs they take, each pad once per input.
    let mut used_by: BTreeMap<&str, Vec<Location>> = BTreeMap::new();
    for (p, _) in pads.kept() {
        let taken: BTreeSet<&str> = p.assignment.inputs.values().map(String::as_str).collect();
        for input in taken {
            used_by.entry(input).or_default().push(p.at.clone());
        }
    }
    parsed
        .manifest
        .inputs
        .iter()
        .map(|def| PlannedInput {
            def: def.clone(),
            value: decisions.inputs.get(&def.id).cloned(),
            issues: answers.issues.get(&def.id).cloned().unwrap_or_default(),
            used_by: used_by.remove(def.id.as_str()).unwrap_or_default(),
        })
        .collect()
}

/// The inputs' defaults that are valid answers here.
fn defaults(defs: &[InputDef], namespace: Option<&str>) -> BTreeMap<String, InputValue> {
    defs.iter()
        .filter_map(|def| {
            let value = def.default.as_ref()?;
            inputs::check_value(def, value, namespace, Platform::current())
                .is_empty()
                .then(|| (def.id.clone(), value.clone()))
        })
        .collect()
}

fn summary(parsed: &ParsedArtifact) -> ArtifactSummary {
    let m = &parsed.manifest;
    ArtifactSummary {
        kind: parsed.kind,
        id: m.id.clone(),
        name: m.name.clone(),
        version: m.version.clone(),
        description: m.description.clone(),
        author: m.author.clone(),
        license: m.license.clone(),
        created_with: m.created_with.clone(),
        tested_with: m.tested_with.clone(),
    }
}

/// `sha256:` and the digest of the canonical JSON of the effects.
fn digest(effects: &PlanEffects) -> String {
    let value = serde_json::to_value(effects).unwrap_or(Value::Null);
    content_digest(&to_canonical(&value))
}

/// 32 hexadecimal digits from the system's random source.
fn new_plan_id() -> String {
    let mut bytes = [0u8; 16];
    getrandom::fill(&mut bytes).expect("the operating system provides randomness");
    let mut id = String::with_capacity(32);
    for b in bytes {
        let _ = write!(id, "{b:02x}");
    }
    id
}
