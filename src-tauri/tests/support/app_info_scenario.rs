//! What `app_info` answers, with a placeholder for the data folder so that no
//! one's real path is written into the repository. Shared by the
//! `dump_app_info` example and the contract test.

use std::path::Path;

use launchdeck_lib::AppInfo;
use serde_json::Value;

/// Stands in for the app data folder in `examples/app-info.json`.
pub const DATA_FOLDER: &str = "<the app data folder>";

pub fn capture() -> Value {
    serde_json::to_value(AppInfo::new(Path::new(DATA_FOLDER))).expect("serialise")
}
