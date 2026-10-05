//! The starter workspace: the starter surfaces in order, and the blank
//! surface "Build your own" starts from.

use super::accessibility::accessibility;
use super::agent_control;
use super::development::development;
use super::navigation::surface_switchers;
use super::productivity::productivity;
use super::smart_home::smart_home;
use crate::core::model::{Page, Preferences, Surface, Workspace, SCHEMA_VERSION};
use crate::platform::Platform;

pub fn starter_surfaces(platform: Platform) -> Vec<Surface> {
    vec![
        agent_control::agent_control(),
        development(platform),
        productivity(platform),
        smart_home(),
        accessibility(),
    ]
}

/// The workspace a new installation on this system starts with.
pub fn default_workspace() -> Workspace {
    default_workspace_for(Platform::current())
}

/// The workspace a new installation on `platform` starts with. Tests that
/// compare with a file recorded on one system ask for that system.
pub fn default_workspace_for(platform: Platform) -> Workspace {
    Workspace {
        schema_version: SCHEMA_VERSION,
        surfaces: starter_surfaces(platform),
        last_active_surface: Some(agent_control::SURFACE_ID.into()),
        preferences: Preferences::default(),
        context: Default::default(),
    }
}

/// An empty surface for "Build your own".
pub fn blank_surface(id: &str, name: &str) -> Surface {
    Surface {
        id: id.into(),
        name: name.into(),
        description: "A blank surface. Select a pad and give it an action.".into(),
        icon: Some("＋".into()),
        category: "custom".into(),
        pages: vec![Page {
            id: "home".into(),
            name: "Home".into(),
            controls: surface_switchers(""),
        }],
        context: None,
        imported_from: None,
    }
}
