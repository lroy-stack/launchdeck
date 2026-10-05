//! Editing: the changes a person makes to the workspace (controls, surfaces,
//! pages, a reset, the preferences), checked, saved and announced; and the
//! commit of a whole new workspace, saved before it is shown.
//!
//! `commit_workspace` is the transactional path: the change is made on a
//! copy, the copy is saved, and only then does it replace the workspace.
//! Import, and renaming and deleting surfaces and pages, go through it.

use std::collections::{HashMap, HashSet};
use std::fmt;

use super::navigation::Shown;
use super::{control_key, By, Inner, Runtime};
use crate::context::rules::Fallback;
use crate::core::model::{Assignment, Control, Page, Preferences, Surface, Workspace};
use crate::core::validate::validate_control;
use crate::state::feedback::prepare_rules;
use crate::state::index::ControlRef;
use crate::surfaces::{blank_surface, default_workspace};

fn describe_issues(issues: &[crate::state::feedback::RuleIssue]) -> String {
    issues
        .iter()
        .map(|i| format!("Feedback rule {}: {}", i.rule + 1, i.message))
        .collect::<Vec<_>>()
        .join("; ")
}

/// Why [`Runtime::commit_workspace`] left everything as it was.
#[derive(Debug)]
pub(crate) enum CommitError<E> {
    /// The new workspace could not be built: what the builder said.
    Build(E),
    /// The new workspace could not be saved: why.
    Save(String),
}

impl<E: fmt::Display> fmt::Display for CommitError<E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CommitError::Build(e) => e.fmt(f),
            CommitError::Save(why) => f.write_str(&not_saved(why)),
        }
    }
}

impl Runtime {
    /// Replace the workspace with one built from the current one, saved
    /// before it is shown. Under the lock, `build` makes the candidate from
    /// the workspace as it is, the store saves it, and only then is it
    /// swapped in; any error returns before the swap, so the workspace in
    /// memory and `workspace.json` stay as they were. Import, and renaming
    /// and deleting surfaces and pages, go through here, never through
    /// `persist`, which only logs a failed save. What is saved never names,
    /// as the surface to open on, one that is missing.
    ///
    /// After the swap, as after a reset: the index is rebuilt, every
    /// control's look derived again, the surface on screen replaced by the
    /// first one if it is gone (and the page by the surface's first if that
    /// is gone, shown `By::Runtime`), and Auto Context told that its settings
    /// may have changed; the target it gives is shown as `update_context`
    /// shows it. Controls the commit changed or removed lose their temporary
    /// looks, as an edited control does. The workspace and context are
    /// announced once.
    pub(crate) fn commit_workspace<E>(
        &self,
        build: impl FnOnce(&Workspace) -> Result<Workspace, E>,
    ) -> Result<(), CommitError<E>> {
        let (shown, target) = {
            let mut guard = self.lock();
            let inner: &mut Inner = &mut guard;
            let mut candidate = build(&inner.workspace).map_err(CommitError::Build)?;
            if candidate.surface(&inner.active_surface).is_none() {
                // Saved as it will be shown, so the next start opens it too.
                candidate.last_active_surface = candidate.surfaces.first().map(|s| s.id.clone());
            }
            keep_last_active_valid(&mut candidate);
            if let Some(store) = &self.store {
                store
                    .save(&candidate)
                    .map_err(|e| CommitError::Save(e.to_string()))?;
            }
            let was_auto = inner.workspace.context.auto;
            let previous = std::mem::replace(&mut inner.workspace, candidate);
            let unchanged = unchanged_controls(&previous, &inner.workspace);
            inner.overlay.retain(|key, _| unchanged.contains(key));
            let shown = self.keep_something_on_screen(inner);
            self.rebuild_index(inner);
            self.evaluate_all(inner);
            let target = Self::with_context(inner, |engine, world| {
                engine.settings_changed(was_auto, world)
            });
            (shown, target)
        };
        if let Some(shown) = shown {
            self.announce_shown(&shown);
        }
        self.emit_workspace();
        self.show_for_context(target);
        self.emit_context();
        Ok(())
    }

    /// The first surface (on its first page) when the one on screen is
    /// gone; the surface's first page when only the page is. Shown through
    /// the same path as any other surface, as the runtime's own choice.
    fn keep_something_on_screen(&self, inner: &mut Inner) -> Option<Shown> {
        let (surface, page) = replacement_on_screen(inner)?;
        self.put_on_screen(inner, &surface, Some(&page), By::Runtime)
            .inspect_err(|e| log::warn!("could not show {surface}/{page}: {e}"))
            .ok()
    }

