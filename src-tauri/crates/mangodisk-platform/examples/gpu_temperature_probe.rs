//! Read-only engineering probe for identified GPU temperatures.
#[cfg(any(target_os = "macos", windows))]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    use mangodisk_platform::system_resources::gpu::GpuReader;
    use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
    let count = std::env::args()
        .nth(1)
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(1)
        .clamp(1, 1000);
    let mut reader = GpuReader::default();
    for tick in 0..count {
        let started = Instant::now();
        let samples = reader
            .read_temperatures()
            .ok_or("temperature cadence not due")??;
        let elapsed = started.elapsed().as_micros();
        let samples: Vec<_> = samples.into_iter().map(|sample| match sample.reading {
            Ok(value) => serde_json::json!({"adapterId":sample.adapter_id,"value":value}),
            Err(error) => serde_json::json!({"adapterId":sample.adapter_id,"code":format!("{:?}",error.code()),"error":error.to_string()}),
        }).collect();
        println!(
            "{}",
            serde_json::json!({"sampledAtMs":SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis(),"readMicros":elapsed,"samples":samples})
        );
        if tick + 1 < count {
            std::thread::sleep(Duration::from_millis(4_010));
        }
    }
    Ok(())
}
#[cfg(not(any(target_os = "macos", windows)))]
fn main() {
    eprintln!("GPU temperature probe requires macOS or Windows");
}
