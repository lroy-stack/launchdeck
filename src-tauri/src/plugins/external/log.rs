//! Plugin-scoped diagnostics.
//!
//! Every plugin gets a small ring buffer of log lines. Lines the plugin writes
//! (through `launchdeck.log`) and lines the host writes about the plugin (load
//! failures, traps, rejected output) land in the same place, so one read tells
//! the whole story. Lines are also forwarded to the normal `log` facade with
//! the target `plugin:<id>`.

use std::collections::VecDeque;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Serialize;

/// Lines kept per plugin.
pub const RING: usize = 100;
/// Longest line kept; longer ones are cut.
pub const MAX_LINE_BYTES: usize = 1024;

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Level {
    Debug,
    Info,
    Warn,
    Error,
}

impl Level {
    /// Levels as the guest sends them: 0 debug, 1 info, 2 warn, anything higher is an error.
    pub fn from_guest(n: i32) -> Level {
        match n {
            i32::MIN..=0 => Level::Debug,
            1 => Level::Info,
            2 => Level::Warn,
            _ => Level::Error,
        }
    }

    fn facade(self) -> log::Level {
        match self {
            Level::Debug => log::Level::Debug,
            Level::Info => log::Level::Info,
            Level::Warn => log::Level::Warn,
            Level::Error => log::Level::Error,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Origin {
    /// Written by the plugin.
    Plugin,
    /// Written by LaunchDeck about the plugin.
    Host,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LogEntry {
    pub ts: u64,
    pub level: Level,
    pub origin: Origin,
    pub message: String,
}

pub struct PluginLog {
    target: String,
    entries: Mutex<VecDeque<LogEntry>>,
}

/// One line, no control characters (so a plugin cannot forge extra log lines),
/// cut to [`MAX_LINE_BYTES`] on a character boundary.
pub fn clean_line(bytes: &[u8]) -> String {
    let text = String::from_utf8_lossy(bytes);
    let mut out = String::with_capacity(text.len().min(MAX_LINE_BYTES));
    for c in text.chars() {
        let c = if c.is_control() { ' ' } else { c };
        if out.len() + c.len_utf8() > MAX_LINE_BYTES {
            out.push('…');
            break;
        }
        out.push(c);
    }
    out
}

impl PluginLog {
    pub fn new(plugin_id: &str) -> Self {
        Self {
            target: format!("plugin:{plugin_id}"),
            entries: Mutex::new(VecDeque::with_capacity(RING)),
        }
    }

    pub fn push(&self, level: Level, origin: Origin, message: &str) {
        let message = clean_line(message.as_bytes());
        log::log!(target: &self.target, level.facade(), "{message}");
        let ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);
        let mut entries = self.entries.lock().unwrap_or_else(|e| e.into_inner());
        if entries.len() == RING {
            entries.pop_front();
        }
        entries.push_back(LogEntry {
            ts,
            level,
            origin,
            message,
        });
    }

    pub fn host(&self, level: Level, message: &str) {
        self.push(level, Origin::Host, message);
    }

    pub fn entries(&self) -> Vec<LogEntry> {
        self.entries
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .iter()
            .cloned()
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_only_the_last_lines() {
        let log = PluginLog::new("a.b");
        for i in 0..(RING + 20) {
            log.host(Level::Info, &format!("line {i}"));
        }
        let e = log.entries();
        assert_eq!(e.len(), RING);
        assert_eq!(e[0].message, "line 20");
        assert_eq!(e.last().unwrap().message, format!("line {}", RING + 19));
    }

    #[test]
    fn lines_are_single_line_and_bounded() {
        let l = clean_line(b"a\nFAKE host line\r\x1b[31mred");
        assert!(!l.contains('\n') && !l.contains('\r') && !l.contains('\x1b'));
        let long = clean_line("é".repeat(5000).as_bytes());
        assert!(long.len() <= MAX_LINE_BYTES + '…'.len_utf8());
        assert!(long.ends_with('…'));
        assert_eq!(clean_line(&[0xff, 0xfe, b'x']), "\u{fffd}\u{fffd}x");
    }

    #[test]
    fn guest_levels_map() {
        assert_eq!(Level::from_guest(-5), Level::Debug);
        assert_eq!(Level::from_guest(1), Level::Info);
        assert_eq!(Level::from_guest(2), Level::Warn);
        assert_eq!(Level::from_guest(99), Level::Error);
    }
}
