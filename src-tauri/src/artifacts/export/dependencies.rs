//! The plugins a file's content uses (plan 7.6). A plugin a press or long
//! press uses is required at a caret of the installed version; one only
//! feedback reads is optional. A plugin id that is not installed here is
//! declared for any version, with a warning. Host providers (one segment:
//! `system`, `shell`, `agent-control`...) are part of LaunchDeck and never
//! dependencies, and an event source that is not an installed plugin is an
//! outside program, not one either.

use std::collections::BTreeMap;

use super::preview::{Collected, ExportCode};
use crate::actions::definition::ProviderKind;
use crate::artifacts::env::{Environment, KnownProvider};
use crate::artifacts::format::{ContentV1, DependencyV1};
use crate::artifacts::limits::MAX_DEPENDENCY_NAME;
use crate::plugins::external::manifest::Version;
use crate::state::feedback::When;

const ANY_VERSION: &str = "*";

/// How the content uses one provider, and where first.
struct Use {
    required: bool,
    /// By a state rule (always a declared dependency) and not only by event rules.
    beyond_events: bool,
    at: String,
}

pub(super) fn dependencies(
    content: &ContentV1,
    env: &Environment,
    found: &mut Collected,
) -> Vec<DependencyV1> {
    uses(content)
        .into_iter()
        .filter(|(id, _)| id.contains('.'))
        .filter_map(|(id, used)| {
            let installed = env.provider(&id).filter(|p| p.kind == ProviderKind::Plugin);
            if installed.is_none() && !used.beyond_events {
                return None;
            }
            let version = installed.and_then(caret);
            if version.is_none() {
                let why = match installed {
                    Some(_) => "its installed version cannot be read",
                    None => "it is not installed here",
                };
                found.warn(
                    ExportCode::NotInstalled,
                    format!(
                        "The file uses the plugin \"{id}\", and {why}, so it accepts any version of it."
                    ),
                    Some(used.at),
                );
            }
            Some(DependencyV1 {
                name: installed.and_then(|p| display_name(&p.name)),
                version: version.unwrap_or_else(|| ANY_VERSION.into()),
                required: used.required,
                plugin_id: id,
            })
        })
        .collect()
}

/// Every provider an assignment or a feedback rule names, by id.
fn uses(content: &ContentV1) -> BTreeMap<String, Use> {
    let mut found: BTreeMap<String, Use> = BTreeMap::new();
    let mut note = |id: &str, required: bool, beyond_events: bool, at: String| {
        let used = found.entry(id.to_string()).or_insert(Use {
            required: false,
            beyond_events: false,
            at,
        });
        used.required |= required;
        used.beyond_events |= beyond_events;
    };
    for (s, surface) in content.surfaces.iter().enumerate() {
        for (p, page) in surface.pages.iter().enumerate() {
            for (c, control) in page.controls.iter().enumerate() {
                let at = format!("/content/surfaces/{s}/pages/{p}/controls/{c}");
                for (slot, a) in [
                    ("press", &control.press),
                    ("longPress", &control.long_press),
                ] {
                    if let Some(a) = a {
                        note(&a.provider, true, true, format!("{at}/{slot}/provider"));
                    }
                }
                for (i, rule) in control.feedback.iter().enumerate() {
                    let at = format!("{at}/feedback/{i}/when");
                    match &rule.when {
                        When::State(s) => note(&s.state.provider, false, true, at),
                        When::Event(e) => note(&e.event.provider, false, false, at),
                    }
                }
            }
        }
    }
    found
}

/// `^` and the installed version (plugin manifests write theirs in their
/// own grammar, read here as SemVer).
fn caret(plugin: &KnownProvider) -> Option<String> {
    Version::parse(&plugin.version).map(|v| format!("^{}", v.to_semver()))
}

/// A plugin's name, when the format can show it.
fn display_name(name: &str) -> Option<String> {
    let fits = name == name.trim()
        && (1..=MAX_DEPENDENCY_NAME).contains(&name.chars().count())
        && !name.chars().any(char::is_control);
    fits.then(|| name.to_string())
}
