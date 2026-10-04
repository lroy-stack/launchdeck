//! Context rules: which surface an application calls for.
//!
//! A rule is one application and one target. Rules are tried in order and the
//! first that names the application wins; when none does, the fallback says
//! what happens. Evaluation is a pure function of the settings, the workspace
//! and the application: it decides, it does not act.

use serde::{Deserialize, Serialize};

use super::identity::ApplicationIdentity;
use crate::core::model::Workspace;

pub const MAX_RULES: usize = 64;

/// `when application = …, activate surface (and page)`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextRule {
    pub application: ApplicationIdentity,
    pub surface_id: String,
    /// A page of that surface. Without one, the surface opens on its first
    /// page, or stays on the page it shows if it is already on screen.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub page_id: Option<String>,
}

/// What happens for an application no rule names.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Fallback {
    /// Nothing: the surface on screen stays.
    #[default]
    Keep,
    /// This surface is shown.
    #[serde(rename_all = "camelCase")]
    Surface {
        surface_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        page_id: Option<String>,
    },
}

/// Everything about context that is saved: the choice of Auto, the rules, the
/// fallback. Nothing about which application is or was in front.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextSettings {
    #[serde(default)]
    pub auto: bool,
    #[serde(default)]
    pub rules: Vec<ContextRule>,
    #[serde(default)]
    pub fallback: Fallback,
}

impl ContextSettings {
    /// Whether there is anything to write. A workspace that never used the
    /// engine is saved exactly as before.
    pub fn is_default(&self) -> bool {
        self == &ContextSettings::default()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SettingsError {
    #[error("more than {MAX_RULES} rules")]
    TooManyRules,
    #[error("rule {rule}: {reason}")]
    Rule { rule: usize, reason: String },
    #[error("the fallback names {0}")]
    Fallback(String),
}

fn target_problem(
    workspace: &Workspace,
    surface_id: &str,
    page_id: Option<&str>,
) -> Option<String> {
    let Some(surface) = workspace.surface(surface_id) else {
        return Some(format!("a surface that does not exist (`{surface_id}`)"));
    };
    match page_id {
        Some(page) if surface.page(page).is_none() => Some(format!(
            "a page `{page}` that the surface {} does not have",
            surface.name
        )),
        _ => None,
    }
}

/// Settings as they are saved: identities normalised, and everything that is
/// new checked. A rule that was valid when it was written and whose surface
/// was removed since is kept (`previous`), so it can still be seen and fixed.
pub fn checked(
    settings: &ContextSettings,
    previous: &ContextSettings,
    workspace: &Workspace,
) -> Result<ContextSettings, SettingsError> {
    if settings.rules.len() > MAX_RULES {
        return Err(SettingsError::TooManyRules);
    }
    let mut out = settings.clone();
    for (i, rule) in out.rules.iter_mut().enumerate() {
        let number = i + 1;
        rule.application
            .validate()
            .map_err(|e| SettingsError::Rule {
                rule: number,
                reason: e.to_string(),
            })?;
        rule.application = rule.application.normalized();
        if previous.rules.contains(rule) {
            continue;
        }
        if let Some(problem) = target_problem(workspace, &rule.surface_id, rule.page_id.as_deref())
        {
            return Err(SettingsError::Rule {
                rule: number,
                reason: format!("it names {problem}"),
            });
        }
    }
    if out.fallback != previous.fallback {
        if let Fallback::Surface {
            surface_id,
            page_id,
        } = &out.fallback
        {
            if let Some(problem) = target_problem(workspace, surface_id, page_id.as_deref()) {
                return Err(SettingsError::Fallback(problem));
            }
        }
    }
    Ok(out)
}

/// Why a decision came out as it did.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Matched {
    /// The rule at this position (from 0).
    Rule(usize),
    /// No rule named the application.
    Fallback,
}

/// A surface and a page to show.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Target {
    pub surface_id: String,
    /// `None`: whatever page the surface shows or opens on.
    pub page_id: Option<String>,
}

