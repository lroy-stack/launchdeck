//! The Auto / Manual Override state machine.
//!
//! It is told what the observer saw, what the person did and what time it is,
//! and answers with at most one thing: a surface (and page) to show now. It
//! holds no handle on the runtime, a device or a clock, so every rule about
//! *when* a surface may change by itself is in this file and nowhere else.
//!
//! A decision is applied at exactly five moments: a different application is
//! accepted, Resume Auto, Auto switched on, the rules changed, and Edit left
//! for Live. Between them nothing changes by itself, which is why a choice
//! made by hand holds.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use serde::Serialize;

use super::identity::ApplicationIdentity;
use super::observer::{ObserverStatus, Seen};
use super::rules::{evaluate, ContextSettings, Matched, Target, Verdict};
use crate::core::model::Workspace;

/// How long an application has to stay in front before it counts.
pub const SETTLE: Duration = Duration::from_millis(150);
/// How many applications the rule picker is offered. In memory only.
pub const RECENT: usize = 12;

/// What the engine needs to know about the rest of LaunchDeck to decide.
pub struct World<'a> {
    pub settings: &'a ContextSettings,
    pub workspace: &'a Workspace,
    /// The surface and page on screen now.
    pub surface: &'a str,
    pub page: &'a str,
}

/// Who decides what is shown, in one word.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Mode {
    /// Auto Context is switched off.
    Off,
    /// The application in front decides.
    Auto,
    /// A surface was chosen by hand; it holds until another application comes
    /// to the front or Resume Auto is pressed.
    Manual,
    /// Edit mode: nothing changes by itself.
    Paused,
    /// Auto is on, and this system cannot tell which application is in front.
    Unavailable,
}

impl Mode {
    pub fn as_str(self) -> &'static str {
        match self {
            Mode::Off => "off",
            Mode::Auto => "auto",
            Mode::Manual => "manual",
            Mode::Paused => "paused",
            Mode::Unavailable => "unavailable",
        }
    }
}

/// What the rules called for, compared with what was on screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Outcome {
    /// Another surface or page: it is activated, unless something holds it back.
    Activated,
    /// What they call for is already shown.
    AlreadyShown,
    /// No rule names the application and the fallback is to keep what is shown.
    Kept,
    /// The rule or the fallback names a surface that no longer exists.
    MissingSurface,
}

/// Why a decision was not acted on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Held {
    /// Auto Context is off.
    Off,
    /// Edit mode.
    Paused,
    /// A surface chosen by hand is being kept.
    Manual,
}

/// Why the surface is what it is: the application the rules were asked about,
/// what they said, and what was done about it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Decision {
    pub application: ApplicationIdentity,
    /// The rule that matched, counted from 0. Absent when none did.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rule: Option<usize>,
    pub outcome: Outcome,
    /// Set when the decision was not acted on, with the reason.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub held: Option<Held>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub surface_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub page_id: Option<String>,
    /// The rule named a page that is gone; the surface's first page stood in.
    pub page_missing: bool,
}

#[derive(Debug)]
pub struct Engine {
    observer: ObserverStatus,
    /// The kind of identity the observer reports, so that a rule of another
    /// kind, which can never match, is pointed out.
    namespace: Option<&'static str>,
    /// The last thing the observer reported, whatever it was.
    observed: Option<Seen>,
    /// An application that came to the front and has not stayed there long enough yet.
    pending: Option<(ApplicationIdentity, Instant)>,
    /// The application rules are evaluated against.
    accepted: Option<ApplicationIdentity>,
    overridden: bool,
    editing: bool,
    decision: Option<Decision>,
    recent: VecDeque<ApplicationIdentity>,
}

impl Engine {
    pub fn new(observer: ObserverStatus, namespace: Option<&'static str>, editing: bool) -> Engine {
        Engine {
            observer,
            namespace,
            observed: None,
            pending: None,
            accepted: None,
            overridden: false,
            editing,
            decision: None,
            recent: VecDeque::new(),
        }
    }

    pub fn observer(&self) -> &ObserverStatus {
        &self.observer
    }

    pub fn set_observer(&mut self, status: ObserverStatus) {
        self.observer = status;
    }

