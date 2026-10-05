//! The Home starter (id `smart-home`): lights and switches through the Home
//! Assistant example plugin. Until that plugin is installed, given its
//! address and token, and each pad its own entity, the pads read as needing
//! the plugin and nothing is sent anywhere.

use serde_json::json;

use super::build::{ctl, g, plugin, with_state_visuals};
use super::navigation::surface_switchers;
use super::palette::{BLUE, CYAN, ORANGE, PURPLE, RED, YELLOW};
use crate::core::model::{Page, Span, Surface};

/// The example plugin in `examples/plugins/home-assistant`.
const PROVIDER: &str = "example.home-assistant";

pub(super) fn smart_home() -> Surface {
    let note = "Example entity: set your own in the inspector.";
    let mut controls = Vec::new();
    // The plugin switches lights, switches and input booleans, so a fan is a switch here.
    for (i, (label, icon, color, entity)) in [
        ("Living room", "☀", YELLOW, "light.living_room"),
        ("Kitchen", "☀", ORANGE, "light.kitchen"),
        ("Desk lamp", "✦", BLUE, "light.desk"),
        ("Porch", "⌂", PURPLE, "light.porch"),
        ("Fan", "✣", CYAN, "switch.bedroom_fan"),
        ("Coffee", "☕", RED, "switch.coffee_machine"),
    ]
    .iter()
    .enumerate()
    {
        let mut c = with_state_visuals(ctl(
            g((i % 3) as u8 * 2, (i / 3) as u8 * 2),
            label,
            icon,
            *color,
            plugin(PROVIDER, "toggle", json!({ "entity": entity })),
        ));
        c.span = Span { w: 2, h: 2 };
        c.description = note.into();
        controls.push(c);
    }
    controls.extend(surface_switchers("smart-home"));
    Surface {
        id: "smart-home".into(),
        name: "Home".into(),
        description: "Lights and switches through Home Assistant. Needs the Home Assistant example plugin (Plugins, Install), its address and token, and your own entity on each pad."
            .into(),
        icon: Some("⌂".into()),
        category: "smart-home".into(),
        pages: vec![Page {
            id: "home".into(),
            name: "Home".into(),
            controls,
        }],
        context: None,
        imported_from: None,
    }
}
