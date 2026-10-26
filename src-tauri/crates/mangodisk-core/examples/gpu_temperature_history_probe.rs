//! Native reader and Core history probe across resident detail resets.
#[cfg(any(target_os = "macos", windows))]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    use mangodisk_core::system_resources::readings::ResourceCache;
    use mangodisk_platform::system_resources::gpu::GpuReader;
    use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
    let count = std::env::args()
        .nth(1)
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(30)
        .clamp(1, 300);
    let independent = std::env::args().nth(2).as_deref() == Some("--independent");
    let mut reader = GpuReader::default();
    let mut cache = ResourceCache::default();
    for tick in 0..count {
        let started = Instant::now();
        if !independent && (tick == 10 || tick == 20) {
            reader.reset();
        }
        let usage = reader.read();
        let temperature = reader.read_temperatures();
        let now = SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis() as u64;
        let adapters = reader.adapters();
        cache.gpu_adapters(adapters.clone());
        let first = adapters.first().map(|adapter| adapter.id.as_str());
        let last = adapters.last().map(|adapter| adapter.id.as_str());
        let phase = (tick / 4) % 4;
        let selected_usage = independent
            .then_some(if phase >= 2 { last } else { first })
            .flatten();
        if let Ok(usage) = usage {
            cache.gpu(usage, selected_usage, now);
        }
        if let Some(temperature) = temperature {
            cache.gpu_temperatures(temperature, now);
        }
        let snapshot = cache.snapshot(now);
        println!(
            "{}",
            serde_json::json!({"tick":tick,"phase":phase,"observedAtMs":now,"readMicros":started.elapsed().as_micros(),"adapters":adapters,"devices":snapshot.gpu_devices,"usageSelection":selected_usage,"temperature":snapshot.gpu_temperature,"history":snapshot.gpu_temperature_history,"gpu":snapshot.gpu})
        );
        if tick + 1 < count {
            std::thread::sleep(Duration::from_millis(2000).saturating_sub(started.elapsed()));
        }
    }
    Ok(())
}
#[cfg(not(any(target_os = "macos", windows)))]
fn main() {
    eprintln!("GPU temperature history probe requires macOS or Windows");
}
