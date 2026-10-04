//! Codex, through command hooks and a `curl` relay.
//!
//! Codex runs hooks as commands (OpenAI's hook documentation, read on
//! 2026-10-04, lists `command` and `mcp_tool` handlers), so the relay pipes
//! the hook payload to LaunchDeck's local endpoint and prints the reply.
//!
//! Honesty note: this dialect was written from that documentation (event
//! names, payload fields and the `PermissionRequest` decision format) and is
//! covered by tests on payloads built from the documented shape. It has not
//! been run against a Codex installation. For full control Codex also offers
//! `codex app-server`; this adapter does not use it.

use super::hooks::Dialect;

pub const ENV_VAR: &str = "LAUNCHDECK_HOOK_TOKEN";

pub static DIALECT: Dialect = Dialect {
    adapter_id: "codex",
    adapter_name: "Codex (hooks)",
    description: "Watches Codex sessions through its command hooks and lets you approve or reject \
                  permission requests from the pads. Needs the small curl relay shown in Setup.",
    limits: &[
        "Hooks observe and can answer permission requests. They cannot pause, resume, cancel, \
         retry or instruct a running session, so those controls stay unavailable.",
        "You can answer from LaunchDeck only if control was granted before the request arrived; \
         otherwise Codex asks you in its own prompt.",
        "A held request falls back to Codex's own prompt after the hold time. Nothing is ever \
         approved on your behalf.",
        "Written from OpenAI's published hook documentation (2026-10-04) and not yet run against \
         a Codex installation: treat it as untested.",
        "Codex's app-server protocol would allow real control (start, interrupt, steer); it is \
         not implemented here.",
        "The relay uses curl and an sh-compatible shell.",
    ],
    agent_id: "codex",
    agent_name: "Codex",
    provider: "openai",
    harness: "codex",
    session_prefix: "cx-",
    route: "/hooks/codex",
};

const EVENTS: &[(&str, bool)] = &[
    ("SessionStart", false),
    ("UserPromptSubmit", false),
    ("PreToolUse", false),
    ("PermissionRequest", true),
    ("PostToolUse", false),
    ("Stop", false),
    ("Interrupt", false),
    ("SessionEnd", false),
];

/// A `~/.codex/config.toml` fragment. `|| true` keeps a hook that cannot reach
/// LaunchDeck (not running, endpoint off) from failing the Codex turn: no
/// output means "no decision".
pub fn config_snippet(port: u16, hold_secs: u64) -> String {
    let url = format!("http://127.0.0.1:{port}{}", DIALECT.route);
    let mut out = String::new();
    for (event, waits) in EVENTS {
        let timeout = if *waits { hold_secs + 30 } else { 10 };
        let curl = format!(
            "curl -s -m {timeout} -X POST -H \"Authorization: Bearer ${ENV_VAR}\" \
             -H \"Content-Type: application/json\" --data-binary @- {url} || true"
        );
        out.push_str(&format!(
            "[[hooks.{event}]]\n\n[[hooks.{event}.hooks]]\ntype = \"command\"\ncommand = '{curl}'\ntimeout = {timeout}\n\n"
        ));
    }
    out.trim_end().to_string() + "\n"
}
