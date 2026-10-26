use super::{ata_temperature, nvme_temperature, unsupported, DiskTemperatureSource};
use crate::{
    system_resources::disk::ResourceVolume, PlatformError, PlatformErrorCode, PlatformResult,
};
use std::{
    mem::{offset_of, size_of},
    ptr,
};
use windows_sys::Win32::{
    Foundation::{
        CloseHandle, GetLastError, ERROR_ACCESS_DENIED, ERROR_INVALID_FUNCTION,
        ERROR_INVALID_PARAMETER, ERROR_NOT_SUPPORTED, GENERIC_READ, GENERIC_WRITE, HANDLE,
        INVALID_HANDLE_VALUE,
    },
    Storage::FileSystem::{
        BusTypeAta, BusTypeNvme, BusTypeSata, CreateFileW, FILE_SHARE_READ, FILE_SHARE_WRITE,
        IOCTL_VOLUME_GET_VOLUME_DISK_EXTENTS, OPEN_EXISTING,
    },
    System::{Ioctl::*, IO::DeviceIoControl},
};

struct Handle(HANDLE);
impl Drop for Handle {
    fn drop(&mut self) {
        unsafe {
            CloseHandle(self.0);
        }
    }
}
fn error(stage: &str) -> PlatformError {
    let native = unsafe { GetLastError() };
    PlatformError::new(
        match native {
            ERROR_ACCESS_DENIED => PlatformErrorCode::AccessDenied,
            ERROR_INVALID_FUNCTION | ERROR_NOT_SUPPORTED | ERROR_INVALID_PARAMETER => {
                PlatformErrorCode::Unsupported
            }
            _ => PlatformErrorCode::OperationFailed,
        },
        format!("disk_temperature stage={stage} native_code={native}"),
    )
}
fn open(path: &str, access: u32, stage: &str) -> PlatformResult<Handle> {
    let path: Vec<u16> = path.encode_utf16().chain(Some(0)).collect();
    // Metadata queries request no data access and do not install a driver or elevate.
    let handle = unsafe {
        CreateFileW(
            path.as_ptr(),
            access,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            ptr::null(),
            OPEN_EXISTING,
            0,
            ptr::null_mut(),
        )
    };
    if handle == INVALID_HANDLE_VALUE {
        Err(error(stage))
    } else {
        Ok(Handle(handle))
    }
}
fn ioctl(handle: &Handle, code: u32, input: &[u8], stage: &str) -> PlatformResult<Vec<u8>> {
    // Aligned, bounded storage handles both variable-length extents and descriptors.
    let mut output = vec![0u64; 8192];
    let mut returned = 0;
    let ok = unsafe {
        DeviceIoControl(
            handle.0,
            code,
            input.as_ptr().cast(),
            input.len() as _,
            output.as_mut_ptr().cast(),
            (output.len() * 8) as _,
            &mut returned,
            ptr::null_mut(),
        )
    };
    if ok == 0 {
        return Err(error(stage));
    }
    if returned as usize > output.len() * 8 {
        return Err(PlatformError::new(
            PlatformErrorCode::InvalidData,
            format!("disk_temperature stage={stage} invalid_length={returned}"),
        ));
    }
    Ok(
        unsafe { std::slice::from_raw_parts(output.as_ptr().cast::<u8>(), returned as usize) }
            .to_vec(),
    )
}
fn device_numbers(volume: &ResourceVolume) -> PlatformResult<Vec<u32>> {
    let volume = open(volume.id.trim_end_matches('\\'), 0, "volume_open")?;
    let bytes = ioctl(
        &volume,
        IOCTL_VOLUME_GET_VOLUME_DISK_EXTENTS,
        &[],
        "volume_extents",
    )?;
    parse_device_numbers(&bytes)
}

fn parse_device_numbers(bytes: &[u8]) -> PlatformResult<Vec<u32>> {
    let header = offset_of!(VOLUME_DISK_EXTENTS, Extents);
    let count = bytes
        .get(..4)
        .map(|value| u32::from_le_bytes(value.try_into().unwrap()) as usize)
        .ok_or_else(|| unsupported("volume_extents_length"))?;
    if count == 0 || count > (bytes.len().saturating_sub(header)) / size_of::<DISK_EXTENT>() {
        return Err(unsupported("volume_extents_length"));
    }
    let mut disks = Vec::new();
    for index in 0..count {
        let start = header + index * size_of::<DISK_EXTENT>();
        let number = u32::from_le_bytes(bytes[start..start + 4].try_into().unwrap());
        if !disks.contains(&number) {
            disks.push(number);
        }
    }
    disks.sort_unstable();
    Ok(disks)
}

