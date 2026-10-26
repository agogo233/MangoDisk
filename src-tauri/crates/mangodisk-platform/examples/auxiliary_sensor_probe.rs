//! Bounded, read-only probe for battery, system power and Windows GPU tachometers.
fn main() {
    use mangodisk_platform::{
        system_resources::{
            battery::{BatteryPresenceReader, BatteryReader},
            power::SystemPowerReader,
        },
        PlatformResult,
    };
    use serde::Serialize;
    use std::time::{Duration, Instant};
    fn result<T: Serialize>(value: PlatformResult<T>) -> serde_json::Value {
        match value {
            Ok(value) => serde_json::json!({"status":"ready","value":value}),
            Err(error) => {
                serde_json::json!({"status":format!("{:?}",error.code()),"error":error.to_string()})
            }
        }
    }
    let count = std::env::args()
        .nth(1)
        .and_then(|arg| arg.parse::<u32>().ok())
        .unwrap_or(1)
        .clamp(1, 300);
    let mut presence = BatteryPresenceReader::default();
    let mut battery = BatteryReader::default();
    let mut power = SystemPowerReader::default();
    #[cfg(windows)]
    let mut gpu = mangodisk_platform::system_resources::gpu::GpuReader::default();
    for tick in 0..count {
        let start = Instant::now();
        let present = presence.read();
        let battery = battery.read().map(|sample| serde_json::json!({"temperature":sample.temperature,"chargingPower":sample.charging_power,"batteryPower":sample.battery_power}));
        let mut value = serde_json::json!({"tick":tick,"batteryPresent":present,"battery":result(battery),"systemPower":result(power.read())});
        #[cfg(windows)]
        {
            value["gpuFans"] = result(gpu.read_fans());
        }
        value["readMicros"] = serde_json::json!(start.elapsed().as_micros());
        println!("{value}");
        if tick + 1 < count {
            std::thread::sleep(Duration::from_secs(2).saturating_sub(start.elapsed()));
        }
    }
}
