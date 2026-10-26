//! Shared native storage access for temperature and low-frequency health reads.
use super::{
    disk::ResourceVolume,
    disk_health::DiskHealth,
    disk_temperature::{ata_temperature, nvme_temperature, unsupported, DiskTemperatureSource},
};
use crate::PlatformResult;

#[cfg(target_os = "macos")]
mod macos;
#[cfg(windows)]
mod windows;

pub(super) fn read_temperature(
    volume: &ResourceVolume,
) -> PlatformResult<(f64, DiskTemperatureSource)> {
    verify_identity(volume)?;
    #[cfg(target_os = "macos")]
    let result = macos::read(volume)?;
    #[cfg(windows)]
    let result = windows::read(volume)?;
    verify_identity(volume)?;
    Ok(result)
}

pub(super) fn read_health(volume: &ResourceVolume) -> PlatformResult<DiskHealth> {
    #[cfg(target_os = "macos")]
    {
        macos::read_health(volume)
    }
    #[cfg(windows)]
    {
        windows::read_health(volume)
    }
}

/// Read-only topology metadata, independent of SMART capability or temperature support.
pub(super) fn physical_devices(
    volume: &ResourceVolume,
) -> PlatformResult<Vec<super::disk::ResourceDisk>> {
    #[cfg(target_os = "macos")]
    {
        macos::physical_devices(volume)
    }
    #[cfg(windows)]
    {
        windows::physical_devices(volume)
    }
}

fn verify_identity(volume: &ResourceVolume) -> PlatformResult<()> {
    if volume.devices.is_empty() {
        return Ok(());
    }
    let current = physical_devices(volume)?;
    let identity = |devices: &[super::disk::ResourceDisk]| {
        let mut identities = devices
            .iter()
            .map(|device| (device.id.clone(), device.number))
            .collect::<Vec<_>>();
        identities.sort();
        identities
    };
    if identity(&current) != identity(&volume.devices) {
        return Err(crate::PlatformError::item_changed(
            "disk_temperature stage=device_verify physical_identity_changed",
        ));
    }
    Ok(())
}