fn device_number(volume: &ResourceVolume) -> PlatformResult<u32> {
    let devices = device_numbers(volume)?;
    if devices.len() != 1 {
        return Err(unsupported("ambiguous_physical_device"));
    }
    Ok(devices[0])
}

pub(super) fn physical_devices(
    volume: &ResourceVolume,
) -> PlatformResult<Vec<super::super::disk::ResourceDisk>> {
    use super::super::disk::{device_identity, ResourceDisk};
    let numbers = device_numbers(volume)?;
    let mut devices = Vec::new();
    for number in &numbers {
        let handle = open(
            &format!("\\\\.\\PhysicalDrive{number}"),
            0,
            "catalogue_device_open",
        )?;
        let mut query = [0u8; 12];
        query[..4].copy_from_slice(&StorageDeviceProperty.to_le_bytes());
        // A missing descriptor must not erase a known volume-to-device relationship.
        let descriptor = ioctl(
            &handle,
            IOCTL_STORAGE_QUERY_PROPERTY,
            &query,
            "catalogue_identity",
        )
        .unwrap_or_default();
        let model = descriptor_text(
            &descriptor,
            offset_of!(STORAGE_DEVICE_DESCRIPTOR, ProductIdOffset),
        );
        let serial = descriptor_text(
            &descriptor,
            offset_of!(STORAGE_DEVICE_DESCRIPTOR, SerialNumberOffset),
        );
        let id = if serial.is_some() {
            device_identity(
                "windows-disk",
                model.as_deref(),
                serial.as_deref(),
                &volume.id,
            )
        } else {
            // GPT disk GUID / MBR signature remain stable when drive letters or disk numbers change.
            let layout = ioctl(
                &handle,
                IOCTL_DISK_GET_DRIVE_LAYOUT_EX,
                &[],
                "catalogue_layout",
            )
            .ok();
            let disk_id = layout.as_deref().and_then(layout_identity);
            device_identity(
                "windows-disk",
                None,
                disk_id.as_deref(),
                &format!("{}:{number}", volume.id),
            )
        };
        devices.push(ResourceDisk {
            id,
            number: Some(*number),
            model,
        });
    }
    if device_numbers(volume)? != numbers {
        return Err(PlatformError::item_changed(
            "disk_topology stage=volume_verify physical_device_changed",
        ));
    }
    Ok(devices)
}

fn layout_identity(bytes: &[u8]) -> Option<String> {
    let header = offset_of!(DRIVE_LAYOUT_INFORMATION_EX, PartitionEntry);
    if bytes.len() < header {
        return None;
    }
    let style = u32::from_le_bytes(bytes[..4].try_into().ok()?);
    let start = offset_of!(DRIVE_LAYOUT_INFORMATION_EX, Anonymous);
    let length = match style {
        0 => 4,
        1 => 16,
        _ => return None,
    };
    let id = bytes.get(start..start + length)?;
    if id.iter().all(|byte| *byte == 0) {
        return None;
    }
    Some(format!(
        "{style}:{}",
        id.iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>()
    ))
}

fn storage_temperature(bytes: &[u8]) -> PlatformResult<f64> {
    let header = offset_of!(STORAGE_TEMPERATURE_DATA_DESCRIPTOR, TemperatureInfo);
    if bytes.len() < header {
        return Err(unsupported("temperature_descriptor"));
    }
    let version = u32::from_le_bytes(bytes[..4].try_into().unwrap()) as usize;
    let size = u32::from_le_bytes(bytes[4..8].try_into().unwrap()) as usize;
    let count = u16::from_le_bytes(bytes[12..14].try_into().unwrap()) as usize;
    if version < size_of::<STORAGE_TEMPERATURE_DATA_DESCRIPTOR>()
        || size > bytes.len()
        || size < header
        || count > (size - header) / size_of::<STORAGE_TEMPERATURE_INFO>()
    {
        return Err(PlatformError::new(
            PlatformErrorCode::InvalidData,
            "disk_temperature stage=temperature_descriptor invalid_bounds",
        ));
    }
    if count == 0 {
        return Err(unsupported("temperature_sensor"));
    }
    let mut maximum: Option<f64> = None;
    for sensor in bytes[header..size].as_chunks::<16>().0.iter().take(count) {
        let index = u16::from_le_bytes(sensor[..2].try_into().unwrap());
        let value = f64::from(i16::from_le_bytes(sensor[2..4].try_into().unwrap()));
        if !super::super::disk_temperature::valid_celsius(value) {
            continue;
        }
        if index == 0 {
            return Ok(value);
        }
        maximum = Some(maximum.map_or(value, |previous| previous.max(value)));
    }
    maximum.ok_or_else(|| unsupported("temperature_sensor"))
}

