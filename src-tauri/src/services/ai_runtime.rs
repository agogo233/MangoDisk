use mangodisk_core::ai::{AiError, AiPreferences};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::Instant,
};
use tokio::sync::watch;

struct ActiveRequest {
    cancel: watch::Sender<bool>,
    running: bool,
    created: Instant,
}

#[derive(Default, Clone)]
pub(crate) struct AiRuntime(Arc<Mutex<AiRuntimeState>>);

#[derive(Default)]
struct AiRuntimeState {
    preferences: Option<AiPreferences>,
    active: HashMap<String, ActiveRequest>,
    // Each disable invalidates existing quota reads even after a quick re-enable.
    disabled_revision: watch::Sender<u64>,
}

impl AiRuntimeState {
    fn require_enabled(&self) -> Result<(), AiError> {
        if self.preferences.is_some_and(|value| value.enabled) {
            Ok(())
        } else {
            Err(AiError::Disabled)
        }
    }

    fn apply_preferences(&mut self, preferences: AiPreferences) {
        self.preferences = Some(preferences);
        if !preferences.enabled {
            self.disabled_revision
                .send_modify(|revision| *revision += 1);
            for request in self.active.values() {
                request.cancel.send_replace(true);
            }
            self.active.retain(|_, request| request.running);
        }
    }
}

// Eight module streams and one connection test can coexist. Keep reservations
// bounded so abandoned IPC calls cannot consume unbounded client resources.
const MAX_ACTIVE_REQUESTS: usize = 9;

impl AiRuntime {
    pub(crate) fn preferences(&self) -> Result<AiPreferences, AiError> {
        let mut state = self.0.lock().map_err(|_| AiError::Busy)?;
        if let Some(preferences) = state.preferences {
            return Ok(preferences);
        }
        let preferences = AiPreferences::load()?;
        state.apply_preferences(preferences);
        Ok(preferences)
    }

    pub(crate) fn save_preferences(&self, enabled: bool) -> Result<AiPreferences, AiError> {
        let mut state = self.0.lock().map_err(|_| AiError::Busy)?;
        // Serialize persistence with request admission. A failed atomic write
        // leaves the last confirmed setting intact; success cancels all modules
        // and connection tests before acknowledging the toggle to the UI.
        let preferences = AiPreferences::save(enabled)?;
        let active_count = state.active.len();
        state.apply_preferences(preferences);
        log::info!("ai_feature_preference_saved enabled={enabled} active_count={active_count}");
        Ok(preferences)
    }

    pub(crate) fn quota_permit(&self) -> Result<watch::Receiver<u64>, AiError> {
        let state = self.0.lock().map_err(|_| AiError::Busy)?;
        state.require_enabled()?;
        Ok(state.disabled_revision.subscribe())
    }

    pub(crate) fn reserve_request(&self) -> Result<String, AiError> {
        let mut state = self.0.lock().map_err(|_| AiError::Busy)?;
        state.require_enabled()?;
        let active = &mut state.active;
        let before = active.len();
        active.retain(|_, value| value.running || value.created.elapsed().as_secs() < 60);
        if before != active.len() {
            log::warn!("ai_reservations_expired count={}", before - active.len());
        }
        if active.len() >= MAX_ACTIVE_REQUESTS {
            log::warn!("ai_request_capacity_reached active_count={}", active.len());
            return Err(AiError::Busy);
        }
        let id = uuid::Uuid::new_v4().to_string();
        let (cancel, _) = watch::channel(false);
        active.insert(
            id.clone(),
            ActiveRequest {
                cancel,
                running: false,
                created: Instant::now(),
            },
        );
        log::debug!(
            "ai_request_reserved operation_id={id} active_count={}",
            active.len()
        );
        Ok(id)
    }

    pub(crate) fn start_reserved_request(
        &self,
        id: &str,
    ) -> Result<watch::Receiver<bool>, AiError> {
        let mut state = self.0.lock().map_err(|_| AiError::Busy)?;
        state.require_enabled()?;
        let active = &mut state.active;
        let value = active
            .get_mut(id)
            .filter(|value| !value.running)
            .ok_or(AiError::Cancelled)?;
        value.running = true;
        Ok(value.cancel.subscribe())
    }

    pub(crate) fn cancel(&self, id: &str) -> Result<(), AiError> {
        let mut state = self.0.lock().map_err(|_| AiError::Busy)?;
        let active = &mut state.active;
        if let Some(value) = active.get_mut(id) {
            value.cancel.send_replace(true);
            log::debug!(
                "ai_request_cancelled operation_id={id} running={}",
                value.running
            );
            // Running requests own their slot until transport cleanup completes.
            if !value.running {
                active.remove(id);
            }
        }
        Ok(())
    }