    pub fn update_control(
        &self,
        surface_id: &str,
        page_id: &str,
        control: Control,
    ) -> Result<(), String> {
        {
            let mut inner = self.lock();
            let surface = inner
                .workspace
                .surface_mut(surface_id)
                .ok_or_else(|| format!("unknown surface `{surface_id}`"))?;
            let page = surface
                .pages
                .iter_mut()
                .find(|p| p.id == page_id)
                .ok_or_else(|| format!("unknown page `{page_id}`"))?;
            validate_control(page, &control)?;
            let mut control = control;
            let stored = page.controls.iter().find(|c| c.id == control.id);
            // Rules that are new or changed must name a state something
            // offers, with arguments that fit it. Saved ones stay as they are.
            let hub = &self.hub;
            control.feedback = prepare_rules(
                &control.feedback,
                stored.map_or(&[][..], |c| &c.feedback[..]),
                &|provider, id| hub.definition(provider, id),
            )
            .map_err(|issues| describe_issues(&issues))?;
            // Only an assignment that changed is checked. One that is saved
            // already stays editable even when its provider is gone, so a pad
            // can still be relabelled while its plugin is uninstalled.
            let registry = self.engine.registry();
            let prepare = |new: &mut Option<Assignment>,
                           old: Option<&Assignment>,
                           slot: &str|
             -> Result<(), String> {
                if let Some(a) = new.as_ref().filter(|a| Some(*a) != old) {
                    let prepared = registry
                        .prepare(a)
                        .map_err(|e| format!("{slot} action: {e}"))?;
                    *new = Some(prepared);
                }
                Ok(())
            };
            prepare(
                &mut control.press,
                stored.and_then(|c| c.press.as_ref()),
                "Press",
            )?;
            prepare(
                &mut control.long_press,
                stored.and_then(|c| c.long_press.as_ref()),
                "Long press",
            )?;
            let control_id = control.id.clone();
            let key = control_key(surface_id, page_id, &control_id);
            match page.controls.iter_mut().find(|c| c.id == control.id) {
                Some(slot) => *slot = control,
                None => page.controls.push(control),
            }
            // An edit starts the control over: no temporary look survives it,
            // and its look is derived again from the values the hub holds.
            inner.overlay.remove(&key);
            self.rebuild_index(&mut inner);
            let r = ControlRef {
                surface: surface_id.to_string(),
                page: page_id.to_string(),
                control: control_id,
            };
            inner.derived.remove(&key);
            self.derive_into(&mut inner, &r);
            self.persist(&inner);
        }
        self.emit_workspace();
        Ok(())
    }

    pub fn remove_control(
        &self,
        surface_id: &str,
        page_id: &str,
        control_id: &str,
    ) -> Result<(), String> {
        {
            let mut inner = self.lock();
            let surface = inner
                .workspace
                .surface_mut(surface_id)
                .ok_or_else(|| format!("unknown surface `{surface_id}`"))?;
            let page = surface
                .pages
                .iter_mut()
                .find(|p| p.id == page_id)
                .ok_or_else(|| format!("unknown page `{page_id}`"))?;
            let before = page.controls.len();
            page.controls.retain(|c| c.id != control_id);
            if page.controls.len() == before {
                return Err(format!("unknown control `{control_id}`"));
            }
            let key = control_key(surface_id, page_id, control_id);
            inner.overlay.remove(&key);
            inner.derived.remove(&key);
            self.rebuild_index(&mut inner);
            self.persist(&inner);
        }
        self.emit_workspace();
        Ok(())
    }

    pub fn create_surface(&self, name: &str) -> Result<String, String> {
        let name = name.trim();
        if name.is_empty() || name.chars().count() > 40 {
            return Err("name must be 1-40 characters".into());
        }
        let id = {
            let mut inner = self.lock();
            let base: String = name
                .to_ascii_lowercase()
                .chars()
                .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
                .collect::<String>()
                .trim_matches('-')
                .to_string();
            let base = if base.is_empty() {
                "surface".to_string()
            } else {
                base
            };
            let mut id = base.clone();
            let mut n = 2;
            while inner.workspace.surface(&id).is_some() {
                id = format!("{base}-{n}");
                n += 1;
            }
            inner.workspace.surfaces.push(blank_surface(&id, name));
            self.persist(&inner);
            id
        };
        self.emit_workspace();
        Ok(id)
    }

