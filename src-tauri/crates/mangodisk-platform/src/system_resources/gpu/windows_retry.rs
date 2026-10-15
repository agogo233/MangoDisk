//! Bounded recovery for transient PDH data loss without discarding a valid query.
use super::windows::Failure;
use std::time::{Duration, Instant};
use windows_sys::Win32::System::Performance::*;

#[derive(Default)]
pub(super) struct RetryState {
    failure: Option<Failure>,
    due: Option<Instant>,
    started: Option<Instant>,
    attempts: u32,
    failed_samples: u32,
}

pub(super) struct RetryDecision {
    pub delay: Duration,
    pub retain_query: bool,
    pub log: bool,
    pub attempts: u32,
    pub quick_retry: bool,
}

pub(super) struct Recovery {
    pub failure: Failure,
    pub attempts: u32,
    pub elapsed: Duration,
    pub failed_samples: u32,
}

pub(super) struct ReadAttempt<T> {
    pub result: Result<T, Failure>,
    pub retain_query: bool,
    pub quick_retries: u32,
}

impl RetryState {
    pub fn read<T>(
        &mut self,
        mut read: impl FnMut() -> Result<T, Failure>,
        mut wait: impl FnMut(Duration),
    ) -> ReadAttempt<T> {
        let mut quick_retries = 0;
        loop {
            let attempted_at = Instant::now();
            let error = match read() {
                Ok(value) => {
                    return ReadAttempt {
                        result: Ok(value),
                        retain_query: true,
                        quick_retries,
                    };
                }
                Err(error) => error,
            };
            let decision = self.failed(error, Instant::now());
            if decision.log {
                log::warn!("gpu_source_read_failed source=pdh_gpu_engine stage={} native_code={:#x} native_name={} failed_attempts={} retry_after_ms={} action={} quick_retry={}", error.stage, error.code, native_name(error.code), decision.attempts, decision.delay.as_millis(), if decision.retain_query { "retain_query" } else { "reopen_query" }, decision.quick_retry);
            }
            if !decision.quick_retry {
                self.sample_failed();
                return ReadAttempt {
                    result: Err(error),
                    retain_query: decision.retain_query,
                    quick_retries,
                };
            }
            // Wait only on the dedicated GPU worker. Each retry collects a new
            // native interval; successful observations never sleep or reuse data.
            wait(decision.delay);
            quick_retries += 1;
            log::info!("gpu_source_retry source=pdh_gpu_engine retry={} scheduled_after_ms={} actual_interval_ms={}", quick_retries, decision.delay.as_millis(), attempted_at.elapsed().as_millis());
        }
    }
    pub fn pending(&self, now: Instant) -> Option<Failure> {
        self.failure
            .filter(|_| self.due.is_some_and(|due| now < due))
    }

    pub fn failed(&mut self, failure: Failure, now: Instant) -> RetryDecision {
        self.attempts = self.attempts.saturating_add(1);
        self.started.get_or_insert(now);
        let transient = matches!(
            failure.stage,
            "collect" | "size" | "format" | "empty" | "resize"
        ) && matches!(
            failure.code,
            PDH_CSTATUS_INVALID_DATA | PDH_INVALID_DATA | PDH_NO_DATA | PDH_MORE_DATA
        );
        let retain_query = transient && self.attempts <= 3;
        let delay = Duration::from_millis(if retain_query {
            250 << (self.attempts - 1)
        } else {
            30_000
        });
        let log =
            self.failure != Some(failure) || self.attempts <= 4 || self.attempts.is_multiple_of(10);
        self.failure = Some(failure);
        self.due = Some(now + delay);
        RetryDecision {
            delay,
            retain_query,
            log,
            attempts: self.attempts,
            quick_retry: retain_query && self.attempts <= 2,
        }
    }

    pub fn sample_failed(&mut self) {
        self.failed_samples = self.failed_samples.saturating_add(1);
    }

    // A baseline only primes a new rate counter; recovery requires a valid value.
    pub fn baseline(&mut self) {
        self.due = None;
    }

    pub fn recovered(&mut self, now: Instant) -> Option<Recovery> {
        let recovery = self.failure.map(|failure| Recovery {
            failure,
            attempts: self.attempts,
            elapsed: now.saturating_duration_since(self.started.unwrap_or(now)),
            failed_samples: self.failed_samples,
        });
        *self = Self::default();
        recovery
    }
}

