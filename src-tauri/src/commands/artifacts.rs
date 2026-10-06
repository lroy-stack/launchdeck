//! Portable Surface files and Packs: export into the exports folder
//! (Downloads) and import with a plan the person reviews. Thin:
//! `ArtifactService` owns the behaviour. The webview never sends a path: an
//! export gets a new name of its own, and a file to import arrives as its
//! bytes. Planning asks installed plugins to check pads, and saving may wait
//! for the system to allow Downloads, so those run off the UI thread.

use tauri::ipc::{InvokeBody, Request};
use tauri::{AppHandle, Manager, State};

use super::{CmdResult, CommandError};
use crate::artifacts::{
    ApplyOutcome, ArtifactLimits, ExportPreview, ExportRequest, ExportSaved, ImportDecisions,
    InspectOutcome, PreviewOutcome, Rejection, RejectionCode,
};
use crate::AppState;

/// The header that carries the file's name, as `encodeURIComponent` wrote it.
const FILE_NAME_HEADER: &str = "x-launchdeck-file-name";

#[tauri::command]
pub(crate) fn artifacts_limits(state: State<'_, AppState>) -> ArtifactLimits {
    state.artifacts.limits()
}

#[tauri::command]
pub(crate) fn artifacts_export_preview(
    state: State<'_, AppState>,
    request: ExportRequest,
) -> ExportPreview {
    state.artifacts.export_preview(&request)
}

#[tauri::command]
pub(crate) async fn artifacts_export_save(
    app: AppHandle,
    request: ExportRequest,
) -> CmdResult<ExportSaved> {
    tauri::async_runtime::spawn_blocking(move || {
        app.state::<AppState>().artifacts.export_save(&request)
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| CommandError::from(e.to_string()))
}

/// Show the folder of the last exported file.
#[tauri::command]
pub(crate) fn artifacts_show_export(state: State<'_, AppState>) -> CmdResult<()> {
    state
        .artifacts
        .show_export()
        .map_err(|e| CommandError::from(e.to_string()))
}

/// The file's bytes are the raw body; anything else is no file. Its name,
/// shown and compared with its kind only, is in a header.
#[tauri::command]
pub(crate) async fn artifacts_inspect(
    app: AppHandle,
    request: Request<'_>,
) -> CmdResult<InspectOutcome> {
    let InvokeBody::Raw(bytes) = request.body() else {
        return Ok(InspectOutcome::Rejected {
            rejection: Rejection::new(RejectionCode::Empty, "LaunchDeck received no file."),
        });
    };
    let bytes = bytes.clone();
    let name = request
        .headers()
        .get(FILE_NAME_HEADER)
        .and_then(|v| v.to_str().ok())
        .map(percent_decoded)
        .unwrap_or_default();
    tauri::async_runtime::spawn_blocking(move || {
        app.state::<AppState>().artifacts.inspect(&bytes, &name)
    })
    .await
    .map_err(|e| CommandError::from(e.to_string()))
}

#[tauri::command]
pub(crate) async fn artifacts_preview(
    app: AppHandle,
    plan_id: String,
    decisions: ImportDecisions,
) -> CmdResult<PreviewOutcome> {
    tauri::async_runtime::spawn_blocking(move || {
        app.state::<AppState>()
            .artifacts
            .preview(&plan_id, &decisions)
    })
    .await
    .map_err(|e| CommandError::from(e.to_string()))
}

#[tauri::command]
pub(crate) async fn artifacts_apply(
    app: AppHandle,
    plan_id: String,
    decisions: ImportDecisions,
    plan_digest: String,
) -> CmdResult<ApplyOutcome> {
    tauri::async_runtime::spawn_blocking(move || {
        app.state::<AppState>()
            .artifacts
            .apply(&plan_id, &decisions, &plan_digest)
    })
    .await
    .map_err(|e| CommandError::from(e.to_string()))
}

#[tauri::command]
pub(crate) fn artifacts_discard(state: State<'_, AppState>, plan_id: String) {
    state.artifacts.discard(&plan_id);
}

/// `%XX` sequences as the bytes they stand for, read as UTF-8 (a sequence
/// that is not one is replaced). Anything else is kept as it is. The result
/// is only shown, after `inspect` removes what a person cannot see.
fn percent_decoded(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let hex = bytes
            .get(i + 1..i + 3)
            .filter(|h| bytes[i] == b'%' && h.iter().all(u8::is_ascii_hexdigit))
            .and_then(|h| std::str::from_utf8(h).ok())
            .and_then(|h| u8::from_str_radix(h, 16).ok());
        match hex {
            Some(byte) => {
                out.push(byte);
                i += 3;
            }
            None => {
                out.push(bytes[i]);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::percent_decoded;

    #[test]
    fn a_file_name_is_read_as_encode_uri_component_wrote_it() {
        assert_eq!(
            percent_decoded("studio-1.0.0.ldpack"),
            "studio-1.0.0.ldpack"
        );
        assert_eq!(
            percent_decoded("My%20Deck%20%E2%9C%93.ldpack"),
            "My Deck ✓.ldpack"
        );
        assert_eq!(percent_decoded("100%25"), "100%");
        assert_eq!(percent_decoded("a%2Fb%5Cc"), "a/b\\c");
    }

    #[test]
    fn what_is_not_an_escape_is_kept() {
        assert_eq!(percent_decoded("50%"), "50%");
        assert_eq!(percent_decoded("%zz%4"), "%zz%4");
        assert_eq!(percent_decoded("%FF"), "\u{FFFD}");
        assert_eq!(percent_decoded("%+1"), "%+1");
        assert_eq!(percent_decoded(""), "");
    }
}
