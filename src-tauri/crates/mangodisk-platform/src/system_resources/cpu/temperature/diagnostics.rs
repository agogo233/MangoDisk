//! Aggregate real sensor cadence separately from request cadence and cache hits.
use crate::system_resources::cpu::temperature::{
    CpuTemperature, CpuTemperatureSource, TEMPERATURE_INTERVAL_MS,
};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const SUMMARY_INTERVAL: Duration = Duration::from_secs(60);

#[derive(Default)]
pub(super) struct Diagnostics {
    previous_sample: Option<(CpuTemperatureSource, u64)>,
    previous_request: Option<Instant>,
    reported_at: Option<Instant>,
    requests: u64,
    samples: u64,
    cache_hits: u64,
    cache_hits_since_sample: u64,
    max_sample_interval_ms: u64,
    max_request_interval_ms: u64,
    max_age_ms: u64,
}

impl Diagnostics {
    pub(super) fn observe(&mut self, value: &CpuTemperature, requested_at: Instant) {
        let observed_at_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        self.observe_at(value, requested_at, observed_at_ms);
    }

    fn observe_at(&mut self, value: &CpuTemperature, requested_at: Instant, observed_at_ms: u64) {
        let sampled_at_ms = value.sampled_at_ms.unwrap_or(observed_at_ms);
        let request_interval_ms = self.previous_request.map_or(0, |previous| {
            requested_at.duration_since(previous).as_millis() as u64
        });
        let previous_sampled_at_ms = self
            .previous_sample
            .filter(|(source, _)| *source == value.source)
            .map(|(_, sampled)| sampled);
        let cached = previous_sampled_at_ms == Some(sampled_at_ms);
        let sample_interval_ms =
            previous_sampled_at_ms.map_or(0, |previous| sampled_at_ms.saturating_sub(previous));
        let age_ms = observed_at_ms.saturating_sub(sampled_at_ms);
        if self.previous_sample.is_none() {
            log::info!("cpu_temperature_first_sample source={:?} sampled_at_ms={sampled_at_ms} observed_at_ms={observed_at_ms} age_ms={age_ms}", value.source);
        }
        self.requests += 1;
        self.max_request_interval_ms = self.max_request_interval_ms.max(request_interval_ms);
        self.max_age_ms = self.max_age_ms.max(age_ms);
        if cached {
            // One cache-hit detail per window suffices; the summary counts the rest.
            if self.cache_hits == 0 {
                log::info!("cpu_temperature_cache_hit source={:?} sampled_at_ms={sampled_at_ms} observed_at_ms={observed_at_ms} age_ms={age_ms} request_interval_ms={request_interval_ms}", value.source);
            }
            self.cache_hits += 1;
            self.cache_hits_since_sample += 1;
        } else {
            self.samples += 1;
            self.max_sample_interval_ms = self.max_sample_interval_ms.max(sample_interval_ms);
            if sample_interval_ms > TEMPERATURE_INTERVAL_MS * 2 {
                log::warn!("cpu_temperature_sample_gap source={:?} previous_sampled_at_ms={} sampled_at_ms={sampled_at_ms} observed_at_ms={observed_at_ms} sample_interval_ms={sample_interval_ms} request_interval_ms={request_interval_ms} age_ms={age_ms} cache_hits_since_sample={}", value.source, previous_sampled_at_ms.unwrap_or(0), self.cache_hits_since_sample);
            }
            self.cache_hits_since_sample = 0;
        }
        self.previous_sample = Some((value.source, sampled_at_ms));
        self.previous_request = Some(requested_at);
        let reported_at = self.reported_at.get_or_insert(requested_at);
        if requested_at.duration_since(*reported_at) >= SUMMARY_INTERVAL {
            log::info!("cpu_temperature_sampling_summary source={:?} window_ms={} requests={} native_samples={} cache_hits={} max_sample_interval_ms={} max_request_interval_ms={} max_age_ms={} sampled_at_ms={sampled_at_ms}", value.source, requested_at.duration_since(*reported_at).as_millis(), self.requests, self.samples, self.cache_hits, self.max_sample_interval_ms, self.max_request_interval_ms, self.max_age_ms);
            *reported_at = requested_at;
            self.requests = 0;
            self.samples = 0;
            self.cache_hits = 0;
            self.max_sample_interval_ms = 0;
            self.max_request_interval_ms = 0;
            self.max_age_ms = 0;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::system_resources::cpu::temperature::CpuTemperatureKind;

    fn reading(sampled_at_ms: u64) -> CpuTemperature {
        CpuTemperature {
            celsius: 64.0,
            kind: CpuTemperatureKind::Package,
            source: CpuTemperatureSource::AmdZenPawnIo,
            sensor_count: 1,
            sampled_at_ms: Some(sampled_at_ms),
        }
    }

    #[test]
    fn cached_observations_do_not_advance_the_native_sample_clock() {
        let start = Instant::now();
        let mut diagnostics = Diagnostics::default();
        diagnostics.observe_at(&reading(1000), start, 1001);
        diagnostics.observe_at(&reading(1000), start + Duration::from_millis(4000), 5001);
        diagnostics.observe_at(&reading(9001), start + Duration::from_millis(8000), 9002);
        assert_eq!(diagnostics.requests, 3);
        assert_eq!(diagnostics.samples, 2);
        assert_eq!(diagnostics.cache_hits, 1);
        assert_eq!(diagnostics.max_sample_interval_ms, 8001);
        assert_eq!(diagnostics.max_request_interval_ms, 4000);
        assert_eq!(diagnostics.max_age_ms, 4001);
        assert_eq!(diagnostics.cache_hits_since_sample, 0);
    }

    #[test]
    fn summaries_keep_the_previous_sample_for_cross_window_gaps() {
        let start = Instant::now();
        let mut diagnostics = Diagnostics::default();
        diagnostics.observe_at(&reading(1000), start, 1001);
        diagnostics.observe_at(&reading(61000), start + Duration::from_secs(60), 61001);
        assert_eq!(diagnostics.requests, 0);
        diagnostics.observe_at(&reading(61000), start + Duration::from_secs(64), 65001);
        diagnostics.observe_at(&reading(69001), start + Duration::from_secs(68), 69002);
        assert_eq!(diagnostics.samples, 1);
        assert_eq!(diagnostics.cache_hits, 1);
        assert_eq!(diagnostics.max_sample_interval_ms, 8001);
    }
}
