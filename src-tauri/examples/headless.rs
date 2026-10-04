//! Runs the LaunchDeck runtime without a window: the same Agent Control, the
//! same virtual Launchpad, the same localhost WebSocket. Useful to try the
//! Agent Bridge with `examples/generic-agent` on a machine without a display,
//! and in CI.
//!
//! A person drives it from stdin, exactly like they would use the app:
//!
//! ```text
//! grant <agentId>      give LaunchDeck control of that agent (it starts monitor-only)
//! revoke <agentId>
//! press <col> <row>    tap a pad on the virtual Launchpad (0-7, 0-7)
//! agents               list agents and sessions
//! pads                 draw the virtual Launchpad
//! quit
//! ```
//!
//! Nothing is ever approved on your behalf: approving means `press 0 2`
//! (the APPROVE pad), which only you can type.
//!
//! `cargo run --example headless` (port 47821, or `LAUNCHDECK_PORT=0` for any free port)

use std::io::BufRead;
use std::sync::Arc;

use launchdeck_lib::agents::model::AccessLevel;
use launchdeck_lib::core::model::ControlPos;
use launchdeck_lib::core::runtime::{NullEmitter, Runtime, RuntimeConfig};
use launchdeck_lib::plugins::keyboard::{Combo, Keyboard};
use launchdeck_lib::plugins::ActionError;
use launchdeck_lib::surfaces::default_workspace;

struct NoKeyboard;
impl Keyboard for NoKeyboard {
    fn press(&self, _combo: &Combo) -> Result<(), ActionError> {
        Ok(())
    }
}

#[tokio::main]
async fn main() {
    let mut workspace = default_workspace();
    workspace.preferences.external_events_enabled = true;
    if let Ok(p) = std::env::var("LAUNCHDECK_PORT") {
        workspace.preferences.external_events_port = p.parse().expect("LAUNCHDECK_PORT is a port");
    }
    let rt = Runtime::start(RuntimeConfig {
        workspace,
        store: None,
        emitter: Arc::new(NullEmitter),
        keyboard: Arc::new(NoKeyboard),
        scan_hardware: false,
        serve_external: true,
        demo_pace: 1.0,
        plugin_dirs: launchdeck_lib::plugins::external::plugin_roots(
            std::path::Path::new("."),
            std::env::var_os(launchdeck_lib::plugins::external::PLUGINS_ENV),
        )
        .into_iter()
        .skip(1)
        .collect(),
    })
    .await;
    let ext = rt.state().external;
    match ext.error {
        Some(e) => {
            eprintln!("{e}");
            std::process::exit(1);
        }
        None => println!("LaunchDeck headless: ws://127.0.0.1:{}", ext.port),
    }

    // Optional: also serve the hook endpoint for real harnesses.
    if let Ok(p) = std::env::var("LAUNCHDECK_HOOKS_PORT") {
        let port: u16 = p.parse().expect("LAUNCHDECK_HOOKS_PORT is a port");
        rt.agents().update_settings(|s| {
            s.hooks.enabled = true;
            s.hooks.port = port;
        });
        match rt.hooks().apply().await {
            Ok(Some(addr)) => {
                let setup = rt.hooks().setup();
                println!("hook endpoint: http://{addr}");
                println!("{}", setup.shell_export);
                if let Ok(dir) = std::env::var("LAUNCHDECK_SNIPPETS") {
                    let dir = std::path::Path::new(&dir);
                    std::fs::create_dir_all(dir).expect("snippet dir");
                    std::fs::write(
                        dir.join("claude-settings.json"),
                        &setup.claude_code_settings,
                    )
                    .expect("write claude settings");
                    std::fs::write(dir.join("codex-config.toml"), &setup.codex_config)
                        .expect("write codex config");
                    println!("snippets written to {}", dir.display());
                }
            }
            Ok(None) => {}
            Err(e) => eprintln!("{e}"),
        }
    }

    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<String>();
    std::thread::spawn(move || {
        for line in std::io::stdin().lock().lines().map_while(Result::ok) {
            if tx.send(line).is_err() {
                break;
            }
        }
    });

    while let Some(line) = rx.recv().await {
        let words: Vec<&str> = line.split_whitespace().collect();
        match words.as_slice() {
            ["grant", id] => report(rt.agents().set_access(id, AccessLevel::Control), "granted"),
            ["revoke", id] => report(rt.agents().set_access(id, AccessLevel::Monitor), "revoked"),
            ["press", c, r] => match (c.parse::<u8>(), r.parse::<u8>()) {
                (Ok(c), Ok(r)) if c < 8 && r < 8 => {
                    let h = rt.virtual_handle();
                    let pos = ControlPos::grid(c, r);
                    let _ = h.press(pos, 100);
                    let _ = h.release(pos);
                    println!("pressed ({c},{r})");
                }
                _ => println!("usage: press <col 0-7> <row 0-7>"),
            },
            ["agents"] => {
                let snap = rt.agents().snapshot();
                for a in &snap.agents {
                    println!(
                        "agent {} [{}] access={:?} online={}",
                        a.agent.id,
                        a.status.as_str(),
                        a.agent.access,
                        a.agent.online
                    );
                }
                for s in &snap.sessions {
                    println!(
                        "  session {} agent={} status={}{}",
                        s.session.id,
                        s.session.agent_id,
                        s.session.status.as_str(),
                        s.session
                            .pending_approval
                            .as_ref()
                            .map(|a| format!(" approval={}", a.id))
                            .unwrap_or_default()
                    );
                }
            }
            ["pads"] => {
                let frame = rt.virtual_handle().frame();
                for row in 0..8u8 {
                    let line: Vec<String> = (0..8u8)
                        .map(|col| match frame.get(ControlPos::grid(col, row)) {
                            Some(led) => format!("{}{:?}", led.color.to_hex(), led.animation),
                            None => "-".into(),
                        })
                        .collect();
                    println!("{}", line.join(" "));
                }
            }
            ["quit"] | ["exit"] => break,
            [] => {}
            _ => {
                println!("commands: grant <id>, revoke <id>, press <col> <row>, agents, pads, quit")
            }
        }
    }
}

fn report(result: Result<(), String>, ok: &str) {
    match result {
        Ok(()) => println!("{ok}"),
        Err(e) => println!("error: {e}"),
    }
}
