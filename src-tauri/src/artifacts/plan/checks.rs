//! What the file needs of this installation (plan 7.6, 7.7, 7.16): its
//! plugins, its devices, the providers its pads and rules name, and what the
//! installed providers say of them. None of it ever stops an import: the
//! content is kept as the file has it, and a person is told what will not
//! work until something is installed or changed.

use std::collections::BTreeMap;

use serde_json::Value;

use super::inputs::Answers;
use super::types::{
    Compatibility, DependencyCheck, DependencyStatus, DeviceFit, PlanWarning, PlanWarningCode,
    UsedBy,
};
use super::walk::{self, Placed, Pressed};
use crate::actions::builtin::{request_destination, HTTP, HTTP_REQUEST};
use crate::actions::definition::{Assignment, ProviderStatus};
use crate::actions::params::{ParamKind, RefTarget};
use crate::artifacts::env::{Environment, KnownProvider};
use crate::artifacts::format::{
    echo, plain_message, ContentV1, ControlV1, DependencyV1, InputDef, InputValue, Pos, Span,
};
use crate::artifacts::inspect::ParsedArtifact;
use crate::artifacts::json::segment;
use crate::artifacts::review::visible;
use crate::core::model::ControlPos;
use crate::devices::DeviceCapabilities;
use crate::plugins::external::manifest::Version;
use crate::state::feedback::{prepare_rules, When};

use PlanWarningCode as W;

fn warn(warnings: &mut Vec<PlanWarning>, code: PlanWarningCode, message: String, at: String) {
    warnings.push(PlanWarning {
        code,
        message,
        at: Some(at),
    });
}

/// A provider something here is: one with actions, or one that offers
/// states or events.
fn known(env: &Environment, provider: &str) -> bool {
    env.providers.contains_key(provider) || env.observable_sources.contains(provider)
}

/// A provider of one segment is part of LaunchDeck; one of two or more is a plugin.
fn is_plugin(provider: &str) -> bool {
    provider.contains('.')
}

/// Each declared plugin, whether it is here in a version the file accepts,
/// and the controls that use it.
pub(super) fn dependencies(
    parsed: &ParsedArtifact,
    env: &Environment,
    warnings: &mut Vec<PlanWarning>,
) -> Vec<DependencyCheck> {
    let controls = walk::controls(&parsed.content);
    parsed
        .manifest
        .dependencies
        .iter()
        .enumerate()
        .map(|(i, dep)| {
            let at = format!("/manifest/dependencies/{i}");
            let installed = env.provider(&dep.plugin_id);
            // An installed plugin by its own name: the file's could be any.
            let name = installed
                .map(|p| p.name.clone())
                .or_else(|| dep.name.clone())
                .unwrap_or_else(|| dep.plugin_id.clone());
            let status = status(dep, installed);
            let used_by = used_by(&controls, &dep.plugin_id);
            if used_by.is_empty() {
                warn(
                    warnings,
                    W::UnusedDependency,
                    format!(
                        "The file lists the plugin {}, and no pad uses it.",
                        echo(&name)
                    ),
                    at.clone(),
                );
            }
            if let Some((code, message)) = status_warning(&name, &dep.version, &status) {
                warn(warnings, code, message, at);
            }
            DependencyCheck {
                plugin_id: dep.plugin_id.clone(),
                name: visible(&name, false),
                requirement: dep.version.clone(),
                required: dep.required,
                status,
                used_by,
            }
        })
        .collect()
}

fn status(dep: &DependencyV1, installed: Option<&KnownProvider>) -> DependencyStatus {
    let Some(plugin) = installed else {
        return DependencyStatus::Missing;
    };
    let version = plugin.version.clone();
    let accepted = semver::VersionReq::parse(&dep.version)
        .ok()
        .zip(Version::parse(&plugin.version))
        .is_some_and(|(req, v)| req.matches(&v.to_semver()));
    if !accepted {
        DependencyStatus::Incompatible { installed: version }
    } else if plugin.status == ProviderStatus::Ready {
        DependencyStatus::Compatible { installed: version }
    } else {
        DependencyStatus::Disabled { installed: version }
    }
}

