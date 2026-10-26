use super::metrics::{MetricId, MetricStatus};
use super::readings::ResourceCache;
use mangodisk_platform::{
    system_resources::gpu::{
        temperature::{
            GpuTemperature, GpuTemperatureKind, GpuTemperatureSample, GpuTemperatureSource,
        },
        GpuAdapter, GpuAdapterUsage, GpuSample,
    },
    PlatformError, PlatformErrorCode,
};

fn usage() -> GpuSample {
    GpuSample::Usage(
        [("a", 80.0), ("b", 20.0)]
            .into_iter()
            .map(|(id, used_percent)| GpuAdapterUsage {
                id: id.into(),
                name: id.into(),
                used_percent,
                details: None,
            })
            .collect(),
    )
}
fn temperatures(failed_b: bool) -> Vec<GpuTemperatureSample> {
    ["a", "b"]
        .into_iter()
        .map(|id| GpuTemperatureSample {
            adapter_id: id.into(),
            reading: if id == "b" && failed_b {
                Err(PlatformError::new(
                    PlatformErrorCode::OperationFailed,
                    "sensor read failed",
                ))
            } else {
                Ok(GpuTemperature {
                    adapter_id: id.into(),
                    adapter_name: id.into(),
                    celsius: if id == "a" { 40.0 } else { 60.0 },
                    kind: GpuTemperatureKind::Device,
                    source: GpuTemperatureSource::Driver,
                    sensor_count: 1,
                })
            },
        })
        .collect()
}
#[test]
fn all_device_snapshots_preserve_independent_failures_timestamps_and_removed_device_boundaries() {
    let mut cache = ResourceCache::default();
    let catalogue = || {
        ["a", "b"]
            .into_iter()
            .map(|id| GpuAdapter {
                id: id.into(),
                name: id.into(),
            })
            .collect()
    };
    cache.gpu_adapters(catalogue());
    cache.gpu(usage(), Some("missing"), 1000);
    cache.gpu_temperatures(Ok(temperatures(false)), 1000);
    let first = cache.snapshot(1000);
    assert_eq!(first.gpu.status, MetricStatus::Disconnected);
    assert_eq!(first.gpu_devices.len(), 2);
    assert!(first
        .gpu_devices
        .iter()
        .all(|device| device.usage.status == MetricStatus::Ready));
    assert_eq!(
        first.gpu_devices[0]
            .usage
            .value
            .as_ref()
            .unwrap()
            .used_percent,
        80.0
    );
    assert_eq!(
        first.gpu_devices[1]
            .temperature
            .value
            .as_ref()
            .unwrap()
            .celsius,
        60.0
    );
    cache.fail(MetricId::Gpu, MetricStatus::Disconnected);
    cache.gpu_temperatures(Ok(temperatures(true)), 5000);
    let failed = cache.snapshot(5000);
    assert!(failed
        .gpu_devices
        .iter()
        .all(|device| device.usage.status == MetricStatus::Failed
            && device.usage.sampled_at_ms == Some(1000)));
    assert_eq!(
        failed.gpu_devices[0].temperature.status,
        MetricStatus::Ready
    );
    assert_eq!(
        failed.gpu_devices[1].temperature.status,
        MetricStatus::Failed
    );
    cache.gpu(usage(), Some("b"), 17000);
    cache.gpu_temperatures(Ok(temperatures(false)), 17000);
    let recovered = cache.snapshot(17000);
    for device in &recovered.gpu_devices {
        assert_eq!(
            device
                .usage_history
                .iter()
                .map(|point| point.sampled_at_ms)
                .collect::<Vec<_>>(),
            [1000, 17000]
        );
        assert_eq!(
            device.temperature_history.last().unwrap().sampled_at_ms,
            17000
        );
    }
    let again = cache.snapshot(17000);
    assert!(std::sync::Arc::ptr_eq(
        &recovered.gpu_devices[0].usage_history,
        &again.gpu_devices[0].usage_history
    ));
    cache.gpu_adapters(vec![GpuAdapter {
        id: "b".into(),
        name: "GPU 0 · b".into(),
    }]);
    let removed = cache.snapshot(17000);
    assert_eq!(removed.gpu_devices.len(), 1);
    assert_eq!(removed.gpu_devices[0].adapter_id, "b");
    assert_eq!(removed.gpu_devices[0].usage_history.len(), 2);
    cache.gpu_adapters(catalogue());
    let returned = cache.snapshot(17000);
    assert!(returned.gpu_devices[0].usage_history.is_empty());
    assert!(returned.gpu_devices[0].temperature_history.is_empty());
    assert_eq!(
        cache.snapshot(28001).gpu_devices[1].usage.status,
        MetricStatus::Stale
    );
}
