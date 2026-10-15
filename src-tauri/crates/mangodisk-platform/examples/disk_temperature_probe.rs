//! Engineering-only read-only probe for sensor identity, lifecycle and latency.
use mangodisk_platform::system_resources::{disk, disk_temperature, network::NetworkReader};
use std::time::{Duration, Instant};

fn main() {
    let samples = std::env::args()
        .nth(1)
        .and_then(|value| value.parse::<u32>().ok())
        .unwrap_or(3)
        .clamp(1, 20);
    for index in 0..samples {
        for volume in disk::list().expect("local volume enumeration") {
            let started = Instant::now();
            let result = disk_temperature::read(&volume);
            let outcome = match result {
                Ok(value) => {
                    serde_json::json!({ "status": "ready", "celsius": value.celsius, "source": value.source })
                }
                Err(error) => {
                    serde_json::json!({ "status": format!("{:?}", error.code()), "error": error.diagnostic() })
                }
            };
            println!(
                "{}",
                serde_json::json!({ "sample": index + 1, "volume": volume.name, "system": volume.system, "elapsedUs": started.elapsed().as_micros(), "reading": outcome })
            );
        }
        if index + 1 < samples {
            std::thread::sleep(Duration::from_secs(1));
        }
    }
    let mut reader = NetworkReader::default();
    let started = Instant::now();
    let interfaces = reader.read().expect("network enumeration");
    println!(
        "{}",
        serde_json::json!({ "networkElapsedUs": started.elapsed().as_micros(), "interfaces": interfaces.iter().map(|sample| serde_json::json!({ "name": sample.interface.name, "kind": sample.interface.kind, "connected": sample.interface.connected, "physical": sample.interface.physical, "defaultRouteMetric": sample.interface.default_route_metric })).collect::<Vec<_>>() })
    );
}
