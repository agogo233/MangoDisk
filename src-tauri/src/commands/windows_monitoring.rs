use crate::{
    commands::error::{run_blocking, CommandError, CommandResult},
    services::windows_monitoring::{self, Observation, SetupResult},
};

fn authorize_label(
    window_label: &str,
    operation: &'static str,
    allow_panel: bool,
) -> CommandResult<()> {
    if window_label != crate::MAIN_WINDOW_LABEL
        && !(allow_panel && window_label == crate::resident::PANEL_LABEL)
    {
        return Err(CommandError::invalid_input(operation));
    }
    Ok(())
}

fn authorize(
    window: &tauri::WebviewWindow,
    operation: &'static str,
    allow_panel: bool,
) -> CommandResult<()> {
    authorize_label(window.label(), operation, allow_panel)
}

#[tauri::command]
pub async fn windows_monitoring_observe(
    window: tauri::WebviewWindow,
) -> CommandResult<Observation> {
    authorize(&window, "windows_monitoring_observe", true)?;
    run_blocking("windows_monitoring_observe", || {
        Ok::<_, std::io::Error>(windows_monitoring::observe())
    })
    .await
}
#[tauri::command]
pub async fn windows_monitoring_install(
    window: tauri::WebviewWindow,
    app: tauri::AppHandle,
) -> CommandResult<SetupResult> {
    authorize(&window, "windows_monitoring_install", false)?;
    Ok(windows_monitoring::install(app).await)
}
#[tauri::command]
pub async fn windows_monitoring_uninstall(
    remove_driver: bool,
    window: tauri::WebviewWindow,
    app: tauri::AppHandle,
) -> CommandResult<SetupResult> {
    authorize(&window, "windows_monitoring_uninstall", false)?;
    Ok(windows_monitoring::uninstall(app, remove_driver).await)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tray_panel_can_read_monitoring_capability_but_cannot_manage_service() {
        assert!(authorize_label(
            crate::resident::PANEL_LABEL,
            "windows_monitoring_observe",
            true
        )
        .is_ok());
        assert!(authorize_label(
            crate::resident::PANEL_LABEL,
            "windows_monitoring_install",
            false
        )
        .is_err());
        assert!(authorize_label(
            crate::resident::PANEL_LABEL,
            "windows_monitoring_uninstall",
            false
        )
        .is_err());
    }
}
