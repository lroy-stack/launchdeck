//! The app itself, apart from any one feature: its data folder, and every
//! version a tester needs to say which LaunchDeck they run (Settings > About).

use std::path::{Path, PathBuf};

use serde::Serialize;
use tauri::{AppHandle, Manager};

use super::CmdResult;
use crate::agents::bridge::PROTOCOL;
use crate::artifacts::FORMAT_VERSION;
use crate::core::model::SCHEMA_VERSION;
use crate::plugins::external::manifest::{MANIFEST_SCHEMA, OLDEST_SCHEMA};
use crate::plugins::external::{ABI_VERSION, SDK_VERSION};

/// What this build is offered as. The version stays plain numeric (installers
/// refuse pre-release suffixes), so the channel says it.
pub const CHANNEL: &str = "alpha";

/// Which LaunchDeck this is, and every number that decides what it can read
/// and talk to. The integers are compatibility numbers, not release numbers
/// (docs/architecture/versions.md).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppInfo {
    /// `Cargo.toml`'s version, the only source of the app version.
    pub app_version: &'static str,
    pub channel: &'static str,
    /// The plugin SDK this host understands.
    pub sdk_version: &'static str,
    /// The protocol between the host and a plugin's module.
    pub abi_version: u32,
    /// The plugin manifest schemas this host reads.
    pub manifest_schemas: SchemaRange,
    /// The schema of `workspace.json` this build writes.
    pub workspace_schema: u32,
    /// The Agent Bridge protocol this build speaks.
    pub bridge_protocol: u64,
    /// The Surface and Pack file format this build reads and writes.
    pub artifact_format: u32,
    /// `std::env::consts::OS` and `ARCH`, as Rust names them.
    pub os: &'static str,
    pub arch: &'static str,
    /// Where the workspace, its backups, plugins and settings are, for people to read.
    pub data_folder: String,
}

/// The oldest and the newest schema that is read, both included.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct SchemaRange {
    pub oldest: u32,
    pub newest: u32,
}

impl AppInfo {
    pub fn new(data_folder: &Path) -> Self {
        Self {
            app_version: env!("CARGO_PKG_VERSION"),
            channel: CHANNEL,
            sdk_version: SDK_VERSION,
            abi_version: ABI_VERSION,
            manifest_schemas: SchemaRange {
                oldest: OLDEST_SCHEMA,
                newest: MANIFEST_SCHEMA,
            },
            workspace_schema: SCHEMA_VERSION,
            bridge_protocol: PROTOCOL,
            artifact_format: FORMAT_VERSION,
            os: std::env::consts::OS,
            arch: std::env::consts::ARCH,
            data_folder: data_folder.display().to_string(),
        }
    }
}

fn data_folder(app: &AppHandle) -> CmdResult<PathBuf> {
    app.path()
        .app_data_dir()
        .map_err(|e| format!("LaunchDeck could not find its data folder ({e}).").into())
}

/// Every version a tester needs, and where the data folder is.
#[tauri::command]
pub(crate) fn app_info(app: AppHandle) -> CmdResult<AppInfo> {
    Ok(AppInfo::new(&data_folder(&app)?))
}

/// Show the app data folder (the workspace, its backups, plugins and
/// settings) in the system's file manager.
#[tauri::command]
pub(crate) fn app_reveal_data_folder(app: AppHandle) -> CmdResult<()> {
    let folder = data_folder(&app)?;
    Ok(open::that_detached(&folder)
        .map_err(|e| format!("LaunchDeck could not open {} ({e}).", folder.display()))?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn app_info_reports_the_constants_of_this_build() {
        let info = AppInfo::new(Path::new("/data/LaunchDeck"));
        assert_eq!(
            info,
            AppInfo {
                app_version: env!("CARGO_PKG_VERSION"),
                channel: "alpha",
                sdk_version: SDK_VERSION,
                abi_version: ABI_VERSION,
                manifest_schemas: SchemaRange {
                    oldest: OLDEST_SCHEMA,
                    newest: MANIFEST_SCHEMA,
                },
                workspace_schema: SCHEMA_VERSION,
                bridge_protocol: PROTOCOL,
                artifact_format: FORMAT_VERSION,
                os: std::env::consts::OS,
                arch: std::env::consts::ARCH,
                data_folder: "/data/LaunchDeck".into(),
            }
        );
        // The numbers docs/architecture/versions.md lists: a change here is a
        // compatibility decision, and that table changes with it.
        assert_eq!(
            (info.sdk_version, info.abi_version, info.workspace_schema),
            ("0.2.1", 1, 4)
        );
        assert_eq!(
            (info.manifest_schemas.oldest, info.manifest_schemas.newest),
            (1, 4)
        );
        assert_eq!((info.bridge_protocol, info.artifact_format), (1, 1));
    }

    #[test]
    fn app_info_is_camel_case_on_the_wire() {
        let json = serde_json::to_value(AppInfo::new(Path::new("x"))).unwrap();
        let keys: std::collections::BTreeSet<_> = json
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        assert_eq!(
            keys,
            std::collections::BTreeSet::from([
                "abiVersion",
                "appVersion",
                "arch",
                "artifactFormat",
                "bridgeProtocol",
                "channel",
                "dataFolder",
                "manifestSchemas",
                "os",
                "sdkVersion",
                "workspaceSchema"
            ])
        );
        assert_eq!(json["manifestSchemas"]["oldest"], 1);
    }
}
