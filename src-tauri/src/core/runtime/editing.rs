//! Editing: the changes a person makes to the workspace (controls, surfaces,
//! pages, a reset, the preferences), checked, saved and announced.

use super::{control_key, Runtime};
use crate::core::model::{Assignment, Control, Page, Preferences, Surface};
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

impl Runtime {
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
