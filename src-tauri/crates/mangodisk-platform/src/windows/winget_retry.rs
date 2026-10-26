//! Short-lived timeout evidence for optional WinGet enrichment across app restarts.
use std::{
    fs,
    os::windows::fs::MetadataExt,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};
use winreg::{
    enums::{HKEY_CURRENT_USER, KEY_READ, KEY_WRITE},
    RegKey,
};

use crate::{ControlledCommandError, ControlledExecutable, PlatformCancellation};

const RETRY_KEY: &str = r"Software\MangoDisk\Inventory";
const RETRY_VALUE: &str = "WingetTimeoutV1";
const RETRY_DELAY: Duration = Duration::from_secs(15 * 60);

#[derive(Deserialize, Serialize)]
struct TimeoutRecord {
    schema_version: u32,
    executable_fingerprint: String,
    timed_out_at_ms: u64,
}

impl TimeoutRecord {
    fn remaining_ms(&self, fingerprint: &str, now_ms: u64) -> Option<u64> {
        if self.schema_version != 1 || self.executable_fingerprint != fingerprint {
            return None;
        }
        // Clock rollback or an incompatible record must never suppress discovery indefinitely.
        let elapsed = now_ms.checked_sub(self.timed_out_at_ms)?;
        (RETRY_DELAY.as_millis() as u64)
            .checked_sub(elapsed)
            .filter(|remaining| *remaining > 0)
    }
}

pub(super) struct QueryOutcome<T> {
    /// None means deferred, never that the optional package facts are complete.
    pub result: Option<Result<T, ControlledCommandError>>,
    pub retry_at: Option<Instant>,
}

pub(super) fn run<T>(
    executable: &ControlledExecutable,
    cancellation: &PlatformCancellation,
    command: impl FnOnce() -> Result<T, ControlledCommandError>,
) -> QueryOutcome<T> {
    let key = RegKey::predef(HKEY_CURRENT_USER)
        .create_subkey_with_flags(RETRY_KEY, KEY_READ | KEY_WRITE)
        .map(|(key, _)| key);
    if let Err(error) = &key {
        log::warn!("windows_winget_retry_state_unavailable stage=open error={} outcome=query_without_backoff", crate::diagnostics::text(error));
    }
    let now_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;
    run_at(executable, cancellation, key.as_ref().ok(), now_ms, command)
}