fn descriptor_text(bytes: &[u8], field: usize) -> Option<String> {
    let minimum = offset_of!(STORAGE_DEVICE_DESCRIPTOR, RawDeviceProperties);
    if bytes.len() < minimum {
        return None;
    }
    let size = u32::from_le_bytes(bytes[4..8].try_into().ok()?) as usize;
    let version = u32::from_le_bytes(bytes[..4].try_into().ok()?) as usize;
    if version < minimum || size < minimum || size > bytes.len() {
        return None;
    }
    let offset = u32::from_le_bytes(bytes.get(field..field + 4)?.try_into().ok()?) as usize;
    if offset < minimum || offset >= size {
        return None;
    }
    let value = &bytes[offset..size];
    let end = value.iter().position(|byte| *byte == 0)?;
    if end > 128
        || value[..end]
            .iter()
            .any(|byte| !byte.is_ascii() || byte.is_ascii_control())
    {
        return None;
    }
    let value = std::str::from_utf8(&value[..end]).ok()?.trim();
    (!value.is_empty()).then(|| value.to_owned())
}

fn ata_identity(bytes: &[u8]) -> super::super::disk_health::ata::AtaIdentity {
    super::super::disk_health::ata::AtaIdentity {
        model: descriptor_text(
            bytes,
            offset_of!(STORAGE_DEVICE_DESCRIPTOR, ProductIdOffset),
        ),
        firmware: descriptor_text(
            bytes,
            offset_of!(STORAGE_DEVICE_DESCRIPTOR, ProductRevisionOffset),
        ),
        logical_sector_bytes: None,
    }
}

fn logical_sector_bytes(handle: &Handle) -> Option<u32> {
    let mut query = [0u8; 12];
    query[..4].copy_from_slice(&StorageAccessAlignmentProperty.to_le_bytes());
    let bytes = ioctl(
        handle,
        IOCTL_STORAGE_QUERY_PROPERTY,
        &query,
        "health_sector_size",
    )
    .ok()?;
    let minimum = size_of::<STORAGE_ACCESS_ALIGNMENT_DESCRIPTOR>();
    if bytes.len() < minimum {
        return None;
    }
    let size = u32::from_le_bytes(bytes[4..8].try_into().unwrap()) as usize;
    let version = u32::from_le_bytes(bytes[..4].try_into().unwrap()) as usize;
    if version < minimum || size < minimum || size > bytes.len() {
        return None;
    }
    let offset = offset_of!(STORAGE_ACCESS_ALIGNMENT_DESCRIPTOR, BytesPerLogicalSector);
    Some(u32::from_le_bytes(
        bytes[offset..offset + 4].try_into().unwrap(),
    ))
}
pub(super) fn read(volume: &ResourceVolume) -> PlatformResult<(f64, DiskTemperatureSource)> {
    let number = device_number(volume)?;
    let handle = open(&format!("\\\\.\\PhysicalDrive{number}"), 0, "device_open")?;
    let reading = read_device(&handle, number)?;
    let current = device_number(volume).map_err(|error| {
        PlatformError::item_changed(format!(
            "disk_temperature stage=volume_verify error={}",
            error.diagnostic()
        ))
    })?;
    if current != number {
        return Err(PlatformError::item_changed(
            "disk_temperature stage=volume_verify physical_device_changed",
        ));
    }
    Ok(reading)
}

