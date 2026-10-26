//! Read-only physical-device health. Missing vendor data is never a healthy zero.
use super::{disk::ResourceVolume, disk_temperature::DiskTemperatureSource};
use crate::{PlatformError, PlatformErrorCode, PlatformResult};
use serde::Serialize;

#[cfg(any(target_os = "macos", windows, test))]
pub(super) mod ata;
pub use ata_types::{
    AtaSmart, AtaSmartAttribute, AtaSmartMetric, AtaSmartProfile, AtaSmartUnit, AtaSmartValue,
};
mod ata_types;

pub const DISK_HEALTH_FRESHNESS_MS: u64 = 600_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum DiskHealthStatus {
    Healthy,
    Warning,
    Unhealthy,
    Unknown,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiskHealth {
    pub volume_id: String,
    pub volume_name: String,
    pub status: DiskHealthStatus,
    pub source: DiskTemperatureSource,
    pub percentage_used: Option<u8>,
    /// Decimal strings preserve NVMe's 128-bit lifetime counters across JSON.
    pub total_written_bytes: Option<String>,
    pub total_read_bytes: Option<String>,
    pub power_cycles: Option<u64>,
    pub available_spare: Option<u8>,
    pub critical_warning: Option<u8>,
    pub power_on_hours: Option<u64>,
    pub media_errors: Option<String>,
    pub temperature_celsius: Option<f64>,
    pub ata: Option<Box<AtaSmart>>,
    pub nvme: Option<Box<NvmeSmart>>,
}

/// Additional standardized NVMe log fields, kept distinct from ATA attributes.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NvmeSmart {
    pub endurance_group_critical_warning: u8,
    pub available_spare_threshold: u8,
    pub data_units_read: String,
    pub data_units_written: String,
    pub power_cycles: String,
    pub power_on_hours: String,
    pub host_read_commands: String,
    pub host_write_commands: String,
    pub controller_busy_minutes: String,
    pub unsafe_shutdowns: String,
    pub error_information_log_entries: String,
    pub warning_temperature_minutes: u32,
    pub critical_temperature_minutes: u32,
    pub temperature_sensors_celsius: [Option<f64>; 8],
    pub thermal_management_transition_counts: [u32; 2],
    pub thermal_management_total_seconds: [u32; 2],
}

#[cfg(any(target_os = "macos", windows, test))]
impl DiskHealth {
    pub(super) fn basic(
        volume: &ResourceVolume,
        status: DiskHealthStatus,
        source: DiskTemperatureSource,
    ) -> Self {
        Self {
            volume_id: volume.id.clone(),
            volume_name: volume.name.clone(),
            status,
            source,
            percentage_used: None,
            total_written_bytes: None,
            total_read_bytes: None,
            power_cycles: None,
            available_spare: None,
            critical_warning: None,
            power_on_hours: None,
            media_errors: None,
            temperature_celsius: None,
            ata: None,
            nvme: None,
        }
    }
}

pub fn read(volume: &ResourceVolume) -> PlatformResult<DiskHealth> {
    #[cfg(any(target_os = "macos", windows))]
    {
        super::disk_smart::read_health(volume)
    }
    #[cfg(not(any(target_os = "macos", windows)))]
    {
        let _ = volume;
        Err(PlatformError::new(
            PlatformErrorCode::Unsupported,
            "disk_health platform unsupported",
        ))
    }
}