fn status_warning(
    name: &str,
    requirement: &str,
    status: &DependencyStatus,
) -> Option<(PlanWarningCode, String)> {
    let name = echo(name);
    match status {
        DependencyStatus::Compatible { .. } => None,
        DependencyStatus::Missing => Some((
            W::MissingDependency,
            format!("Some pads will not work until you install {name} {requirement}."),
        )),
        DependencyStatus::Disabled { .. } => Some((
            W::DisabledDependency,
            format!("Some pads will not work until {name} is turned on and running in Plugins."),
        )),
        DependencyStatus::Incompatible { installed } => Some((
            W::IncompatibleDependency,
            format!(
                "{name} {} is installed and the file asks for {requirement}: some pads may not \
                 work until you install a version it accepts.",
                echo(installed)
            ),
        )),
    }
}

/// The controls whose pads or feedback name `provider`.
fn used_by(controls: &[Placed<'_>], provider: &str) -> Vec<UsedBy> {
    controls
        .iter()
        .filter(|p| uses(p.control, provider))
        .map(|p| UsedBy {
            surface_source_id: p.surface.id.clone(),
            page_id: p.page_id.to_string(),
            control_id: p.control.id.clone(),
            label: visible(&p.control.label, false),
        })
        .collect()
}

fn uses(control: &ControlV1, provider: &str) -> bool {
    let pads = [&control.press, &control.long_press]
        .into_iter()
        .flatten()
        .any(|a| a.provider == provider);
    let feedback = control.feedback.iter().any(|rule| match &rule.when {
        When::State(s) => s.state.provider == provider,
        When::Event(e) => e.event.provider == provider,
    });
    pads || feedback
}

/// How the file fits each device the runtime lists. Never blocks.
pub(super) fn compatibility(parsed: &ParsedArtifact, env: &Environment) -> Compatibility {
    let requirements = parsed.manifest.requirements;
    let controls = walk::controls(&parsed.content);
    let devices = env
        .devices
        .iter()
        .map(|d| {
            let outside = controls
                .iter()
                .filter(|p| !fits_on(p.control, &d.capabilities))
                .count();
            DeviceFit {
                name: d.name.clone(),
                model: d.model.clone(),
                kind: d.kind,
                connected: d.connected,
                fits: outside == 0,
                outside,
                rgb_ok: !requirements.rgb || d.capabilities.rgb,
            }
        })
        .collect();
    Compatibility {
        namespace: env.namespace.clone(),
        requirements,
        devices,
    }
}

/// Every pad the control covers is on the device.
fn fits_on(control: &ControlV1, caps: &DeviceCapabilities) -> bool {
    match control.pos {
        Pos::Grid { col, row } => {
            let span = control.span.unwrap_or(Span { w: 1, h: 1 });
            (0..span.w).all(|dx| {
                (0..span.h).all(|dy| {
                    caps.has_position(ControlPos::Grid {
                        col: col.saturating_add(dx),
                        row: row.saturating_add(dy),
                    })
                })
            })
        }
        Pos::Top { index } => caps.has_position(ControlPos::Top { index }),
        Pos::Side { index } => caps.has_position(ControlPos::Side { index }),
    }
}

/// Providers this version of LaunchDeck does not have, outside programs
/// feedback listens to, and pads that open a surface by an id the file
/// does not rename. A pad needs a provider with actions: one that only
/// offers states and events (the runtime, the devices) has none to run.
pub(super) fn provider_warnings(
    content: &ContentV1,
    env: &Environment,
    warnings: &mut Vec<PlanWarning>,
) {
    for p in walk::assignments(content) {
        let a = p.assignment;
        if !is_plugin(&a.provider) && env.provider(&a.provider).is_none() {
            warn(
                warnings,
                W::UnknownHostProvider,
                unknown_host(&a.provider, p.label),
                format!("{}/provider", p.pointer),
            );
        }
        external_surfaces(&p, env, warnings);
    }
    for placed in walk::controls(content) {
        for (i, rule) in placed.control.feedback.iter().enumerate() {
            let at = format!("{}/feedback/{i}/when", placed.pointer);
            match &rule.when {
                When::State(s)
                    if !is_plugin(&s.state.provider) && !known(env, &s.state.provider) =>
                {
                    warn(
                        warnings,
                        W::UnknownHostProvider,
                        unknown_host(&s.state.provider, &placed.control.label),
                        at,
                    )
                }
                When::Event(e) if !known(env, &e.event.provider) => warn(
                    warnings,
                    W::ExternalEventSource,
                    format!(
                        "The pad \"{}\" listens to events from \"{}\", which nothing in \
                         LaunchDeck sends: another program must send them.",
                        echo(&placed.control.label),
                        echo(&e.event.provider)
                    ),
                    at,
                ),
                _ => {}
            }
        }
    }
}

fn unknown_host(provider: &str, label: &str) -> String {
    format!(
        "The pad \"{}\" uses \"{}\", which is part of another version of LaunchDeck and not of \
         this one: it will not work here.",
        echo(label),
        echo(provider)
    )
}

/// An argument whose definition names a surface, which the file does not
/// mark as one of its own, keeps its value: it opens the surface of that id
/// here, if there is one.
fn external_surfaces(p: &Pressed<'_>, env: &Environment, warnings: &mut Vec<PlanWarning>) {
    let a = p.assignment;
    let Some(def) = env.action(&a.provider, &a.action) else {
        return;
    };
    for param in &def.params {
        let names_surface =
            param.kind == ParamKind::Reference && param.target == Some(RefTarget::Surface);
        let value = match a.args.get(&param.name) {
            Some(Value::String(v)) if !v.is_empty() => v,
            _ => continue,
        };
        if names_surface && !a.surface_refs.contains(&param.name) {
            warn(
                warnings,
                W::ExternalSurfaceReference,
                format!(
                    "The pad \"{}\" opens the surface \"{}\", which is not in this file: it opens \
                     a surface of that id here, if there is one.",
                    echo(p.label),
                    echo(value)
                ),
                format!("{}/args/{}", p.pointer, segment(&param.name)),
            );
        }
    }
}

/// What the installed providers say of the file's state rules, through the
/// hub's definitions. A rule of a plugin that is not here is the
/// dependency's to report.
pub(super) fn state_rules(content: &ContentV1, env: &Environment, warnings: &mut Vec<PlanWarning>) {
    let Some(host) = &env.host else {
        return;
    };
    for placed in walk::controls(content) {
        for (i, rule) in placed.control.feedback.iter().enumerate() {
            let When::State(s) = &rule.when else {
                continue;
            };
            if !known(env, &s.state.provider) {
                continue;
            }
            let checked = prepare_rules(std::slice::from_ref(rule), &[], &|provider, id| {
                host.state_definition(provider, id)
            });
            for issue in checked.err().unwrap_or_default() {
                warn(
                    warnings,
                    W::InvalidForInstalledVersion,
                    format!(
                        "A feedback rule of the pad \"{}\" does not fit what is installed here, \
                         and is kept as it is: {}.",
                        echo(&placed.control.label),
                        plain_message(&issue.message)
                    ),
                    format!("{}/feedback/{i}", placed.pointer),
                );
            }
        }
    }
}

/// What the installed provider says of a pad as it would be written. An
/// issue about an argument bound to an input belongs to the input, once it
/// is answered (or, optional, left without an answer); any other is a
/// warning, and the pad is kept as the file has it. An address given as an
/// answer is read as the runtime reads it, whatever the provider says.
pub(super) fn check_pad(
    p: &Pressed<'_>,
    written: &Assignment,
    env: &Environment,
    defs: &[InputDef],
    given: &BTreeMap<String, InputValue>,
    answers: &mut Answers,
    warnings: &mut Vec<PlanWarning>,
) {
    let bound_to = |param: &str| p.assignment.inputs.get(param);
    let answerable = |input: &str, answers: &Answers| {
        answers.values.contains_key(input)
            || (!given.contains_key(input) && defs.iter().any(|d| d.id == input && !d.required))
    };
    if let Some(host) = env
        .host
        .as_ref()
        .filter(|_| env.provider(&written.provider).is_some())
    {
        for issue in host.validate_assignment(written).err().unwrap_or_default() {
            let message = plain_message(&issue.message);
            match issue.param.as_deref().and_then(bound_to) {
                Some(input) if answerable(input, answers) => answers.add_issue(input, message),
                // Not answered yet: the input says it needs an answer.
                Some(_) => {}
                None => {
                    let at = match &issue.param {
                        Some(param) => format!("{}/args/{}", p.pointer, segment(param)),
                        None => p.pointer.clone(),
                    };
                    warn(
                        warnings,
                        W::InvalidForInstalledVersion,
                        format!(
                            "The pad \"{}\" does not fit what is installed here, and is kept as \
                             it is: {message}.",
                            echo(p.label)
                        ),
                        at,
                    );
                }
            }
        }
    }
    if (written.provider.as_str(), written.action.as_str()) == (HTTP, HTTP_REQUEST) {
        let input = bound_to("url").filter(|input| answers.values.contains_key(*input));
        if let (Some(input), Some(Value::String(url))) = (input, written.args.get("url")) {
            if let Err(refused) = request_destination(url) {
                answers.add_issue(input, refused.message());
            }
        }
    }
}