    pub fn namespace(&self) -> Option<&'static str> {
        self.namespace
    }

    /// What is in front now, as last reported. Not what rules are asked about.
    pub fn observed(&self) -> Option<&Seen> {
        self.observed.as_ref()
    }

    /// The application rules are evaluated against.
    pub fn application(&self) -> Option<&ApplicationIdentity> {
        self.accepted.as_ref()
    }

    pub fn decision(&self) -> Option<&Decision> {
        self.decision.as_ref()
    }

    /// Applications seen in front since LaunchDeck started, newest first.
    pub fn recent(&self) -> Vec<ApplicationIdentity> {
        self.recent.iter().cloned().collect()
    }

    pub fn overridden(&self) -> bool {
        self.overridden
    }

    pub fn mode(&self, settings: &ContextSettings) -> Mode {
        if !settings.auto {
            Mode::Off
        } else if !self.observer.can_observe() {
            Mode::Unavailable
        } else if self.editing {
            Mode::Paused
        } else if self.overridden {
            Mode::Manual
        } else {
            Mode::Auto
        }
    }

    /// The observer saw something. Returns when [`settle`](Engine::settle)
    /// should be called, if this may become the accepted application.
    pub fn observe(&mut self, seen: Seen, now: Instant) -> Option<Instant> {
        let candidate = match &seen {
            // LaunchDeck's own window and "nothing in front" are not
            // applications: the accepted one stays what it was.
            Seen::Application {
                identity,
                own: false,
            } => Some(identity.clone()),
            _ => None,
        };
        self.observed = Some(seen);
        let Some(candidate) = candidate else {
            // Whatever was on its way to being accepted did not stay in front.
            self.pending = None;
            return None;
        };
        if self.accepted.as_ref().is_some_and(|a| a.is(&candidate)) {
            // Back to (or still) the accepted application: nothing to wait for.
            self.pending = None;
            // Its name may have changed; the identity has not.
            self.accepted = Some(candidate);
            return None;
        }
        let since = match &self.pending {
            Some((waiting, since)) if waiting.is(&candidate) => *since,
            _ => now,
        };
        self.pending = Some((candidate, since));
        Some(since + SETTLE)
    }

    /// Time has passed. If an application has stayed in front long enough it
    /// becomes the accepted one, any manual override ends, and its rule is
    /// applied.
    pub fn settle(&mut self, now: Instant, world: &World) -> Option<Target> {
        match &self.pending {
            Some((_, since)) if now.duration_since(*since) >= SETTLE => {}
            _ => return None,
        }
        let (application, _) = self.pending.take()?;
        self.recent.retain(|a| !a.is(&application));
        self.recent.push_front(application.clone());
        self.recent.truncate(RECENT);
        self.accepted = Some(application);
        // Another application came to the front: what was chosen by hand for
        // the last one no longer holds.
        self.overridden = false;
        self.apply(world)
    }

    /// A surface or page was activated by something other than this engine.
    /// `changed`: it actually changed what is shown.
    pub fn manual(&mut self, changed: bool, world: &World) {
        if changed && world.settings.auto && !self.editing && self.observer.can_observe() {
            self.overridden = true;
            if let Some(decision) = &mut self.decision {
                decision.held = Some(Held::Manual);
            }
        }
    }

    /// Resume Auto: the override ends and the current application's rule is applied now.
    pub fn resume(&mut self, world: &World) -> Option<Target> {
        self.overridden = false;
        self.apply(world)
    }

    /// The settings changed: Auto switched on or off, a rule added, moved or
    /// removed, another fallback. The current application's rule is applied
    /// under the new settings, so a rule written for the application just used
    /// takes effect when it is saved. A surface chosen by hand is still kept,
    /// except that switching Auto on (or off) starts afresh.
    pub fn settings_changed(&mut self, was_auto: bool, world: &World) -> Option<Target> {
        if world.settings.auto != was_auto {
            self.overridden = false;
        }
        self.apply(world)
    }

    /// The mode changed. Leaving Edit applies the current application's rule
    /// once, unless a surface chosen by hand is being kept.
    pub fn set_editing(&mut self, editing: bool, world: &World) -> Option<Target> {
        let was = std::mem::replace(&mut self.editing, editing);
        if was && !editing {
            return self.apply(world);
        }
        None
    }

    /// Ask the rules about the accepted application and, if nothing holds it
    /// back, say what to show. Records the decision either way.
    fn apply(&mut self, world: &World) -> Option<Target> {
        let application = self.accepted.clone()?;
        let verdict = evaluate(world.settings, world.workspace, &application);
        let held = if !world.settings.auto {
            Some(Held::Off)
        } else if self.editing {
            Some(Held::Paused)
        } else if self.overridden {
            Some(Held::Manual)
        } else {
            None
        };
        let (rule, outcome, target, page_missing) = match verdict {
            Verdict::Keep => (None, Outcome::Kept, None, false),
            Verdict::MissingSurface {
                matched,
                surface_id,
            } => (
                rule_of(matched),
                Outcome::MissingSurface,
                Some(Target {
                    surface_id,
                    page_id: None,
                }),
                false,
            ),
            Verdict::Activate {
                matched,
                target,
                page_missing,
            } => {
                let shown = target.surface_id == world.surface
                    && target.page_id.as_deref().is_none_or(|p| p == world.page);
                let outcome = if shown {
                    Outcome::AlreadyShown
                } else {
                    Outcome::Activated
                };
                (rule_of(matched), outcome, Some(target), page_missing)
            }
        };
        let activate = outcome == Outcome::Activated && held.is_none();
        self.decision = Some(Decision {
            application,
            rule,
            outcome,
            held,
            surface_id: target.as_ref().map(|t| t.surface_id.clone()),
            page_id: target.as_ref().and_then(|t| t.page_id.clone()),
            page_missing,
        });
        if activate {
            target
        } else {
            None
        }
    }
}