fn read_device(handle: &Handle, number: u32) -> PlatformResult<(f64, DiskTemperatureSource)> {
    let mut input = [0u8; 12];
    input[..4].copy_from_slice(&StorageDeviceTemperatureProperty.to_le_bytes());
    input[4..8].copy_from_slice(&PropertyStandardQuery.to_le_bytes());
    let temperature = ioctl(
        handle,
        IOCTL_STORAGE_QUERY_PROPERTY,
        &input,
        "storage_temperature",
    )
    .and_then(|bytes| storage_temperature(&bytes));
    match temperature {
        Ok(value) => return Ok((value, DiskTemperatureSource::WindowsStorage)),
        Err(error)
            if matches!(
                error.code(),
                PlatformErrorCode::AccessDenied | PlatformErrorCode::InvalidData
            ) =>
        {
            return Err(error)
        }
        Err(_) => {}
    }
    let mut query = [0u8; 12];
    query[..4].copy_from_slice(&StorageDeviceProperty.to_le_bytes());
    let descriptor = ioctl(handle, IOCTL_STORAGE_QUERY_PROPERTY, &query, "device_bus")?;
    let offset = offset_of!(STORAGE_DEVICE_DESCRIPTOR, BusType);
    let bus = descriptor
        .get(offset..offset + 4)
        .map(|bytes| i32::from_le_bytes(bytes.try_into().unwrap()))
        .ok_or_else(|| unsupported("device_bus_length"))?;
    if bus == BusTypeAta || bus == BusTypeSata {
        // This read-only storage query uses FILE_ANY_ACCESS and works with the
        // metadata handle. Raw SMART commands require a separate read/write handle.
        let prediction = ioctl(handle, IOCTL_STORAGE_PREDICT_FAILURE, &[], "storage_smart")
            .and_then(|bytes| prediction_temperature(&bytes));
        match prediction {
            Ok(value) => return Ok((value, DiskTemperatureSource::WindowsStorage)),
            Err(error)
                if matches!(
                    error.code(),
                    PlatformErrorCode::AccessDenied | PlatformErrorCode::Unsupported
                ) => {}
            Err(error) => return Err(error),
        }
        let value = read_ata(number)?;
        return Ok((value, DiskTemperatureSource::AtaSmart));
    }
    if bus != BusTypeNvme {
        return Err(unsupported("storage_bus"));
    }
    let bytes = read_nvme_log(handle)?;
    Ok((nvme_temperature(&bytes)?, DiskTemperatureSource::NvmeSmart))
}

fn read_nvme_log(handle: &Handle) -> PlatformResult<Vec<u8>> {
    let mut input = vec![
        0u8;
        offset_of!(STORAGE_PROPERTY_QUERY, AdditionalParameters)
            + size_of::<STORAGE_PROTOCOL_SPECIFIC_DATA>()
            + 512
    ];
    input[..4].copy_from_slice(&StorageDeviceProtocolSpecificProperty.to_le_bytes());
    input[4..8].copy_from_slice(&PropertyStandardQuery.to_le_bytes());
    let protocol = STORAGE_PROTOCOL_SPECIFIC_DATA {
        ProtocolType: ProtocolTypeNvme,
        DataType: NVMeDataTypeLogPage as _,
        ProtocolDataRequestValue: 2,
        ProtocolDataOffset: size_of::<STORAGE_PROTOCOL_SPECIFIC_DATA>() as _,
        ProtocolDataLength: 512,
        ..Default::default()
    };
    let protocol_bytes = unsafe {
        std::slice::from_raw_parts(
            (&protocol as *const STORAGE_PROTOCOL_SPECIFIC_DATA).cast(),
            size_of::<STORAGE_PROTOCOL_SPECIFIC_DATA>(),
        )
    };
    input[8..8 + protocol_bytes.len()].copy_from_slice(protocol_bytes);
    let bytes = ioctl(handle, IOCTL_STORAGE_QUERY_PROPERTY, &input, "nvme_log")?;
    Ok(nvme_log(&bytes)?.to_vec())
}

fn prediction_temperature(bytes: &[u8]) -> PlatformResult<f64> {
    let start = offset_of!(STORAGE_PREDICT_FAILURE, VendorSpecific);
    if bytes.len() != size_of::<STORAGE_PREDICT_FAILURE>() {
        return Err(PlatformError::new(
            PlatformErrorCode::InvalidData,
            "disk_temperature stage=storage_smart invalid_response_length",
        ));
    }
    // Failure prediction is a health flag, not a temperature or a reason to hide
    // a valid reading. Keep the full ATA sector's checksum and sensor validation.
    ata_temperature(&bytes[start..start + 512])
}

