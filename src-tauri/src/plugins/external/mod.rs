//! External plugins: third-party code in a WebAssembly sandbox.
//!
//! The pieces, from the disk upward:
//!
//! * [`manifest`] parses and validates `plugin.json`.
//! * [`discovery`] finds plugin directories and reads them safely.
//! * [`sandbox`] runs a module with strict limits (the only Wasmtime user).
//! * [`wasm_plugin`] puts a sandboxed module behind the existing `Plugin`
//!   trait, so the registry, action engine, event bus and feedback rules are reused
//!   unchanged.
//! * [`host`] owns discovery results, enabled flags, configuration and
//!   lifecycle, and registers plugins into the shared registry.
//!
//! The contract is described in `PLUGIN_SDK_PLAN.md` and `docs/plugin-sdk/`.

pub mod capabilities;
pub mod discovery;
pub mod host;
mod legacy_secrets;
pub mod log;
pub mod manifest;
pub mod network;
pub mod sandbox;
pub mod secrets;
pub mod wasm_plugin;

#[cfg(test)]
#[allow(dead_code)]
mod testkit;

use serde::Serialize;

pub use discovery::Issue;
pub use host::{
    HostError, LegacyMigration, LegacySecretsView, PluginHost, PluginInfo, PluginsOverview,
    ScanReport,
};
pub use log::LogEntry;
pub use sandbox::Limits;

/// Protocol (ABI) version between host and module.
pub const ABI_VERSION: u32 = 1;
/// SDK version this host understands.
pub const SDK_VERSION: &str = "0.2.1";

/// Environment variable with extra plugin folders (path list), for development.
pub const PLUGINS_ENV: &str = "LAUNCHDECK_PLUGINS";

/// Where plugins are searched: the `plugins` folder in the app data directory
/// first, then every folder in the environment variable. Order decides which
/// copy wins when two folders carry the same plugin id.
pub fn plugin_roots(
    app_data: &std::path::Path,
    extra: Option<std::ffi::OsString>,
) -> Vec<std::path::PathBuf> {
    let mut roots = vec![app_data.join("plugins")];
    if let Some(extra) = extra {
        roots.extend(std::env::split_paths(&extra).filter(|p| !p.as_os_str().is_empty()));
    }
    roots
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum PluginStatus {
    /// Loaded and healthy.
    Enabled,
    /// Never loaded, never invoked.
    Disabled,
    /// Failed to load, or stopped after a failure. Never invoked.
    Error,
}

/// What kind of problem keeps a plugin from running. Decided where the problem
/// happens; the Plugins screen says it in one sentence and keeps the detail
/// (`error`) under Details.
#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ProblemKind {
    /// Made for another LaunchDeck: manifest schema, protocol (ABI) or SDK.
    Incompatible,
    /// `plugin.json` cannot be read or has a mistake.
    InvalidManifest,
    /// The module cannot be read, compiled or linked, lacks an export, or
    /// reports another protocol than its manifest.
    InvalidModule,
    /// A folder found earlier already provides this id.
    Duplicate,
    /// `init` trapped, timed out or answered with a failure.
    InitFailed,
    /// Stopped after a crash or a trap while running.
    Stopped,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct EventInfo {
    #[serde(rename = "type")]
    pub kind: String,
    pub description: String,
}

/// What an external plugin adds to the common plugin manifest.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ExternalInfo {
    pub status: PluginStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// What kind of problem `error` is. Present exactly when `error` is.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<ProblemKind>,
    pub author: String,
    pub abi: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sdk: Option<String>,
    pub events: Vec<EventInfo>,
}

#[cfg(test)]
mod state_tests;
#[cfg(test)]
mod tests;