fn rule_of(matched: Matched) -> Option<usize> {
    match matched {
        Matched::Rule(i) => Some(i),
        Matched::Fallback => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::identity::X11;
    use crate::context::rules::tests::{app, rule};
    use crate::context::rules::Fallback;
    use crate::surfaces::default_workspace;

    const CODE: &str = "Code";
    const TERMINAL: &str = "Gnome-terminal";
    const OBS: &str = "obs";

    /// The engine with everything it is told about, so a test reads as a story:
    /// what came to the front, what the person did, what was shown.
    struct Deck {
        engine: Engine,
        settings: ContextSettings,
        workspace: Workspace,
        surface: String,
        page: String,
        now: Instant,
        /// Every activation the engine asked for, in order.
        activations: Vec<(String, Option<String>)>,
    }

    impl Deck {
        fn new(rules: Vec<crate::context::rules::ContextRule>, fallback: Fallback) -> Deck {
            Deck {
                engine: Engine::new(ObserverStatus::available("test"), Some(X11), false),
                settings: ContextSettings {
                    auto: true,
                    rules,
                    fallback,
                },
                workspace: default_workspace(),
                surface: "agent-control".into(),
                page: "home".into(),
                now: Instant::now(),
                activations: vec![],
            }
        }

        /// Code opens Development, the terminal opens Productivity.
        fn usual() -> Deck {
            Deck::new(
                vec![
                    rule(CODE, "development", None),
                    rule(TERMINAL, "productivity", None),
                ],
                Fallback::Keep,
            )
        }

        fn with<R>(&mut self, f: impl FnOnce(&mut Engine, &World, Instant) -> R) -> R {
            let world = World {
                settings: &self.settings,
                workspace: &self.workspace,
                surface: &self.surface,
                page: &self.page,
            };
            f(&mut self.engine, &world, self.now)
        }

        /// What the runtime does with an answer: activate it, as the engine's own.
        fn show(&mut self, target: Option<Target>) {
            let Some(target) = target else { return };
            let surface = self
                .workspace
                .surface(&target.surface_id)
                .expect("the engine only names surfaces that exist");
            let page = target
                .page_id
                .clone()
                .unwrap_or_else(|| surface.home_page().unwrap().id.clone());
            self.surface = target.surface_id.clone();
            self.page = page;
            self.activations.push((target.surface_id, target.page_id));
        }

        fn pass(&mut self, time: Duration) {
            self.now += time;
            let target = self.with(|e, w, now| e.settle(now, w));
            self.show(target);
        }

        /// Something is seen in front, without waiting.
        fn see(&mut self, seen: Seen) {
            let now = self.now;
            self.engine.observe(seen, now);
        }

        /// An application comes to the front and stays there.
        fn front(&mut self, id: &str) {
            self.see(Seen::Application {
                identity: app(id),
                own: false,
            });
            self.pass(SETTLE);
        }

        fn launchdeck_in_front(&mut self) {
            self.see(Seen::Application {
                identity: ApplicationIdentity::new(X11, "Launchdeck", "LaunchDeck"),
                own: true,
            });
            self.pass(SETTLE);
        }

        /// The person picks a surface themselves.
        fn pick(&mut self, surface: &str) {
            let changed = self.surface != surface;
            self.surface = surface.into();
            self.page = self
                .workspace
                .surface(surface)
                .unwrap()
                .home_page()
                .unwrap()
                .id
                .clone();
            self.with(|e, w, _| e.manual(changed, w));
        }

        fn resume(&mut self) {
            let target = self.with(|e, w, _| e.resume(w));
            self.show(target);
        }

        fn edit(&mut self, editing: bool) {
            let target = self.with(|e, w, _| e.set_editing(editing, w));
            self.show(target);
        }

        fn change_settings(&mut self, change: impl FnOnce(&mut ContextSettings)) {
            let was = self.settings.auto;
            change(&mut self.settings);
            let target = self.with(|e, w, _| e.settings_changed(was, w));
            self.show(target);
        }

        fn mode(&self) -> Mode {
            self.engine.mode(&self.settings)
        }

        fn decision(&self) -> &Decision {
            self.engine.decision().expect("a decision was made")
        }

        fn shown(&self) -> &str {
            &self.surface
        }

        fn count(&self) -> usize {
            self.activations.len()
        }
    }

    // -- the application in front decides ------------------------------------

    #[test]
    fn app_a_shows_surface_a_and_app_b_shows_surface_b() {
        let mut d = Deck::usual();
        d.front(CODE);
        assert_eq!(d.shown(), "development");
        assert_eq!(d.engine.application(), Some(&app(CODE)));
        let decision = d.decision();
        assert_eq!(
            (decision.rule, decision.outcome, decision.held),
            (Some(0), Outcome::Activated, None)
        );
        assert_eq!(decision.surface_id.as_deref(), Some("development"));

        d.front(TERMINAL);
        assert_eq!(d.shown(), "productivity");
        assert_eq!(d.decision().rule, Some(1));
        d.front(CODE);
        assert_eq!(d.shown(), "development");
        assert_eq!(d.count(), 3);
        assert_eq!(d.mode(), Mode::Auto);
    }

    #[test]
    fn an_application_without_a_rule_gets_the_fallback() {
        // Keep: nothing changes.
        let mut d = Deck::usual();
        d.front(CODE);
        d.front(OBS);
        assert_eq!(d.shown(), "development", "the surface on screen stays");
        assert_eq!(d.count(), 1);
        let decision = d.decision().clone();
        assert_eq!((decision.rule, decision.outcome), (None, Outcome::Kept));
        assert_eq!(decision.application, app(OBS));

        // A surface: it is shown.
        let mut d = Deck::new(
            vec![rule(CODE, "development", None)],
            Fallback::Surface {
                surface_id: "smart-home".into(),
                page_id: None,
            },
        );
        d.front(CODE);
        d.front(OBS);
        assert_eq!(d.shown(), "smart-home");
        assert_eq!(
            (d.decision().rule, d.decision().outcome),
            (None, Outcome::Activated)
        );
        d.front(CODE);
        assert_eq!(d.shown(), "development");
    }

    #[test]
    fn the_same_application_reported_again_does_nothing() {
        let mut d = Deck::usual();
        d.front(CODE);
        let before = (d.count(), d.decision().clone(), d.engine.recent());
        for _ in 0..5 {
            d.front(CODE);
        }
        // Under another name it is still the same application.
        d.see(Seen::Application {
            identity: ApplicationIdentity::new(X11, "code", "Visual Studio Code"),
            own: false,
        });
        d.pass(SETTLE);
        assert_eq!((d.count(), d.decision().clone(), d.engine.recent()), before);
        assert_eq!(d.shown(), "development");
        assert_eq!(d.mode(), Mode::Auto);
    }

    #[test]
    fn a_rule_whose_target_is_already_shown_activates_nothing() {
        let mut d = Deck::usual();
        d.settings.rules.push(rule(OBS, "development", None));
        d.front(CODE);
        d.front(OBS);
        assert_eq!(d.count(), 1, "Development was already on screen");
        assert_eq!(
            (d.decision().rule, d.decision().outcome),
            (Some(2), Outcome::AlreadyShown)
        );
    }

    #[test]
    fn a_rule_with_a_page_opens_that_page_and_one_without_leaves_the_page_alone() {
        let mut d = Deck::new(
            vec![
                rule(CODE, "development", Some("git")),
                rule(TERMINAL, "development", None),
            ],
            Fallback::Keep,
        );
        d.front(CODE);
        assert_eq!((d.shown(), d.page.as_str()), ("development", "git"));
        // The terminal's rule names no page: Development is on screen, so nothing moves.
        d.front(TERMINAL);
        assert_eq!((d.page.as_str(), d.count()), ("git", 1));
        // Back to Code while on another page of the same surface: its page is opened.
        d.page = "home".into();
        d.front(CODE);
        assert_eq!((d.page.as_str(), d.count()), ("git", 2));
    }

    #[test]
    fn a_rule_whose_surface_is_gone_changes_nothing_and_says_why() {
        let mut d = Deck::new(
            vec![rule(CODE, "removed", None)],
            Fallback::Surface {
                surface_id: "productivity".into(),
                page_id: None,
            },
        );
        d.front(CODE);
        assert_eq!((d.shown(), d.count()), ("agent-control", 0));
        let decision = d.decision();
        assert_eq!(
            (decision.rule, decision.outcome),
            (Some(0), Outcome::MissingSurface)
        );
        assert_eq!(decision.surface_id.as_deref(), Some("removed"));
    }

    // -- a choice made by hand ------------------------------------------------

    #[test]
    fn a_manual_switch_is_not_undone_while_the_same_application_stays_in_front() {
        let mut d = Deck::usual();
        d.front(CODE);
        d.pick("smart-home");
        assert_eq!(d.mode(), Mode::Manual);
        assert_eq!(d.decision().held, Some(Held::Manual));

        // The same application, again and again, and time passing: it holds.
        for _ in 0..3 {
            d.front(CODE);
            d.pass(Duration::from_secs(60));
        }
        assert_eq!((d.shown(), d.count()), ("smart-home", 1));
        assert_eq!(d.mode(), Mode::Manual);
    }

    #[test]
    fn going_to_the_launchdeck_window_and_back_keeps_a_manual_choice() {
        let mut d = Deck::usual();
        d.front(CODE);
        // The person opens LaunchDeck, picks a surface there, and returns to their editor.
        d.launchdeck_in_front();
        assert_eq!(
            d.engine.application(),
            Some(&app(CODE)),
            "LaunchDeck in front is not an application"
        );
        assert_eq!(d.count(), 1);
        d.pick("smart-home");
        d.front(CODE);
        assert_eq!((d.shown(), d.mode()), ("smart-home", Mode::Manual));
    }

    #[test]
    fn a_different_application_ends_the_override_and_its_rule_applies() {
        let mut d = Deck::usual();
        d.front(CODE);
        d.pick("smart-home");
        d.front(TERMINAL);
        assert_eq!((d.shown(), d.mode()), ("productivity", Mode::Auto));
        // And coming back to the first one applies its rule again.
        d.front(CODE);
        assert_eq!(d.shown(), "development");

        // An application without a rule ends it too; with Keep, what is shown stays.
        d.pick("smart-home");
        d.front(OBS);
        assert_eq!((d.shown(), d.mode()), ("smart-home", Mode::Auto));
    }

    #[test]
    fn resume_auto_applies_the_current_applications_rule_at_once() {
        let mut d = Deck::usual();
        d.front(CODE);
        d.launchdeck_in_front();
        d.pick("smart-home");
        assert_eq!(d.mode(), Mode::Manual);
        d.resume();
        assert_eq!((d.shown(), d.mode()), ("development", Mode::Auto));
        assert_eq!(
            (d.decision().outcome, d.decision().held),
            (Outcome::Activated, None)
        );
        // Resuming when nothing is overridden changes nothing.
        d.resume();
        assert_eq!(d.count(), 2);
    }

    #[test]
    fn picking_what_is_already_shown_is_not_an_override() {
        let mut d = Deck::usual();
        d.front(CODE);
        d.pick("development");
        assert_eq!(d.mode(), Mode::Auto);
    }

    // -- Edit mode -------------------------------------------------------------

    #[test]
    fn edit_mode_activates_nothing_and_keeps_observing() {
        let mut d = Deck::usual();
        d.front(CODE);
        d.edit(true);
        assert_eq!(d.mode(), Mode::Paused);
        // Choosing what to edit is not an override.
        d.pick("accessibility");
        d.front(TERMINAL);
        assert_eq!(
            (d.shown(), d.count()),
            ("accessibility", 1),
            "nothing was activated"
        );
        // The context kept up, and says what it would do.
        assert_eq!(d.engine.application(), Some(&app(TERMINAL)));
        let decision = d.decision();
        assert_eq!(
            (decision.rule, decision.outcome, decision.held),
            (Some(1), Outcome::Activated, Some(Held::Paused))
        );
        assert!(!d.engine.overridden());
    }

    #[test]
    fn leaving_edit_applies_the_current_context_once() {
        let mut d = Deck::usual();
        d.front(CODE);
        d.edit(true);
        d.pick("accessibility");
        d.front(TERMINAL);
        d.front(OBS);
        d.front(TERMINAL);
        d.edit(false);
        assert_eq!(
            (d.shown(), d.count()),
            ("productivity", 2),
            "one activation, for the application in front now"
        );
        assert_eq!(d.mode(), Mode::Auto);
        // Time passing afterwards does nothing more.
        d.pass(Duration::from_secs(5));
        assert_eq!(d.count(), 2);
    }

    #[test]
    fn leaving_edit_keeps_a_surface_that_was_chosen_by_hand_before() {
        let mut d = Deck::usual();
        d.front(CODE);
        d.pick("smart-home");
        d.edit(true);
        d.edit(false);
        assert_eq!((d.shown(), d.mode()), ("smart-home", Mode::Manual));
    }

    // -- stability ---------------------------------------------------------------

    #[test]
    fn an_application_that_flashes_past_is_never_accepted() {
        let mut d = Deck::usual();
        d.front(CODE);
        // The terminal is in front for 100 ms, then Code again.
        d.see(Seen::Application {
            identity: app(TERMINAL),
            own: false,
        });
        d.pass(Duration::from_millis(100));
        assert_eq!(d.shown(), "development", "not long enough yet");
        d.see(Seen::Application {
            identity: app(CODE),
            own: false,
        });
        d.pass(Duration::from_secs(1));
        assert_eq!((d.shown(), d.count()), ("development", 1));
        assert_eq!(
            d.engine.recent(),
            vec![app(CODE)],
            "it is not remembered either"
        );

        // In front for 100 ms and then nothing in front: not accepted.
        d.see(Seen::Application {
            identity: app(TERMINAL),
            own: false,
        });
        d.pass(Duration::from_millis(100));
        d.see(Seen::Nothing);
        d.pass(Duration::from_secs(1));
        assert_eq!(d.count(), 1);

        // Reported twice while waiting: the wait is counted from the first.
        d.see(Seen::Application {
            identity: app(TERMINAL),
            own: false,
        });
        d.pass(Duration::from_millis(100));
        d.see(Seen::Application {
            identity: app(TERMINAL),
            own: false,
        });
        d.pass(Duration::from_millis(60));
        assert_eq!(d.shown(), "productivity");
    }

    #[test]
    fn observing_says_when_to_look_again_and_only_when_there_is_something_to_wait_for() {
        let mut d = Deck::usual();
        let now = d.now;
        let wait = d.engine.observe(
            Seen::Application {
                identity: app(CODE),
                own: false,
            },
            now,
        );
        assert_eq!(wait, Some(now + SETTLE));
        d.pass(SETTLE);
        assert_eq!(
            d.engine.observe(
                Seen::Application {
                    identity: app(CODE),
                    own: false
                },
                d.now
            ),
            None
        );
        assert_eq!(d.engine.observe(Seen::Nothing, d.now), None);
        assert_eq!(
            d.engine.observe(
                Seen::Application {
                    identity: app("Launchdeck"),
                    own: true
                },
                d.now
            ),
            None
        );
    }

    #[test]
    fn nothing_in_front_and_launchdeck_itself_never_change_the_surface() {
        let mut d = Deck::new(
            vec![rule(CODE, "development", None)],
            Fallback::Surface {
                surface_id: "smart-home".into(),
                page_id: None,
            },
        );
        d.front(CODE);
        d.see(Seen::Nothing);
        d.pass(Duration::from_secs(10));
        d.launchdeck_in_front();
        d.pass(Duration::from_secs(10));
        assert_eq!(
            (d.shown(), d.count()),
            ("development", 1),
            "not even the fallback: these are not applications"
        );
        // What is in front is still known, for the person to see.
        assert!(matches!(
            d.engine.observed(),
            Some(Seen::Application { own: true, .. })
        ));
    }

    // -- Auto on and off ---------------------------------------------------------

    #[test]
    fn with_auto_off_nothing_is_activated_and_the_context_is_still_known() {
        let mut d = Deck::usual();
        d.settings.auto = false;
        d.front(CODE);
        assert_eq!(
            (d.shown(), d.count(), d.mode()),
            ("agent-control", 0, Mode::Off)
        );
        assert_eq!(d.engine.application(), Some(&app(CODE)));
        assert_eq!(d.decision().held, Some(Held::Off));
        // A manual switch with Auto off is just a switch.
        d.pick("smart-home");
        assert!(!d.engine.overridden());

        // Switching it on applies the current application's rule.
        d.change_settings(|s| s.auto = true);
        assert_eq!((d.shown(), d.mode()), ("development", Mode::Auto));
        // Switching it off leaves what is shown.
        d.change_settings(|s| s.auto = false);
        d.front(TERMINAL);
        assert_eq!((d.shown(), d.mode()), ("development", Mode::Off));
    }

    #[test]
    fn a_rule_written_for_the_application_just_used_takes_effect_when_it_is_saved() {
        let mut d = Deck::new(vec![], Fallback::Keep);
        d.front(OBS);
        d.launchdeck_in_front();
        assert_eq!(d.count(), 0);
        d.change_settings(|s| s.rules.push(rule(OBS, "productivity", None)));
        assert_eq!(d.shown(), "productivity");
        // But not over a surface chosen by hand.
        d.pick("smart-home");
        d.change_settings(|s| s.rules[0].surface_id = "development".into());
        assert_eq!((d.shown(), d.mode()), ("smart-home", Mode::Manual));
        assert_eq!(d.decision().held, Some(Held::Manual));
    }

    // -- no observer ---------------------------------------------------------------

    #[test]
    fn without_an_observer_auto_does_nothing_and_manual_switching_is_untouched() {
        let mut d = Deck::usual();
        d.engine = Engine::new(
            ObserverStatus::unavailable("Wayland does not say which application is in front."),
            None,
            false,
        );
        assert_eq!(d.mode(), Mode::Unavailable);
        assert_eq!(
            d.engine.observer(),
            &ObserverStatus::Unavailable {
                reason: "Wayland does not say which application is in front.".into()
            }
        );
        d.pick("smart-home");
        d.pick("development");
        assert_eq!((d.shown(), d.count()), ("development", 0));
        assert!(!d.engine.overridden(), "there is nothing to override");
        d.resume();
        d.edit(true);
        d.edit(false);
        d.pass(Duration::from_secs(60));
        assert_eq!((d.shown(), d.count()), ("development", 0));
        assert!(d.engine.decision().is_none() && d.engine.application().is_none());
        // With Auto off it simply reads Off.
        d.settings.auto = false;
        assert_eq!(d.mode(), Mode::Off);
    }

    #[test]
    fn an_observer_that_fails_later_keeps_the_last_application() {
        let mut d = Deck::usual();
        d.front(CODE);
        d.engine.set_observer(ObserverStatus::Degraded {
            mechanism: "X11".into(),
            reason: "the connection was lost".into(),
        });
        assert_eq!(d.mode(), Mode::Auto, "it may come back");
        assert_eq!(d.engine.application(), Some(&app(CODE)));
        assert_eq!((d.shown(), d.count()), ("development", 1));
    }

    // -- what is remembered ----------------------------------------------------------

    #[test]
    fn recently_seen_applications_are_few_newest_first_and_never_launchdeck() {
        let mut d = Deck::new(vec![], Fallback::Keep);
        for i in 0..20 {
            d.front(&format!("app{i}"));
        }
        d.launchdeck_in_front();
        d.front("app5");
        let recent = d.engine.recent();
        assert_eq!(recent.len(), RECENT);
        assert_eq!(recent[0], app("app5"));
        assert_eq!(recent[1], app("app19"));
        assert_eq!(recent.iter().filter(|a| a.is(&app("app5"))).count(), 1);
        assert!(recent.iter().all(|a| a.id != "Launchdeck"));
    }
}