pub(super) fn read_health(
    volume: &ResourceVolume,
) -> PlatformResult<super::super::disk_health::DiskHealth> {
    use super::super::disk_health::{self, DiskHealth, DiskHealthStatus};
    let number = device_number(volume)?;
    let handle = open(&format!("\\\\.\\PhysicalDrive{number}"), 0, "health_open")?;
    let mut query = [0u8; 12];
    query[..4].copy_from_slice(&StorageDeviceProperty.to_le_bytes());
    let descriptor = ioctl(&handle, IOCTL_STORAGE_QUERY_PROPERTY, &query, "health_bus")?;
    let offset = offset_of!(STORAGE_DEVICE_DESCRIPTOR, BusType);
    let bus = descriptor
        .get(offset..offset + 4)
        .map(|bytes| i32::from_le_bytes(bytes.try_into().unwrap()))
        .ok_or_else(|| unsupported("health_bus_length"))?;
    let detailed = if bus == BusTypeNvme {
        read_nvme_log(&handle).and_then(|bytes| disk_health::nvme(volume, &bytes))
    } else {
        ioctl(
            &handle,
            IOCTL_STORAGE_PREDICT_FAILURE,
            &[],
            "health_prediction",
        )
        .and_then(|bytes| {
            if bytes.len() != size_of::<STORAGE_PREDICT_FAILURE>() {
                return Err(PlatformError::new(
                    PlatformErrorCode::InvalidData,
                    "disk_health stage=prediction invalid_length",
                ));
            }
            let status = if u32::from_le_bytes(bytes[..4].try_into().unwrap()) == 0 {
                DiskHealthStatus::Healthy
            } else {
                DiskHealthStatus::Warning
            };
            // SCSI failure-prediction payloads are vendor data, not ATA attribute tables.
            if bus != BusTypeAta && bus != BusTypeSata {
                return Ok(DiskHealth::basic(
                    volume,
                    status,
                    DiskTemperatureSource::WindowsStorage,
                ));
            }
            let mut identity = ata_identity(&descriptor);
            identity.logical_sector_bytes = logical_sector_bytes(&handle);
            let start = offset_of!(STORAGE_PREDICT_FAILURE, VendorSpecific);
            disk_health::ata::read(volume, &bytes[start..start + 512], status, identity)
        })
    };
    let result = match detailed {
        Ok(value) => value,
        Err(error) if error.code() == PlatformErrorCode::InvalidData => return Err(error),
        Err(_) => {
            query[..4].copy_from_slice(&StorageDeviceManagementStatus.to_le_bytes());
            let bytes = ioctl(
                &handle,
                IOCTL_STORAGE_QUERY_PROPERTY,
                &query,
                "health_management",
            )?;
            management_health(volume, &bytes)?
        }
    };
    if device_number(volume)? != number {
        return Err(PlatformError::item_changed(
            "disk_health stage=volume_verify physical_device_changed",
        ));
    }
    Ok(result)
}

fn management_health(
    volume: &ResourceVolume,
    bytes: &[u8],
) -> PlatformResult<super::super::disk_health::DiskHealth> {
    use super::super::disk_health::{DiskHealth, DiskHealthStatus};
    let invalid = || {
        PlatformError::new(
            PlatformErrorCode::InvalidData,
            "disk_health stage=management invalid_descriptor",
        )
    };
    let minimum = offset_of!(STORAGE_DEVICE_MANAGEMENT_STATUS, AdditionalReasons);
    if bytes.len() < minimum {
        return Err(invalid());
    }
    let word = |offset| u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap());
    let size = word(4) as usize;
    if word(0) < minimum as u32 || size < minimum || size > bytes.len() || word(12) > 16 {
        return Err(invalid());
    }
    let reasons = word(16) as usize;
    if reasons > (size - minimum) / size_of::<STORAGE_OPERATIONAL_REASON>() {
        return Err(invalid());
    }
    let status = match word(8) as i32 {
        value if value == DiskHealthHealthy => DiskHealthStatus::Healthy,
        value if value == DiskHealthWarning => DiskHealthStatus::Warning,
        value if value == DiskHealthUnhealthy => DiskHealthStatus::Unhealthy,
        value if value == DiskHealthUnknown => DiskHealthStatus::Unknown,
        _ => return Err(invalid()),
    };
    Ok(DiskHealth::basic(
        volume,
        status,
        DiskTemperatureSource::WindowsStorage,
    ))
}

