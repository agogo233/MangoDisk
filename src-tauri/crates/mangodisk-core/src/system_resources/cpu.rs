use mangodisk_platform::system_resources::cpu::CpuCounters;

use super::metrics::CpuUsage;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CpuBaselineReason {
    FirstSample,
    InvalidInterval,
    CounterReset,
    NoProgress,
}

#[derive(Default)]
pub struct CpuDelta {
    previous: Option<(CpuCounters, u64)>,
}

impl CpuDelta {
    /// elapsed_ms is monotonic time supplied by the worker. Sleep/resume and
    /// counter resets require a new baseline rather than a misleading spike.
    pub fn sample(
        &mut self,
        counters: CpuCounters,
        elapsed_ms: u64,
    ) -> Result<CpuUsage, CpuBaselineReason> {
        use CpuBaselineReason::*;
        let previous = self
            .previous
            .replace((counters, elapsed_ms))
            .ok_or(FirstSample)?;
        let elapsed = elapsed_ms.checked_sub(previous.1).ok_or(InvalidInterval)?;
        if !(100..=5_000).contains(&elapsed) {
            return Err(InvalidInterval);
        }
        let busy = counters
            .busy
            .checked_sub(previous.0.busy)
            .ok_or(CounterReset)?;
        let idle = counters
            .idle
            .checked_sub(previous.0.idle)
            .ok_or(CounterReset)?;
        let total = u128::from(busy) + u128::from(idle);
        if total == 0 {
            return Err(NoProgress);
        }
        Ok(CpuUsage {
            used_percent: busy as f64 / total as f64 * 100.0,
        })
    }

    pub fn reset(&mut self) {
        self.previous = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn whole_machine_cpu_is_normalized_with_no_first_sample_estimate() {
        let mut delta = CpuDelta::default();
        assert!(delta
            .sample(
                CpuCounters {
                    busy: 100,
                    idle: 300
                },
                0
            )
            .is_err());
        let value = delta
            .sample(
                CpuCounters {
                    busy: 200,
                    idle: 600,
                },
                2_000,
            )
            .unwrap();
        assert_eq!(value.used_percent, 25.0);
        assert_eq!(
            delta
                .sample(
                    CpuCounters {
                        busy: 300,
                        idle: 600
                    },
                    4_000
                )
                .unwrap()
                .used_percent,
            100.0
        );
    }

    #[test]
    fn zero_ticks_reset_and_sleep_require_another_valid_interval() {
        let mut delta = CpuDelta::default();
        let counters = CpuCounters {
            busy: 100,
            idle: 300,
        };
        assert!(delta.sample(counters, 0).is_err());
        assert!(delta.sample(counters, 2_000).is_err());
        assert!(delta
            .sample(CpuCounters { busy: 1, idle: 1 }, 4_000)
            .is_err());
        assert!(delta.sample(counters, 60_000).is_err());
        assert!(delta
            .sample(
                CpuCounters {
                    busy: 200,
                    idle: 600
                },
                62_000
            )
            .is_ok());
        delta.reset();
        assert!(delta.sample(counters, 64_000).is_err());
    }
}