fn run_at<T>(
    executable: &ControlledExecutable,
    cancellation: &PlatformCancellation,
    key: Option<&RegKey>,
    now_ms: u64,
    command: impl FnOnce() -> Result<T, ControlledCommandError>,
) -> QueryOutcome<T> {
    let observed_at = Instant::now();
    if cancellation.is_cancelled() {
        return QueryOutcome {
            result: Some(Err(ControlledCommandError::Cancelled)),
            retry_at: None,
        };
    }
    let path = match executable.validated_path() {
        Ok(path) => path,
        Err(error) => {
            return QueryOutcome {
                result: Some(Err(error)),
                retry_at: None,
            };
        }
    };
    let fingerprint = fs::metadata(path).ok().map(|metadata| {
        let mut digest = blake3::Hasher::new();
        digest.update(path.to_string_lossy().as_bytes());
        digest.update(&metadata.file_size().to_le_bytes());
        digest.update(&metadata.last_write_time().to_le_bytes());
        digest.update(&metadata.creation_time().to_le_bytes());
        digest.finalize().to_hex().to_string()
    });
    let record = key
        .and_then(|key| key.get_value::<String, _>(RETRY_VALUE).ok())
        .filter(|value| value.len() <= 2048)
        .and_then(|value| serde_json::from_str::<TimeoutRecord>(&value).ok());
    if let Some(remaining) = record
        .as_ref()
        .and_then(|record| record.remaining_ms(fingerprint.as_deref()?, now_ms))
    {
        log::info!("windows_winget_inventory_deferred reason=recent_timeout retry_after_ms={remaining} fallback=registry_appx complete=false");
        return QueryOutcome {
            result: None,
            retry_at: Some(observed_at + Duration::from_millis(remaining)),
        };
    }
    let started = Instant::now();
    let result = command();
    if let (Some(key), Some(fingerprint)) = (key, fingerprint) {
        if matches!(result, Err(ControlledCommandError::TimedOut)) {
            let record = TimeoutRecord {
                schema_version: 1,
                executable_fingerprint: fingerprint,
                timed_out_at_ms: now_ms.saturating_add(started.elapsed().as_millis() as u64),
            };
            let saved = serde_json::to_string(&record)
                .expect("timeout evidence contains only JSON primitives");
            if let Err(error) = key.set_value(RETRY_VALUE, &saved) {
                log::warn!("windows_winget_retry_state_unavailable stage=save error={} outcome=query_without_backoff", crate::diagnostics::text(&error));
            }
        } else if result.is_ok() {
            // Valid exports and ordinary process exits both end timeout backoff.
            if let Err(error) = key.delete_value(RETRY_VALUE) {
                if error.kind() != std::io::ErrorKind::NotFound {
                    log::warn!("windows_winget_retry_state_unavailable stage=clear error={} outcome=query_completed", crate::diagnostics::text(&error));
                }
            }
        }
    }
    let retry_at = matches!(result, Err(ControlledCommandError::TimedOut))
        .then(|| Instant::now() + RETRY_DELAY);
    QueryOutcome {
        result: Some(result),
        retry_at,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    #[test]
    fn timeout_evidence_expires_and_rejects_other_executables_schemas_and_clock_rollback() {
        let mut record = TimeoutRecord {
            schema_version: 1,
            executable_fingerprint: "first".into(),
            timed_out_at_ms: 1000,
        };
        assert_eq!(record.remaining_ms("first", 1000), Some(900_000));
        assert_eq!(record.remaining_ms("first", 900_999), Some(1));
        assert_eq!(record.remaining_ms("first", 901_000), None);
        assert_eq!(record.remaining_ms("second", 1000), None);
        assert_eq!(record.remaining_ms("first", 999), None);
        record.schema_version = 2;
        assert_eq!(record.remaining_ms("first", 1000), None);
    }

    #[test]
    fn timeout_backoff_survives_reopening_registry_and_retries_after_expiry() {
        let root = RegKey::predef(HKEY_CURRENT_USER);
        let path = format!(
            r"Software\MangoDiskTests\WingetRetry\{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );
        let (key, _) = root.create_subkey(&path).unwrap();
        let executable = ControlledExecutable::capture(&std::env::current_exe().unwrap()).unwrap();
        let cancellation = PlatformCancellation::new(|| false);
        let calls = Cell::new(0);
        let timeout = || {
            calls.set(calls.get() + 1);
            Err::<(), _>(ControlledCommandError::TimedOut)
        };
        let timed_out = run_at(&executable, &cancellation, Some(&key), 1000, timeout);
        assert!(matches!(
            timed_out.result,
            Some(Err(ControlledCommandError::TimedOut))
        ));
        assert!(timed_out.retry_at.is_some());
        drop(key);
        let key = root
            .open_subkey_with_flags(&path, KEY_READ | KEY_WRITE)
            .unwrap();
        assert!(
            run_at(&executable, &cancellation, Some(&key), 2000, timeout)
                .result
                .is_none()
        );
        assert_eq!(
            calls.get(),
            1,
            "a repeated scan must not launch the timed-out tool"
        );
        let mut record: TimeoutRecord =
            serde_json::from_str(&key.get_value::<String, _>(RETRY_VALUE).unwrap()).unwrap();
        record.timed_out_at_ms = 1000;
        key.set_value(RETRY_VALUE, &serde_json::to_string(&record).unwrap())
            .unwrap();
        let before = Instant::now();
        let deferred = run_at(&executable, &cancellation, Some(&key), 900_000, timeout);
        let after = Instant::now();
        assert!(deferred.result.is_none());
        let deadline = deferred.retry_at.unwrap();
        assert!(deadline >= before + Duration::from_secs(1));
        assert!(deadline <= after + Duration::from_secs(1));
        assert_eq!(
            calls.get(),
            1,
            "resuming must preserve the original deadline"
        );
        let completed = run_at(&executable, &cancellation, Some(&key), 902_000, || Ok(()));
        assert_eq!(completed.result, Some(Ok(())));
        assert!(completed.retry_at.is_none());
        assert!(key.get_value::<String, _>(RETRY_VALUE).is_err());
        for error in [
            ControlledCommandError::Cancelled,
            ControlledCommandError::SpawnFailed,
        ] {
            assert_eq!(
                run_at(&executable, &cancellation, Some(&key), 903_000, || {
                    Err::<(), _>(error)
                })
                .result,
                Some(Err(error))
            );
            assert!(key.get_value::<String, _>(RETRY_VALUE).is_err());
        }
        key.set_value(RETRY_VALUE, &"invalid evidence").unwrap();
        assert_eq!(
            run_at(&executable, &cancellation, Some(&key), 904_000, || Ok(())).result,
            Some(Ok(()))
        );
        assert_eq!(
            run_at(
                &executable,
                &PlatformCancellation::new(|| true),
                Some(&key),
                905_000,
                || panic!("cancelled queries must not launch")
            )
            .result,
            Some(Err::<(), _>(ControlledCommandError::Cancelled))
        );
        drop(key);
        root.delete_subkey_all(path).unwrap();
    }

    #[test]
    #[ignore = "controlled child fixture for the explicit timeout benchmark"]
    fn slow_inventory_child() {
        std::thread::sleep(Duration::from_secs(30));
    }

    #[test]
    #[ignore = "measures real five-second process timeouts in an isolated HKCU fixture"]
    fn controlled_timeout_backoff_benchmark() {
        use crate::{run_controlled_command, ControlledCommandLimits, ControlledEnvironmentPolicy};
        let root = RegKey::predef(HKEY_CURRENT_USER);
        let path = format!(
            r"Software\MangoDiskTests\WingetRetryBenchmark\{}",
            std::process::id()
        );
        let (key, _) = root.create_subkey(&path).unwrap();
        let executable = ControlledExecutable::capture(&std::env::current_exe().unwrap()).unwrap();
        let cancellation = PlatformCancellation::new(|| false);
        let query = || {
            run_controlled_command(
                "windows-winget-timeout-fixture",
                &executable,
                &[
                    "--exact",
                    "windows::winget_retry::tests::slow_inventory_child",
                    "--ignored",
                ],
                ControlledEnvironmentPolicy::Inherit,
                ControlledCommandLimits {
                    timeout: Duration::from_secs(5),
                    stdout_bytes: 4096,
                    stderr_bytes: 4096,
                },
                &|| false,
            )
        };
        let started = Instant::now();
        assert!(matches!(query(), Err(ControlledCommandError::TimedOut)));
        let before_ms = started.elapsed().as_millis();
        assert!(matches!(
            run_at(&executable, &cancellation, Some(&key), 1000, query).result,
            Some(Err(ControlledCommandError::TimedOut))
        ));
        drop(key);
        let key = root
            .open_subkey_with_flags(&path, KEY_READ | KEY_WRITE)
            .unwrap();
        let started = Instant::now();
        assert!(
            run_at(&executable, &cancellation, Some(&key), 60_000, query)
                .result
                .is_none()
        );
        let after_ms = started.elapsed().as_millis();
        println!("winget_timeout_backoff_benchmark budget_ms=5000 before_repeat_ms={before_ms} after_repeat_ms={after_ms} storage=reopened_registry query_deferred=true");
        drop(key);
        root.delete_subkey_all(path).unwrap();
    }
}
