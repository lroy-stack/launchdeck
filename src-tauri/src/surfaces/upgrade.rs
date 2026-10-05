//! The v1 -> v2 content step: `persistence::migrate` calls it to give an old
//! workspace the Agent Control surface.

use super::agent_control;
use super::build::{ctl, open_surface};
use super::palette::PURPLE;
use crate::core::model::{ControlPos, Visual, Workspace, SCHEMA_VERSION};

/// Upgrade a v1 workspace in place: add the Agent Control surface (first, it
/// is the flagship) and a button to it in the free top-row slot of every
/// existing surface. Nothing the user made is removed or changed.
pub fn add_agent_control(ws: &mut Workspace) {
    ws.schema_version = SCHEMA_VERSION;
    if ws.surface(agent_control::SURFACE_ID).is_some() {
        return;
    }
    ws.surfaces.insert(0, agent_control::agent_control());
    for surface in ws.surfaces.iter_mut().skip(1) {
        for page in &mut surface.pages {
            let taken = page
                .controls
                .iter()
                .any(|c| c.pos == ControlPos::Top { index: 5 });
            let has_switchers = page
                .controls
                .iter()
                .any(|c| matches!(c.pos, ControlPos::Top { .. }));
            if taken || !has_switchers {
                continue;
            }
            let mut c = ctl(
                ControlPos::Top { index: 5 },
                "Agents",
                "◉",
                PURPLE,
                open_surface(agent_control::SURFACE_ID),
            );
            c.description = "Switch to the Agent Control surface.".into();
            c.visual = Visual::solid(PURPLE.scaled(0.35));
            page.controls.push(c);
        }
    }
}
