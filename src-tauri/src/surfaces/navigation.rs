//! The controls that move between surfaces and pages.

use super::build::{ctl, open_page, open_surface};
use super::palette::{CYAN, GRAY, GREEN, ORANGE, PURPLE, WHITE, YELLOW};
use crate::core::model::{Control, ControlPos, Visual};

/// Top-row buttons switch between surfaces on every surface.
pub(super) fn surface_switchers(active: &str) -> Vec<Control> {
    let list = [
        ("agent-control", "Agents", "◉", PURPLE),
        ("development", "Dev", "⌘", CYAN),
        ("productivity", "Work", "☑", GREEN),
        ("smart-home", "Home", "⌂", ORANGE),
        ("accessibility", "Access", "◎", YELLOW),
    ];
    list.iter()
        .enumerate()
        .map(|(i, (id, label, icon, color))| {
            let mut c = ctl(
                ControlPos::Top { index: i as u8 },
                label,
                icon,
                *color,
                open_surface(id),
            );
            c.description = format!("Switch to the {label} surface.");
            if *id == active {
                c.visual = Visual::solid(WHITE);
            } else {
                c.visual = Visual::solid(color.scaled(0.35));
            }
            c
        })
        .collect()
}

pub(super) fn back_control(pos: ControlPos, page: &str) -> Control {
    let mut c = ctl(pos, "Back", "←", GRAY, open_page(page));
    c.description = "Return to the previous page.".into();
    c
}
