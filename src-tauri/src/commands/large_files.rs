use mangodisk_core::{LargeFileScanMode, LargeFileService, LargeFilesResult};

use crate::events;

use super::error::{run_blocking, CommandResult};

#[tauri::command]
pub async fn find_large_files(
    app: tauri::AppHandle,
    path: Option<String>,
    minimum_bytes: u64,
    scan_mode: LargeFileScanMode,
    excluded_paths: Vec<String>,
) -> CommandResult<LargeFilesResult> {
    run_blocking("find_large_files", move || {
        LargeFileService::find_with_progress(
            path,
            minimum_bytes,
            scan_mode,
            excluded_paths,
            move |progress| {
                events::emit(&app, events::LARGE_FILES_PROGRESS, progress);
            },
        )
    })
    .await
}

#[tauri::command]
pub async fn filter_large_files(
    scan_id: u64,
    minimum_bytes: u64,
) -> CommandResult<LargeFilesResult> {
    run_blocking("filter_large_files", move || {
        LargeFileService::filter(scan_id, minimum_bytes)
    })
    .await
}

#[tauri::command]
pub fn cancel_large_files() {
    LargeFileService::cancel();
}
