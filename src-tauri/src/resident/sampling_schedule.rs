//! Deterministic demand and in-flight accounting shared by every metric worker.

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Demand {
    pub active: bool,
    pub detailed: bool,
    pub selection: Option<String>,
}

#[derive(Debug, Default)]
pub struct SamplingSlot {
    pub demand: Demand,
    generation: u64,
    in_flight: Option<u64>,
    due_ms: u64,
}

impl SamplingSlot {
    pub fn update(&mut self, demand: Demand) -> bool {
        if self.demand == demand {
            return false;
        }
        self.demand = demand;
        self.generation = self.generation.wrapping_add(1);
        self.due_ms = 0;
        // Keep the old job in flight until it actually exits. A timeout or
        // changed device must not spawn more uncancellable native queries.
        true
    }

    pub fn begin(&mut self, now_ms: u64, interval_ms: u64) -> Option<u64> {
        if !self.demand.active || self.in_flight.is_some() || now_ms < self.due_ms {
            return None;
        }
        self.in_flight = Some(self.generation);
        self.due_ms = now_ms.saturating_add(interval_ms);
        Some(self.generation)
    }

    /// Bring forward a bounded recovery attempt after a baseline-only sample.
    pub fn retry_after(&mut self, now_ms: u64, delay_ms: u64) {
        self.due_ms = self.due_ms.min(now_ms.saturating_add(delay_ms));
    }

    /// Sleep until the next due query, with a one-second freshness/appearance check.
    pub fn wait_ms(&self, now_ms: u64) -> u64 {
        if self.demand.active && self.in_flight.is_none() {
            self.due_ms.saturating_sub(now_ms).clamp(1, 1000)
        } else {
            1000
        }
    }

    pub fn complete(&mut self, generation: u64) -> bool {
        if self.in_flight != Some(generation) {
            return false;
        }
        self.in_flight = None;
        self.demand.active && generation == self.generation
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn baseline_retry_waits_for_its_delay_and_never_overlaps_a_job() {
        let mut slot = SamplingSlot::default();
        slot.update(Demand {
            active: true,
            ..Default::default()
        });
        let token = slot.begin(0, 2000).unwrap();
        slot.retry_after(0, 250);
        assert!(slot.begin(250, 2000).is_none());
        assert!(slot.complete(token));
        assert!(slot.begin(249, 2000).is_none());
        assert!(slot.begin(250, 2000).is_some());
        slot.update(Demand::default());
        assert!(slot.begin(5000, 2000).is_none());
    }

    #[test]
    fn disabling_drops_late_results_without_overlapping_a_slow_query() {
        let mut slot = SamplingSlot::default();
        slot.update(Demand {
            active: true,
            ..Default::default()
        });
        let token = slot.begin(0, 1000).unwrap();
        assert!(slot.begin(60_000, 1000).is_none());
        slot.update(Demand::default());
        assert!(!slot.complete(token));
        assert!(slot.begin(60_000, 1000).is_none());
        slot.update(Demand {
            active: true,
            ..Default::default()
        });
        assert!(slot.begin(60_000, 1000).is_some());
    }

    #[test]
    fn changed_device_waits_for_previous_job_and_repeated_wakes_preserve_cadence() {
        let mut slot = SamplingSlot::default();
        slot.update(Demand {
            active: true,
            selection: Some("one".into()),
            ..Default::default()
        });
        let first = slot.begin(0, 1000).unwrap();
        slot.update(Demand {
            active: true,
            selection: Some("two".into()),
            ..Default::default()
        });
        assert!(slot.begin(1000, 1000).is_none());
        assert!(!slot.complete(first));
        let second = slot.begin(1000, 1000).unwrap();
        assert!(slot.complete(second));
        for time in 1000..2000 {
            assert!(slot.begin(time, 1000).is_none());
        }
        assert!(slot.begin(2000, 1000).is_some());
    }
}
