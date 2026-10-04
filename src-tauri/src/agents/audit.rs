//! Lightweight audit trail: `audit.jsonl`, one JSON object per line.
//!
//! What is recorded: lifecycle (session started / completed / failed /
//! cancelled), approvals (requested, approved, rejected, withdrawn) and
//! interventions (commands issued and refused). What is deliberately NOT
//! recorded: prompts, instructions, task text, command lines and tool inputs.
//! Only ids, tool *names* and short codes reach this file.

use std::collections::VecDeque;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use super::model::summarize;
use crate::events::now_ms;

/// Rotate to `audit.1.jsonl` beyond this size. One generation is kept.
pub const MAX_BYTES: u64 = 256 * 1024;
const RECENT: usize = 100;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Actor {
    /// A person acting through a pad or the desktop app.
    User,
    /// An agent or its adapter reporting something.
    Agent,
    /// LaunchDeck itself (timeouts, safety refusals).
    System,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuditEntry {
    pub ts: u64,
    /// Dotted kind, for example `approval.approved` or `session.failed`.
    pub kind: String,
    pub actor: Actor,
    pub agent_id: Option<String>,
    pub session_id: Option<String>,
    /// Tool or operation *name* only.
    pub tool: Option<String>,
    /// Short code or concise summary. Never user content.
    pub detail: Option<String>,
    /// The installed plugin an entry is about (permission changes).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plugin_id: Option<String>,
}

impl AuditEntry {
    pub fn new(kind: &str, actor: Actor) -> Self {
        Self {
            ts: now_ms(),
            kind: kind.to_string(),
            actor,
            agent_id: None,
            session_id: None,
            tool: None,
            detail: None,
            plugin_id: None,
        }
    }

    pub fn plugin(mut self, id: &str) -> Self {
        self.plugin_id = Some(id.to_string());
        self
    }

    pub fn agent(mut self, id: &str) -> Self {
        self.agent_id = Some(id.to_string());
        self
    }

    pub fn session(mut self, id: &str) -> Self {
        self.session_id = Some(id.to_string());
        self
    }

    pub fn tool(mut self, name: Option<&str>) -> Self {
        self.tool = name.map(|t| summarize(t, 60));
        self
    }

    pub fn detail(mut self, text: &str) -> Self {
        self.detail = Some(summarize(text, 120));
        self
    }
}

pub struct Audit {
    path: Option<PathBuf>,
    recent: VecDeque<AuditEntry>,
}

impl Audit {
    /// `path = None` keeps the trail in memory only (tests, read-only dirs).
    pub fn new(path: Option<PathBuf>) -> Self {
        Self {
            path,
            recent: VecDeque::new(),
        }
    }

    pub fn record(&mut self, entry: AuditEntry) {
        if let Some(path) = &self.path {
            if let Err(e) = append_line(path, &entry) {
                log::warn!("could not write the audit log: {e}");
            }
        }
        self.recent.push_back(entry);
        while self.recent.len() > RECENT {
            self.recent.pop_front();
        }
    }

    /// Newest last.
    pub fn recent(&self, limit: usize) -> Vec<AuditEntry> {
        let skip = self.recent.len().saturating_sub(limit);
        self.recent.iter().skip(skip).cloned().collect()
    }
}

fn append_line(path: &PathBuf, entry: &AuditEntry) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    if fs::metadata(path).is_ok_and(|m| m.len() > MAX_BYTES) {
        let rotated = path.with_file_name("audit.1.jsonl");
        let _ = fs::rename(path, rotated);
    }
    let mut line = serde_json::to_string(entry).map_err(std::io::Error::other)?;
    line.push('\n');
    let mut options = OpenOptions::new();
    options.create(true).append(true);
    // Who approved what, and when, is for the person who owns the machine.
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut f = options.open(path)?;
    f.write_all(line.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn the_audit_log_is_readable_by_the_owner_only() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("audit.jsonl");
        let mut audit = Audit::new(Some(path.clone()));
        audit.record(AuditEntry::new("session.started", Actor::Agent).agent("atlas"));
        let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600, "audit.jsonl is {mode:o}");
    }

    #[test]
    fn entries_are_json_lines_with_ids_and_no_free_text() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("audit.jsonl");
        let mut audit = Audit::new(Some(path.clone()));
        audit.record(
            AuditEntry::new("approval.approved", Actor::User)
                .agent("atlas")
                .session("s1")
                .tool(Some("Bash")),
        );
        audit.record(AuditEntry::new("session.failed", Actor::Agent).agent("atlas"));
        let text = fs::read_to_string(&path).unwrap();
        let lines: Vec<_> = text.lines().collect();
        assert_eq!(lines.len(), 2);
        let first: AuditEntry = serde_json::from_str(lines[0]).unwrap();
        assert_eq!(first.kind, "approval.approved");
        assert_eq!(first.tool.as_deref(), Some("Bash"));
        assert_eq!(audit.recent(10).len(), 2);
    }

    #[test]
    fn details_are_truncated_to_a_short_summary() {
        let long = "x".repeat(1000);
        let e = AuditEntry::new("command.failed", Actor::System).detail(&long);
        assert!(e.detail.unwrap().chars().count() <= 120);
    }

    #[test]
    fn the_file_rotates_instead_of_growing_forever() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("audit.jsonl");
        fs::write(&path, vec![b'x'; MAX_BYTES as usize + 10]).unwrap();
        let mut audit = Audit::new(Some(path.clone()));
        audit.record(AuditEntry::new("session.started", Actor::Agent));
        assert!(dir.path().join("audit.1.jsonl").exists());
        assert!(fs::metadata(&path).unwrap().len() < 1024);
    }

    #[test]
    fn memory_only_trail_keeps_the_most_recent_entries() {
        let mut audit = Audit::new(None);
        for i in 0..150 {
            audit.record(AuditEntry::new("session.started", Actor::Agent).detail(&i.to_string()));
        }
        let r = audit.recent(500);
        assert_eq!(r.len(), RECENT);
        assert_eq!(r.last().unwrap().detail.as_deref(), Some("149"));
    }
}
