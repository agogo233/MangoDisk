use crate::{
    filesystem::atomic_replace::replace_file, shared::application_paths, CoreError, CoreResult,
};
use mangodisk_platform::{PlatformSystemSettingSnapshot, PlatformSystemSettingValue};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeSet,
    fs::{self, OpenOptions},
    io::Write,
    path::Path,
};

pub(super) const MAX_RECOVERY_ITEMS: usize = 1024;
pub(super) const RECOVERY_SCHEMA_VERSION: u32 = 2;
const MIN_SUPPORTED_RECOVERY_SCHEMA_VERSION: u32 = 1;
const RECOVERY_MAX_BYTES: u64 = 1024 * 1024;
const RECOVERY_MAX_TEXT_BYTES: usize = 1024;
pub(super) const RECOVERY_FILE_NAME: &str = "system-settings-recovery.json";
pub(super) const INVALID_RECOVERY_FILE_NAME: &str = "system-settings-recovery.invalid.json";
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct RecoveryDocument {
    pub(super) schema_version: u32,
    pub(super) recovery_id: String,
    pub(super) created_at_ms: u64,
    pub(super) items: Vec<RecoveryItem>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct RecoveryItem {
    pub(super) setting_id: String,
    pub(super) original_value: PlatformSystemSettingValue,
    pub(super) optimized_value: PlatformSystemSettingValue,
}

fn recovery_path() -> CoreResult<std::path::PathBuf> {
    Ok(application_paths()?
        .data_directory()
        .join(RECOVERY_FILE_NAME))
}

pub(super) fn recovery_exists() -> bool {
    recovery_path().is_ok_and(|path| path.is_file())
}

pub(super) fn save_recovery(
    recovery_id: &str,
    items: Vec<RecoveryItem>,
    operation_id: u64,
) -> CoreResult<()> {
    if items.len() > MAX_RECOVERY_ITEMS {
        log::error!(
            "system_settings_recovery_save_rejected operation_id={} item_count={} max_item_count={}",
            operation_id,
            items.len(),
            MAX_RECOVERY_ITEMS
        );
        return Err(CoreError::persistence(
            "system settings recovery record exceeds its item limit",
        ));
    }
    let path = recovery_path()?;
    let parent = path
        .parent()
        .ok_or_else(|| CoreError::persistence("recovery path has no parent"))?;
    fs::create_dir_all(parent).map_err(|error| {
        CoreError::persistence(format!("failed to create recovery directory: {error}"))
    })?;
    let document = RecoveryDocument {
        schema_version: RECOVERY_SCHEMA_VERSION,
        recovery_id: recovery_id.to_string(),
        created_at_ms: crate::shared::clock::unix_time_ms(),
        items,
    };
    let content = serde_json::to_vec_pretty(&document).map_err(|error| {
        CoreError::persistence(format!("failed to serialize recovery record: {error}"))
    })?;
    write_atomic(&path, &content)?;
    log::info!(
        "system_settings_recovery_saved operation_id={} schema_version={} item_count={}",
        operation_id,
        RECOVERY_SCHEMA_VERSION,
        document.items.len()
    );
    Ok(())
}

fn write_atomic(path: &Path, content: &[u8]) -> CoreResult<()> {
    let temporary = path.with_file_name("system-settings-recovery.json.tmp");
    let mut file = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(&temporary)
        .map_err(|error| {
            CoreError::persistence(format!(
                "failed to create system settings recovery temporary file: {error}"
            ))
        })?;
    file.write_all(content)
        .and_then(|_| file.sync_all())
        .map_err(|error| {
            CoreError::persistence(format!(
                "failed to write system settings recovery temporary file: {error}"
            ))
        })?;
    replace_file(&temporary, path).map_err(|error| {
        CoreError::persistence(format!("failed to save system settings recovery: {error}"))
    })
}

pub(super) fn load_recovery() -> CoreResult<Option<RecoveryDocument>> {
    let path = recovery_path()?;
    if !path.exists() {
        return Ok(None);
    }
    let metadata = fs::metadata(&path).map_err(|error| {
        CoreError::persistence(format!("failed to inspect recovery record: {error}"))
    })?;
    if metadata.len() > RECOVERY_MAX_BYTES {
        log::warn!(
            "system_settings_recovery_quarantined reason=oversized byte_count={}",
            metadata.len()
        );
        quarantine_invalid_recovery(&path)?;
        return Ok(None);
    }
    let content = fs::read(&path).map_err(|error| {
        CoreError::persistence(format!("failed to read recovery record: {error}"))
    })?;
    let document = match serde_json::from_slice::<RecoveryDocument>(&content) {
        Ok(document) => document,
        Err(error) => {
            log::warn!(
                "system_settings_recovery_quarantined reason=invalid_format error={}",
                mangodisk_platform::diagnostics::text(&error)
            );
            quarantine_invalid_recovery(&path)?;
            return Ok(None);
        }
    };
    let unique_ids = document
        .items
        .iter()
        .map(|item| item.setting_id.as_str())
        .collect::<BTreeSet<_>>();
    let invalid_item = document.items.iter().any(|item| {
        item.setting_id.is_empty()
            || item.setting_id.len() > 128
            || !(item.setting_id.starts_with("macos.") || item.setting_id.starts_with("windows."))
            || recovery_value_is_too_large(&item.original_value)
            || recovery_value_is_too_large(&item.optimized_value)
    });
    if !(MIN_SUPPORTED_RECOVERY_SCHEMA_VERSION..=RECOVERY_SCHEMA_VERSION)
        .contains(&document.schema_version)
        || document.items.len() > MAX_RECOVERY_ITEMS
        || unique_ids.len() != document.items.len()
        || invalid_item
    {
        log::warn!(
            "system_settings_recovery_quarantined reason=unsupported_schema schema_version={} item_count={}",
            document.schema_version,
            document.items.len()
        );
        quarantine_invalid_recovery(&path)?;
        return Ok(None);
    }
    Ok(Some(document))
}

fn recovery_value_is_too_large(value: &PlatformSystemSettingValue) -> bool {
    match value {
        PlatformSystemSettingValue::Text(value)
        | PlatformSystemSettingValue::Snapshot(PlatformSystemSettingSnapshot::Text(value)) => {
            value.len() > RECOVERY_MAX_TEXT_BYTES
        }
        PlatformSystemSettingValue::Snapshot(PlatformSystemSettingSnapshot::IntegerMap(values)) => {
            values.len() > MAX_RECOVERY_ITEMS
                || values.iter().any(|(key, value)| {
                    key.is_empty()
                        || key.len() > 256
                        || value.is_some_and(|value| !(0..=i64::from(u32::MAX)).contains(&value))
                })
        }
        _ => false,
    }
}

pub(super) fn quarantine_invalid_recovery(path: &Path) -> CoreResult<()> {
    let quarantine = path.with_file_name(INVALID_RECOVERY_FILE_NAME);
    replace_file(path, &quarantine).map_err(|error| {
        CoreError::persistence(format!(
            "failed to quarantine unsupported system settings recovery: {error}"
        ))
    })
}

pub(super) fn remove_recovery() -> CoreResult<()> {
    let path = recovery_path()?;
    if path.exists() {
        fs::remove_file(path).map_err(|error| {
            CoreError::persistence(format!("failed to remove recovery record: {error}"))
        })?;
    }
    Ok(())
}
