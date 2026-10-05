//! The Smart Home starter (template: needs the user's own webhooks).

use super::build::{ctl, g, http, with_state_visuals};
use super::navigation::surface_switchers;
use super::palette::{BLUE, CYAN, ORANGE, PURPLE, RED, YELLOW};
use crate::core::model::{Page, Span, Surface};

pub(super) fn smart_home() -> Surface {
    let hook = |name: &str| {
        http(
            "POST",
            &format!("http://homeassistant.local:8123/api/webhook/launchdeck-{name}"),
        )
    };
    let note = "Template: edit the webhook URL in the inspector. Until you do, pressing this shows an error state.";
    let mut controls = Vec::new();
    for (i, (label, icon, color, name)) in [
        ("Lights", "☀", YELLOW, "lights"),
        ("Movie", "▶", PURPLE, "movie"),
        ("Work", "⌨", BLUE, "work"),
        ("Warmer", "▲", ORANGE, "warmer"),
        ("Cooler", "▼", CYAN, "cooler"),
        ("Lock", "🔒", RED, "lock"),
    ]
    .iter()
    .enumerate()
    {
        let mut c = with_state_visuals(ctl(
            g((i % 3) as u8 * 2, (i / 3) as u8 * 2),
            label,
            icon,
            *color,
            hook(name),
        ));
        c.span = Span { w: 2, h: 2 };
        c.description = note.into();
        controls.push(c);
    }
    controls.extend(surface_switchers("smart-home"));
    Surface {
        id: "smart-home".into(),
        name: "Smart Home".into(),
        description: "Template for Home Assistant style webhooks. Edit the URLs to make it yours."
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
