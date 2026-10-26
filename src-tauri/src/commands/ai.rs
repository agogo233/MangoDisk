use crate::services::ai_runtime::AiRuntime;
use mangodisk_core::ai::{
    discover_local_models, explain, AiConfiguration, AiConfigurationUpdate, AiDelta, AiError,
    AiPreferences, AiRequest, AiSettings, AiUsage, InstalledLocalModel,
};
use std::time::Instant;
use tauri::{ipc::Channel, State};

#[tauri::command]
pub(crate) async fn ai_get_preferences(
    state: State<'_, AiRuntime>,
) -> Result<AiPreferences, AiError> {
    let runtime = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || runtime.preferences())
        .await
        .map_err(|_| AiError::ConfigurationUnavailable)?
}

#[tauri::command]
pub(crate) async fn ai_set_enabled(
    enabled: bool,
    state: State<'_, AiRuntime>,
) -> Result<AiPreferences, AiError> {
    let runtime = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || runtime.save_preferences(enabled))
        .await
        .map_err(|_| AiError::ConfigurationUnavailable)?
}

#[tauri::command]
pub(crate) async fn ai_get_settings() -> Result<Option<AiSettings>, AiError> {
    let result = tauri::async_runtime::spawn_blocking(|| {
        AiConfiguration::load()
            .map(|value| Some(value.unwrap_or_else(AiConfiguration::initial).settings()))
    })
    .await
    .map_err(|_| AiError::ConfigurationUnavailable)?;
    if let Err(reason) = &result {
        log::warn!("ai_configuration_read_failed reason={reason:?}");
    }
    result
}

#[tauri::command]
pub(crate) async fn ai_get_configuration() -> Result<AiEditorState, AiError> {
    let started = Instant::now();
    // Read once for the editor rather than loading the same file through both
    // configuration and public-settings commands. The capability is build-only.
    let result = tauri::async_runtime::spawn_blocking(|| {
        AiConfiguration::load().map(|configuration| AiEditorState {
            configuration,
            free_available: AiConfiguration::initial().settings().free_available,
        })
    })
    .await
    .map_err(|_| AiError::ConfigurationUnavailable)
    .and_then(|result| result);
    log::info!(
        "ai_editor_configuration_loaded duration_ms={} success={} reason={:?}",
        started.elapsed().as_millis(),
        result.is_ok(),
        result.as_ref().err()
    );
    result
}

// This IPC snapshot is not persisted. Never derive Debug: it carries a secret.
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AiEditorState {
    configuration: Option<AiConfiguration>,
    free_available: bool,
}

#[tauri::command]
pub(crate) async fn ai_save_settings(update: AiConfigurationUpdate) -> Result<AiSettings, AiError> {
    let result = tauri::async_runtime::spawn_blocking(|| AiConfiguration::save(update))
        .await
        .map_err(|_| AiError::ConfigurationUnavailable)?;
    log::info!(
        "ai_configuration_saved success={} reason={:?}",
        result.is_ok(),
        result.as_ref().err()
    );
    result
}

#[tauri::command]
pub(crate) async fn ai_delete_settings() -> Result<(), AiError> {
    let result = tauri::async_runtime::spawn_blocking(AiConfiguration::delete)
        .await
        .map_err(|_| AiError::ConfigurationUnavailable)?;
    log::info!(
        "ai_configuration_deleted success={} reason={:?}",
        result.is_ok(),
        result.as_ref().err()
    );
    result
}

/// Reserving before streaming lets the UI cancel even while settings are
/// being read. Only backend-generated IDs are accepted or written to logs.
#[tauri::command]
pub(crate) fn ai_begin(state: State<'_, AiRuntime>) -> Result<String, AiError> {
    state.reserve_request()
}

#[tauri::command]
pub(crate) fn ai_cancel(id: String, state: State<'_, AiRuntime>) -> Result<(), AiError> {
    state.cancel(&id)
}

