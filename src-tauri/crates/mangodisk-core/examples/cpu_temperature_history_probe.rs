//! CPU sensor observations and Core history across panel detail resets.
fn main() {
    use mangodisk_core::system_resources::readings::ResourceCache;
    use mangodisk_platform::system_resources::cpu::details::CpuDetailsReader;
    use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

    let count = std::env::args()
        .nth(1)
        .and_then(|v| v.parse::<u32>().ok())
        .unwrap_or(31)
        .clamp(1, 1000);
    let mut reader = CpuDetailsReader::default();
    let mut cache = ResourceCache::default();
    for tick in 0..count {
        let started = Instant::now();
        if tick == 10 || tick == 20 {
            reader.reset();
        }
        let details = reader.read(false, true);
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        cache.cpu_details(details, now);
        let snapshot = cache.snapshot(now);
        println!(
            "{}",
            serde_json::json!({"tick":tick,"observedAtMs":now,"temperature":snapshot.cpu_temperature,"history":snapshot.cpu_temperature_history})
        );
        if tick + 1 < count {
            std::thread::sleep(Duration::from_millis(2000).saturating_sub(started.elapsed()));
        }
    }
}
