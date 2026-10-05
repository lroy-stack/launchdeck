//! Navigation: which surface and page are on screen, and Live or Edit.
//! `activate` is the one way a surface comes on screen, whoever asked.

use serde_json::{json, Value};

use super::{By, Mode, Runtime};
use crate::actions::builtin::{OPEN_PAGE, OPEN_SURFACE};
use crate::core::model::Assignment;
use crate::core::observables as native;
use crate::events::types;
use crate::state::Scalar;

impl Runtime {
    /// Show a surface (and a page of it) because a person asked: the sidebar,
    /// the tray, a navigation pad. With Auto Context on, this is what begins a
    /// manual override.
    pub fn activate_surface(&self, surface_id: &str, page_id: Option<&str>) -> Result<(), String> {
        self.activate(surface_id, page_id, By::Person)
    }

    /// The one way a surface comes on screen, whoever asked.
    pub(super) fn activate(
        &self,
        surface_id: &str,
        page_id: Option<&str>,
        by: By,
    ) -> Result<(), String> {
        {
            let mut inner = self.lock();
            let surface = inner
                .workspace
                .surface(surface_id)
                .ok_or_else(|| format!("unknown surface `{surface_id}`"))?;
            let page = match page_id {
                Some(p) => surface
                    .page(p)
                    .ok_or_else(|| format!("unknown page `{p}`"))?,
                None => surface.home_page().ok_or("surface has no pages")?,
            }
            .id
            .clone();
            let surface_changed = inner.active_surface != surface_id;
            let changed = surface_changed || inner.active_page != page;
            inner.active_surface = surface_id.to_string();
            inner.active_page = page.clone();
            inner.held.clear();
            inner.workspace.last_active_surface = Some(surface_id.to_string());
            if by == By::Person {
                Self::with_context(&mut inner, |engine, world| engine.manual(changed, world));
            }
            self.persist(&inner);
            drop(inner);
            if surface_changed {
                self.publish(
                    "runtime",
                    types::SURFACE_CHANGED,
                    json!({ "surfaceId": surface_id, "pageId": page }),
                );
            }
            self.publish(
                "runtime",
                types::PAGE_CHANGED,
                json!({ "surfaceId": surface_id, "pageId": page }),
            );
        }
        // Nothing is replayed to repaint the surface: every control's look is
        // already derived from the states the hub holds, shown or not.
        self.emit_state();
        self.emit_context();
        Ok(())
    }

    /// Show another page of the surface on screen, because a person asked.
    pub fn activate_page(&self, page_id: &str) -> Result<(), String> {
        let surface_id = {
            let mut inner = self.lock();
            let surface = inner
                .workspace
                .surface(&inner.active_surface)
                .ok_or("no active surface")?;
            if surface.page(page_id).is_none() {
                return Err(format!("unknown page `{page_id}`"));
            }
            let changed = inner.active_page != page_id;
            inner.active_page = page_id.to_string();
            inner.held.clear();
            Self::with_context(&mut inner, |engine, world| engine.manual(changed, world));
            inner.active_surface.clone()
        };
        self.publish(
            "runtime",
            types::PAGE_CHANGED,
            json!({ "surfaceId": surface_id, "pageId": page_id }),
        );
        self.emit_state();
        self.emit_context();
        Ok(())
    }

    /// Perform a navigation assignment (already prepared by the registry).
    pub(super) fn navigate(&self, assignment: &Assignment) -> Result<(), String> {
        let text = |key: &str| {
            assignment
                .args
                .get(key)
                .and_then(Value::as_str)
                .map(str::to_string)
        };
        match assignment.action.as_str() {
            OPEN_SURFACE => {
                let surface = text("surfaceId").ok_or("a surface is required")?;
                self.activate_surface(&surface, text("pageId").as_deref())
            }
            OPEN_PAGE => {
                let page = text("pageId").ok_or("a page is required")?;
                self.activate_page(&page)
            }
            other => Err(format!("unknown navigation action `{other}`")),
        }
    }

    pub fn set_mode(&self, mode: Mode) {
        let target = {
            let mut inner = self.lock();
            inner.mode = mode;
            inner.held.clear();
            // Editing suspends automatic switching; coming back to Live
            // applies the current application's rule once.
            Self::with_context(&mut inner, |engine, world| {
                engine.set_editing(mode == Mode::Edit, world)
            })
        };
        self.emit_state();
        self.show_for_context(target);
        self.emit_context();
    }

    /// The surface and page on screen are states like any other.
    pub(super) fn publish_navigation(&self, surface: &str, page: &str) {
        for (address, value) in [
            (native::surface_active(), surface),
            (native::page_active(), page),
        ] {
            if let Err(e) = self.hub.set(&address, Scalar::text(value)) {
                log::warn!("could not publish {address}: {e}");
            }
        }
    }
}
