//! The text rules and id grammars of the format.
//!
//! No text holds a control character (`\n` only in a description) or an
//! explicit direction control (U+202A-U+202E, U+2066-U+2069), and a name or
//! label has no space at either end.

use super::rejection::{echo, is_bidi, Rejection, RejectionCode};

fn forbidden(c: char, newline_ok: bool) -> bool {
    (c.is_control() && !(newline_ok && c == '\n')) || is_bidi(c)
}

fn plain(value: &str, newline_ok: bool) -> bool {
    !value.chars().any(|c| forbidden(c, newline_ok))
}

/// A name or label: 1 to `max` characters, no space at either end, no
/// control or direction character.
pub(super) fn name(
    code: RejectionCode,
    at: &str,
    what: &str,
    value: &str,
    max: usize,
) -> Result<(), Rejection> {
    let count = value.chars().count();
    if count == 0 || count > max || value != value.trim() {
        return Err(Rejection::at(
            code,
            at,
            format!("{what} must be 1 to {max} characters, with no space at either end."),
        ));
    }
    no_forbidden(code, at, what, value, false)
}

/// A description: at most `max` characters; line breaks allowed.
pub(super) fn description(
    code: RejectionCode,
    at: &str,
    what: &str,
    value: &str,
    max: usize,
) -> Result<(), Rejection> {
    if value.chars().count() > max {
        return Err(Rejection::at(
            code,
            at,
            format!("{what} is longer than {max} characters."),
        ));
    }
    no_forbidden(code, at, what, value, true)
}

/// Any other text of at most `max` characters, on one line.
pub(super) fn line(
    code: RejectionCode,
    at: &str,
    what: &str,
    value: &str,
    max: usize,
) -> Result<(), Rejection> {
    if value.chars().count() > max {
        return Err(Rejection::at(
            code,
            at,
            format!("{what} is longer than {max} characters."),
        ));
    }
    no_forbidden(code, at, what, value, false)
}

fn no_forbidden(
    code: RejectionCode,
    at: &str,
    what: &str,
    value: &str,
    newline_ok: bool,
) -> Result<(), Rejection> {
    if plain(value, newline_ok) {
        Ok(())
    } else {
        Err(Rejection::at(
            code,
            at,
            format!("{what} holds a control character or a direction override."),
        ))
    }
}

/// An id that must follow `grammar` (described for a person in `rule`).
pub(super) fn id(
    code: RejectionCode,
    at: &str,
    what: &str,
    value: &str,
    grammar: fn(&str) -> bool,
    rule: &str,
) -> Result<(), Rejection> {
    if grammar(value) {
        Ok(())
    } else {
        Err(Rejection::at(
            code,
            at,
            format!("{what} \"{}\" is not valid: {rule}.", echo(value)),
        ))
    }
}

fn lower_or_digit(c: char) -> bool {
    c.is_ascii_lowercase() || c.is_ascii_digit()
}

/// A surface id: `[a-z0-9][a-z0-9-]{0,63}`.
pub(super) fn surface_id(s: &str) -> bool {
    s.len() <= 64
        && s.starts_with(lower_or_digit)
        && s.chars().all(|c| lower_or_digit(c) || c == '-')
}

/// A page or control id: `[A-Za-z0-9._:-]{1,64}`, the characters of the
/// event identifier rule (`events::valid_ident`, which this module may not
/// import).
pub(super) fn local_id(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 64
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | ':' | '-'))
}

/// An input id: `[a-z][a-z0-9_]{0,31}`.
pub(super) fn input_id(s: &str) -> bool {
    s.len() <= 32
        && s.starts_with(|c: char| c.is_ascii_lowercase())
        && s.chars().all(|c| lower_or_digit(c) || c == '_')
}

/// A surface category: `[a-z][a-z0-9-]{0,31}`.
pub(super) fn category(s: &str) -> bool {
    s.len() <= 32
        && s.starts_with(|c: char| c.is_ascii_lowercase())
        && s.chars().all(|c| lower_or_digit(c) || c == '-')
}

/// A device model of `testedWith`: `[a-z0-9-]{1,32}`.
pub(super) fn model(s: &str) -> bool {
    !s.is_empty() && s.len() <= 32 && s.chars().all(|c| lower_or_digit(c) || c == '-')
}

#[cfg(test)]
mod tests {
    use super::*;

    const C: RejectionCode = RejectionCode::InvalidContent;

    #[test]
    fn names_are_trimmed_short_and_plain() {
        assert!(name(C, "/n", "The name", "Video", 60).is_ok());
        assert!(name(C, "/n", "The name", &"é".repeat(60), 60).is_ok());
        for bad in ["", " Video", "Video ", "a\tb", "Vi\u{202E}deo", "x\u{2069}"] {
            let r = name(C, "/n", "The name", bad, 60).unwrap_err();
            assert_eq!((r.code, r.at.as_deref()), (C, Some("/n")), "{bad:?}");
        }
        assert_eq!(
            name(C, "/n", "The name", &"x".repeat(61), 60)
                .unwrap_err()
                .message,
            "The name must be 1 to 60 characters, with no space at either end."
        );
    }

    #[test]
    fn only_a_description_breaks_lines() {
        assert!(description(C, "/d", "It", "one\ntwo", 10).is_ok());
        assert!(description(C, "/d", "It", "", 10).is_ok());
        assert!(description(C, "/d", "It", "one\rtwo", 10).is_err());
        assert!(description(C, "/d", "It", "\u{202A}", 10).is_err());
        assert!(description(C, "/d", "It", &"x".repeat(11), 10).is_err());
        assert!(line(C, "/l", "It", "one\ntwo", 10).is_err());
        assert!(line(C, "/l", "It", " padded ", 10).is_ok());
    }

    #[test]
    fn id_grammars() {
        for ok in ["dev", "0-pad", &"a".repeat(64)] {
            assert!(surface_id(ok), "{ok}");
        }
        for bad in ["", "-dev", "Dev", "dev_1", "dev.x", &"a".repeat(65)] {
            assert!(!surface_id(bad), "{bad}");
        }
        for ok in ["home", "g.3.4", "A_b:c-d", &"x".repeat(64)] {
            assert!(local_id(ok), "{ok}");
        }
        for bad in ["", "a b", "a/b", "é", &"x".repeat(65)] {
            assert!(!local_id(bad), "{bad}");
        }
        for ok in ["project", "p2_dir", &format!("a{}", "b".repeat(31))] {
            assert!(input_id(ok), "{ok}");
        }
        for bad in [
            "",
            "2p",
            "_p",
            "Project",
            "p-dir",
            &format!("a{}", "b".repeat(32)),
        ] {
            assert!(!input_id(bad), "{bad}");
        }
        assert!(category("smart-home") && !category("Smart") && !category("1x"));
        assert!(model("launchpad-x") && model("x") && !model("Launchpad") && !model(""));
        let r = id(C, "/id", "The id", "Bad Id", surface_id, "lowercase").unwrap_err();
        assert_eq!(r.message, "The id \"Bad Id\" is not valid: lowercase.");
    }
}