    pub fn add_page(&self, surface_id: &str, name: &str) -> Result<String, String> {
        let name = name.trim();
        if name.is_empty() || name.chars().count() > 40 {
            return Err("name must be 1-40 characters".into());
        }
        let id = {
            let mut inner = self.lock();
            let surface = inner
                .workspace
                .surface_mut(surface_id)
                .ok_or_else(|| format!("unknown surface `{surface_id}`"))?;
            let base: String = name
                .to_ascii_lowercase()
                .chars()
                .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
                .collect::<String>()
                .trim_matches('-')
                .to_string();
            let base = if base.is_empty() {
                "page".to_string()
            } else {
                base
            };
            let mut id = base.clone();
            let mut n = 2;
            while surface.page(&id).is_some() {
                id = format!("{base}-{n}");
                n += 1;
            }
            surface.pages.push(Page {
                id: id.clone(),
                name: name.to_string(),
                controls: Vec::new(),
            });
            self.persist(&inner);
            id
        };
        self.emit_workspace();
        Ok(id)
    }

    pub fn reset_workspace(&self) {
        {
            let mut inner = self.lock();
            let prefs = inner.workspace.preferences.clone();
            let was_auto = inner.workspace.context.auto;
            let mut ws = default_workspace();
            ws.preferences = prefs;
            let first = ws
                .surfaces
                .first()
                .map(|s: &Surface| s.id.clone())
                .unwrap_or_default();
            inner.active_page = ws
                .surface(&first)
                .and_then(|s| s.home_page())
                .map(|p| p.id.clone())
                .unwrap_or_default();
            inner.active_surface = first;
            ws.last_active_surface = Some(inner.active_surface.clone());
            inner.workspace = ws;
            inner.overlay.clear();
            inner.held.clear();
            self.rebuild_index(&mut inner);
            self.evaluate_all(&mut inner);
            self.persist(&inner);
            // The rules named surfaces of the workspace that is gone: Auto
            // Context starts again, off and without rules.
            let _ = Self::with_context(&mut inner, |engine, world| {
                engine.settings_changed(was_auto, world)
            });
        }
        self.emit_workspace();
        self.emit_context();
    }

    pub fn update_preferences(&self, preferences: Preferences) {
        {
            let mut inner = self.lock();
            inner.workspace.preferences = preferences;
            self.persist(&inner);
        }
        self.emit_workspace();
    }
}

/// The keys of the controls that are in both workspaces, exactly as they were.
fn unchanged_controls(before: &Workspace, after: &Workspace) -> HashSet<String> {
    let before = controls_by_key(before);
    controls_by_key(after)
        .into_iter()
        .filter(|(key, control)| before.get(key) == Some(control))
        .map(|(key, _)| key)
        .collect()
}

fn controls_by_key(ws: &Workspace) -> HashMap<String, &Control> {
    ws.surfaces
        .iter()
        .flat_map(|s| {
            s.pages.iter().flat_map(move |p| {
                p.controls
                    .iter()
                    .map(move |c| (control_key(&s.id, &p.id, &c.id), c))
            })
        })
        .collect()
}

/// The surface the next start opens on must exist.
fn keep_last_active_valid(workspace: &mut Workspace) {
    let gone = workspace
        .last_active_surface
        .as_deref()
        .is_some_and(|id| workspace.surface(id).is_none());
    if gone {
        workspace.last_active_surface = workspace.surfaces.first().map(|s| s.id.clone());
    }
}

/// What to show when the surface or the page on screen is gone: the
/// surface's first page, or the first surface.
fn replacement_on_screen(inner: &Inner) -> Option<(String, String)> {
    let home = |s: &Surface| s.home_page().map(|p| (s.id.clone(), p.id.clone()));
    match inner.workspace.surface(&inner.active_surface) {
        Some(s) if s.page(&inner.active_page).is_some() => None,
        Some(s) => home(s),
        None => inner.workspace.surfaces.first().and_then(home),
    }
}

// ---------------------------------------------------------------------------
// Managing surfaces and pages, and the onboarding flag
// ---------------------------------------------------------------------------

/// The most characters a surface or page name, a surface description and a
/// surface icon may have.
const NAME_MAX: usize = 40;
const DESCRIPTION_MAX: usize = 400;
const ICON_MAX: usize = 8;