    pub(crate) fn complete_request(&self, id: &str) {
        if let Ok(mut active) = self.0.lock() {
            active.active.remove(id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn enabled_runtime() -> AiRuntime {
        let runtime = AiRuntime::default();
        runtime
            .0
            .lock()
            .unwrap()
            .apply_preferences(AiPreferences::default());
        runtime
    }

    #[test]
    fn unloaded_and_disabled_preferences_block_all_network_admission() {
        let runtime = AiRuntime::default();
        assert!(matches!(runtime.reserve_request(), Err(AiError::Disabled)));
        assert!(matches!(runtime.quota_permit(), Err(AiError::Disabled)));
        runtime.0.lock().unwrap().apply_preferences(AiPreferences {
            enabled: false,
            ..AiPreferences::default()
        });
        assert!(matches!(runtime.reserve_request(), Err(AiError::Disabled)));
        assert!(matches!(
            runtime.start_reserved_request("unknown"),
            Err(AiError::Disabled)
        ));
        assert!(matches!(runtime.quota_permit(), Err(AiError::Disabled)));
    }

    #[test]
    fn disabling_cancels_every_stream_reservation_and_quota_without_reviving_them() {
        let runtime = enabled_runtime();
        let reserved = runtime.reserve_request().unwrap();
        let running = runtime.reserve_request().unwrap();
        let cancellation = runtime.start_reserved_request(&running).unwrap();
        let quota = runtime.quota_permit().unwrap();
        runtime.0.lock().unwrap().apply_preferences(AiPreferences {
            enabled: false,
            ..AiPreferences::default()
        });
        assert!(*cancellation.borrow());
        assert!(quota.has_changed().unwrap());
        assert!(matches!(runtime.reserve_request(), Err(AiError::Disabled)));
        runtime
            .0
            .lock()
            .unwrap()
            .apply_preferences(AiPreferences::default());
        assert!(*cancellation.borrow());
        assert!(quota.has_changed().unwrap());
        assert!(matches!(
            runtime.start_reserved_request(&reserved),
            Err(AiError::Cancelled)
        ));
        runtime.complete_request(&running);
        let fresh = runtime.reserve_request().unwrap();
        assert!(!*runtime.start_reserved_request(&fresh).unwrap().borrow());
        assert!(!runtime.quota_permit().unwrap().has_changed().unwrap());
    }

    #[test]
    fn parallel_requests_cancel_and_finish_independently() {
        let runtime = enabled_runtime();
        let first = runtime.reserve_request().unwrap();
        let second = runtime.reserve_request().unwrap();
        let first_cancel = runtime.start_reserved_request(&first).unwrap();
        let second_cancel = runtime.start_reserved_request(&second).unwrap();
        runtime.cancel(&first).unwrap();
        assert!(*first_cancel.borrow());
        assert!(!*second_cancel.borrow());
        runtime.complete_request(&first);
        assert!(runtime.0.lock().unwrap().active.contains_key(&second));
        runtime.cancel(&first).unwrap();
        assert!(!*second_cancel.borrow());
        runtime.complete_request(&second);
        assert!(runtime.0.lock().unwrap().active.is_empty());
    }

    #[test]
    fn eight_modules_and_connection_test_fit_with_bounded_capacity() {
        let runtime = enabled_runtime();
        let ids: Vec<_> = (0..MAX_ACTIVE_REQUESTS)
            .map(|_| runtime.reserve_request().unwrap())
            .collect();
        for id in &ids {
            runtime.start_reserved_request(id).unwrap();
        }
        assert!(matches!(runtime.reserve_request(), Err(AiError::Busy)));
        runtime.cancel(&ids[0]).unwrap();
        assert!(matches!(runtime.reserve_request(), Err(AiError::Busy)));
        runtime.complete_request(&ids[0]);
        assert!(runtime.reserve_request().is_ok());
    }

    #[test]
    fn reservations_expire_without_evicting_running_streams() {
        let runtime = enabled_runtime();
        let reserved = runtime.reserve_request().unwrap();
        let running = runtime.reserve_request().unwrap();
        runtime.start_reserved_request(&running).unwrap();
        for request in runtime.0.lock().unwrap().active.values_mut() {
            request.created = Instant::now() - Duration::from_secs(61);
        }
        runtime.reserve_request().unwrap();
        let state = runtime.0.lock().unwrap();
        let active = &state.active;
        assert!(!active.contains_key(&reserved));
        assert!(active.contains_key(&running));
    }

    #[test]
    fn cancelled_unknown_and_replayed_reservations_cannot_start() {
        let runtime = enabled_runtime();
        let cancelled = runtime.reserve_request().unwrap();
        runtime.cancel(&cancelled).unwrap();
        assert!(matches!(
            runtime.start_reserved_request(&cancelled),
            Err(AiError::Cancelled)
        ));
        assert!(matches!(
            runtime.start_reserved_request("unknown"),
            Err(AiError::Cancelled)
        ));
        let running = runtime.reserve_request().unwrap();
        runtime.start_reserved_request(&running).unwrap();
        assert!(matches!(
            runtime.start_reserved_request(&running),
            Err(AiError::Cancelled)
        ));
        runtime.complete_request(&running);
        assert!(matches!(
            runtime.start_reserved_request(&running),
            Err(AiError::Cancelled)
        ));
    }
}
