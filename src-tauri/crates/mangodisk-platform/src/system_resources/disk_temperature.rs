//! Read-only, volume-associated physical storage temperature. No device settings change.
use super::disk::ResourceVolume;
use crate::{PlatformError, PlatformErrorCode, PlatformResult};
use serde::Serialize;

#[cfg(target_os = "macos")]
mod macos;
#[cfg(windows)]
mod windows;

pub const DISK_TEMPERATURE_FRESHNESS_MS: u64 = 90_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum DiskTemperatureSource {
    NvmeSmart,
    AtaSmart,
    WindowsStorage,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiskTemperature {
    pub volume_id: String,
    pub volume_name: String,
    pub celsius: f64,
    pub source: DiskTemperatureSource,
}
impl DiskTemperature {
    pub fn valid(&self) -> bool {
        !self.volume_id.is_empty() && valid_celsius(self.celsius)
    }
}

fn valid_celsius(value: f64) -> bool {
    value.is_finite() && value > 0.0 && value <= 150.0
}

pub fn read(volume: &ResourceVolume) -> PlatformResult<DiskTemperature> {
    #[cfg(any(target_os = "macos", windows))]
    {
        #[cfg(target_os = "macos")]
        let (celsius, source) = macos::read(volume)?;
        #[cfg(windows)]
        let (celsius, source) = windows::read(volume)?;
        if !valid_celsius(celsius) {
            return Err(PlatformError::new(
                PlatformErrorCode::InvalidData,
                format!("disk_temperature stage=validate celsius={celsius}"),
            ));
        }
        Ok(DiskTemperature {
            volume_id: volume.id.clone(),
            volume_name: volume.name.clone(),
            celsius,
            source,
        })
    }
    #[cfg(not(any(target_os = "macos", windows)))]
    {
        let _ = volume;
        Err(unsupported("platform"))
    }
}

fn unsupported(stage: &str) -> PlatformError {
    PlatformError::new(
        PlatformErrorCode::Unsupported,
        format!("disk_temperature stage={stage} outcome=unsupported"),
    )
}

/// NVMe's composite temperature is an unsigned little-endian Kelvin field.
#[cfg(any(target_os = "macos", windows, test))]
fn nvme_temperature(bytes: &[u8]) -> PlatformResult<f64> {
    if bytes.len() != 512 {
        return Err(PlatformError::new(
            PlatformErrorCode::InvalidData,
            "disk_temperature stage=nvme_parse invalid_length",
        ));
    }
    let kelvin = u16::from_le_bytes([bytes[1], bytes[2]]);
    let celsius = f64::from(kelvin) - 273.15;
    if kelvin == 0 {
        return Err(unsupported("nvme_temperature"));
    }
    if !valid_celsius(celsius) {
        return Err(PlatformError::new(
            PlatformErrorCode::InvalidData,
            format!("disk_temperature stage=nvme_temperature kelvin={kelvin} celsius={celsius}"),
        ));
    }
    Ok(celsius)
}

#[cfg(any(target_os = "macos", windows, test))]
fn ata_temperature(bytes: &[u8]) -> PlatformResult<f64> {
    if bytes.len() != 512 || bytes.iter().fold(0u8, |sum, byte| sum.wrapping_add(*byte)) != 0 {
        return Err(PlatformError::new(
            PlatformErrorCode::InvalidData,
            "disk_temperature stage=ata_parse invalid_length_or_checksum",
        ));
    }
    for id in [194, 190] {
        for attribute in bytes[2..362].as_chunks::<12>().0 {
            if attribute[0] == id && valid_celsius(f64::from(attribute[5])) {
                return Ok(f64::from(attribute[5]));
            }
        }
    }
    Err(unsupported("ata_temperature"))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn nvme_rejects_missing_kelvin_truncated_and_implausible_values() {
        for kelvin in [0u16, 1, 273, u16::MAX] {
            let mut bytes = [0; 512];
            bytes[1..3].copy_from_slice(&kelvin.to_le_bytes());
            assert!(nvme_temperature(&bytes).is_err());
        }
        assert!(nvme_temperature(&[0; 511]).is_err());
        let mut bytes = [0; 512];
        bytes[1..3].copy_from_slice(&308u16.to_le_bytes());
        assert!((nvme_temperature(&bytes).unwrap() - 34.85).abs() < 0.001);
    }
    #[test]
    fn ata_checks_integrity_and_prefers_actual_temperature_over_airflow() {
        let mut bytes = [0; 512];
        bytes[2] = 190;
        bytes[7] = 30;
        bytes[14] = 194;
        bytes[19] = 45;
        bytes[511] = 0u8.wrapping_sub(
            bytes[..511]
                .iter()
                .fold(0u8, |sum, byte| sum.wrapping_add(*byte)),
        );
        assert_eq!(ata_temperature(&bytes).unwrap(), 45.0);
        bytes[100] = 1;
        assert!(ata_temperature(&bytes).is_err());
        assert!(ata_temperature(&[0; 512]).is_err());
    }
}
