//! Validation of user-edited controls. Pure functions, unit tested.

use super::model::{Control, ControlPos, Page};

/// Upper bound for grid coordinates. The device decides what is visible; the
/// model is not hardcoded to 8x8.
pub const MAX_GRID_DIM: u8 = 16;
const MAX_CONTROLS_PER_PAGE: usize = 128;

/// Whether `control` fits on `page`; a sentence for the person otherwise.
pub fn validate_control(page: &Page, control: &Control) -> Result<(), String> {
    if control.id.is_empty() || control.id.len() > 64 {
        return Err("A pad needs an id of 1 to 64 characters.".into());
    }
    let label_len = control.label.trim().chars().count();
    if label_len == 0 || label_len > 40 {
        return Err("A label needs 1 to 40 characters.".into());
    }
    if control.description.chars().count() > 400 {
        return Err("A description can have at most 400 characters.".into());
    }
    match control.pos {
        ControlPos::Grid { col, row } => {
            if col >= MAX_GRID_DIM || row >= MAX_GRID_DIM {
                return Err("That position is outside the grid.".into());
            }
            let (w, h) = (control.span.w, control.span.h);
            if !(1..=8).contains(&w) || !(1..=8).contains(&h) {
                return Err("A pad can span 1 to 8 columns and 1 to 8 rows.".into());
            }
        }
        ControlPos::Top { index } | ControlPos::Side { index } => {
            if index >= MAX_GRID_DIM {
                return Err("That button does not exist on any device.".into());
            }
            if control.span.w != 1 || control.span.h != 1 {
                return Err("Only pads on the grid can span more than one position.".into());
            }
        }
        ControlPos::Logo => {
            return Err("The logo light only shows colours; it cannot hold a pad.".into())
        }
    }
    let others = page.controls.iter().filter(|c| c.id != control.id);
    if others.clone().count() >= MAX_CONTROLS_PER_PAGE {
        return Err(format!(
            "A page can hold at most {MAX_CONTROLS_PER_PAGE} pads."
        ));
    }
    let mine = control.cells();
    for other in others {
        let clash = match control.pos {
            ControlPos::Grid { .. } => other.cells().iter().any(|c| mine.contains(c)),
            pos => other.pos == pos,
        };
        if clash {
            return Err(format!("This pad would overlap “{}”.", other.label));
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
        assert_eq!(err, "This pad would overlap “A”.");
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

    /// What a person reads when the editor refuses a pad: a sentence each.
    #[test]
    fn each_refusal_is_a_sentence_for_a_person() {
        let none = page(vec![]);
        let refused = |c: &Control| validate_control(&none, c).unwrap_err();
        let mut c = pad(0, 0, "A");
        c.id = String::new();
        assert_eq!(refused(&c), "A pad needs an id of 1 to 64 characters.");
        assert_eq!(
            refused(&pad(0, 0, " ")),
            "A label needs 1 to 40 characters."
        );
        let mut c = pad(0, 0, "A");
        c.description = "d".repeat(401);
        assert_eq!(
            refused(&c),
            "A description can have at most 400 characters."
        );
        assert_eq!(
            refused(&pad(16, 0, "Far")),
            "That position is outside the grid."
        );
        let mut c = pad(0, 0, "Wide");
        c.span = Span { w: 9, h: 1 };
        assert_eq!(
            refused(&c),
            "A pad can span 1 to 8 columns and 1 to 8 rows."
        );
        let mut c = Control::new(ControlPos::Top { index: 16 }, "T", Rgb::new(1, 1, 1));
        assert_eq!(refused(&c), "That button does not exist on any device.");
        c.pos = ControlPos::Side { index: 0 };
        c.span = Span { w: 2, h: 1 };
        assert_eq!(
            refused(&c),
            "Only pads on the grid can span more than one position."
        );
        let logo = Control::new(ControlPos::Logo, "Logo", Rgb::new(1, 1, 1));
        assert_eq!(
            refused(&logo),
            "The logo light only shows colours; it cannot hold a pad."
        );
        let full = page(
            (0..MAX_CONTROLS_PER_PAGE)
                .map(|i| {
                    let mut c = pad(0, 0, "Side");
                    c.id = format!("c{i}");
                    c.pos = ControlPos::Side { index: 0 };
                    c
                })
                .collect(),
        );
        assert_eq!(
            validate_control(&full, &pad(5, 5, "New")).unwrap_err(),
            "A page can hold at most 128 pads."
        );
        let mut top = Control::new(ControlPos::Top { index: 2 }, "Up", Rgb::new(1, 1, 1));
        let taken = page(vec![top.clone()]);
        top.id = "other".into();
        assert_eq!(
            validate_control(&taken, &top).unwrap_err(),
            "This pad would overlap “Up”."
        );
    }
}
