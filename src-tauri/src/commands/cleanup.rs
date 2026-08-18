use mangodisk_core::{
    ApplicationCloseBatchResult, CleanupApplicationCloseRequest, CleanupRequest, CleanupResult,
    CleanupScanResult, CleanupScanService, CleanupService,
};

use crate::events;

use super::error::{run_blocking, CommandResult};

#[tauri::command]
pub async fn scan_cleanup_candidates(
    app: tauri::AppHandle,
    deep_project_discovery: Option<bool>,
) -> CommandResult<CleanupScanResult> {
    run_blocking("scan_cleanup_candidates", move || {
        CleanupScanService::scan_with_deep_project_discovery(
            deep_project_discovery.unwrap_or(false),
            move |progress| events::emit(&app, events::CLEANUP_SCAN_PROGRESS, progress),
        )
    })
    .await
}

#[tauri::command]
pub fn cancel_cleanup_scan() {
    CleanupScanService::cancel();
}

#[tauri::command]
pub fn cancel_cleanup_execution() {
    CleanupService::cancel();
}

#[tauri::command]
pub async fn close_cleanup_applications(
    request: CleanupApplicationCloseRequest,
) -> CommandResult<ApplicationCloseBatchResult> {
    run_blocking("close_cleanup_applications", move || {
        CleanupService::close_applications(request)
    })
    .await
}

#[tauri::command]
pub async fn execute_cleanup(
    app: tauri::AppHandle,
    mut request: CleanupRequest,
    deep_cleanup_operation_id: String,
) -> CommandResult<CleanupResult> {
    // The desktop workflow discovers project roots itself. Rejecting transport-
    // supplied roots prevents a compromised WebView from widening the cleanup
    // scope beyond the preview that the user reviewed.
    request.project_roots.clear();
    run_blocking("execute_cleanup", move || {
        CleanupService::execute_deep_cleanup_step_with_progress(
            request,
            deep_cleanup_operation_id,
            move |progress| {
                events::emit(&app, events::CLEANUP_EXECUTION_PROGRESS, progress);
            },
        )
    })
    .await
}
