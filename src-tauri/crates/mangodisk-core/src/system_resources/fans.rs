//! Fan selection is deterministic and never substitutes a missing manual choice.
use super::metrics::{MetricReading, MetricStatus};
use mangodisk_platform::system_resources::fans::{FanSample, FanSpeed, FAN_FRESHNESS_MS};

pub fn current_sample(reading: &MetricReading<FanSample>, now_ms: u64) -> Option<&FanSample> {
    reading.value.as_ref().filter(|sample| {
        reading.status == MetricStatus::Ready
            && sample.is_valid()
            && reading
                .sampled_at_ms
                .and_then(|at| now_ms.checked_sub(at))
                .is_some_and(|age| age <= FAN_FRESHNESS_MS)
    })
}

pub fn selected_fan(
    reading: &MetricReading<FanSample>,
    now_ms: u64,
    selected: Option<u8>,
) -> Option<&FanSpeed> {
    let fans = &current_sample(reading, now_ms)?.fans;
    if let Some(id) = selected {
        fans.iter().find(|fan| fan.id == id && fan.rpm.is_some())
    } else {
        fans.iter()
            .filter(|fan| fan.rpm.is_some())
            .max_by(|left, right| {
                left.rpm
                    .unwrap()
                    .total_cmp(&right.rpm.unwrap())
                    .then_with(|| right.id.cmp(&left.id))
            })
    }
}

/// Average only available tachometers; a stopped fan participates as zero.
pub fn average_rpm(reading: &MetricReading<FanSample>, now_ms: u64) -> Option<f64> {
    let sample = current_sample(reading, now_ms)?;
    let mut count = 0;
    let sum: f64 = sample
        .fans
        .iter()
        .filter_map(|fan| fan.rpm)
        .inspect(|_| count += 1)
        .sum();
    (count > 0).then(|| sum / f64::from(count))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::system_resources::readings::ResourceCache;
    use mangodisk_platform::{PlatformError, PlatformErrorCode};

    fn sample() -> FanSample {
        FanSample {
            fans: vec![
                FanSpeed {
                    id: 0,
                    rpm: Some(0.0),
                },
                FanSpeed {
                    id: 1,
                    rpm: Some(2500.0),
                },
                FanSpeed { id: 2, rpm: None },
            ],
        }
    }

    #[test]
    fn average_includes_stopped_fans_and_excludes_unavailable_tachometers() {
        let mut reading = MetricReading::ready(sample(), 1000);
        assert_eq!(average_rpm(&reading, 1000), Some(1250.0));
        assert_eq!(average_rpm(&reading, 6001), None);
        reading.value.as_mut().unwrap().fans[1].rpm = None;
        assert_eq!(average_rpm(&reading, 1000), Some(0.0));
        reading.value.as_mut().unwrap().fans[0].rpm = None;
        assert_eq!(average_rpm(&reading, 1000), None);
        reading.value.as_mut().unwrap().fans.clear();
        assert_eq!(average_rpm(&reading, 1000), None);
    }

    #[test]
    fn selection_preserves_stopped_fans_and_never_replaces_missing_manual_fans() {
        let reading = MetricReading::ready(sample(), 1000);
        assert_eq!(selected_fan(&reading, 1000, None).unwrap().id, 1);
        assert_eq!(
            selected_fan(&reading, 1000, Some(0)).unwrap().rpm,
            Some(0.0)
        );
        for id in [2, 3] {
            assert!(selected_fan(&reading, 1000, Some(id)).is_none());
        }
        for now in [999, 6001] {
            assert!(selected_fan(&reading, now, None).is_none());
        }
        assert!(selected_fan(&reading, 6000, None).is_some());
        assert!(current_sample(
            &MetricReading::ready(FanSample { fans: vec![] }, 1000),
            1000
        )
        .unwrap()
        .fans
        .is_empty());
    }

    #[test]
    fn confirmed_empty_samples_remain_distinct_from_unsupported_and_failed_readings() {
        let mut cache = ResourceCache::default();
        for (code, status) in [
            (PlatformErrorCode::Unsupported, MetricStatus::Unsupported),
            (PlatformErrorCode::OperationFailed, MetricStatus::Failed),
        ] {
            cache.fan_speed(Ok(FanSample { fans: vec![] }), 1000);
            let snapshot = cache.snapshot(1000);
            assert_eq!(snapshot.fan_speed.status, MetricStatus::Ready);
            assert!(current_sample(&snapshot.fan_speed, 1000)
                .unwrap()
                .fans
                .is_empty());
            let wire = serde_json::to_value(&snapshot.fan_speed).unwrap();
            assert_eq!(wire["status"], "ready");
            assert_eq!(wire["value"]["fans"], serde_json::json!([]));
            assert!(current_sample(&cache.snapshot(6001).fan_speed, 6001).is_none());
            cache.fan_speed(
                Err(PlatformError::new(code, "fan observation unavailable")),
                7000,
            );
            let snapshot = cache.snapshot(7000);
            assert_eq!(snapshot.fan_speed.status, status);
            assert!(snapshot.fan_speed.value.is_none());
            assert!(snapshot.fan_speed.sampled_at_ms.is_none());
        }
    }

    #[test]
    fn failed_invalid_and_stale_samples_cannot_keep_a_previous_valid_rpm() {
        let mut cache = ResourceCache::default();
        cache.fan_speed(Ok(sample()), 1000);
        assert_eq!(cache.snapshot(1000).fan_speed.status, MetricStatus::Ready);
        assert!(selected_fan(&cache.snapshot(6001).fan_speed, 6001, None).is_none());
        for code in [
            PlatformErrorCode::Unsupported,
            PlatformErrorCode::OperationFailed,
        ] {
            cache.fan_speed(Ok(sample()), 7000);
            cache.fan_speed(Err(PlatformError::new(code, "SMC read failure")), 8000);
            assert!(cache.snapshot(8000).fan_speed.value.is_none());
        }
        cache.fan_speed(
            Ok(FanSample {
                fans: vec![FanSpeed {
                    id: 0,
                    rpm: Some(f64::NAN),
                }],
            }),
            9000,
        );
        assert_eq!(cache.snapshot(9000).fan_speed.status, MetricStatus::Failed);
        cache.fan_speed(Ok(sample()), 10_000);
        assert!(selected_fan(&cache.snapshot(10_000).fan_speed, 10_000, None).is_some());
    }
}
