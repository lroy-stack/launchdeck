//! Validation of user-edited controls. Pure functions, unit tested.

use super::model::{Control, ControlPos, Page};

/// Upper bound for grid coordinates. The device decides what is visible; the
/// model is not hardcoded to 8x8.
pub const MAX_GRID_DIM: u8 = 16;
const MAX_CONTROLS_PER_PAGE: usize = 128;

pub fn validate_control(page: &Page, control: &Control) -> Result<(), String> {
    if control.id.is_empty() || control.id.len() > 64 {
        return Err("control id must be 1-64 characters".into());
    }
    let label_len = control.label.trim().chars().count();
    if label_len == 0 || label_len > 40 {
        return Err("label must be 1-40 characters".into());
    }
    if control.description.chars().count() > 400 {
        return Err("description is longer than 400 characters".into());
    }
    match control.pos {
        ControlPos::Grid { col, row } => {
            if col >= MAX_GRID_DIM || row >= MAX_GRID_DIM {
                return Err("grid position is out of range".into());
            }
            let (w, h) = (control.span.w, control.span.h);
            if !(1..=8).contains(&w) || !(1..=8).contains(&h) {
                return Err("span must be between 1x1 and 8x8".into());
            }
        }
        ControlPos::Top { index } | ControlPos::Side { index } => {
            if index >= MAX_GRID_DIM {
                return Err("auxiliary index is out of range".into());
            }
            if control.span.w != 1 || control.span.h != 1 {
                return Err("only grid controls can span several cells".into());
            }
        }
        ControlPos::Logo => {
            return Err("the logo LED is output only and cannot hold a control".into())
        }
    }
    let others = page.controls.iter().filter(|c| c.id != control.id);
    if others.clone().count() >= MAX_CONTROLS_PER_PAGE {
        return Err("too many controls on this page".into());
    }
    let mine = control.cells();
    for other in others {
        let clash = match control.pos {
            ControlPos::Grid { .. } => other.cells().iter().any(|c| mine.contains(c)),
            pos => other.pos == pos,
        };
        if clash {
            return Err(format!("overlaps `{}`", other.label));
        }
    }
    // What the press and long press do is checked by the action registry
    // (`ActionRegistry::prepare`), which knows the providers; this function
    // only knows the geometry and text of a control.
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::model::{Rgb, Span};

    fn page(controls: Vec<Control>) -> Page {
        Page {
            id: "p".into(),
            name: "P".into(),
            controls,
        }
    }

    fn pad(col: u8, row: u8, label: &str) -> Control {
        Control::new(ControlPos::grid(col, row), label, Rgb::new(1, 1, 1))
    }

    #[test]
    fn accepts_a_valid_control_and_rejects_overlaps() {
        let existing = pad(0, 0, "A");
        assert!(validate_control(&page(vec![existing.clone()]), &pad(1, 0, "B")).is_ok());
        let mut other = pad(0, 0, "Other");
        other.id = "other".into();
        let err = validate_control(&page(vec![existing]), &other).unwrap_err();
        assert!(err.contains("overlaps"));
        // Editing a control in place never clashes with itself.
        let a = pad(2, 2, "A");
        assert!(validate_control(&page(vec![a.clone()]), &a).is_ok());
    }

    #[test]
    fn spans_are_checked_against_neighbours() {
        let mut big = pad(0, 0, "Big");
        big.span = Span { w: 2, h: 2 };
        let p = page(vec![pad(1, 1, "Inside")]);
        assert!(validate_control(&p, &big).is_err());
        big.span = Span { w: 9, h: 1 };
        assert!(validate_control(&page(vec![]), &big).is_err());
    }

    #[test]
    fn rejects_bad_text_and_positions() {
        let mut c = pad(0, 0, "  ");
        assert!(validate_control(&page(vec![]), &c).is_err());
        c.label = "x".repeat(41);
        assert!(validate_control(&page(vec![]), &c).is_err());
        assert!(validate_control(&page(vec![]), &pad(16, 0, "Far")).is_err());
        let logo = Control::new(ControlPos::Logo, "Logo", Rgb::new(1, 1, 1));
        assert!(validate_control(&page(vec![]), &logo).is_err());
    }
}
