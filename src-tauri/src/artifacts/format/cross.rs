//! The checks that need the whole file (plan 7.3, step 8): what each kind
//! may hold, the device requirements, the declared dependencies, and every
//! reference between parts.

use std::collections::BTreeMap;

use serde_json::Value;

use super::content::{AppRefV1, AssignmentV1, ContentV1, ControlV1, Pos, Span};
use super::envelope::ArtifactKind;
use super::manifest::{
    AuxRequirement, GridRequirement, InputDef, InputType, ManifestV1, Requirements,
};
use super::rejection::{echo, Rejection, RejectionCode};
use crate::artifacts::json::segment;
use crate::state::feedback::{When, IDLE};

/// The cross-checks, in order: kind rules (`kindMismatch`), requirements no
/// weaker than the content (`invalidManifest`), every plugin the content
/// uses declared (`invalidManifest`), every reference resolved
/// (`invalidContent`). For a manifest and content that passed their own checks.
pub fn check_cross(
    kind: ArtifactKind,
    manifest: &ManifestV1,
    content: &ContentV1,
) -> Result<(), Rejection> {
    check_kind(kind, manifest, content)?;
    check_requirements(&manifest.requirements, &compute_requirements(content))?;
    check_declared(manifest, content)?;
    check_references(manifest, content)
}

/// Every control with the pointer to it.
fn controls(content: &ContentV1) -> impl Iterator<Item = (String, &ControlV1)> {
    content
        .surfaces
        .iter()
        .enumerate()
        .flat_map(|(s, surface)| {
            surface.pages.iter().enumerate().flat_map(move |(p, page)| {
                page.controls.iter().enumerate().map(move |(c, control)| {
                    (
                        format!("/content/surfaces/{s}/pages/{p}/controls/{c}"),
                        control,
                    )
                })
            })
        })
}

/// Every assignment with the pointer to it.
fn assignments(content: &ContentV1) -> impl Iterator<Item = (String, &AssignmentV1)> {
    controls(content).flat_map(|(at, c)| {
        [("press", &c.press), ("longPress", &c.long_press)]
            .into_iter()
            .filter_map(move |(slot, a)| a.as_ref().map(|a| (format!("{at}/{slot}"), a)))
    })
}

fn check_kind(
    kind: ArtifactKind,
    manifest: &ManifestV1,
    content: &ContentV1,
) -> Result<(), Rejection> {
    if kind == ArtifactKind::Pack {
        return Ok(());
    }
    let refused = |at: &str, why: &str| {
        Err(Rejection::at(
            RejectionCode::KindMismatch,
            at,
            why.to_string(),
        ))
    };
    if content.surfaces.len() != 1 {
        return refused(
            "/content/surfaces",
            "A Surface file holds exactly one surface; several make a Pack.",
        );
    }
    if !content.context_rules.is_empty() {
        return refused(
            "/content/contextRules",
            "A Surface file has no context rules; they belong in a Pack.",
        );
    }
    if !manifest.inputs.is_empty() {
        return refused(
            "/manifest/inputs",
            "A Surface file has no inputs; they belong in a Pack.",
        );
    }
    if let Some((at, _)) = assignments(content).find(|(_, a)| !a.inputs.is_empty()) {
        return refused(
            &format!("{at}/inputs"),
            "A Surface file has no inputs; they belong in a Pack.",
        );
    }
    Ok(())
}

/// What a device needs to show all of `content` (plan 7.7): the grid's
/// width and height, the pads above and beside it, and colour if any
/// colour is not a grey.
pub fn compute_requirements(content: &ContentV1) -> Requirements {
    let mut grid: Option<GridRequirement> = None;
    let mut aux = AuxRequirement { top: 0, side: 0 };
    let mut rgb = false;
    for (_, c) in controls(content) {
        match c.pos {
            Pos::Grid { col, row } => {
                let span = c.span.unwrap_or(Span { w: 1, h: 1 });
                let (w, h) = (col.saturating_add(span.w), row.saturating_add(span.h));
                let g = grid.get_or_insert(GridRequirement {
                    min_width: w,
                    min_height: h,
                });
                g.min_width = g.min_width.max(w);
                g.min_height = g.min_height.max(h);
            }
            Pos::Top { index } => aux.top = aux.top.max(index.saturating_add(1)),
            Pos::Side { index } => aux.side = aux.side.max(index.saturating_add(1)),
        }
        rgb |= std::iter::once(&c.visual)
            .chain(c.looks.values())
            .any(|v| !(v.color.0.r == v.color.0.g && v.color.0.g == v.color.0.b));
    }
    Requirements { grid, aux, rgb }
}

/// The declared requirements may ask for more than the content needs, never less.
fn check_requirements(declared: &Requirements, needed: &Requirements) -> Result<(), Rejection> {
    let weaker = |at: &str, what: String| {
        Err(Rejection::at(
            RejectionCode::InvalidManifest,
            format!("/manifest/requirements/{at}"),
            format!("The requirements understate the content: it needs {what}."),
        ))
    };
    if let Some(need) = needed.grid {
        let enough = declared
            .grid
            .is_some_and(|d| d.min_width >= need.min_width && d.min_height >= need.min_height);
        if !enough {
            return weaker(
                "grid",
                format!(
                    "a grid of at least {} by {}",
                    need.min_width, need.min_height
                ),
            );
        }
    }
    if declared.aux.top < needed.aux.top || declared.aux.side < needed.aux.side {
        return weaker(
            "aux",
            format!(
                "{} pads above and {} beside the grid",
                needed.aux.top, needed.aux.side
            ),
        );
    }
    if needed.rgb && !declared.rgb {
        return weaker("rgb", "colour".into());
    }
    Ok(())
}