fn nvme_log(bytes: &[u8]) -> PlatformResult<&[u8]> {
    let invalid = || {
        PlatformError::new(
            PlatformErrorCode::InvalidData,
            "disk_temperature stage=nvme_descriptor invalid_protocol_or_bounds",
        )
    };
    if bytes.len() < size_of::<STORAGE_PROTOCOL_DATA_DESCRIPTOR>() {
        return Err(invalid());
    }
    let descriptor =
        unsafe { ptr::read_unaligned(bytes.as_ptr().cast::<STORAGE_PROTOCOL_DATA_DESCRIPTOR>()) };
    let data = descriptor.ProtocolSpecificData;
    let minimum = size_of::<STORAGE_PROTOCOL_DATA_DESCRIPTOR>() as u32;
    if descriptor.Version < minimum
        || descriptor.Size < minimum
        || descriptor.Size as usize > bytes.len()
        || data.ProtocolType != ProtocolTypeNvme
        || data.DataType != NVMeDataTypeLogPage as u32
        || data.ProtocolDataLength != 512
        || data.ProtocolDataOffset < size_of::<STORAGE_PROTOCOL_SPECIFIC_DATA>() as u32
    {
        return Err(invalid());
    }
    let offset = offset_of!(STORAGE_PROTOCOL_DATA_DESCRIPTOR, ProtocolSpecificData)
        .checked_add(data.ProtocolDataOffset as usize)
        .ok_or_else(invalid)?;
    let log = bytes
        .get(offset..offset.checked_add(512).ok_or_else(invalid)?)
        .ok_or_else(invalid)?;
    Ok(log)
}

#[cfg(test)]
fn nvme_log_temperature(bytes: &[u8]) -> PlatformResult<f64> {
    nvme_temperature(nvme_log(bytes)?)
}