impl Runtime {
    /// Delete a surface. The last one is refused. Auto Context rules that
    /// show it go with it, and a fallback that shows it goes back to keeping
    /// what is on screen. Navigation pads elsewhere keep their arguments, so
    /// pressing one says the surface is missing.
    pub fn remove_surface(&self, surface_id: &str) -> Result<(), String> {
        self.commit_workspace(|ws| {
            let surface = ws.surface(surface_id).ok_or_else(missing_surface)?;
            if ws.surfaces.len() == 1 {
                return Err(format!(
                    "{} is the only surface, so it cannot be deleted. Create another one first.",
                    surface.name
                ));
            }
            let mut next = ws.clone();
            next.surfaces.retain(|s| s.id != surface_id);
            next.context.rules.retain(|r| r.surface_id != surface_id);
            if matches!(&next.context.fallback,
                Fallback::Surface { surface_id: shown, .. } if shown == surface_id)
            {
                next.context.fallback = Fallback::Keep;
            }
            Ok(next)
        })
        .map_err(|e| e.to_string())
    }

    /// Rename a surface and change its description and icon. An empty icon
    /// is no icon.
    pub fn update_surface(
        &self,
        surface_id: &str,
        name: &str,
        description: &str,
        icon: Option<String>,
    ) -> Result<(), String> {
        let name = checked_name(name, "surface")?;
        let description = description.trim().to_string();
        if description.chars().count() > DESCRIPTION_MAX {
            return Err(format!(
                "A description can have at most {DESCRIPTION_MAX} characters."
            ));
        }
        let icon = icon.map(|i| i.trim().to_string()).filter(|i| !i.is_empty());
        if icon.as_ref().is_some_and(|i| i.chars().count() > ICON_MAX) {
            return Err(format!("An icon can have at most {ICON_MAX} characters."));
        }
        self.commit_workspace(|ws| -> Result<Workspace, String> {
            let mut next = ws.clone();
            let surface = next.surface_mut(surface_id).ok_or_else(missing_surface)?;
            surface.name = name;
            surface.description = description;
            surface.icon = icon;
            Ok(next)
        })
        .map_err(|e| e.to_string())
    }

    pub fn rename_page(&self, surface_id: &str, page_id: &str, name: &str) -> Result<(), String> {
        let name = checked_name(name, "page")?;
        self.commit_workspace(|ws| -> Result<Workspace, String> {
            let mut next = ws.clone();
            let surface = next.surface_mut(surface_id).ok_or_else(missing_surface)?;
            let page = surface
                .pages
                .iter_mut()
                .find(|p| p.id == page_id)
                .ok_or_else(missing_page)?;
            page.name = name;
            Ok(next)
        })
        .map_err(|e| e.to_string())
    }

    /// Delete a page. A surface's last page is refused. When it is the page
    /// on screen, the surface's first page is shown.
    pub fn remove_page(&self, surface_id: &str, page_id: &str) -> Result<(), String> {
        self.commit_workspace(|ws| {
            let mut next = ws.clone();
            let surface = next.surface_mut(surface_id).ok_or_else(missing_surface)?;
            let page = surface.page(page_id).ok_or_else(missing_page)?;
            if surface.pages.len() == 1 {
                return Err(format!(
                    "{} is the only page of {}, so it cannot be deleted.",
                    page.name, surface.name
                ));
            }
            surface.pages.retain(|p| p.id != page_id);
            Ok(next)
        })
        .map_err(|e| e.to_string())
    }

    /// Whether the first run is over. Only that preference changes, saved
    /// before it is made; a reset keeps it, like every preference.
    pub fn set_onboarding_done(&self, done: bool) -> Result<(), String> {
        {
            let mut inner = self.lock();
            let mut candidate = inner.workspace.clone();
            candidate.preferences.onboarding_done = done;
            if let Some(store) = &self.store {
                store.save(&candidate).map_err(|e| not_saved(&e))?;
            }
            inner.workspace = candidate;
        }
        self.emit_workspace();
        Ok(())
    }
}

fn not_saved(error: &impl fmt::Display) -> String {
    format!("The change could not be saved, so nothing changed ({error}).")
}

fn missing_surface() -> String {
    "That surface no longer exists.".into()
}

fn missing_page() -> String {
    "That page no longer exists.".into()
}

/// `what` names the thing the name is for, in the sentence.
fn checked_name(name: &str, what: &str) -> Result<String, String> {
    let name = name.trim();
    if name.is_empty() || name.chars().count() > NAME_MAX {
        return Err(format!("A {what} name needs 1 to {NAME_MAX} characters."));
    }
    Ok(name.to_string())
}
