//! Real-harness adapters. Everything that knows a vendor lives in this folder;
//! the Agent Control core, the surfaces and the desktop UI do not.
//!
//! * `http`        a small loopback HTTP endpoint (bearer token, bounded)
//! * `hooks`       the shared hook translator and adapter
//! * `claude_code` / `codex`  dialects: names, limits, setup snippets

pub mod claude_code;
pub mod codex;
pub mod hooks;
pub mod http;

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::Serialize;

use self::hooks::HookAdapter;
use super::adapter::AdapterState;
use super::service::AgentControl;

/// A fresh 256-bit token, hex encoded.
pub fn generate_token() -> String {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).expect("the operating system provides randomness");
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// What the desktop shows when the user asks how to connect a harness.
/// Contains the token: only produced on request, never part of a snapshot.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HookSetup {
    pub enabled: bool,
    pub port: u16,
    pub env_var: String,
    pub token: String,
    /// `export LAUNCHDECK_HOOK_TOKEN=...` for the shell that starts the harness.
    pub shell_export: String,
    pub claude_code_settings: String,
    pub codex_config: String,
}

/// Owns the hook endpoint: starts it, stops it and keeps the adapters in step
/// with the user's settings.
pub struct HookManager {
    ctl: Arc<AgentControl>,
    claude: Arc<HookAdapter>,
    codex: Arc<HookAdapter>,
    server: Mutex<Option<http::Server>>,
}

impl HookManager {
    /// Registers both adapters (switched off until the user enables the endpoint).
    pub fn new(ctl: Arc<AgentControl>) -> Arc<Self> {
        let claude = HookAdapter::new(&claude_code::DIALECT);
        let codex = HookAdapter::new(&codex::DIALECT);
        ctl.register_adapter(claude.clone());
        ctl.register_adapter(codex.clone());
        Arc::new(Self {
            ctl,
            claude,
            codex,
            server: Mutex::new(None),
        })
    }

    pub fn claude_code(&self) -> &Arc<HookAdapter> {
        &self.claude
    }

    pub fn codex(&self) -> &Arc<HookAdapter> {
        &self.codex
    }

    /// Bring the endpoint in line with the stored hook settings. Returns where
    /// it listens, if it does.
    pub async fn apply(&self) -> Result<Option<SocketAddr>, String> {
        // Stop whatever is running first; held requests are released.
        let previous = self.server.lock().unwrap_or_else(|p| p.into_inner()).take();
        drop(previous);

        let settings = self.ctl.settings().hooks;
        let hold = Duration::from_secs(settings.hold_secs.clamp(5, 600));
        let off = |a: &Arc<HookAdapter>, why: &str| {
            a.set_enabled(false, AdapterState::Disabled, why, hold);
        };
        if !settings.enabled {
            let why = "Turn on the local hook endpoint in Agents > Adapters.";
            off(&self.claude, why);
            off(&self.codex, why);
            return Ok(None);
        }

        // The token is created the first time the endpoint is enabled and then kept.
        if settings.token.is_empty() {
            let token = generate_token();
            self.ctl.update_settings(|s| s.hooks.token = token);
        }
        let token = self.ctl.settings().hooks.token;

        let adapters = [
            (
                &self.claude,
                claude_code::DIALECT.adapter_id,
                claude_code::DIALECT.route,
            ),
            (&self.codex, codex::DIALECT.adapter_id, codex::DIALECT.route),
        ];
        let mut routes: HashMap<String, Arc<dyn http::Route>> = HashMap::new();
        for (adapter, id, route) in &adapters {
            if settings.harness_enabled(id) {
                routes.insert((*route).into(), adapter.route());
            }
        }
        match http::serve(settings.port, token, routes).await {
            Ok(server) => {
                let addr = server.addr();
                *self.server.lock().unwrap_or_else(|p| p.into_inner()) = Some(server);
                let listening = format!("Listening on http://{addr}");
                for (adapter, id, _) in &adapters {
                    if settings.harness_enabled(id) {
                        adapter.set_enabled(true, AdapterState::Active, &listening, hold);
                    } else {
                        off(adapter, "Switched off in the hook settings.");
                    }
                }
                Ok(Some(addr))
            }
            Err(e) => {
                let why = format!("Could not listen on 127.0.0.1:{}: {e}", settings.port);
                for (adapter, _, _) in &adapters {
                    adapter.set_enabled(false, AdapterState::Error, &why, hold);
                }
                Err(why)
            }
        }
    }

    /// The text to copy into each harness's configuration.
    pub fn setup(&self) -> HookSetup {
        let h = self.ctl.settings().hooks;
        HookSetup {
            enabled: h.enabled,
            port: h.port,
            env_var: claude_code::ENV_VAR.to_string(),
            shell_export: format!("export {}={}", claude_code::ENV_VAR, h.token),
            token: h.token,
            claude_code_settings: claude_code::settings_snippet(h.port, h.hold_secs),
            codex_config: codex::config_snippet(h.port, h.hold_secs),
        }
    }

    /// Replace the token. Harness configurations must be refreshed afterwards,
    /// which is the point: a token that leaked stops working.
    pub async fn rotate_token(&self) -> Result<Option<SocketAddr>, String> {
        let token = generate_token();
        self.ctl.update_settings(|s| s.hooks.token = token);
        self.apply().await
    }
}

#[cfg(test)]
mod tests;
