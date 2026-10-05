//! What the interface is told about context: the settings, whether the
//! observer works, which application is in front, which rule matched and what
//! was done about it. Everything a person needs to see why a surface is on
//! screen, without reading a log. None of it is saved.

use serde::Serialize;

use super::engine::{Decision, Engine, Mode};
use super::identity::ApplicationIdentity;
use super::observer::{ObserverStatus, Seen};
use super::rules::ContextSettings;
use crate::core::model::Workspace;

/// What is in front now, as last seen.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Foreground {
    /// Nothing has been seen (the observer is not available, or has not reported).
    Unknown,
    /// No application: the desktop, or a window that says nothing about itself.
    Nothing,
    /// LaunchDeck's own window. It never counts as an application.
    LaunchDeck,
    Application {
        application: ApplicationIdentity,
    },
}

/// Something about a rule the person should know before relying on it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum RuleProblem {
    /// Its surface no longer exists. The rule still wins for its application, and shows nothing.
    MissingSurface,
    /// Its page no longer exists. The surface opens on its first page.
    MissingPage,
    /// An earlier rule names the same application, so this one is never reached.
    Shadowed { by: usize },
    /// It names an application of another kind of system, so it never matches on this one.
    OtherSystem,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextStatus {
    pub settings: ContextSettings,
    pub observer: ObserverStatus,
    pub mode: Mode,
    pub foreground: Foreground,
    /// The application the rules are asked about: the last one that came to
    /// the front and stayed. LaunchDeck itself and "nothing" never replace it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub application: Option<ApplicationIdentity>,
    /// What the rules said about it and what was done.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub decision: Option<Decision>,
    /// Applications seen in front since LaunchDeck started, newest first. For
    /// choosing one when writing a rule. Never saved.
    pub recent: Vec<ApplicationIdentity>,
    /// One entry per rule, in order: what is wrong with it, if anything.
    pub problems: Vec<Option<RuleProblem>>,
    /// The kind of identity applications have on this system.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub namespace: Option<&'static str>,
}

/// The first thing wrong with each rule, most serious first.
pub fn rule_problems(
    settings: &ContextSettings,
    workspace: &Workspace,
    namespace: Option<&str>,
) -> Vec<Option<RuleProblem>> {
    settings
        .rules
        .iter()
        .enumerate()
        .map(|(i, rule)| {
            if let Some(by) = settings.rules[..i]
                .iter()
                .position(|earlier| earlier.application.is(&rule.application))
            {
                return Some(RuleProblem::Shadowed { by });
            }
            let Some(surface) = workspace.surface(&rule.surface_id) else {
                return Some(RuleProblem::MissingSurface);
            };
            if rule
                .page_id
                .as_deref()
                .is_some_and(|p| surface.page(p).is_none())
            {
                return Some(RuleProblem::MissingPage);
            }
            if namespace.is_some_and(|ns| ns != rule.application.namespace) {
                return Some(RuleProblem::OtherSystem);
            }
            None
        })
        .collect()
}

pub fn status(engine: &Engine, settings: &ContextSettings, workspace: &Workspace) -> ContextStatus {
    let namespace = engine.namespace();
    ContextStatus {
        settings: settings.clone(),
        observer: engine.observer().clone(),
        mode: engine.mode(settings),
        foreground: match engine.observed() {
            None => Foreground::Unknown,
            Some(Seen::Nothing) => Foreground::Nothing,
            Some(Seen::Application { own: true, .. }) => Foreground::LaunchDeck,
            Some(Seen::Application { identity, .. }) => Foreground::Application {
                application: identity.clone(),
            },
        },
        application: engine.application().cloned(),
        decision: engine.decision().cloned(),
        recent: engine.recent(),
        problems: rule_problems(settings, workspace, namespace),
        namespace,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::identity::{MACOS, X11};
    use crate::context::rules::tests::rule;
    use crate::context::rules::{ContextRule, Fallback};
    use crate::surfaces::default_workspace;

    #[test]
    fn each_rule_is_told_what_is_wrong_with_it() {
        let ws = default_workspace();
        let settings = ContextSettings {
            auto: true,
            rules: vec![
                rule("Code", "development", None),
                rule("Code", "productivity", None),
                rule("obs", "removed", None),
                rule("term", "development", Some("removed-page")),
                ContextRule {
                    application: ApplicationIdentity::new(MACOS, "com.apple.Terminal", "Terminal"),
                    surface_id: "development".into(),
                    page_id: None,
                },
                rule("fine", "development", Some("git")),
            ],
            fallback: Fallback::Keep,
        };
        assert_eq!(
            rule_problems(&settings, &ws, Some(X11)),
            vec![
                None,
                Some(RuleProblem::Shadowed { by: 0 }),
                Some(RuleProblem::MissingSurface),
                Some(RuleProblem::MissingPage),
                Some(RuleProblem::OtherSystem),
                None,
            ]
        );
        // On a system with no observer nothing is "another system's".
        assert_eq!(rule_problems(&settings, &ws, None)[4], None);
    }

    #[test]
    fn the_status_says_what_is_in_front_without_keeping_any_of_it() {
        let ws = default_workspace();
        let settings = ContextSettings::default();
        let mut engine = Engine::new(ObserverStatus::available("test"), Some(X11), false);
        assert_eq!(
            status(&engine, &settings, &ws).foreground,
            Foreground::Unknown
        );
        let now = std::time::Instant::now();
        engine.observe(Seen::Nothing, now);
        assert_eq!(
            status(&engine, &settings, &ws).foreground,
            Foreground::Nothing
        );
        engine.observe(
            Seen::Application {
                identity: ApplicationIdentity::new(X11, "Launchdeck", "LaunchDeck"),
                own: true,
            },
            now,
        );
        let seen = status(&engine, &settings, &ws);
        assert_eq!(seen.foreground, Foreground::LaunchDeck);
        assert_eq!(seen.mode, Mode::Off);
        let json = serde_json::to_value(&seen).unwrap();
        assert_eq!(
            json["foreground"],
            serde_json::json!({ "kind": "launchDeck" })
        );
        assert_eq!(
            json["observer"],
            serde_json::json!({ "state": "available", "mechanism": "test" })
        );
        assert!(json.get("application").is_none() && json.get("decision").is_none());
    }
}