/// What the rules say for one application.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    /// Show this.
    Activate {
        matched: Matched,
        target: Target,
        /// The rule named a page the surface no longer has: its first page is shown.
        page_missing: bool,
    },
    /// The fallback is to keep what is shown.
    Keep,
    /// The rule (or the fallback) names a surface that no longer exists.
    /// Nothing is shown in its place, and no lower rule takes over.
    MissingSurface {
        matched: Matched,
        surface_id: String,
    },
}

fn resolve(
    workspace: &Workspace,
    matched: Matched,
    surface_id: &str,
    page_id: Option<&str>,
) -> Verdict {
    let Some(surface) = workspace.surface(surface_id) else {
        return Verdict::MissingSurface {
            matched,
            surface_id: surface_id.to_string(),
        };
    };
    let (page_id, page_missing) = match page_id {
        Some(page) if surface.page(page).is_some() => (Some(page.to_string()), false),
        // The page is gone: the surface's first page stands in.
        Some(_) => (surface.home_page().map(|p| p.id.clone()), true),
        None => (None, false),
    };
    Verdict::Activate {
        matched,
        target: Target {
            surface_id: surface_id.to_string(),
            page_id,
        },
        page_missing,
    }
}

/// The first rule that names `application` wins; otherwise the fallback.
pub fn evaluate(
    settings: &ContextSettings,
    workspace: &Workspace,
    application: &ApplicationIdentity,
) -> Verdict {
    match settings
        .rules
        .iter()
        .enumerate()
        .find(|(_, rule)| rule.application.is(application))
    {
        Some((i, rule)) => resolve(
            workspace,
            Matched::Rule(i),
            &rule.surface_id,
            rule.page_id.as_deref(),
        ),
        None => match &settings.fallback {
            Fallback::Keep => Verdict::Keep,
            Fallback::Surface {
                surface_id,
                page_id,
            } => resolve(workspace, Matched::Fallback, surface_id, page_id.as_deref()),
        },
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::context::identity::{MACOS, X11};
    use crate::surfaces::default_workspace;

    pub fn app(id: &str) -> ApplicationIdentity {
        ApplicationIdentity::new(X11, id, id)
    }

    pub fn rule(id: &str, surface: &str, page: Option<&str>) -> ContextRule {
        ContextRule {
            application: app(id),
            surface_id: surface.into(),
            page_id: page.map(str::to_string),
        }
    }

    fn settings(rules: Vec<ContextRule>, fallback: Fallback) -> ContextSettings {
        ContextSettings {
            auto: true,
            rules,
            fallback,
        }
    }

    fn activates(verdict: &Verdict) -> Option<(Matched, &str, Option<&str>)> {
        match verdict {
            Verdict::Activate {
                matched, target, ..
            } => Some((
                *matched,
                target.surface_id.as_str(),
                target.page_id.as_deref(),
            )),
            _ => None,
        }
    }

    #[test]
    fn an_application_with_a_rule_gets_its_surface() {
        let ws = default_workspace();
        let s = settings(
            vec![
                rule("Code", "development", None),
                rule("obs", "productivity", None),
            ],
            Fallback::Keep,
        );
        assert_eq!(
            activates(&evaluate(&s, &ws, &app("Code"))),
            Some((Matched::Rule(0), "development", None))
        );
        assert_eq!(
            activates(&evaluate(&s, &ws, &app("obs"))),
            Some((Matched::Rule(1), "productivity", None))
        );
        // The id is compared without regard to case, and the name not at all.
        let seen = ApplicationIdentity::new(X11, "code", "Something else entirely");
        assert_eq!(
            activates(&evaluate(&s, &ws, &seen)),
            Some((Matched::Rule(0), "development", None))
        );
        // The same id on another kind of system is another application.
        let mac = ApplicationIdentity::new(MACOS, "Code", "Code");
        assert_eq!(evaluate(&s, &ws, &mac), Verdict::Keep);
    }

    #[test]
    fn the_first_rule_that_matches_wins_and_order_is_the_only_precedence() {
        let ws = default_workspace();
        let first = settings(
            vec![
                rule("Code", "development", None),
                rule("Code", "productivity", None),
            ],
            Fallback::Keep,
        );
        assert_eq!(
            activates(&evaluate(&first, &ws, &app("Code"))),
            Some((Matched::Rule(0), "development", None))
        );
        // Reordered, the other one wins.
        let mut swapped = first.clone();
        swapped.rules.swap(0, 1);
        assert_eq!(
            activates(&evaluate(&swapped, &ws, &app("Code"))),
            Some((Matched::Rule(0), "productivity", None))
        );
    }

    #[test]
    fn without_a_rule_the_fallback_decides_and_says_so() {
        let ws = default_workspace();
        let keep = settings(vec![rule("Code", "development", None)], Fallback::Keep);
        assert_eq!(evaluate(&keep, &ws, &app("unknown")), Verdict::Keep);
        let home = settings(
            vec![rule("Code", "development", None)],
            Fallback::Surface {
                surface_id: "agent-control".into(),
                page_id: None,
            },
        );
        assert_eq!(
            activates(&evaluate(&home, &ws, &app("unknown"))),
            Some((Matched::Fallback, "agent-control", None))
        );
        // A rule still wins over the fallback.
        assert_eq!(
            activates(&evaluate(&home, &ws, &app("Code"))),
            Some((Matched::Rule(0), "development", None))
        );
        // With no rules at all, everything is the fallback.
        assert_eq!(
            evaluate(&settings(vec![], Fallback::Keep), &ws, &app("Code")),
            Verdict::Keep
        );
    }

    #[test]
    fn a_rule_may_name_a_page() {
        let ws = default_workspace();
        let page = ws.surface("development").unwrap().pages[1].id.clone();
        let s = settings(
            vec![rule("Code", "development", Some(&page))],
            Fallback::Keep,
        );
        assert_eq!(
            activates(&evaluate(&s, &ws, &app("Code"))),
            Some((Matched::Rule(0), "development", Some(page.as_str())))
        );
    }

    #[test]
    fn a_rule_whose_surface_is_gone_still_wins_and_shows_nothing() {
        let ws = default_workspace();
        let s = settings(
            vec![
                rule("Code", "removed", None),
                rule("Code", "development", None),
            ],
            Fallback::Surface {
                surface_id: "productivity".into(),
                page_id: None,
            },
        );
        // Not the lower rule, not the fallback: a broken rule is reported, not skipped.
        assert_eq!(
            evaluate(&s, &ws, &app("Code")),
            Verdict::MissingSurface {
                matched: Matched::Rule(0),
                surface_id: "removed".into()
            }
        );
        // The same for a fallback that names a surface that is gone.
        let s = settings(
            vec![],
            Fallback::Surface {
                surface_id: "removed".into(),
                page_id: None,
            },
        );
        assert_eq!(
            evaluate(&s, &ws, &app("Code")),
            Verdict::MissingSurface {
                matched: Matched::Fallback,
                surface_id: "removed".into()
            }
        );
    }

    #[test]
    fn a_rule_whose_page_is_gone_opens_the_surface_on_its_first_page() {
        let ws = default_workspace();
        let first = ws.surface("development").unwrap().pages[0].id.clone();
        let s = settings(
            vec![rule("Code", "development", Some("removed-page"))],
            Fallback::Keep,
        );
        assert_eq!(
            evaluate(&s, &ws, &app("Code")),
            Verdict::Activate {
                matched: Matched::Rule(0),
                target: Target {
                    surface_id: "development".into(),
                    page_id: Some(first),
                },
                page_missing: true
            }
        );
    }

    #[test]
    fn new_rules_are_checked_and_old_ones_are_kept_as_they_are() {
        let ws = default_workspace();
        let none = ContextSettings::default();
        let good = settings(vec![rule("Code", "development", None)], Fallback::Keep);
        assert_eq!(checked(&good, &none, &ws).unwrap(), good);

        let e = checked(
            &settings(vec![rule("Code", "nope", None)], Fallback::Keep),
            &none,
            &ws,
        )
        .unwrap_err();
        assert_eq!(
            e.to_string(),
            "rule 1: it names a surface that does not exist (`nope`)"
        );
        let e = checked(
            &settings(
                vec![rule("Code", "development", Some("nope"))],
                Fallback::Keep,
            ),
            &none,
            &ws,
        )
        .unwrap_err();
        assert!(
            e.to_string()
                .contains("a page `nope` that the surface Development does not have"),
            "{e}"
        );
        let mut bad = good.clone();
        bad.rules[0].application.id = String::new();
        assert_eq!(
            checked(&bad, &none, &ws).unwrap_err().to_string(),
            "rule 1: an application needs an identifier"
        );
        let e = checked(
            &settings(
                vec![],
                Fallback::Surface {
                    surface_id: "nope".into(),
                    page_id: None,
                },
            ),
            &none,
            &ws,
        )
        .unwrap_err();
        assert_eq!(
            e.to_string(),
            "the fallback names a surface that does not exist (`nope`)"
        );
        let many = settings(
            (0..65)
                .map(|i| rule(&format!("app{i}"), "development", None))
                .collect(),
            Fallback::Keep,
        );
        assert_eq!(
            checked(&many, &none, &ws).unwrap_err(),
            SettingsError::TooManyRules
        );

        // A rule that was saved while its surface existed survives a save of
        // something else (reordering, a new rule), so it can be seen and fixed.
        let stale = settings(vec![rule("Code", "removed", None)], Fallback::Keep);
        let mut more = stale.clone();
        more.rules.insert(0, rule("obs", "productivity", None));
        assert_eq!(checked(&more, &stale, &ws).unwrap().rules.len(), 2);
    }

    #[test]
    fn a_full_list_of_rules_is_evaluated_in_microseconds() {
        let ws = default_workspace();
        let full = settings(
            (0..MAX_RULES)
                .map(|i| rule(&format!("application-{i}"), "development", None))
                .collect(),
            Fallback::Keep,
        );
        // The worst case: the application no rule names, so every rule is compared.
        let stranger = app("not-in-the-list");
        let rounds = 10_000;
        let started = std::time::Instant::now();
        for _ in 0..rounds {
            assert_eq!(evaluate(&full, &ws, &stranger), Verdict::Keep);
        }
        let each = started.elapsed() / rounds;
        eprintln!("{MAX_RULES} rules, no match: {each:?} per evaluation (debug build)");
        assert!(each < std::time::Duration::from_millis(1), "{each:?}");
    }

    #[test]
    fn what_is_saved_is_the_choice_the_rules_and_the_fallback_and_nothing_else() {
        let s = settings(
            vec![rule("Code", "development", Some("main"))],
            Fallback::Surface {
                surface_id: "productivity".into(),
                page_id: None,
            },
        );
        let json = serde_json::to_value(&s).unwrap();
        assert_eq!(
            json,
            serde_json::json!({
                "auto": true,
                "rules": [{ "application": { "namespace": "x11", "id": "Code", "displayName": "Code" }, "surfaceId": "development", "pageId": "main" }],
                "fallback": { "kind": "surface", "surfaceId": "productivity" }
            })
        );
        assert_eq!(serde_json::from_value::<ContextSettings>(json).unwrap(), s);
        // Nothing in a file means: off, no rules, keep.
        let empty: ContextSettings = serde_json::from_value(serde_json::json!({})).unwrap();
        assert!(empty.is_default() && !empty.auto && empty.fallback == Fallback::Keep);
        assert_eq!(
            serde_json::to_value(Fallback::Keep).unwrap(),
            serde_json::json!({ "kind": "keep" })
        );
    }
}
