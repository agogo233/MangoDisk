//! Read-only fan capability and sampling probe; never changes fan control.
use mangodisk_platform::system_resources::fans::FanReader;
use std::time::{Duration, Instant};

fn main() {
    let samples = std::env::args()
        .nth(1)
        .and_then(|value| value.parse::<u32>().ok())
        .unwrap_or(3)
        .clamp(1, 120);
    let mut reader = FanReader::default();
    for index in 0..samples {
        let start = Instant::now();
        let result = reader.read();
        let micros = start.elapsed().as_micros();
        let record = match result {
            Ok(sample) => {
                serde_json::json!({"sample": index + 1, "micros": micros, "status": "ready", "value": sample})
            }
            Err(error) => {
                serde_json::json!({"sample": index + 1, "micros": micros, "status": "unavailable", "code": format!("{:?}", error.code()), "diagnostic": error.diagnostic()})
            }
        };
        println!("{record}");
        if index + 1 < samples {
            std::thread::sleep(Duration::from_secs(2));
        }
    }
}
