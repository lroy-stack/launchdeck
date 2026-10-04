//! Claude Code, through its HTTP hooks.
//!
//! Verified against Claude Code 2.1.289 on 2026-10-04: the payloads in
//! `tests/fixtures/claude-code/` were captured from a real session, and a held
//! `PermissionRequest` answered with the decision body below was honoured.
//! Event names and fields follow the public hooks reference
//! (<https://code.claude.com/docs/en/hooks>).

use serde_json::json;

use super::hooks::Dialect;

pub const ENV_VAR: &str = "LAUNCHDECK_HOOK_TOKEN";

pub static DIALECT: Dialect = Dialect {
    adapter_id: "claude-code",
    adapter_name: "Claude Code (hooks)",
    description: "Watches Claude Code sessions through its hooks and lets you approve or reject \
                  permission requests from the pads. Sessions appear when their first hook fires.",
    limits: &[
        "Hooks observe and can answer permission requests. They cannot pause, resume, cancel, \
         retry or instruct a running session, so those controls stay unavailable.",
        "You can answer from LaunchDeck only if control was granted before the request arrived; \
         otherwise Claude Code asks you in its own prompt.",
        "A held request falls back to Claude Code's own prompt after the hold time. Nothing is \
         ever approved on your behalf.",
        "A turn that ends in an API error is shown as failed; failures inside a tool call are not.",
        "Sessions that started before the hooks were configured show up at their next event.",
        "Checked against Claude Code 2.1.289 (2026-10-04).",
    ],
    agent_id: "claude-code",
    agent_name: "Claude Code",
    provider: "anthropic",
    harness: "claude-code",
    session_prefix: "cc-",
    route: "/hooks/claude-code",
};

/// Events worth subscribing to, and whether they may wait for a human.
const EVENTS: &[(&str, bool)] = &[
    ("SessionStart", false),
    ("UserPromptSubmit", false),
    ("PreToolUse", false),
    ("PermissionRequest", true),
    ("PostToolUse", false),
    ("PostToolUseFailure", false),
    ("Stop", false),
    ("StopFailure", false),
    ("SessionEnd", false),
];

/// The `settings.json` fragment to paste into `~/.claude/settings.json`
/// (or a project's `.claude/settings.json`). The token is read from the
/// environment, so the file never contains it.
pub fn settings_snippet(port: u16, hold_secs: u64) -> String {
    let url = format!("http://127.0.0.1:{port}{}", DIALECT.route);
    let mut hooks = serde_json::Map::new();
    for (event, waits) in EVENTS {
        let timeout = if *waits { hold_secs + 30 } else { 10 };
        hooks.insert(
            (*event).into(),
            json!([{ "hooks": [{
                "type": "http",
                "url": url,
                "headers": { "Authorization": format!("Bearer ${ENV_VAR}") },
                "allowedEnvVars": [ENV_VAR],
                "timeout": timeout,
            }]}]),
        );
    }
    serde_json::to_string_pretty(&json!({ "hooks": hooks })).unwrap_or_default()
}
