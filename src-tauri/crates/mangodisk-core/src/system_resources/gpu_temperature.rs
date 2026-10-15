//! Per-device temperature history remains independent of utilization failures.
use super::metrics::{MetricReading, MetricStatus, Trend, TrendPoint};
use mangodisk_platform::{
    system_resources::gpu::temperature::{
        GpuTemperature, GpuTemperatureSample, GPU_TEMPERATURE_FRESHNESS_MS,
    },
    PlatformError, PlatformErrorCode,
};
#[derive(Default)]
pub(super) struct GpuTemperatureCache {
    pub reading: MetricReading<GpuTemperature>,
    history: Trend,
    adapter_id: Option<String>,
}
impl GpuTemperatureCache {
    pub fn select(&mut self, id: Option<&str>) {
        if let Some(id) = id {
            if self.adapter_id.as_deref() != Some(id) {
                self.clear();
                self.adapter_id = Some(id.into());
            }
        }
    }
    pub fn apply(
        &mut self,
        samples: Vec<GpuTemperatureSample>,
        selected: Option<&str>,
        automatic: Option<&str>,
        now: u64,
    ) {
        let id = selected
            .or(automatic)
            .map(str::to_owned)
            .or_else(|| {
                self.adapter_id
                    .clone()
                    .filter(|id| samples.iter().any(|sample| sample.adapter_id == *id))
            })
            .or_else(|| {
                samples
                    .iter()
                    .filter(|sample| sample.reading.as_ref().is_ok_and(GpuTemperature::valid))
                    .map(|sample| &sample.adapter_id)
                    .min()
                    .cloned()
            })
            .or_else(|| {
                samples
                    .iter()
                    .map(|sample| &sample.adapter_id)
                    .min()
                    .cloned()
            });
        self.select(id.as_deref());
        let Some(sample) = samples
            .into_iter()
            .find(|sample| Some(&sample.adapter_id) == id.as_ref())
        else {
            self.fail(if selected.is_some() {
                MetricStatus::Disconnected
            } else {
                MetricStatus::Unsupported
            });
            return;
        };
        match sample.reading {
            Ok(value) if value.valid() && value.adapter_id == sample.adapter_id => {
                if self.reading.value.as_ref().is_some_and(|previous| {
                    previous.kind != value.kind
                        || previous.source != value.source
                        || previous.sensor_count != value.sensor_count
                }) {
                    self.history.clear();
                }
                self.history.push(TrendPoint {
                    sampled_at_ms: now,
                    primary: value.celsius,
                    secondary: None,
                });
                self.reading = MetricReading::ready(value, now);
            }
            Ok(_) => self.fail(MetricStatus::Failed),
            Err(error) => self.error(&error),
        }
    }
    pub fn error(&mut self, error: &PlatformError) {
        self.fail(if error.code() == PlatformErrorCode::Unsupported {
            MetricStatus::Unsupported
        } else {
            MetricStatus::Failed
        });
    }
    fn fail(&mut self, status: MetricStatus) {
        self.reading.status = status;
        if matches!(
            status,
            MetricStatus::Unsupported | MetricStatus::Disconnected
        ) {
            self.reading.value = None;
            self.reading.sampled_at_ms = None;
            self.history.clear();
        }
    }
    pub fn clear(&mut self) {
        *self = Self::default();
    }
    pub fn expire(&mut self, now: u64) {
        self.reading.expire(now, GPU_TEMPERATURE_FRESHNESS_MS);
    }
    pub fn history(&self, now: u64) -> Vec<TrendPoint> {
        self.history.snapshot(now)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use mangodisk_platform::system_resources::gpu::temperature::{
        GpuTemperatureKind, GpuTemperatureSource,
    };
    fn sample(id: &str, celsius: f64) -> GpuTemperatureSample {
        GpuTemperatureSample {
            adapter_id: id.into(),
            reading: Ok(GpuTemperature {
                adapter_id: id.into(),
                adapter_name: id.into(),
                celsius,
                kind: GpuTemperatureKind::SensorAverage,
                source: GpuTemperatureSource::AppleSmc,
                sensor_count: 8,
            }),
        }
    }
    #[test]
    fn temperature_survives_utilization_absence_and_display_pauses_with_real_timestamps() {
        let mut cache = GpuTemperatureCache::default();
        for time in [1000, 5000, 9000, 13000] {
            cache.apply(vec![sample("a", 51.0)], None, None, time);
        }
        assert_eq!(cache.history(13000).len(), 4);
        assert_eq!(cache.reading.sampled_at_ms, Some(13000));
        cache.expire(23001);
        assert_eq!(cache.reading.status, MetricStatus::Stale);
    }
    #[test]
    fn manual_selection_never_falls_back_and_device_changes_do_not_mix_history() {
        let mut cache = GpuTemperatureCache::default();
        cache.apply(
            vec![sample("a", 51.0), sample("b", 80.0)],
            Some("a"),
            Some("b"),
            1000,
        );
        assert_eq!(cache.reading.value.as_ref().unwrap().adapter_id, "a");
        cache.apply(vec![sample("b", 80.0)], Some("a"), Some("b"), 5000);
        assert_eq!(cache.reading.status, MetricStatus::Disconnected);
        assert!(cache.history(5000).is_empty());
        cache.apply(vec![sample("b", 80.0)], Some("b"), None, 9000);
        assert_eq!(cache.history(9000).len(), 1);
    }
    #[test]
    fn temporary_failures_preserve_history_without_manufacturing_new_samples() {
        let mut cache = GpuTemperatureCache::default();
        cache.apply(vec![sample("a", 51.0)], None, None, 1000);
        cache.error(&PlatformError::new(
            PlatformErrorCode::OperationFailed,
            "read failed",
        ));
        assert_eq!(cache.reading.status, MetricStatus::Failed);
        assert_eq!(cache.reading.sampled_at_ms, Some(1000));
        cache.apply(vec![sample("a", 52.0)], None, None, 9000);
        assert_eq!(cache.history(9000).len(), 2);
    }
    #[test]
    fn changing_sensor_meaning_resets_history_and_unsupported_drops_retained_values() {
        let mut cache = GpuTemperatureCache::default();
        cache.apply(vec![sample("a", 51.0)], Some("a"), None, 1000);
        let mut next = sample("a", 52.0);
        let value = next.reading.as_mut().unwrap();
        value.kind = GpuTemperatureKind::Device;
        value.source = GpuTemperatureSource::Driver;
        value.sensor_count = 1;
        cache.apply(vec![next], Some("a"), None, 5000);
        assert_eq!(cache.history(5000).len(), 1);
        cache.error(&PlatformError::new(
            PlatformErrorCode::Unsupported,
            "no sensor",
        ));
        assert!(cache.history(5000).is_empty());
        assert!(cache.reading.value.is_none());
        assert!(cache.reading.sampled_at_ms.is_none());
    }

    #[test]
    fn invalid_temperatures_and_mismatched_sensor_identity_are_rejected() {
        for value in [0.0, -1.0, f64::NAN, f64::INFINITY, 151.0, 128.0] {
            let mut cache = GpuTemperatureCache::default();
            cache.apply(vec![sample("a", value)], Some("a"), None, 1000);
            assert_eq!(cache.reading.status, MetricStatus::Failed);
            assert!(cache.history(1000).is_empty());
        }
        let mut bad = sample("a", 51.0);
        bad.reading.as_mut().unwrap().adapter_id = "b".into();
        let mut cache = GpuTemperatureCache::default();
        cache.apply(vec![bad], Some("a"), None, 1000);
        assert_eq!(cache.reading.status, MetricStatus::Failed);
    }
}