#[tauri::command]
pub(crate) async fn ai_explain(
    id: String,
    request: AiRequest,
    metadata: Option<mangodisk_core::ai::AiClientMetadata>,
    expected_mode: mangodisk_core::ai::AiServiceMode,
    on_delta: Channel<AiDelta>,
    state: State<'_, AiRuntime>,
) -> Result<AiUsage, AiError> {
    let cancel = state.start_reserved_request(&id)?;
    let started = Instant::now();
    log::info!(
        "ai_request_started operation_id={id} module={} context_schema={:?} connection_test={}",
        request
            .context
            .as_ref()
            .map_or("connectionTest", |context| context.subject.module_name()),
        request
            .context
            .as_ref()
            .map(|context| context.schema_version),
        request.context.is_none()
    );
    let result = async {
        let config = tauri::async_runtime::spawn_blocking(AiConfiguration::load)
            .await
            .map_err(|_| AiError::ConfigurationUnavailable)??
            .unwrap_or_else(AiConfiguration::initial);
        if *cancel.borrow() {
            return Err(AiError::Cancelled);
        }
        if config.mode != expected_mode {
            return Err(AiError::InvalidConfiguration);
        }
        if config.mode == mangodisk_core::ai::AiServiceMode::Free {
            return mangodisk_core::ai::official_explain(
                config,
                request,
                metadata.ok_or(AiError::InvalidContext)?,
                &id,
                cancel,
                |text| on_delta.send(text).is_ok(),
            )
            .await;
        }
        explain(config, request, &id, cancel, |text| {
            on_delta.send(text).is_ok()
        })
        .await
    }
    .await;
    state.complete_request(&id);
    match &result {
        Ok(usage) => log::info!("ai_request_completed operation_id={id} elapsed_ms={} prompt_tokens={:?} completion_tokens={:?}", started.elapsed().as_millis(), usage.prompt_tokens, usage.completion_tokens),
        Err(AiError::Cancelled) => log::info!("ai_request_cancelled operation_id={id} elapsed_ms={}", started.elapsed().as_millis()),
        Err(reason) => log::warn!("ai_request_failed operation_id={id} elapsed_ms={} reason={reason:?}", started.elapsed().as_millis()),
    }
    result
}

#[tauri::command]
pub(crate) async fn ai_get_quota(
    metadata: mangodisk_core::ai::AiClientMetadata,
    state: State<'_, AiRuntime>,
) -> Result<mangodisk_core::ai::AiQuota, AiError> {
    let mut disabled = state.quota_permit()?;
    tokio::select! {
        biased;
        _ = disabled.changed() => Err(AiError::Cancelled),
        result = mangodisk_core::ai::official_quota(metadata) => result,
    }
}

#[tauri::command]
pub(crate) async fn ai_set_feedback(
    request_id: String,
    rating: Option<mangodisk_core::ai::AiFeedbackRating>,
    metadata: mangodisk_core::ai::AiClientMetadata,
    state: State<'_, AiRuntime>,
) -> Result<mangodisk_core::ai::AiFeedback, AiError> {
    let mut disabled = state.quota_permit()?;
    let config = tauri::async_runtime::spawn_blocking(AiConfiguration::load)
        .await
        .map_err(|_| AiError::ConfigurationUnavailable)??
        .unwrap_or_else(AiConfiguration::initial);
    if config.mode != mangodisk_core::ai::AiServiceMode::Free {
        return Err(AiError::InvalidConfiguration);
    }
    tokio::select! {
        biased;
        _ = disabled.changed() => Err(AiError::Cancelled),
        result = mangodisk_core::ai::official_feedback(metadata, request_id, rating) => result,
    }
}

#[tauri::command]
pub(crate) async fn ai_list_local_models() -> super::error::CommandResult<Vec<InstalledLocalModel>>
{
    super::error::run_blocking("ai_list_local_models", discover_local_models).await
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn editor_snapshot_uses_the_frontend_contract_without_changing_persisted_configuration() {
        let state = AiEditorState {
            configuration: None,
            free_available: true,
        };
        assert_eq!(
            serde_json::to_value(state).unwrap(),
            serde_json::json!({"configuration": null, "freeAvailable": true})
        );
        let state = AiEditorState {
            configuration: Some(AiConfiguration::initial()),
            free_available: false,
        };
        let value = serde_json::to_value(state).unwrap();
        assert_eq!(value["configuration"]["schemaVersion"], 2);
        assert!(value["configuration"].get("freeAvailable").is_none());
    }
}
