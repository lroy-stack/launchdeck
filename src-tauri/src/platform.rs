//! The operating system LaunchDeck is built for.
//!
//! The only product-logic place that reads `cfg!(target_os)`. Everything else
//! takes a [`Platform`] as a parameter, so what it does on each system can be
//! tested on any machine. Adapter files that talk to an outside system (the
//! foreground observers, the keyboard backend, the credential store) keep the
//! `cfg` their crates need.

/// The system LaunchDeck is built for. A few starter pads differ by system
/// (the terminal, the editor, the file manager, the screenshot, and how a demo
/// command waits), as do the Context Engine's application namespace, the name
/// of the credential store and the modifier key of a shortcut; everything else
/// is the same everywhere.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Platform {
    /// Linux, and any other system that is neither macOS nor Windows.
    Linux,
    MacOs,
    Windows,
}

impl Platform {
    /// The system this was built for.
    pub fn current() -> Platform {
        if cfg!(target_os = "macos") {
            Platform::MacOs
        } else if cfg!(windows) {
            Platform::Windows
        } else {
            Platform::Linux
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn current_agrees_with_the_standard_library() {
        let expected = match std::env::consts::OS {
            "macos" => Platform::MacOs,
            "windows" => Platform::Windows,
            _ => Platform::Linux,
        };
        assert_eq!(Platform::current(), expected);
    }
}