pub(super) fn native_name(code: u32) -> &'static str {
    match code {
        PDH_CSTATUS_INVALID_DATA => "PDH_CSTATUS_INVALID_DATA",
        PDH_INVALID_DATA => "PDH_INVALID_DATA",
        PDH_NO_DATA => "PDH_NO_DATA",
        PDH_MORE_DATA => "PDH_MORE_DATA",
        PDH_CSTATUS_NO_OBJECT => "PDH_CSTATUS_NO_OBJECT",
        PDH_CSTATUS_NO_COUNTER => "PDH_CSTATUS_NO_COUNTER",
        _ => "PDH_ERROR",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invalid_data_can_recover_before_the_next_scheduled_sample() {
        let now = Instant::now();
        let failure = Failure {
            stage: "format",
            code: PDH_CSTATUS_INVALID_DATA,
        };
        let mut state = RetryState::default();
        let decision = state.failed(failure, now);
        assert!(decision.retain_query);
        assert!(decision.quick_retry);
        assert_eq!(decision.delay, Duration::from_millis(250));
        assert_eq!(state.pending(now), Some(failure));
        assert_eq!(state.pending(now + decision.delay), None);
        let recovery = state.recovered(now + decision.delay).unwrap();
        assert_eq!(recovery.attempts, 1);
        assert_eq!(recovery.elapsed, Duration::from_millis(250));
        assert_eq!(recovery.failed_samples, 0);
        assert!(state.recovered(now).is_none());
        assert_eq!(state.failed(failure, now).delay, Duration::from_millis(250));
    }

    #[test]
    fn repeated_data_loss_is_bounded_and_baselines_do_not_claim_recovery() {
        let now = Instant::now();
        let failure = Failure {
            stage: "empty",
            code: PDH_NO_DATA,
        };
        let mut state = RetryState::default();
        for (attempt, milliseconds) in [250, 500, 1000, 30_000, 30_000].into_iter().enumerate() {
            let decision = state.failed(failure, now);
            assert_eq!(decision.delay, Duration::from_millis(milliseconds));
            assert_eq!(decision.retain_query, milliseconds < 30_000);
            assert_eq!(decision.quick_retry, attempt < 2);
            if !decision.quick_retry {
                state.sample_failed();
            }
        }
        state.baseline();
        assert!(state.pending(now).is_none());
        let recovery = state.recovered(now).unwrap();
        assert_eq!(recovery.attempts, 5);
        assert_eq!(recovery.failed_samples, 3);
    }

    #[test]
    fn missing_counters_and_malformed_native_buffers_require_reopening() {
        for failure in [
            Failure {
                stage: "add",
                code: PDH_CSTATUS_NO_COUNTER,
            },
            Failure {
                stage: "bounds",
                code: PDH_INVALID_DATA,
            },
        ] {
            let decision = RetryState::default().failed(failure, Instant::now());
            assert!(!decision.retain_query);
            assert!(!decision.quick_retry);
            assert_eq!(decision.delay, Duration::from_secs(30));
        }
    }
    #[test]
    fn quick_recovery_returns_only_a_fresh_successful_reading() {
        for failures in 0..=2 {
            let failure = Failure {
                stage: "format",
                code: PDH_CSTATUS_INVALID_DATA,
            };
            let mut calls = 0;
            let mut waits = Vec::new();
            let mut state = RetryState::default();
            let attempt = state.read(
                || {
                    calls += 1;
                    if calls <= failures {
                        Err(failure)
                    } else {
                        Ok(73.0)
                    }
                },
                |delay| waits.push(delay),
            );
            assert_eq!(attempt.result.unwrap(), 73.0);
            assert_eq!(calls, failures + 1);
            assert_eq!(attempt.quick_retries, failures);
            assert_eq!(
                waits,
                [250, 500]
                    .into_iter()
                    .take(failures as usize)
                    .map(Duration::from_millis)
                    .collect::<Vec<_>>()
            );
            if failures > 0 {
                assert_eq!(state.recovered(Instant::now()).unwrap().failed_samples, 0);
            }
        }
    }

    #[test]
    fn persistent_failure_returns_an_error_after_two_quick_retries() {
        let failure = Failure {
            stage: "format",
            code: PDH_CSTATUS_INVALID_DATA,
        };
        let mut state = RetryState::default();
        let mut calls = 0;
        let mut waited = Duration::ZERO;
        let attempt = state.read::<f64>(
            || {
                calls += 1;
                Err(failure)
            },
            |delay| waited += delay,
        );
        assert_eq!(attempt.result.unwrap_err(), failure);
        assert_eq!(calls, 3);
        assert_eq!(waited, Duration::from_millis(750));
        assert!(attempt.retain_query);
        assert_eq!(state.recovered(Instant::now()).unwrap().failed_samples, 1);
    }

    #[test]
    fn permanent_failure_does_not_retry_or_retain_the_query() {
        let failure = Failure {
            stage: "add",
            code: PDH_CSTATUS_NO_COUNTER,
        };
        let mut state = RetryState::default();
        let attempt = state.read::<f64>(
            || Err(failure),
            |_| panic!("permanent failure must not wait"),
        );
        assert_eq!(attempt.result.unwrap_err(), failure);
        assert_eq!(attempt.quick_retries, 0);
        assert!(!attempt.retain_query);
    }
}