/// A provider of two or more segments (`acme.notes`) is a plugin; one of one
/// segment (`system`, `agent-control`) is part of LaunchDeck.
fn is_plugin(provider: &str) -> bool {
    provider.contains('.')
}

/// Every plugin an assignment or a state rule uses is a dependency.
fn check_declared(manifest: &ManifestV1, content: &ContentV1) -> Result<(), Rejection> {
    let declared = |p: &str| manifest.dependencies.iter().any(|d| d.plugin_id == p);
    let used = assignments(content)
        .map(|(at, a)| (format!("{at}/provider"), a.provider.as_str()))
        .chain(controls(content).flat_map(|(at, c)| {
            c.feedback
                .iter()
                .enumerate()
                .filter_map(move |(i, rule)| match &rule.when {
                    When::State(s) => Some((
                        format!("{at}/feedback/{i}/when/state/provider"),
                        s.state.provider.as_str(),
                    )),
                    When::Event(_) => None,
                })
        }));
    for (at, provider) in used {
        if is_plugin(provider) && !declared(provider) {
            return Err(Rejection::at(
                RejectionCode::InvalidManifest,
                at,
                format!(
                    "The content uses the plugin \"{}\", which the manifest does not list as a dependency.",
                    echo(provider)
                ),
            ));
        }
    }
    Ok(())
}

fn unresolved(at: String, why: String) -> Rejection {
    Rejection::at(RejectionCode::InvalidContent, at, why)
}

/// Context rules name a surface and page of the file, a look a control has,
/// surface references a text argument naming a surface of the file, input
/// bindings a declared input of the right type.
fn check_references(manifest: &ManifestV1, content: &ContentV1) -> Result<(), Rejection> {
    let inputs: BTreeMap<&str, &InputDef> =
        manifest.inputs.iter().map(|i| (i.id.as_str(), i)).collect();
    for (i, rule) in content.context_rules.iter().enumerate() {
        let at = format!("/content/contextRules/{i}");
        let Some(surface) = content.surfaces.iter().find(|s| s.id == rule.surface) else {
            return Err(unresolved(
                format!("{at}/surface"),
                format!(
                    "A context rule shows the surface \"{}\", which is not in the file.",
                    echo(&rule.surface)
                ),
            ));
        };
        if let Some(page) = &rule.page {
            if !surface.pages.iter().any(|p| &p.id == page) {
                return Err(unresolved(
                    format!("{at}/page"),
                    format!(
                        "A context rule shows the page \"{}\", which its surface does not have.",
                        echo(page)
                    ),
                ));
            }
        }
        if let AppRefV1::Input(r) = &rule.application {
            match inputs.get(r.input.as_str()) {
                Some(def) if def.kind == InputType::Application => {}
                found => {
                    return Err(unresolved(
                        format!("{at}/application/input"),
                        input_problem(&r.input, found.is_some(), "context rule"),
                    ))
                }
            }
        }
    }
    for (at, control) in controls(content) {
        for (i, rule) in control.feedback.iter().enumerate() {
            let Some(look) = &rule.show else { continue };
            if look != IDLE && !control.looks.contains_key(look) {
                return Err(unresolved(
                    format!("{at}/feedback/{i}/show"),
                    format!(
                        "A feedback rule shows the look \"{}\", which its control does not have.",
                        echo(look)
                    ),
                ));
            }
        }
    }
    for (at, a) in assignments(content) {
        check_assignment_references(&at, a, content, &inputs)?;
    }
    Ok(())
}

/// Why a binding to `input` does not resolve, for a `bound_to` ("context
/// rule" or "argument").
fn input_problem(input: &str, declared: bool, bound_to: &str) -> String {
    if declared {
        format!(
            "The input \"{}\" cannot be bound to a {bound_to}: an application input binds to context rules, every other input to text arguments.",
            echo(input)
        )
    } else {
        format!(
            "A {bound_to} is bound to the input \"{}\", which the manifest does not declare.",
            echo(input)
        )
    }
}

fn check_assignment_references(
    at: &str,
    a: &AssignmentV1,
    content: &ContentV1,
    inputs: &BTreeMap<&str, &InputDef>,
) -> Result<(), Rejection> {
    for (i, arg) in a.surface_refs.iter().enumerate() {
        let names_surface = match a.args.get(arg) {
            Some(Value::String(id)) => content.surfaces.iter().any(|s| &s.id == id),
            _ => false,
        };
        if !names_surface {
            return Err(unresolved(
                format!("{at}/surfaceRefs/{i}"),
                format!(
                    "The argument \"{}\" is marked as naming a surface of the file, and does not.",
                    echo(arg)
                ),
            ));
        }
    }
    for (arg, input) in &a.inputs {
        let at = format!("{at}/inputs/{}", segment(arg));
        match inputs.get(input.as_str()) {
            Some(def) if def.kind != InputType::Application => {}
            found => {
                return Err(unresolved(
                    at,
                    input_problem(input, found.is_some(), "argument"),
                ))
            }
        }
        if !matches!(a.args.get(arg), Some(Value::String(_))) {
            return Err(unresolved(
                at,
                format!(
                    "The argument \"{}\" is bound to an input, and only a text argument can be.",
                    echo(arg)
                ),
            ));
        }
    }
    Ok(())
}
