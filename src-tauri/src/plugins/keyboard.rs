//! Keyboard shortcut parsing and a pluggable keyboard backend.
//!
//! Parsing is pure and tested. The real backend (`enigo`) is the only code
//! that talks to the OS; tests use a recording mock.

use super::ActionError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Modifier {
    Ctrl,
    Shift,
    Alt,
    /// Command on macOS, Windows key elsewhere.
    Meta,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Named {
    Left,
    Right,
    Up,
    Down,
    PageUp,
    PageDown,
    Home,
    End,
    Enter,
    Escape,
    Tab,
    Space,
    Backspace,
    Delete,
    F(u8),
    VolumeUp,
    VolumeDown,
    VolumeMute,
    MediaPlayPause,
    MediaNext,
    MediaPrevious,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyKind {
    Char(char),
    Named(Named),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Combo {
    pub modifiers: Vec<Modifier>,
    pub key: KeyKind,
}

pub trait Keyboard: Send + Sync {
    fn press(&self, combo: &Combo) -> Result<(), ActionError>;
}

fn parse_named(s: &str) -> Option<Named> {
    Some(match s {
        "left" | "arrowleft" => Named::Left,
        "right" | "arrowright" => Named::Right,
        "up" | "arrowup" => Named::Up,
        "down" | "arrowdown" => Named::Down,
        "pageup" => Named::PageUp,
        "pagedown" => Named::PageDown,
        "home" => Named::Home,
        "end" => Named::End,
        "enter" | "return" => Named::Enter,
        "escape" | "esc" => Named::Escape,
        "tab" => Named::Tab,
        "space" => Named::Space,
        "backspace" => Named::Backspace,
        "delete" | "del" => Named::Delete,
        "volumeup" => Named::VolumeUp,
        "volumedown" => Named::VolumeDown,
        "volumemute" | "mute" => Named::VolumeMute,
        "playpause" | "mediaplaypause" => Named::MediaPlayPause,
        "medianext" | "nexttrack" => Named::MediaNext,
        "mediaprevious" | "previoustrack" => Named::MediaPrevious,
        f if f.len() >= 2 && f.starts_with('f') => {
            let n: u8 = f[1..].parse().ok()?;
            if (1..=12).contains(&n) {
                Named::F(n)
            } else {
                return None;
            }
        }
        _ => return None,
    })
}

/// Parse `["mod", "shift", "z"]`-style key lists. `mod` is Command on macOS and
/// Control elsewhere, so one surface works on every OS. The last entry is the
/// key; everything before it must be a modifier.
pub fn parse_combo(keys: &[String], on_macos: bool) -> Result<Combo, ActionError> {
    let Some((last, mods)) = keys.split_last() else {
        return Err(ActionError::InvalidArgs(
            "at least one key is required".into(),
        ));
    };
    let mut modifiers = Vec::new();
    for m in mods {
        let modifier = match m.to_ascii_lowercase().as_str() {
            "ctrl" | "control" => Modifier::Ctrl,
            "shift" => Modifier::Shift,
            "alt" | "option" => Modifier::Alt,
            "cmd" | "command" | "meta" | "super" | "win" => Modifier::Meta,
            "mod" => {
                if on_macos {
                    Modifier::Meta
                } else {
                    Modifier::Ctrl
                }
            }
            other => {
                return Err(ActionError::InvalidArgs(format!(
                    "`{other}` is not a modifier"
                )))
            }
        };
        if !modifiers.contains(&modifier) {
            modifiers.push(modifier);
        }
    }
    let lower = last.to_ascii_lowercase();
    let key = if let Some(named) = parse_named(&lower) {
        KeyKind::Named(named)
    } else {
        let mut chars = last.chars();
        match (chars.next(), chars.next()) {
            (Some(c), None) => KeyKind::Char(c.to_ascii_lowercase()),
            _ => return Err(ActionError::InvalidArgs(format!("unknown key `{last}`"))),
        }
    };
    Ok(Combo { modifiers, key })
}

// ---------------------------------------------------------------------------
// Real backend
// ---------------------------------------------------------------------------

pub struct EnigoKeyboard;

impl Keyboard for EnigoKeyboard {
    fn press(&self, combo: &Combo) -> Result<(), ActionError> {
        use enigo::{Direction, Enigo, Key, Keyboard as _, Settings};

        let fail = |e: &dyn std::fmt::Display| {
            ActionError::Failed(format!(
                "keyboard automation failed: {e}. On macOS grant Accessibility access; on Linux an X11 session is required."
            ))
        };
        let mut enigo = Enigo::new(&Settings::default()).map_err(|e| fail(&e))?;

        let modifier_key = |m: Modifier| match m {
            Modifier::Ctrl => Key::Control,
            Modifier::Shift => Key::Shift,
            Modifier::Alt => Key::Alt,
            Modifier::Meta => Key::Meta,
        };
        let key = match combo.key {
            KeyKind::Char(c) => Key::Unicode(c),
            KeyKind::Named(n) => match n {
                Named::Left => Key::LeftArrow,
                Named::Right => Key::RightArrow,
                Named::Up => Key::UpArrow,
                Named::Down => Key::DownArrow,
                Named::PageUp => Key::PageUp,
                Named::PageDown => Key::PageDown,
                Named::Home => Key::Home,
                Named::End => Key::End,
                Named::Enter => Key::Return,
                Named::Escape => Key::Escape,
                Named::Tab => Key::Tab,
                Named::Space => Key::Space,
                Named::Backspace => Key::Backspace,
                Named::Delete => Key::Delete,
                Named::F(n) => match n {
                    1 => Key::F1,
                    2 => Key::F2,
                    3 => Key::F3,
                    4 => Key::F4,
                    5 => Key::F5,
                    6 => Key::F6,
                    7 => Key::F7,
                    8 => Key::F8,
                    9 => Key::F9,
                    10 => Key::F10,
                    11 => Key::F11,
                    _ => Key::F12,
                },
                Named::VolumeUp => Key::VolumeUp,
                Named::VolumeDown => Key::VolumeDown,
                Named::VolumeMute => Key::VolumeMute,
                Named::MediaPlayPause => Key::MediaPlayPause,
                Named::MediaNext => Key::MediaNextTrack,
                Named::MediaPrevious => Key::MediaPrevTrack,
            },
        };

        for m in &combo.modifiers {
            enigo
                .key(modifier_key(*m), Direction::Press)
                .map_err(|e| fail(&e))?;
        }
        let click = enigo.key(key, Direction::Click);
        // Always release modifiers, even if the click failed.
        for m in combo.modifiers.iter().rev() {
            let _ = enigo.key(modifier_key(*m), Direction::Release);
        }
        click.map_err(|e| fail(&e))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn keys(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn mod_is_platform_aware() {
        let mac = parse_combo(&keys(&["mod", "c"]), true).unwrap();
        let other = parse_combo(&keys(&["mod", "c"]), false).unwrap();
        assert_eq!(mac.modifiers, vec![Modifier::Meta]);
        assert_eq!(other.modifiers, vec![Modifier::Ctrl]);
        assert_eq!(mac.key, KeyKind::Char('c'));
    }

    #[test]
    fn parses_named_keys_and_multiple_modifiers() {
        let c = parse_combo(&keys(&["ctrl", "shift", "PageUp"]), false).unwrap();
        assert_eq!(c.modifiers, vec![Modifier::Ctrl, Modifier::Shift]);
        assert_eq!(c.key, KeyKind::Named(Named::PageUp));
        assert_eq!(
            parse_combo(&keys(&["f5"]), false).unwrap().key,
            KeyKind::Named(Named::F(5))
        );
        assert_eq!(
            parse_combo(&keys(&["alt", "left"]), false).unwrap().key,
            KeyKind::Named(Named::Left)
        );
    }

    #[test]
    fn rejects_garbage() {
        assert!(parse_combo(&[], false).is_err());
        assert!(parse_combo(&keys(&["banana", "c"]), false).is_err());
        assert!(parse_combo(&keys(&["ctrl", "notakey"]), false).is_err());
        assert!(parse_combo(&keys(&["f13"]), false).is_err());
    }
}