#[cfg(any(target_os = "macos", windows, test))]
pub(super) fn nvme(volume: &ResourceVolume, bytes: &[u8]) -> PlatformResult<DiskHealth> {
    if bytes.len() != 512 || bytes.iter().all(|byte| *byte == 0) || bytes[3] > 100 || bytes[4] > 100
    {
        return Err(PlatformError::new(
            PlatformErrorCode::InvalidData,
            "disk_health stage=nvme_parse invalid_record",
        ));
    }
    let counter = |offset| u128::from_le_bytes(bytes[offset..offset + 16].try_into().unwrap());
    let status = if bytes[0] & 8 != 0 {
        DiskHealthStatus::Unhealthy
    } else if bytes[0] != 0 {
        DiskHealthStatus::Warning
    } else {
        DiskHealthStatus::Healthy
    };
    let mut result = DiskHealth::basic(volume, status, DiskTemperatureSource::NvmeSmart);
    result.percentage_used = Some(bytes[5]);
    // Zero data units explicitly means that the controller does not report this counter.
    result.total_written_bytes = (counter(48) != 0)
        .then(|| counter(48))
        .and_then(|value| value.checked_mul(512_000).map(|value| value.to_string()));
    result.total_read_bytes = (counter(32) != 0)
        .then(|| counter(32))
        .and_then(|value| value.checked_mul(512_000).map(|value| value.to_string()));
    result.power_cycles = counter(112).try_into().ok();
    result.available_spare = Some(bytes[3]);
    result.critical_warning = Some(bytes[0]);
    result.power_on_hours = counter(128).try_into().ok();
    result.media_errors = Some(counter(160).to_string());
    result.temperature_celsius = super::disk_temperature::nvme_temperature(bytes).ok();
    let word = |offset| u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap());
    result.nvme = Some(Box::new(NvmeSmart {
        endurance_group_critical_warning: bytes[6],
        available_spare_threshold: bytes[4],
        data_units_read: counter(32).to_string(),
        data_units_written: counter(48).to_string(),
        power_cycles: counter(112).to_string(),
        power_on_hours: counter(128).to_string(),
        host_read_commands: counter(64).to_string(),
        host_write_commands: counter(80).to_string(),
        controller_busy_minutes: counter(96).to_string(),
        unsafe_shutdowns: counter(144).to_string(),
        error_information_log_entries: counter(176).to_string(),
        warning_temperature_minutes: word(192),
        critical_temperature_minutes: word(196),
        temperature_sensors_celsius: std::array::from_fn(|index| {
            let offset = 200 + index * 2;
            let kelvin = u16::from_le_bytes(bytes[offset..offset + 2].try_into().unwrap());
            let value = f64::from(kelvin) - 273.15;
            super::disk_temperature::valid_celsius(value).then_some(value)
        }),
        thermal_management_transition_counts: [word(216), word(220)],
        thermal_management_total_seconds: [word(224), word(228)],
    }));
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn volume() -> ResourceVolume {
        ResourceVolume {
            devices: Vec::new(),
            mount_points: Vec::new(),
            file_system: None,
            id: "disk".into(),
            name: "System".into(),
            system: true,
            mount_point: "/".into(),
        }
    }
    fn data() -> [u8; 512] {
        let mut bytes = [0; 512];
        bytes[3] = 100;
        bytes[4] = 10;
        bytes
    }
    #[test]
    fn nvme_decodes_standard_counters_and_optional_sensors_at_their_exact_offsets() {
        let mut bytes = data();
        bytes[6] = 3;
        for (offset, value) in [
            (64, 123u128),
            (80, 456),
            (96, 789),
            (144, u128::MAX),
            (176, 42),
        ] {
            bytes[offset..offset + 16].copy_from_slice(&value.to_le_bytes());
        }
        for (offset, value) in [
            (192, 1u32),
            (196, 2),
            (216, 3),
            (220, 4),
            (224, 5),
            (228, 6),
        ] {
            bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
        }
        bytes[200..202].copy_from_slice(&308u16.to_le_bytes());
        bytes[214..216].copy_from_slice(&u16::MAX.to_le_bytes());
        let health = nvme(&volume(), &bytes).unwrap();
        assert_eq!(health.total_written_bytes, None);
        assert_eq!(health.total_read_bytes, None);
        let value = health.nvme.unwrap();
        assert_eq!(value.endurance_group_critical_warning, 3);
        assert_eq!(value.available_spare_threshold, 10);
        assert_eq!(value.host_read_commands, "123");
        assert_eq!(value.host_write_commands, "456");
        assert_eq!(value.controller_busy_minutes, "789");
        assert_eq!(value.unsafe_shutdowns, u128::MAX.to_string());
        assert_eq!(value.error_information_log_entries, "42");
        assert_eq!(value.warning_temperature_minutes, 1);
        assert_eq!(value.critical_temperature_minutes, 2);
        assert!((value.temperature_sensors_celsius[0].unwrap() - 34.85).abs() < 0.001);
        assert!(value.temperature_sensors_celsius[1..]
            .iter()
            .all(Option::is_none));
        assert_eq!(value.thermal_management_transition_counts, [3, 4]);
        assert_eq!(value.thermal_management_total_seconds, [5, 6]);
    }
    #[test]
    fn nvme_preserves_wear_above_one_hundred_and_exact_lifetime_counters() {
        let mut bytes = data();
        bytes[5] = 255;
        let units = u128::from(u64::MAX) + 1;
        bytes[32..48].copy_from_slice(&units.to_le_bytes());
        bytes[112..128].copy_from_slice(&42u128.to_le_bytes());
        bytes[48..64].copy_from_slice(&units.to_le_bytes());
        bytes[160..176].copy_from_slice(&u128::MAX.to_le_bytes());
        bytes[128..144].copy_from_slice(&units.to_le_bytes());
        let result = nvme(&volume(), &bytes).unwrap();
        assert_eq!(result.percentage_used, Some(255));
        assert_eq!(result.total_read_bytes, Some((units * 512_000).to_string()));
        assert_eq!(result.power_cycles, Some(42));
        assert_eq!(result.available_spare, Some(100));
        assert_eq!(result.critical_warning, Some(0));
        assert_eq!(
            result.total_written_bytes,
            Some((units * 512_000).to_string())
        );
        assert_eq!(result.media_errors, Some(u128::MAX.to_string()));
        assert_eq!(result.power_on_hours, None);
        assert_eq!(result.temperature_celsius, None);
        assert_eq!(
            result.nvme.as_ref().unwrap().power_on_hours,
            (u128::from(u64::MAX) + 1).to_string()
        );
        bytes[32..48].copy_from_slice(&u128::MAX.to_le_bytes());
        bytes[112..128].copy_from_slice(&u128::MAX.to_le_bytes());
        bytes[48..64].copy_from_slice(&u128::MAX.to_le_bytes());
        let overflow = nvme(&volume(), &bytes).unwrap();
        assert!(overflow.total_read_bytes.is_none());
        assert!(overflow.power_cycles.is_none());
        assert!(nvme(&volume(), &bytes)
            .unwrap()
            .total_written_bytes
            .is_none());
        assert_eq!(
            overflow.nvme.unwrap().data_units_read,
            u128::MAX.to_string()
        );
    }
    #[test]
    fn missing_corrupt_and_warning_records_are_never_healthy_defaults() {
        for bytes in [vec![], vec![0; 511], vec![0; 512]] {
            assert!(nvme(&volume(), &bytes).is_err());
        }
        let mut bytes = data();
        bytes[3] = 101;
        assert!(nvme(&volume(), &bytes).is_err());
        bytes = data();
        bytes[0] = 4;
        assert_eq!(
            nvme(&volume(), &bytes).unwrap().status,
            DiskHealthStatus::Warning
        );
        bytes[0] = 8;
        assert_eq!(
            nvme(&volume(), &bytes).unwrap().status,
            DiskHealthStatus::Unhealthy
        );
    }
}
