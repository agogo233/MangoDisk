use super::error::{into_command_result, run_blocking, CommandResult};
use crate::services::analysis_launch;

use crate::services::explorer_integration::{self, Status};

#[tauri::command]
pub async fn explorer_integration_status() -> CommandResult<Status> {
    run_blocking("explorer_integration_status", explorer_integration::status).await
}

#[tauri::command]
pub async fn explorer_integration_set_enabled(
    enabled: bool,
    title: String,
) -> CommandResult<Status> {
    run_blocking("explorer_integration_set_enabled", move || {
        explorer_integration::set_enabled(enabled, &title)
    })
    .await
}

#[tauri::command]
pub fn take_analysis_launch(
    app: tauri::AppHandle,
) -> CommandResult<Option<analysis_launch::Request>> {
    into_command_result(
        "take_analysis_launch",
        Ok::<_, std::io::Error>(analysis_launch::take(&app)),
    )
}