fn read_ata(number: u32) -> PlatformResult<f64> {
    let drive = u8::try_from(number).map_err(|_| unsupported("ata_drive_number"))?;
    let handle = open(
        &format!("\\\\.\\PhysicalDrive{number}"),
        GENERIC_READ | GENERIC_WRITE,
        "ata_smart_open",
    )?;
    let mut input = [0u8; 32];
    input[..4].copy_from_slice(&512u32.to_le_bytes());
    // Fixed SMART READ DATA registers. No SMART enable, self-test, log writes,
    // arbitrary register access, or drive settings are exposed to callers.
    input[4..12].copy_from_slice(&[0xd0, 1, 1, 0x4f, 0xc2, 0xa0 | ((drive & 1) << 4), 0xb0, 0]);
    input[12] = drive;
    let bytes = ioctl(&handle, SMART_RCV_DRIVE_DATA, &input, "ata_smart")?;
    let start = offset_of!(SENDCMDOUTPARAMS, bBuffer);
    if bytes.get(4).copied() != Some(0) {
        return Err(PlatformError::new(
            PlatformErrorCode::OperationFailed,
            format!(
                "disk_temperature stage=ata_smart driver_error={} ide_status=0x{:02x}",
                bytes.get(4).copied().unwrap_or(0),
                bytes.get(5).copied().unwrap_or(0)
            ),
        ));
    }
    ata_temperature(
        bytes
            .get(start..start + 512)
            .ok_or_else(|| unsupported("ata_response_length"))?,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn identity_strings_are_bounded_and_never_read_serial_or_unterminated_data() {
        let minimum = size_of::<STORAGE_DEVICE_DESCRIPTOR>();
        let mut bytes = vec![0; minimum + 48];
        bytes[..4].copy_from_slice(&(minimum as u32).to_le_bytes());
        let size = bytes.len() as u32;
        bytes[4..8].copy_from_slice(&size.to_le_bytes());
        let product = offset_of!(STORAGE_DEVICE_DESCRIPTOR, ProductIdOffset);
        let firmware = offset_of!(STORAGE_DEVICE_DESCRIPTOR, ProductRevisionOffset);
        bytes[product..product + 4].copy_from_slice(&(minimum as u32).to_le_bytes());
        bytes[firmware..firmware + 4].copy_from_slice(&((minimum + 24) as u32).to_le_bytes());
        bytes[minimum..minimum + 17].copy_from_slice(b"Acer AS500A 128G ");
        bytes[minimum + 24..minimum + 31].copy_from_slice(b"Q1115A0");
        let identity = ata_identity(&bytes);
        assert_eq!(identity.model.as_deref(), Some("Acer AS500A 128G"));
        assert_eq!(identity.firmware.as_deref(), Some("Q1115A0"));
        assert!(ata_identity(&bytes[..minimum - 1]).model.is_none());
        bytes[minimum] = b'\n';
        assert!(ata_identity(&bytes).model.is_none());
        bytes[minimum..].fill(b'A');
        assert!(ata_identity(&bytes).model.is_none());
        bytes[product..product + 4].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(ata_identity(&bytes).model.is_none());
        bytes[4..8].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(ata_identity(&bytes).firmware.is_none());
    }
    #[test]
    fn identity_accepts_strings_immediately_after_the_flexible_array_header() {
        let header = offset_of!(STORAGE_DEVICE_DESCRIPTOR, RawDeviceProperties);
        let model = b"Acer AS500A 128G\0";
        let mut bytes = vec![0; header + model.len()];
        bytes[..4].copy_from_slice(&(size_of::<STORAGE_DEVICE_DESCRIPTOR>() as u32).to_le_bytes());
        let size = bytes.len() as u32;
        bytes[4..8].copy_from_slice(&size.to_le_bytes());
        let product = offset_of!(STORAGE_DEVICE_DESCRIPTOR, ProductIdOffset);
        bytes[product..product + 4].copy_from_slice(&(header as u32).to_le_bytes());
        bytes[header..].copy_from_slice(model);
        assert_eq!(
            ata_identity(&bytes).model.as_deref(),
            Some("Acer AS500A 128G")
        );
    }
    #[test]
    fn management_only_health_validates_bounds_and_keeps_unknown_distinct_from_healthy() {
        use super::super::super::disk_health::DiskHealthStatus;
        let volume = ResourceVolume {
            devices: Vec::new(),
            mount_points: Vec::new(),
            file_system: None,
            id: "volume".into(),
            name: "Disk".into(),
            system: true,
            mount_point: "C:\\".into(),
        };
        let minimum = offset_of!(STORAGE_DEVICE_MANAGEMENT_STATUS, AdditionalReasons);
        let mut bytes = vec![0u8; minimum];
        bytes[..4].copy_from_slice(&(minimum as u32).to_le_bytes());
        bytes[4..8].copy_from_slice(&(minimum as u32).to_le_bytes());
        assert_eq!(
            management_health(&volume, &bytes).unwrap().status,
            DiskHealthStatus::Unknown
        );
        bytes[8..12].copy_from_slice(&(DiskHealthHealthy as u32).to_le_bytes());
        let result = management_health(&volume, &bytes).unwrap();
        assert_eq!(result.status, DiskHealthStatus::Healthy);
        assert_eq!(result.source, DiskTemperatureSource::WindowsStorage);
        assert!(result.percentage_used.is_none());
        assert!(result.total_written_bytes.is_none());
        for (offset, value) in [(0, 0), (4, u32::MAX), (8, 99), (12, 17), (16, u32::MAX)] {
            let mut invalid = bytes.clone();
            invalid[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
            assert!(management_health(&volume, &invalid).is_err());
        }
        assert!(management_health(&volume, &bytes[..minimum - 1]).is_err());
    }
    #[test]
    fn storage_prediction_reads_valid_smart_without_data_access_and_keeps_integrity_checks() {
        assert_eq!((IOCTL_STORAGE_PREDICT_FAILURE >> 14) & 3, 0);
        assert_eq!(size_of::<STORAGE_PREDICT_FAILURE>(), 516);
        let start = offset_of!(STORAGE_PREDICT_FAILURE, VendorSpecific);
        let mut response = vec![0u8; size_of::<STORAGE_PREDICT_FAILURE>()];
        response[start..start + 2].copy_from_slice(&1u16.to_le_bytes());
        response[start + 2] = 190;
        response[start + 7] = 30;
        response[start + 350] = 194;
        response[start + 355] = 45;
        response[start + 511] = 0u8.wrapping_sub(
            response[start..start + 511]
                .iter()
                .fold(0u8, |sum, byte| sum.wrapping_add(*byte)),
        );
        for health in [0u32, 1, u32::MAX] {
            response[..4].copy_from_slice(&health.to_le_bytes());
            assert_eq!(prediction_temperature(&response).unwrap(), 45.0);
        }
        let mut corrupt = response.clone();
        corrupt[start + 100] ^= 1;
        assert_eq!(
            prediction_temperature(&corrupt).unwrap_err().code(),
            PlatformErrorCode::InvalidData
        );
        for length in [0, 4, 515, 517, 1024] {
            assert_eq!(
                prediction_temperature(&vec![0; length]).unwrap_err().code(),
                PlatformErrorCode::InvalidData
            );
        }
        assert_eq!(
            prediction_temperature(&vec![0; 516]).unwrap_err().code(),
            PlatformErrorCode::Unsupported
        );
    }
    #[test]
    fn nvme_response_requires_matching_protocol_and_bounded_payload() {
        let mut bytes = vec![0u8; 560];
        bytes[..4].copy_from_slice(&48u32.to_le_bytes());
        bytes[4..8].copy_from_slice(&48u32.to_le_bytes());
        bytes[8..12].copy_from_slice(&ProtocolTypeNvme.to_le_bytes());
        bytes[12..16].copy_from_slice(&(NVMeDataTypeLogPage as u32).to_le_bytes());
        bytes[24..28].copy_from_slice(&40u32.to_le_bytes());
        bytes[28..32].copy_from_slice(&512u32.to_le_bytes());
        bytes[49..51].copy_from_slice(&308u16.to_le_bytes());
        assert!((nvme_log_temperature(&bytes).unwrap() - 34.85).abs() < 0.001);
        for (offset, value) in [
            (0, 0u32),
            (4, 1000),
            (8, ProtocolTypeAta as u32),
            (12, 0),
            (24, 0),
            (24, u32::MAX),
            (28, 513),
        ] {
            let mut invalid = bytes.clone();
            invalid[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
            assert_eq!(
                nvme_log_temperature(&invalid).unwrap_err().code(),
                PlatformErrorCode::InvalidData
            );
        }
        assert!(nvme_log_temperature(&bytes[..559]).is_err());
    }

    #[test]
    fn native_temperature_descriptor_checks_composite_sentinel_and_bounds() {
        let mut bytes = vec![0; 56];
        bytes[..4].copy_from_slice(&40u32.to_le_bytes());
        bytes[4..8].copy_from_slice(&56u32.to_le_bytes());
        bytes[12..14].copy_from_slice(&2u16.to_le_bytes());
        bytes[24..26].copy_from_slice(&1u16.to_le_bytes());
        bytes[26..28].copy_from_slice(&70i16.to_le_bytes());
        bytes[42..44].copy_from_slice(&40i16.to_le_bytes());
        assert_eq!(storage_temperature(&bytes).unwrap(), 40.0);
        bytes[42..44].copy_from_slice(&i16::MIN.to_le_bytes());
        assert_eq!(storage_temperature(&bytes).unwrap(), 70.0);
        bytes[12..14].copy_from_slice(&3u16.to_le_bytes());
        assert!(storage_temperature(&bytes).is_err());
        bytes[12..14].copy_from_slice(&1u16.to_le_bytes());
        bytes[26..28].copy_from_slice(&i16::MIN.to_le_bytes());
        assert_eq!(
            storage_temperature(&bytes).unwrap_err().code(),
            PlatformErrorCode::Unsupported
        );
        bytes[..4].copy_from_slice(&0u32.to_le_bytes());
        assert_eq!(
            storage_temperature(&bytes).unwrap_err().code(),
            PlatformErrorCode::InvalidData
        );
        assert_eq!(size_of::<STORAGE_TEMPERATURE_INFO>(), 16);
    }
    #[test]
    fn device_extents_deduplicate_partitions_and_reject_truncation() {
        let header = offset_of!(VOLUME_DISK_EXTENTS, Extents);
        let mut bytes = vec![0u8; header + size_of::<DISK_EXTENT>() * 3];
        bytes[..4].copy_from_slice(&3u32.to_le_bytes());
        for (index, number) in [2u32, 1, 2].into_iter().enumerate() {
            let offset = header + index * size_of::<DISK_EXTENT>();
            bytes[offset..offset + 4].copy_from_slice(&number.to_le_bytes());
        }
        assert_eq!(parse_device_numbers(&bytes).unwrap(), vec![1, 2]);
        assert!(parse_device_numbers(&bytes[..bytes.len() - 1]).is_err());
        assert!(parse_device_numbers(&[]).is_err());
        bytes[..4].fill(0);
        assert!(parse_device_numbers(&bytes).is_err());
    }

    #[test]
    fn missing_or_uninitialized_disk_layout_cannot_become_a_shared_identity() {
        assert!(layout_identity(&[]).is_none());
        let mut bytes = vec![0u8; size_of::<DRIVE_LAYOUT_INFORMATION_EX>()];
        assert!(layout_identity(&bytes).is_none());
        let start = offset_of!(DRIVE_LAYOUT_INFORMATION_EX, Anonymous);
        bytes[start] = 7;
        assert!(layout_identity(&bytes).is_some());
        bytes[..4].copy_from_slice(&1u32.to_le_bytes());
        assert!(layout_identity(&bytes).is_some());
        bytes[..4].copy_from_slice(&2u32.to_le_bytes());
        assert!(layout_identity(&bytes).is_none());
    }
}
