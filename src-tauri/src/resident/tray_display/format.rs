//! Shared text for native resident surfaces.
use mangodisk_core::system_resources::{
    metrics::{MetricId, MetricStatus},
    readings::ResourceReadings,
};
use serde::Serialize;

use crate::resident::{preference_schema::DisplayMetricId, preferences::ResidentPreferences};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum DisplayId {
    App,
    Cpu,
    CpuTemperature,
    Gpu,
    GpuTemperature,
    Memory,
    Upload,
    Download,
    Disk,
    DiskTemperature,
    BatteryTemperature,
    ChargingPower,
    BatteryPower,
    SystemPower,
}

impl DisplayId {
    pub const ALL: [Self; 14] = [
        Self::App,
        Self::Cpu,
        Self::CpuTemperature,
        Self::Gpu,
        Self::GpuTemperature,
        Self::Memory,
        Self::Upload,
        Self::Download,
        Self::Disk,
        Self::DiskTemperature,
        Self::BatteryTemperature,
        Self::ChargingPower,
        Self::BatteryPower,
        Self::SystemPower,
    ];
    pub fn tray_id(self) -> &'static str {
        match self {
            Self::App => "resident",
            Self::Cpu => "resident-cpu",
            Self::CpuTemperature => "resident-cpu-temperature",
            Self::Gpu => "resident-gpu",
            Self::GpuTemperature => "resident-gpu-temperature",
            Self::Memory => "resident-memory",
            Self::Upload => "resident-upload",
            Self::Download => "resident-download",
            Self::Disk => "resident-disk",
            Self::DiskTemperature => "resident-disk-temperature",
            Self::BatteryTemperature => "resident-battery-temperature",
            Self::ChargingPower => "resident-charging-power",
            Self::BatteryPower => "resident-battery-power",
            Self::SystemPower => "resident-system-power",
        }
    }
    pub fn metric(self) -> Option<MetricId> {
        match self {
            Self::App
            | Self::BatteryTemperature
            | Self::ChargingPower
            | Self::BatteryPower
            | Self::SystemPower => None,
            Self::Cpu | Self::CpuTemperature => Some(MetricId::Cpu),
            Self::Gpu | Self::GpuTemperature => Some(MetricId::Gpu),
            Self::Memory => Some(MetricId::Memory),
            Self::Upload | Self::Download => Some(MetricId::Network),
            Self::Disk | Self::DiskTemperature => Some(MetricId::Disk),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DisplayEntry {
    pub id: DisplayId,
    pub tone: super::usage_color::UsageTone,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub power_state: Option<mangodisk_platform::system_resources::battery::BatteryPowerState>,
    pub color_value: Option<u16>,
    pub marker: String,
    pub digits: String,
    pub text: String,
    pub tooltip: String,
}

/// Linux AppIndicator exposes frequently changing status through its title
/// label. Keep the icon stable and compose only the enabled metric fields.
#[cfg(target_os = "linux")]
pub fn indicator_title(entries: &[DisplayEntry], compact: bool) -> Option<String> {
    let title = entries
        .iter()
        .filter_map(|entry| {
            let text = if compact {
                match entry.id {
                    DisplayId::Cpu | DisplayId::Gpu | DisplayId::Memory | DisplayId::Disk => {
                        format!("{}{}%", entry.marker, entry.digits)
                    }
                    DisplayId::Upload | DisplayId::Download => {
                        let mut marker = entry.marker.chars();
                        let direction = marker.next()?;
                        format!("{direction}{}{}", entry.digits, marker.as_str())
                    }
                    DisplayId::App
                    | DisplayId::CpuTemperature
                    | DisplayId::GpuTemperature
                    | DisplayId::DiskTemperature
                    | DisplayId::BatteryTemperature
                    | DisplayId::ChargingPower
                    | DisplayId::BatteryPower
                    | DisplayId::SystemPower => return None,
                }
            } else {
                entry.text.trim().to_string()
            };
            (!text.is_empty()).then_some(text)
        })
        .collect::<Vec<_>>()
        .join(if compact { " " } else { "  " });
    (!title.is_empty()).then_some(title)
}

pub fn desired(preferences: &ResidentPreferences) -> Vec<DisplayId> {
    if !preferences.enabled {
        return Vec::new();
    }
    DisplayId::ALL
        .into_iter()
        .filter(|id| match id {
            DisplayId::CpuTemperature => preferences.shows_cpu_temperature(),
            DisplayId::GpuTemperature => preferences.shows_gpu_temperature(),
            DisplayId::DiskTemperature => preferences.shows_disk_temperature(),
            DisplayId::BatteryTemperature => {
                preferences.shows_battery_item(DisplayMetricId::BatteryTemperature)
            }
            DisplayId::SystemPower => preferences.shows_system_power(),
            DisplayId::ChargingPower => {
                preferences.shows_battery_item(DisplayMetricId::ChargingPower)
            }
            DisplayId::BatteryPower => {
                preferences.shows_battery_item(DisplayMetricId::BatteryPower)
            }
            id => match id.metric() {
                Some(metric) => preferences.shows(metric),
                None => preferences.effective_icon(),
            },
        })
        .collect()
}

#[cfg(any(windows, test))]
pub fn windows_desired(preferences: &ResidentPreferences, taskbar_active: bool) -> Vec<DisplayId> {
    if !preferences.enabled {
        return Vec::new();
    }
    if taskbar_active {
        if preferences.effective_icon() {
            vec![DisplayId::App]
        } else {
            Vec::new()
        }
    } else {
        desired(preferences)
    }
}

pub fn compact_rate(bytes: f64, base: f64) -> (String, &'static str) {
    if !bytes.is_finite() || bytes < 0.0 {
        return ("—".into(), "");
    }
    let units = ["B", "K", "M", "G", "T"];
    let mut number = bytes;
    let mut unit = 0;
    while number >= base - 0.5 && unit < units.len() - 1 {
        number /= base;
        unit += 1;
    }
    // Three digits fit at the normal numeric font size. Never shrink until an
    // arbitrary value fits; the last unit explicitly caps exceptionally large rates.
    (format!("{:.0}", number.min(999.0)), units[unit])
}

pub fn byte_text(bytes: f64, base: f64) -> String {
    byte_text_with_precision(bytes, base, 1)
}

fn byte_text_with_precision(bytes: f64, base: f64, precision: usize) -> String {
    let mut value = bytes;
    let units = ["B", "KB", "MB", "GB", "TB"];
    let mut index = 0;
    while value >= base && index < units.len() - 1 {
        value /= base;
        index += 1;
    }
    format!("{value:.precision$} {}", units[index])
}

/// Constant-width fields keep native status-bar layout stable across unit changes.
fn rate_title(bytes: f64, base: f64) -> String {
    let units = ["B", "KB", "MB", "GB", "TB"];
    let mut value = bytes;
    let mut index = 0;
    while (value * 10.0).round() / 10.0 >= base && index < units.len() - 1 {
        value /= base;
        index += 1;
    }
    format!("{:>5.1} {:>2}/s", value.min(999.9), units[index])
}

pub fn entries(
    preferences: &ResidentPreferences,
    values: &ResourceReadings,
    labels: &super::labels::Labels,
    base: f64,
) -> Vec<DisplayEntry> {
    let mut entries = Vec::new();
    for metric in &preferences.metrics {
        if !metric.enabled {
            continue;
        }
        if metric.id == DisplayMetricId::CpuTemperature {
            if preferences.shows_cpu_temperature() {
                entries.push(temperature_entry(values, labels));
            }
            continue;
        }
        if metric.id == DisplayMetricId::DiskTemperature {
            if preferences.shows_disk_temperature() {
                entries.push(disk_temperature_entry(
                    values,
                    labels,
                    preferences.disk_volume.as_deref(),
                ));
            }
            continue;
        }
        if metric.id == DisplayMetricId::GpuTemperature {
            if preferences.shows_gpu_temperature() {
                entries.push(gpu_temperature_entry(values, labels));
            }
            continue;
        }
        if matches!(
            metric.id,
            DisplayMetricId::BatteryTemperature
                | DisplayMetricId::ChargingPower
                | DisplayMetricId::BatteryPower
        ) {
            if values.battery_present != Some(false) && preferences.shows_battery_item(metric.id) {
                entries.push(battery_entry(values, labels, metric.id));
            }
            continue;
        }
        if metric.id == DisplayMetricId::SystemPower {
            if preferences.shows_system_power() {
                entries.push(system_power_entry(values, labels));
            }
            continue;
        }
        let Some(metric_id) = metric.id.metric() else {
            continue;
        };
        let (ids, status) = match metric_id {
            MetricId::Cpu => (vec![DisplayId::Cpu], values.cpu.status),
            MetricId::Gpu => (vec![DisplayId::Gpu], values.gpu.status),
            MetricId::Memory => (vec![DisplayId::Memory], values.memory.status),
            MetricId::Network => (
                vec![DisplayId::Upload, DisplayId::Download],
                values.network.status,
            ),
            MetricId::Disk => (vec![DisplayId::Disk], values.disk.status),
        };
        for id in ids {
            let (marker, value) = match id {
                DisplayId::Cpu => (
                    "C",
                    values.cpu.value.as_ref().map(|value| value.used_percent),
                ),
                DisplayId::Gpu => (
                    "G",
                    values.gpu.value.as_ref().map(|value| value.used_percent),
                ),
                DisplayId::Memory => (
                    "M",
                    values
                        .memory
                        .value
                        .as_ref()
                        .map(|value| f64::from(value.memory.used_percent)),
                ),
                DisplayId::Disk => (
                    "D",
                    values.disk.value.as_ref().map(|value| value.used_percent),
                ),
                DisplayId::Upload => (
                    "↑",
                    values
                        .network
                        .value
                        .as_ref()
                        .map(|value| value.transmitted_bytes_per_second),
                ),
                DisplayId::Download => (
                    "↓",
                    values
                        .network
                        .value
                        .as_ref()
                        .map(|value| value.received_bytes_per_second),
                ),
                DisplayId::App
                | DisplayId::CpuTemperature
                | DisplayId::GpuTemperature
                | DisplayId::DiskTemperature
                | DisplayId::BatteryTemperature
                | DisplayId::ChargingPower
                | DisplayId::BatteryPower
                | DisplayId::SystemPower => {
                    unreachable!()
                }
            };
            // Invalid samples must not become NaN%, infinity, or a misleading
            // valid-looking rate. Keep the normal unavailable presentation.
            let status = if status == MetricStatus::Ready
                && value.is_some_and(|value| {
                    !value.is_finite() || (metric_id == MetricId::Network && value < 0.0)
                }) {
                MetricStatus::Failed
            } else {
                status
            };
            let name = labels.metric(metric_id);
            if status != MetricStatus::Ready || value.is_none() {
                entries.push(DisplayEntry {
                    tone: Default::default(),
                    power_state: None,
                    color_value: None,
                    id,
                    marker: marker.into(),
                    digits: "—".into(),
                    text: format!(
                        "{} —",
                        match id {
                            DisplayId::Cpu => "CPU",
                            DisplayId::Gpu => "GPU",
                            DisplayId::Memory => "MEM",
                            DisplayId::Disk => "DISK",
                            _ => marker,
                        }
                    ),
                    tooltip: format!("{name} · {}", labels.status(status)),
                });
                continue;
            }
            let value = value.unwrap_or_default();
            let entry = if metric_id == MetricId::Network {
                let (digits, unit) = compact_rate(value, base);
                let speed = byte_text(value, base);
                let interface = values
                    .network
                    .value
                    .as_ref()
                    .map(|value| value.interface.name.as_str())
                    .unwrap_or("");
                DisplayEntry {
                    tone: Default::default(),
                    power_state: None,
                    color_value: None,
                    id,
                    marker: format!("{marker}{unit}"),
                    digits,
                    text: format!("{marker}{}", rate_title(value, base)),
                    tooltip: format!("{name} {marker} {speed}/s · {interface}"),
                }
            } else {
                // Display and threshold classification must share one rounded value.
                let percent = value.clamp(0.0, 100.0).round() as u8;
                let digits = percent.to_string();
                let details = match metric_id {
                    MetricId::Gpu => values
                        .gpu
                        .value
                        .as_ref()
                        .map(|value| {
                            format!(
                                " · {}",
                                mangodisk_platform::diagnostics::text(&value.adapter_name)
                            )
                        })
                        .unwrap_or_default(),
                    MetricId::Memory => values
                        .memory
                        .value
                        .as_ref()
                        .map(|value| {
                            format!(
                                " · {} / {}",
                                byte_text(value.memory.used_bytes as f64, 1024.0),
                                byte_text(value.memory.total_bytes as f64, 1024.0)
                            )
                        })
                        .unwrap_or_default(),
                    MetricId::Disk => values
                        .disk
                        .value
                        .as_ref()
                        .map(|value| {
                            format!(
                                " · {} · {} / {}",
                                if value.volume.system {
                                    labels.text("systemDisk")
                                } else {
                                    &value.volume.name
                                },
                                byte_text_with_precision(value.used_bytes as f64, base, 2),
                                byte_text_with_precision(value.total_bytes as f64, base, 2)
                            )
                        })
                        .unwrap_or_default(),
                    _ => String::new(),
                };
                let short = match metric_id {
                    MetricId::Cpu => "CPU",
                    MetricId::Gpu => "GPU",
                    MetricId::Memory => "MEM",
                    _ => "DISK",
                };
                DisplayEntry {
                    tone: Default::default(),
                    power_state: None,
                    color_value: Some(u16::from(percent)),
                    id,
                    marker: marker.into(),
                    text: format!("{short} {digits:>3}%"),
                    tooltip: format!("{name} {digits}%{details}"),
                    digits,
                }
            };
            entries.push(entry);
        }
    }
    entries
}

fn temperature_entry(values: &ResourceReadings, labels: &super::labels::Labels) -> DisplayEntry {
    use mangodisk_platform::system_resources::cpu::temperature::TEMPERATURE_FRESHNESS_MS;
    let reading = &values.cpu_temperature;
    let value = reading.value.as_ref().filter(|value| {
        reading.status == MetricStatus::Ready
            && reading.sampled_at_ms.is_some_and(|sampled| {
                sampled <= values.observed_at_ms
                    && values.observed_at_ms - sampled <= TEMPERATURE_FRESHNESS_MS
            })
            && value.celsius.is_finite()
            && value.celsius > 0.0
            && value.celsius <= 150.0
            && value.sensor_count > 0
    });
    let digits = value
        // Match the panel's positive half-degree rounding instead of Rust's
        // formatting rule, which rounds ties to the nearest even integer.
        .map(|value| format!("{:.0}", value.celsius.round()))
        .unwrap_or_else(|| "—".into());
    let tooltip = if value.is_some() {
        format!("{} {digits}°C", labels.temperature("label"))
    } else {
        let status = if reading.status == MetricStatus::Ready {
            MetricStatus::Stale
        } else {
            reading.status
        };
        format!(
            "{} · {}",
            labels.temperature("label"),
            labels.status(status)
        )
    };
    DisplayEntry {
        id: DisplayId::CpuTemperature,
        tone: Default::default(),
        power_state: None,
        // Rules are selected by metric ID, so Celsius keeps its own thresholds.
        color_value: value.map(|value| value.celsius.round() as u16),
        marker: "°C".into(),
        text: if value.is_some() {
            format!("CPU {digits}°C")
        } else {
            "CPU —".into()
        },
        digits,
        tooltip,
    }
}

fn gpu_temperature_entry(
    values: &ResourceReadings,
    labels: &super::labels::Labels,
) -> DisplayEntry {
    use mangodisk_platform::system_resources::gpu::temperature::GPU_TEMPERATURE_FRESHNESS_MS;
    let reading = &values.gpu_temperature;
    let value = reading.value.as_ref().filter(|value| {
        reading.status == MetricStatus::Ready
            && reading.sampled_at_ms.is_some_and(|sampled| {
                sampled <= values.observed_at_ms
                    && values.observed_at_ms - sampled <= GPU_TEMPERATURE_FRESHNESS_MS
            })
            && value.valid()
    });
    let digits = value
        .map(|v| format!("{:.0}", v.celsius.round()))
        .unwrap_or_else(|| "—".into());
    let tooltip = if let Some(value) = value {
        format!(
            "{} {digits}°C · {}",
            labels.gpu_temperature("label"),
            value.adapter_name
        )
    } else {
        format!(
            "{} · {}",
            labels.gpu_temperature("label"),
            labels.status(if reading.status == MetricStatus::Ready {
                MetricStatus::Stale
            } else {
                reading.status
            })
        )
    };
    DisplayEntry {
        id: DisplayId::GpuTemperature,
        tone: Default::default(),
        power_state: None,
        color_value: value.map(|value| value.celsius.round() as u16),
        marker: "°C".into(),
        text: if value.is_some() {
            format!("GPU {digits}°C")
        } else {
            "GPU —".into()
        },
        digits,
        tooltip,
    }
}

fn disk_temperature_entry(
    values: &ResourceReadings,
    labels: &super::labels::Labels,
    manual: Option<&str>,
) -> DisplayEntry {
    use mangodisk_platform::system_resources::disk_temperature::DISK_TEMPERATURE_FRESHNESS_MS;
    let reading = &values.disk_temperature;
    let selected = manual.or_else(|| {
        values
            .volumes
            .iter()
            .find(|volume| volume.system)
            .map(|volume| volume.id.as_str())
    });
    let value = reading.value.as_ref().filter(|value| {
        reading.status == MetricStatus::Ready
            && selected == Some(value.volume_id.as_str())
            && value.valid()
            && reading.sampled_at_ms.is_some_and(|sampled| {
                sampled <= values.observed_at_ms
                    && values.observed_at_ms - sampled <= DISK_TEMPERATURE_FRESHNESS_MS
            })
    });
    let digits = value
        .map(|value| format!("{:.0}", value.celsius.round()))
        .unwrap_or_else(|| "—".into());
    let tooltip = if let Some(value) = value {
        format!(
            "{} {digits}°C · {}",
            labels.text("diskTemperature"),
            value.volume_name
        )
    } else {
        format!(
            "{} · {}",
            labels.text("diskTemperature"),
            labels.status(
                if reading.status == MetricStatus::Ready
                    && reading
                        .value
                        .as_ref()
                        .is_some_and(|value| selected != Some(value.volume_id.as_str()))
                {
                    MetricStatus::Loading
                } else if reading.status == MetricStatus::Ready {
                    MetricStatus::Stale
                } else {
                    reading.status
                }
            )
        )
    };
    DisplayEntry {
        id: DisplayId::DiskTemperature,
        tone: Default::default(),
        power_state: None,
        color_value: value.map(|value| value.celsius.round() as u16),
        marker: "°C".into(),
        text: if value.is_some() {
            format!("DISK {digits}°C")
        } else {
            "DISK —".into()
        },
        digits,
        tooltip,
    }
}

fn displayed_power(value: f64) -> f64 {
    // Sub-watt readings display as a range, so integer color rules use its
    // lower bound rather than treating a nonzero reading as idle.
    if value < 1.0 {
        0.0
    } else {
        value.round()
    }
}

fn power_digits(value: f64) -> String {
    if value > 0.0 && value < 1.0 {
        "<1".into()
    } else {
        format!("{:.0}", value.round())
    }
}

fn system_power_entry(values: &ResourceReadings, labels: &super::labels::Labels) -> DisplayEntry {
    use mangodisk_platform::system_resources::power::{
        SYSTEM_POWER_FRESHNESS_MS, SYSTEM_POWER_MAX_WATTS,
    };
    let reading = &values.system_power;
    let value = reading
        .value
        .as_ref()
        .filter(|value| {
            reading.status == MetricStatus::Ready
                && value.watts.is_finite()
                && (0.0..=SYSTEM_POWER_MAX_WATTS).contains(&value.watts)
                && reading
                    .sampled_at_ms
                    .and_then(|at| values.observed_at_ms.checked_sub(at))
                    .is_some_and(|age| age <= SYSTEM_POWER_FRESHNESS_MS)
        })
        .map(|value| value.watts);
    let displayed = value.map(displayed_power);
    let digits = value.map(power_digits).unwrap_or_else(|| "—".into());
    let name = labels.text("systemPower");
    let text = if displayed.is_some() {
        format!("{name} {digits}W")
    } else {
        format!("{name} —")
    };
    let tooltip = if let Some(value) = value {
        format!("{name} {value:.1}W")
    } else {
        format!(
            "{name} · {}",
            labels.status(if reading.status == MetricStatus::Ready {
                MetricStatus::Stale
            } else {
                reading.status
            })
        )
    };
    DisplayEntry {
        id: DisplayId::SystemPower,
        tone: Default::default(),
        power_state: None,
        color_value: displayed.map(|value| value.floor() as u16),
        marker: "W".into(),
        digits,
        text,
        tooltip,
    }
}

fn battery_entry(
    values: &ResourceReadings,
    labels: &super::labels::Labels,
    id: DisplayMetricId,
) -> DisplayEntry {
    use mangodisk_platform::system_resources::battery::BATTERY_FRESHNESS_MS;
    let temperature = id == DisplayMetricId::BatteryTemperature;
    let battery_power = id == DisplayMetricId::BatteryPower;
    let (status, sampled_at, value) = if temperature {
        (
            values.battery_temperature.status,
            values.battery_temperature.sampled_at_ms,
            values
                .battery_temperature
                .value
                .as_ref()
                .map(|value| value.celsius),
        )
    } else if battery_power {
        (
            values.battery_power.status,
            values.battery_power.sampled_at_ms,
            values.battery_power.value.as_ref().map(|value| value.watts),
        )
    } else {
        (
            values.charging_power.status,
            values.charging_power.sampled_at_ms,
            values
                .charging_power
                .value
                .as_ref()
                .map(|value| value.watts),
        )
    };
    let value = value.filter(|value| {
        status == MetricStatus::Ready
            && value.is_finite()
            && (0.0..=if temperature { 100.0 } else { 300.0 }).contains(value)
            && sampled_at
                .and_then(|at| values.observed_at_ms.checked_sub(at))
                .is_some_and(|age| age <= BATTERY_FRESHNESS_MS)
    });
    let displayed = value.map(|value| {
        if temperature {
            value.round()
        } else {
            displayed_power(value)
        }
    });
    let digits = value
        .map(|value| {
            if temperature {
                format!("{:.0}", value.round())
            } else {
                power_digits(value)
            }
        })
        .unwrap_or_else(|| "—".into());
    let unit = if temperature { "°C" } else { "W" };
    let name = labels.text(if temperature {
        "batteryTemperature"
    } else if battery_power {
        "batteryPower"
    } else {
        "chargingPower"
    });
    let power_state = battery_power
        .then(|| values.battery_power.value.as_ref().map(|value| value.state))
        .flatten()
        .filter(|_| value.is_some());
    let state_name = power_state.map(|state| {
        use mangodisk_platform::system_resources::battery::BatteryPowerState;
        labels.text(match state {
            BatteryPowerState::Charging => "batteryCharging",
            BatteryPowerState::Discharging => "batteryDischarging",
            BatteryPowerState::Idle => "batteryIdle",
        })
    });
    let format_reading = |digits: &str| {
        let reading = format!("{name} {digits}{unit}");
        if let Some(state) = state_name {
            format!("{reading} · {state}")
        } else {
            reading
        }
    };
    let text = if value.is_some() {
        format_reading(&digits)
    } else {
        format!("{name} —")
    };
    let tooltip = if let Some(value) = value {
        if temperature {
            text.clone()
        } else {
            format_reading(&format!("{value:.1}"))
        }
    } else {
        format!(
            "{name} · {}",
            labels.status(if status == MetricStatus::Ready {
                MetricStatus::Stale
            } else {
                status
            })
        )
    };
    DisplayEntry {
        id: if temperature {
            DisplayId::BatteryTemperature
        } else if battery_power {
            DisplayId::BatteryPower
        } else {
            DisplayId::ChargingPower
        },
        tone: Default::default(),
        power_state,
        color_value: displayed.map(|value| value.floor() as u16),
        marker: unit.into(),
        digits,
        text,
        tooltip,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn disk_temperature_formats_fresh_samples_without_usage_or_history() {
        use mangodisk_core::system_resources::metrics::MetricReading;
        use mangodisk_platform::system_resources::disk_temperature::{
            DiskTemperature, DiskTemperatureSource,
        };
        let labels = super::super::labels::Labels::for_locale("en-US");
        let mut readings = ResourceReadings {
            observed_at_ms: 1000,
            volumes: vec![mangodisk_platform::system_resources::disk::ResourceVolume {
                id: "disk".into(),
                name: "System".into(),
                system: true,
                mount_point: String::new(),
            }],
            disk_temperature: MetricReading::ready(
                DiskTemperature {
                    volume_id: "disk".into(),
                    volume_name: "System".into(),
                    celsius: 34.5,
                    source: DiskTemperatureSource::NvmeSmart,
                },
                1000,
            ),
            ..Default::default()
        };
        let entry = disk_temperature_entry(&readings, &labels, None);
        assert_eq!(entry.digits, "35");
        assert_eq!(entry.text, "DISK 35°C");
        assert_eq!(entry.tooltip, "Disk temperature 35°C · System");
        assert_eq!(
            disk_temperature_entry(&readings, &labels, Some("another-disk")).digits,
            "—"
        );
        assert_eq!(
            disk_temperature_entry(&readings, &labels, Some("disk")).digits,
            "35"
        );
        readings.observed_at_ms = 91_001;
        assert_eq!(disk_temperature_entry(&readings, &labels, None).digits, "—");
        readings.observed_at_ms = 999;
        assert_eq!(disk_temperature_entry(&readings, &labels, None).digits, "—");
        readings.observed_at_ms = 1000;
        for status in [
            MetricStatus::Loading,
            MetricStatus::Failed,
            MetricStatus::Disconnected,
            MetricStatus::Unsupported,
        ] {
            readings.disk_temperature.status = status;
            assert_eq!(disk_temperature_entry(&readings, &labels, None).digits, "—");
        }
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn physical_absence_hides_selected_battery_items_without_changing_preferences() {
        let mut preferences = ResidentPreferences::default();
        for metric in &mut preferences.metrics {
            metric.enabled = matches!(
                metric.id,
                DisplayMetricId::BatteryTemperature
                    | DisplayMetricId::ChargingPower
                    | DisplayMetricId::BatteryPower
                    | DisplayMetricId::SystemPower
            );
        }
        let before = preferences.clone();
        let mut readings = ResourceReadings {
            battery_present: Some(false),
            ..Default::default()
        };
        let labels = super::super::labels::Labels::for_locale("en-US");
        let visible = entries(&preferences, &readings, &labels, 1000.0);
        assert_eq!(visible.len(), 1);
        assert_eq!(visible[0].id, DisplayId::SystemPower);
        for present in [None, Some(true)] {
            readings.battery_present = present;
            readings.battery_temperature.status = MetricStatus::Unsupported;
            assert_eq!(entries(&preferences, &readings, &labels, 1000.0).len(), 4);
        }
        assert_eq!(preferences, before);
    }
    #[test]
    fn power_precision_preserves_sub_watt_ranges_and_rounds_once() {
        use mangodisk_core::system_resources::metrics::MetricReading;
        use mangodisk_platform::system_resources::battery::{
            BatteryPower, BatteryPowerState, BatterySource, ChargingPower,
        };
        let labels = super::super::labels::Labels::for_locale("en-US");
        for (watts, expected) in [
            (0.0, "0"),
            (0.01, "<1"),
            (0.99, "<1"),
            (1.0, "1"),
            (19.8, "20"),
            (99.94, "100"),
            (134.49, "134"),
            (134.5, "135"),
            (99.96, "100"),
            (100.0, "100"),
            (135.46, "135"),
            (135.5, "136"),
            (300.0, "300"),
        ] {
            let values = ResourceReadings {
                observed_at_ms: 1000,
                system_power: MetricReading::ready(
                    mangodisk_platform::system_resources::power::SystemPower { watts },
                    1000,
                ),
                charging_power: MetricReading::ready(
                    ChargingPower {
                        watts,
                        charging: true,
                        external_connected: true,
                        source: BatterySource::AppleSmartBattery,
                    },
                    1000,
                ),
                battery_power: MetricReading::ready(
                    BatteryPower {
                        watts,
                        state: BatteryPowerState::Charging,
                        source: BatterySource::AppleSmartBattery,
                    },
                    1000,
                ),
                ..Default::default()
            };
            for id in [
                DisplayMetricId::ChargingPower,
                DisplayMetricId::BatteryPower,
            ] {
                let entry = battery_entry(&values, &labels, id);
                assert_eq!(entry.digits, expected, "{id:?} at {watts} W");
                assert!(entry.tooltip.contains(&format!("{watts:.1}W")));
                assert_eq!(entry.color_value, Some(displayed_power(watts) as u16));
            }
            let entry = system_power_entry(&values, &labels);
            assert_eq!(entry.digits, expected, "system power at {watts} W");
            assert!(entry.tooltip.contains(&format!("{watts:.1}W")));
        }
    }

    #[test]
    fn battery_power_preserves_direction_and_clears_it_when_stale() {
        use mangodisk_core::system_resources::metrics::MetricReading;
        use mangodisk_platform::system_resources::battery::{
            BatteryPower, BatteryPowerState, BatterySource,
        };
        for (state, watts, heading) in [
            (BatteryPowerState::Charging, 75.0, "BAT"),
            (BatteryPowerState::Discharging, 24.0, "BAT"),
            (BatteryPowerState::Idle, 0.0, "IDLE"),
        ] {
            let mut values = ResourceReadings {
                observed_at_ms: 1000,
                battery_power: MetricReading::ready(
                    BatteryPower {
                        watts,
                        state,
                        source: BatterySource::AppleSmartBattery,
                    },
                    1000,
                ),
                ..Default::default()
            };
            let labels = super::super::labels::Labels::for_locale("en-US");
            let entry = battery_entry(&values, &labels, DisplayMetricId::BatteryPower);
            assert_eq!(entry.power_state, Some(state));
            let column = &super::super::macos_presentation::columns(&[entry], true)[0];
            assert_eq!(column.top, heading);
            assert_eq!(column.power_state, Some(state));
            values.observed_at_ms = 6001;
            let expired = battery_entry(&values, &labels, DisplayMetricId::BatteryPower);
            assert_eq!(expired.digits, "—");
            assert_eq!(expired.power_state, None);
        }
    }

    #[test]
    fn battery_temperature_rounding_matches_the_other_temperature_items() {
        use mangodisk_core::system_resources::metrics::MetricReading;
        use mangodisk_platform::system_resources::battery::{BatterySource, BatteryTemperature};
        let values = ResourceReadings {
            observed_at_ms: 1000,
            battery_temperature: MetricReading::ready(
                BatteryTemperature {
                    celsius: 48.5,
                    source: BatterySource::AppleSmc,
                },
                1000,
            ),
            ..Default::default()
        };
        let entry = battery_entry(
            &values,
            &super::super::labels::Labels::for_locale("en-US"),
            DisplayMetricId::BatteryTemperature,
        );
        assert_eq!(entry.digits, "49");
        assert_eq!(entry.color_value, Some(49));
    }
    #[test]
    fn charging_color_thresholds_match_the_displayed_integer_and_expire() {
        use mangodisk_core::system_resources::metrics::MetricReading;
        use mangodisk_platform::system_resources::battery::{BatterySource, ChargingPower};
        let mut values = ResourceReadings {
            observed_at_ms: 1000,
            charging_power: MetricReading::ready(
                ChargingPower {
                    watts: 49.99,
                    charging: true,
                    external_connected: true,
                    source: BatterySource::AppleSmc,
                },
                1000,
            ),
            ..Default::default()
        };
        let labels = super::super::labels::Labels::for_locale("en-US");
        let entry = battery_entry(&values, &labels, DisplayMetricId::ChargingPower);
        assert_eq!(entry.digits, "50");
        assert_eq!(entry.color_value, Some(50));
        values.observed_at_ms = 6001;
        assert_eq!(
            battery_entry(&values, &labels, DisplayMetricId::ChargingPower).digits,
            "—"
        );
        values.observed_at_ms = 999;
        assert_eq!(
            battery_entry(&values, &labels, DisplayMetricId::ChargingPower).color_value,
            None
        );
    }

    #[test]
    fn gpu_temperature_preserves_celsius_and_rounded_color_value() {
        use mangodisk_core::system_resources::metrics::MetricReading;
        use mangodisk_platform::system_resources::gpu::temperature::{
            GpuTemperature, GpuTemperatureKind, GpuTemperatureSource,
        };
        let labels = super::super::labels::Labels::for_locale("en-US");
        let mut readings = ResourceReadings {
            observed_at_ms: 1000,
            gpu_temperature: MetricReading::ready(
                GpuTemperature {
                    adapter_id: "a".into(),
                    adapter_name: "Apple M3 Max".into(),
                    celsius: 48.5,
                    kind: GpuTemperatureKind::SensorAverage,
                    source: GpuTemperatureSource::AppleSmc,
                    sensor_count: 8,
                },
                1000,
            ),
            ..Default::default()
        };
        let mut prefs = ResidentPreferences {
            show_icon: false,
            ..Default::default()
        };
        for metric in &mut prefs.metrics {
            metric.enabled = metric.id == DisplayMetricId::GpuTemperature;
        }
        let entry = gpu_temperature_entry(&readings, &labels);
        assert_eq!(entry.text, "GPU 49°C");
        assert_eq!(entry.color_value, Some(49));
        assert!(entry.tooltip.contains("Apple M3 Max"));
        assert_eq!(
            entries(&prefs, &readings, &labels, 1000.0).len(),
            usize::from(cfg!(any(target_os = "macos", windows)))
        );
        readings.observed_at_ms = 11001;
        assert_eq!(gpu_temperature_entry(&readings, &labels).digits, "—");
        readings.observed_at_ms = 999;
        assert_eq!(gpu_temperature_entry(&readings, &labels).digits, "—");
    }

    #[test]
    fn temperature_rounding_matches_panel_at_half_degree_boundaries() {
        use mangodisk_core::system_resources::metrics::MetricReading;
        use mangodisk_platform::system_resources::cpu::temperature::{
            CpuTemperature, CpuTemperatureKind, CpuTemperatureSource,
        };
        let labels = super::super::labels::Labels::for_locale("en-US");
        for (celsius, expected) in [(48.5, "49"), (49.5, "50"), (50.5, "51"), (148.5, "149")] {
            let readings = ResourceReadings {
                observed_at_ms: 1_000,
                cpu_temperature: MetricReading::ready(
                    CpuTemperature {
                        celsius,
                        kind: CpuTemperatureKind::CoreAverage,
                        source: CpuTemperatureSource::AppleSmc,
                        sensor_count: 16,
                        sampled_at_ms: None,
                    },
                    1_000,
                ),
                ..Default::default()
            };
            let entry = temperature_entry(&readings, &labels);
            assert_eq!(entry.digits, expected, "celsius={celsius}");
            assert!(entry.tooltip.contains(&format!("{expected}°C")));
        }
    }

    #[test]
    fn temperature_formatting_rejects_expired_invalid_and_future_samples() {
        use mangodisk_core::system_resources::metrics::MetricReading;
        use mangodisk_platform::system_resources::cpu::temperature::{
            CpuTemperature, CpuTemperatureKind, CpuTemperatureSource,
        };
        let mut readings = ResourceReadings {
            observed_at_ms: 5_000,
            cpu_temperature: MetricReading::ready(
                CpuTemperature {
                    celsius: 49.4,
                    kind: CpuTemperatureKind::CoreAverage,
                    source: CpuTemperatureSource::AppleSmc,
                    sensor_count: 16,
                    sampled_at_ms: None,
                },
                1_000,
            ),
            ..Default::default()
        };
        let labels = super::super::labels::Labels::for_locale("en-US");
        let ready = temperature_entry(&readings, &labels);
        assert_eq!(ready.digits, "49");
        assert_eq!(ready.tooltip, "CPU temperature 49°C");
        assert_eq!(ready.color_value, Some(49));
        assert_eq!(ready.tone, Default::default());
        for status in [
            MetricStatus::Stale,
            MetricStatus::Failed,
            MetricStatus::Loading,
            MetricStatus::Unsupported,
        ] {
            readings.cpu_temperature.status = status;
            assert_eq!(temperature_entry(&readings, &labels).digits, "—");
        }
        readings.cpu_temperature.status = MetricStatus::Ready;
        for timestamp in [0, 999, 11_001] {
            readings.observed_at_ms = timestamp;
            assert_eq!(temperature_entry(&readings, &labels).digits, "—");
        }
        readings.observed_at_ms = 11_000;
        assert_eq!(temperature_entry(&readings, &labels).digits, "49");
        for celsius in [f64::NAN, f64::INFINITY, 0.0, -1.0, 151.0] {
            readings.cpu_temperature.value.as_mut().unwrap().celsius = celsius;
            assert_eq!(temperature_entry(&readings, &labels).digits, "—");
        }
        readings.cpu_temperature.value.as_mut().unwrap().celsius = 49.0;
        readings
            .cpu_temperature
            .value
            .as_mut()
            .unwrap()
            .sensor_count = 0;
        assert_eq!(temperature_entry(&readings, &labels).digits, "—");
    }

    #[test]
    fn temperature_selection_is_platform_gated_and_independent_of_cpu() {
        let mut prefs = ResidentPreferences {
            show_icon: false,
            ..Default::default()
        };
        for metric in &mut prefs.metrics {
            metric.enabled = metric.id == DisplayMetricId::CpuTemperature;
        }
        prefs.metrics.reverse();
        let ids = desired(&prefs);
        if cfg!(target_os = "macos") {
            assert_eq!(ids, [DisplayId::CpuTemperature]);
            let display = entries(
                &prefs,
                &ResourceReadings::default(),
                &super::super::labels::Labels::for_locale("zh-CN"),
                1024.0,
            );
            assert_eq!(display.len(), 1);
            assert_eq!(display[0].id.metric(), Some(MetricId::Cpu));
            assert!(display[0].tooltip.starts_with(
                super::super::labels::Labels::for_locale("zh-CN").temperature("label")
            ));
        } else {
            assert_eq!(ids, [DisplayId::App]);
            assert!(entries(
                &prefs,
                &ResourceReadings::default(),
                &super::super::labels::Labels::for_locale("en-US"),
                1024.0
            )
            .is_empty());
        }
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn temperature_entries_follow_saved_order_including_without_cpu_usage() {
        let labels = super::super::labels::Labels::for_locale("en-US");
        let mut prefs = ResidentPreferences::default();
        for metric in &mut prefs.metrics {
            metric.enabled = matches!(
                metric.id,
                DisplayMetricId::CpuTemperature | DisplayMetricId::Memory | DisplayMetricId::Cpu
            );
        }
        prefs.metrics.reverse();
        let ids = entries(&prefs, &ResourceReadings::default(), &labels, 1024.0)
            .into_iter()
            .map(|entry| entry.id)
            .collect::<Vec<_>>();
        assert_eq!(
            ids,
            [DisplayId::Memory, DisplayId::CpuTemperature, DisplayId::Cpu]
        );
        prefs
            .metrics
            .iter_mut()
            .find(|metric| metric.id == DisplayMetricId::Cpu)
            .unwrap()
            .enabled = false;
        let ids = entries(&prefs, &ResourceReadings::default(), &labels, 1024.0)
            .into_iter()
            .map(|entry| entry.id)
            .collect::<Vec<_>>();
        assert_eq!(ids, [DisplayId::Memory, DisplayId::CpuTemperature]);
    }

    #[cfg(target_os = "linux")]
    fn display_entry(id: DisplayId, text: &str) -> DisplayEntry {
        DisplayEntry {
            id,
            tone: Default::default(),
            power_state: None,
            color_value: None,
            marker: String::new(),
            digits: String::new(),
            text: text.to_string(),
            tooltip: String::new(),
        }
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn indicator_title_contains_only_non_empty_enabled_metrics() {
        assert_eq!(indicator_title(&[], false), None);
        assert_eq!(
            indicator_title(
                &[
                    display_entry(DisplayId::Cpu, "CPU  12%"),
                    display_entry(DisplayId::Memory, "MEM  48%"),
                    display_entry(DisplayId::Disk, ""),
                ],
                false,
            ),
            Some("CPU  12%  MEM  48%".to_string())
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn compact_indicator_title_reduces_width_and_keeps_values() {
        let mut cpu = display_entry(DisplayId::Cpu, "CPU  12%");
        cpu.marker = "C".into();
        cpu.digits = "12".into();
        let mut memory = display_entry(DisplayId::Memory, "MEM  48%");
        memory.marker = "M".into();
        memory.digits = "48".into();
        let mut upload = display_entry(DisplayId::Upload, "↑ 82.0 KB/s");
        upload.marker = "↑K".into();
        upload.digits = "82".into();

        let standard =
            indicator_title(&[cpu.clone(), memory.clone(), upload.clone()], false).unwrap();
        let compact = indicator_title(&[cpu, memory, upload], true).unwrap();

        assert_eq!(compact, "C12% M48% ↑82K");
        assert!(compact.len() < standard.len());
    }

    #[test]
    fn all_display_combinations_have_unique_entries_and_paired_network_directions() {
        for bits in 0..(1 << ResidentPreferences::default().metrics.len()) {
            for show_icon in [false, true] {
                let mut prefs = ResidentPreferences {
                    show_icon,
                    ..Default::default()
                };
                for (index, metric) in prefs.metrics.iter_mut().enumerate() {
                    metric.enabled = bits & (1 << index) != 0;
                }
                let ids = desired(&prefs);
                assert_eq!(
                    ids.iter().collect::<std::collections::HashSet<_>>().len(),
                    ids.len()
                );
                assert!(!ids.is_empty());
                assert!(ids.len() <= DisplayId::ALL.len());
                assert_eq!(
                    ids.contains(&DisplayId::Upload),
                    prefs.shows(MetricId::Network)
                );
                assert_eq!(
                    ids.contains(&DisplayId::Download),
                    prefs.shows(MetricId::Network)
                );
                assert_eq!(ids.contains(&DisplayId::App), prefs.effective_icon());
                prefs.enabled = false;
                assert!(desired(&prefs).is_empty());
            }
        }
    }

    #[test]
    fn disk_tooltips_use_shared_capacity_precision_without_explanatory_text() {
        use mangodisk_core::system_resources::{disk, metrics::MetricReading};
        use mangodisk_platform::system_resources::disk::{ResourceVolume, VolumeCapacity};
        let value = disk::usage(
            ResourceVolume {
                id: "test".into(),
                name: "Test".into(),
                system: true,
                mount_point: "/".into(),
            },
            VolumeCapacity {
                total_bytes: 100_000_000_000,
                available_bytes: 40_000_000_000,
            },
        )
        .unwrap();
        let readings = ResourceReadings {
            disk: MetricReading::ready(value, 0),
            ..Default::default()
        };
        let mut preferences = ResidentPreferences::default();
        for metric in &mut preferences.metrics {
            metric.enabled = metric.id == DisplayMetricId::Disk;
        }
        for locale in [
            "en-US", "zh-CN", "zh-TW", "ja-JP", "ko-KR", "tr-TR", "pt-BR",
        ] {
            let labels = super::super::labels::Labels::for_locale(locale);
            let entries = entries(&preferences, &readings, &labels, 1000.0);
            let disk = entries
                .iter()
                .find(|entry| entry.id == DisplayId::Disk)
                .unwrap();
            assert_eq!(disk.color_value, Some(60));
            assert!(disk.tooltip.contains("60.00 GB / 100.00 GB"));
            assert!(!disk.tooltip.contains(labels.text("diskCapacityHint")));
            assert!(!disk.tooltip.contains('\n'));
        }
    }

    #[test]
    fn taskbar_fallback_restores_all_metrics_and_never_loses_the_last_entry() {
        for bits in 0..(1 << ResidentPreferences::default().metrics.len()) {
            for show_icon in [false, true] {
                let mut prefs = ResidentPreferences {
                    show_icon,
                    ..Default::default()
                };
                for (i, item) in prefs.metrics.iter_mut().enumerate() {
                    item.enabled = bits & (1 << i) != 0;
                }
                assert_eq!(windows_desired(&prefs, false), desired(&prefs));
                let active = windows_desired(&prefs, true);
                assert_eq!(active.contains(&DisplayId::App), prefs.effective_icon());
                assert!(active.len() <= 1);
                prefs.enabled = false;
                assert!(windows_desired(&prefs, true).is_empty());
                assert!(windows_desired(&prefs, false).is_empty());
            }
        }
    }
    #[test]
    fn native_ids_are_stable_and_network_contributes_two_entries() {
        let mut prefs = ResidentPreferences::default();
        for metric in &mut prefs.metrics {
            metric.enabled = true;
        }
        assert_eq!(
            desired(&prefs),
            DisplayId::ALL
                .into_iter()
                .filter(|id| {
                    (cfg!(target_os = "macos") || !matches!(id, DisplayId::CpuTemperature))
                        && (cfg!(any(target_os = "macos", windows))
                            || !matches!(
                                id,
                                DisplayId::GpuTemperature | DisplayId::DiskTemperature
                            ))
                        && (cfg!(target_os = "macos")
                            || !matches!(
                                id,
                                DisplayId::BatteryTemperature
                                    | DisplayId::ChargingPower
                                    | DisplayId::BatteryPower
                                    | DisplayId::SystemPower
                            ))
                })
                .collect::<Vec<_>>()
        );
        assert_eq!(DisplayId::Upload.metric(), DisplayId::Download.metric());
        prefs.enabled = false;
        assert!(desired(&prefs).is_empty());
    }
    #[test]
    fn invalid_ready_samples_use_the_unavailable_presentation() {
        use mangodisk_core::system_resources::metrics::{CpuUsage, MetricReading};
        let mut preferences = ResidentPreferences::default();
        for metric in &mut preferences.metrics {
            metric.enabled = metric.id == DisplayMetricId::Cpu;
        }
        let labels = super::super::labels::Labels::for_locale("en-US");
        for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            let readings = ResourceReadings {
                cpu: MetricReading::ready(
                    CpuUsage {
                        used_percent: value,
                    },
                    0,
                ),
                ..Default::default()
            };
            let entries = entries(&preferences, &readings, &labels, 1000.0);
            assert_eq!(entries[0].digits, "—");
            assert!(!entries[0].tooltip.contains("NaN"));
            assert!(!entries[0].tooltip.contains("inf"));
        }
    }

    #[test]
    fn displayed_percentages_and_color_thresholds_use_the_same_rounding() {
        use super::super::usage_color::UsageTone;
        use mangodisk_core::system_resources::metrics::{CpuUsage, MetricReading};
        let preferences = ResidentPreferences::default();
        let labels = super::super::labels::Labels::for_locale("en-US");
        for (value, expected, tone) in [
            (74.49, 74, UsageTone::Normal),
            (74.5, 75, UsageTone::Warning),
            (84.5, 85, UsageTone::Critical),
            (100.5, 100, UsageTone::Critical),
            (-0.5, 0, UsageTone::Normal),
        ] {
            let readings = ResourceReadings {
                cpu: MetricReading::ready(
                    CpuUsage {
                        used_percent: value,
                    },
                    0,
                ),
                ..Default::default()
            };
            let entries = entries(&preferences, &readings, &labels, 1000.0);
            let cpu = entries
                .iter()
                .find(|entry| entry.id == DisplayId::Cpu)
                .unwrap();
            assert_eq!(cpu.digits, expected.to_string());
            assert_eq!(cpu.color_value, Some(expected));
            let mut rule = crate::resident::color_rules::MetricColorRule::defaults(
                crate::resident::preference_schema::DisplayMetricId::Cpu,
            );
            rule.rules = crate::resident::color_rules::legacy(75, 85)
                .into_iter()
                .find(|updated| updated.id == rule.id)
                .unwrap()
                .rules;
            let matched =
                super::super::usage_color::next_rule(None, cpu.color_value, &rule).unwrap();
            assert_eq!(UsageTone::from(rule.rules[matched].color()), tone);
        }
    }

    #[test]
    fn network_unit_boundaries_stay_within_three_digits() {
        let units = ["B", "K", "M", "G", "T"];
        for base in [1000.0_f64, 1024.0] {
            for index in 0..4 {
                let scale = base.powi(index as i32);
                assert_eq!(compact_rate((base - 0.51) * scale, base).1, units[index]);
                assert_eq!(
                    compact_rate((base - 0.49) * scale, base),
                    ("1".into(), units[index + 1])
                );
            }
            for value in [
                0.0,
                0.49,
                0.51,
                9.49,
                9.51,
                99.49,
                99.51,
                999.49,
                999.51,
                f64::MAX,
            ] {
                assert!(compact_rate(value, base).0.len() <= 3);
            }
        }
        for invalid in [-1.0, f64::NAN, f64::INFINITY] {
            assert_eq!(compact_rate(invalid, 1000.0).0, "—");
        }
    }

    #[test]
    fn compact_rates_keep_units_and_three_digit_bounds() {
        assert_eq!(compact_rate(0.0, 1000.0), ("0".into(), "B"));
        assert_eq!(compact_rate(999_000.0, 1000.0), ("999".into(), "K"));
        assert_eq!(compact_rate(1_000_000.0, 1000.0), ("1".into(), "M"));
        assert_eq!(compact_rate(f64::NAN, 1000.0).0, "—");
        assert_eq!(compact_rate(1024.0, 1024.0), ("1".into(), "K"));
    }
    #[test]
    fn menu_bar_rate_fields_keep_their_width_across_rounding_and_unit_changes() {
        for base in [1000.0, 1024.0] {
            for value in [0.0, 99.9, 999.94, 999.95, 1023.96, 1_000_000.0, f64::MAX] {
                assert_eq!(rate_title(value, base).chars().count(), 10);
            }
        }
        assert_eq!(rate_title(999.95, 1000.0), "  1.0 KB/s");
        assert_eq!(rate_title(0.0, 1000.0), "  0.0  B/s");
    }
}
