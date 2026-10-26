use super::{ata_temperature, nvme_temperature, unsupported, DiskTemperatureSource};
use crate::{
    system_resources::disk::ResourceVolume, PlatformError, PlatformErrorCode, PlatformResult,
};
use core_foundation::{
    base::{CFType, CFTypeRef, TCFType},
    boolean::CFBoolean,
    dictionary::{CFDictionary, CFMutableDictionaryRef},
    string::{CFString, CFStringRef},
    uuid::{CFUUIDBytes, CFUUIDCreateFromUUIDBytes, CFUUIDRef, CFUUID},
};
use std::{
    collections::HashSet,
    ffi::{c_char, c_void, CStr, CString},
    ptr,
};

#[link(name = "IOKit", kind = "framework")]
unsafe extern "C" {
    fn IOBSDNameMatching(port: u32, options: u32, name: *const c_char) -> CFMutableDictionaryRef;
    fn IOServiceGetMatchingService(port: u32, matching: CFMutableDictionaryRef) -> u32;
    fn IOObjectConformsTo(object: u32, class: *const c_char) -> u32;
    fn IOObjectRelease(object: u32) -> i32;
    fn IOIteratorNext(iterator: u32) -> u32;
    fn IORegistryEntryGetChildIterator(entry: u32, plane: *const c_char, iterator: *mut u32)
        -> i32;
    fn IORegistryEntryGetParentIterator(
        entry: u32,
        plane: *const c_char,
        iterator: *mut u32,
    ) -> i32;
    fn IORegistryEntryGetRegistryEntryID(entry: u32, identity: *mut u64) -> i32;
    fn IORegistryEntryCreateCFProperty(
        entry: u32,
        key: CFStringRef,
        allocator: *const c_void,
        options: u32,
    ) -> CFTypeRef;
    fn IOCreatePlugInInterfaceForService(
        service: u32,
        client: CFUUIDRef,
        interface: CFUUIDRef,
        plugin: *mut *mut *mut Unknown,
        score: *mut i32,
    ) -> i32;
    fn IODestroyPlugInInterface(plugin: *mut *mut Unknown) -> i32;
}

struct Object(u32);
impl Drop for Object {
    fn drop(&mut self) {
        unsafe {
            IOObjectRelease(self.0);
        }
    }
}
#[repr(C)]
struct Unknown {
    reserved: *mut c_void,
    query: unsafe extern "C" fn(*mut c_void, CFUUIDBytes, *mut *mut c_void) -> i32,
    add_ref: unsafe extern "C" fn(*mut c_void) -> u32,
    release: unsafe extern "C" fn(*mut c_void) -> u32,
}
type ReadData = unsafe extern "C" fn(*mut c_void, *mut u8) -> i32;
#[repr(C)]
struct NvmeInterface {
    unknown: Unknown,
    version: u16,
    revision: u16,
    read: Option<ReadData>,
}
#[repr(C)]
struct AtaInterface {
    unknown: Unknown,
    version: u16,
    revision: u16,
    enable: *const c_void,
    autosave: *const c_void,
    status: Option<unsafe extern "C" fn(*mut c_void, *mut u8) -> i32>,
    offline: *const c_void,
    read: Option<ReadData>,
}
struct Plugin(*mut *mut Unknown);
impl Drop for Plugin {
    fn drop(&mut self) {
        unsafe {
            IODestroyPlugInInterface(self.0);
        }
    }
}
struct Interface(*mut *mut Unknown);
impl Drop for Interface {
    fn drop(&mut self) {
        unsafe {
            ((**self.0).release)(self.0.cast());
        }
    }
}
fn uuid(bytes: [u8; 16]) -> CFUUID {
    // CFUUIDBytes is a C structure containing exactly sixteen consecutive bytes.
    let bytes = unsafe { std::mem::transmute::<[u8; 16], CFUUIDBytes>(bytes) };
    unsafe { CFUUID::wrap_under_create_rule(CFUUIDCreateFromUUIDBytes(ptr::null(), bytes)) }
}
fn capable(service: u32, property: &str) -> bool {
    let key = CFString::new(property);
    let raw = unsafe {
        IORegistryEntryCreateCFProperty(service, key.as_concrete_TypeRef(), ptr::null(), 0)
    };
    !raw.is_null()
        && unsafe { CFType::wrap_under_create_rule(raw) }
            .downcast_into::<CFBoolean>()
            .is_some_and(bool::from)
}

fn ata_identity(service: u32) -> super::super::disk_health::ata::AtaIdentity {
    use super::super::disk_health::ata::AtaIdentity;
    let key = CFString::new("Device Characteristics");
    let raw = unsafe {
        IORegistryEntryCreateCFProperty(service, key.as_concrete_TypeRef(), ptr::null(), 0)
    };
    if raw.is_null() {
        return AtaIdentity::default();
    }
    let value = unsafe { CFType::wrap_under_create_rule(raw) };
    let Some(dictionary) = value.downcast_into::<CFDictionary>() else {
        return AtaIdentity::default();
    };
    let text = |name| {
        let value = dictionary.find(CFString::new(name).as_CFTypeRef())?;
        let value = unsafe { CFType::wrap_under_get_rule(*value) }
            .downcast_into::<CFString>()?
            .to_string();
        let value = value.trim();
        (!value.is_empty() && value.len() <= 128 && !value.chars().any(char::is_control))
            .then(|| value.to_owned())
    };
    AtaIdentity {
        model: text("Product Name"),
        firmware: text("Product Revision Level"),
        logical_sector_bytes: None,
    }
}
fn native_error(stage: &str, code: i32) -> PlatformError {
    PlatformError::new(
        if code as u32 == 0xe00002c1 {
            PlatformErrorCode::AccessDenied
        } else {
            PlatformErrorCode::OperationFailed
        },
        format!(
            "disk_temperature stage={stage} native_code=0x{:08x}",
            code as u32
        ),
    )
}

fn storage_devices(volume: &ResourceVolume) -> PlatformResult<Vec<Object>> {
    let path = CString::new(volume.mount_point.as_str()).map_err(|_| unsupported("volume_path"))?;
    let mut stats: libc::statfs = unsafe { std::mem::zeroed() };
    if unsafe { libc::statfs(path.as_ptr(), &mut stats) } != 0 {
        return Err(native_error("statfs", unsafe { *libc::__error() }));
    }
    let from = unsafe { CStr::from_ptr(stats.f_mntfromname.as_ptr()) }.to_bytes();
    let bsd = CString::new(
        from.strip_prefix(b"/dev/")
            .ok_or_else(|| unsupported("block_device"))?,
    )
    .map_err(|_| unsupported("bsd_name"))?;
    let media = unsafe { IOServiceGetMatchingService(0, IOBSDNameMatching(0, 0, bsd.as_ptr())) };
    if media == 0 {
        return Err(unsupported("media"));
    }
    let mut pending = vec![Object(media)];
    let mut seen = HashSet::new();
    let mut devices = Vec::new();
    while let Some(entry) = pending.pop() {
        let mut identity = 0;
        let code = unsafe { IORegistryEntryGetRegistryEntryID(entry.0, &mut identity) };
        if code != 0 {
            return Err(native_error("media_identity", code));
        }
        if !seen.insert(identity) {
            continue;
        }
        if seen.len() > 128 {
            return Err(unsupported("media_topology_limit"));
        }
        if unsafe { IOObjectConformsTo(entry.0, c"IOBlockStorageDevice".as_ptr()) } != 0 {
            devices.push(entry);
            continue;
        }
        let mut iterator = 0;
        let code = unsafe {
            IORegistryEntryGetParentIterator(entry.0, c"IOService".as_ptr(), &mut iterator)
        };
        if code != 0 {
            return Err(native_error("media_parents", code));
        }
        let iterator = Object(iterator);
        loop {
            let parent = unsafe { IOIteratorNext(iterator.0) };
            if parent == 0 {
                break;
            }
            pending.push(Object(parent));
            if pending.len() > 128 {
                return Err(unsupported("media_topology_limit"));
            }
        }
    }
    if devices.is_empty() {
        return Err(unsupported("physical_device"));
    }
    Ok(devices)
}

fn storage_device(volume: &ResourceVolume) -> PlatformResult<Object> {
    let mut devices = storage_devices(volume)?;
    // Fusion, RAID and other multi-device volumes have no single disk temperature.
    if devices.len() != 1 {
        return Err(unsupported("ambiguous_physical_device"));
    }
    Ok(devices.pop().unwrap())
}

fn property(service: u32, name: &str) -> Option<CFType> {
    let key = CFString::new(name);
    let raw = unsafe {
        IORegistryEntryCreateCFProperty(service, key.as_concrete_TypeRef(), ptr::null(), 0)
    };
    (!raw.is_null()).then(|| unsafe { CFType::wrap_under_create_rule(raw) })
}

pub(super) fn physical_devices(
    volume: &ResourceVolume,
) -> PlatformResult<Vec<super::super::disk::ResourceDisk>> {
    use super::super::disk::{device_identity, ResourceDisk};
    let mut devices = Vec::new();
    for device in storage_devices(volume)? {
        let characteristics = property(device.0, "Device Characteristics")
            .and_then(|value| value.downcast_into::<CFDictionary>());
        let text = |name: &str| -> Option<String> {
            let raw = characteristics
                .as_ref()?
                .find(CFString::new(name).as_CFTypeRef())?;
            let value = unsafe { CFType::wrap_under_get_rule(*raw) }
                .downcast_into::<CFString>()?
                .to_string();
            let value = value.trim();
            (!value.is_empty() && value.len() <= 128 && !value.chars().any(char::is_control))
                .then(|| value.to_owned())
        };
        let model = text("Product Name");
        let serial = text("Serial Number");

        let number = physical_disk_number(device.0);
        let id = device_identity(
            "macos-disk",
            model.as_deref(),
            serial.as_deref(),
            &format!("{}:{number:?}", volume.id),
        );
        devices.push(ResourceDisk { id, number, model });
    }
    devices.sort_by_key(|device| device.number);
    Ok(devices)
}

fn physical_disk_number(service: u32) -> Option<u32> {
    // IOBlockStorageDriver sits between the device and its whole IOMedia.
    // Stop at the first whole media; do not descend into partitions or APFS volumes.
    let mut parents = vec![service];
    let mut owned = Vec::new();
    for _ in 0..128 {
        let parent = parents.pop()?;
        let mut iterator = 0;
        if unsafe { IORegistryEntryGetChildIterator(parent, c"IOService".as_ptr(), &mut iterator) }
            != 0
        {
            continue;
        }
        let iterator = Object(iterator);
        loop {
            let child = unsafe { IOIteratorNext(iterator.0) };
            if child == 0 {
                break;
            }
            let child = Object(child);
            if property(child.0, "Whole")
                .and_then(|value| value.downcast_into::<CFBoolean>())
                .is_some_and(bool::from)
            {
                return property(child.0, "BSD Name")
                    .and_then(|value| value.downcast_into::<CFString>())
                    .and_then(|name| {
                        name.to_string()
                            .strip_prefix("disk")
                            .and_then(|number| number.parse().ok())
                    });
            }
            if owned.len() >= 128 {
                return None;
            }
            parents.push(child.0);
            owned.push(child);
        }
    }
    None
}

fn read_smart(
    volume: &ResourceVolume,
    include_health: bool,
) -> PlatformResult<(
    [u8; 512],
    bool,
    Option<bool>,
    super::super::disk_health::ata::AtaIdentity,
)> {
    let service = storage_device(volume)?;
    let nvme = capable(service.0, "NVMe SMART Capable");
    if !nvme && !capable(service.0, "SMART Capable") {
        return Err(unsupported("smart_capability"));
    }
    let plugin_id = uuid([
        0xc2, 0x44, 0xe8, 0x58, 0x10, 0x9c, 0x11, 0xd4, 0x91, 0xd4, 0, 0x50, 0xe4, 0xc6, 0x42, 0x6f,
    ]);
    let client_id = uuid(if nvme {
        [
            0xaa, 0x0f, 0xa6, 0xf9, 0xc2, 0xd6, 0x45, 0x7f, 0xb1, 0x0b, 0x59, 0xa1, 0x32, 0x53,
            0x29, 0x2f,
        ]
    } else {
        [
            0x24, 0x51, 0x4b, 0x7a, 0x28, 0x04, 0x11, 0xd6, 0x8a, 0x02, 0, 0x30, 0x65, 0x70, 0x48,
            0x66,
        ]
    });
    let interface_id = uuid(if nvme {
        [
            0xcc, 0xd1, 0xdb, 0x19, 0xfd, 0x9a, 0x4d, 0xaf, 0xbf, 0x95, 0x12, 0x45, 0x4b, 0x23,
            0x0a, 0xb6,
        ]
    } else {
        [
            0x08, 0xab, 0xe2, 0x1c, 0x20, 0xd4, 0x11, 0xd6, 0x8d, 0xf6, 0, 0x03, 0x93, 0x5a, 0x76,
            0xb2,
        ]
    });
    let mut plugin = ptr::null_mut();
    let mut score = 0;
    let code = unsafe {
        IOCreatePlugInInterfaceForService(
            service.0,
            client_id.as_concrete_TypeRef(),
            plugin_id.as_concrete_TypeRef(),
            &mut plugin,
            &mut score,
        )
    };
    if code != 0 {
        return Err(native_error("smart_open", code));
    }
    if plugin.is_null() {
        return Err(unsupported("smart_plugin"));
    }
    let plugin = Plugin(plugin);
    let mut interface = ptr::null_mut();
    let code = unsafe {
        ((**plugin.0).query)(
            plugin.0.cast(),
            core_foundation::uuid::CFUUIDGetUUIDBytes(interface_id.as_concrete_TypeRef()),
            &mut interface,
        )
    };
    if code != 0 {
        return Err(native_error("smart_interface", code));
    }
    if interface.is_null() {
        return Err(unsupported("smart_interface"));
    }
    let interface = Interface(interface.cast());
    let read = unsafe {
        if nvme {
            (**interface.0.cast::<*mut NvmeInterface>()).read
        } else {
            (**interface.0.cast::<*mut AtaInterface>()).read
        }
    };
    let read = read.ok_or_else(|| unsupported("smart_read_method"))?;
    let mut bytes = [0u8; 512];
    // Reading ATA SMART never enables operations, runs tests, or changes autosave.
    let code = unsafe { read(interface.0.cast(), bytes.as_mut_ptr()) };
    if code != 0 {
        return Err(native_error("smart_read", code));
    }
    if !super::super::disk::matches_mounted_identity(volume) {
        return Err(PlatformError::item_changed(
            "disk_temperature stage=volume_verify outcome=changed",
        ));
    }
    let prediction = if nvme || !include_health {
        None
    } else {
        let status = unsafe { (**interface.0.cast::<*mut AtaInterface>()).status };
        status.and_then(|status| {
            let mut exceeded = 0u8;
            (unsafe { status(interface.0.cast(), &mut exceeded) } == 0).then_some(exceeded != 0)
        })
    };
    let identity = if !nvme && include_health {
        ata_identity(service.0)
    } else {
        Default::default()
    };
    Ok((bytes, nvme, prediction, identity))
}

pub(super) fn read(volume: &ResourceVolume) -> PlatformResult<(f64, DiskTemperatureSource)> {
    let (bytes, nvme, _, _) = read_smart(volume, false)?;
    if nvme {
        Ok((nvme_temperature(&bytes)?, DiskTemperatureSource::NvmeSmart))
    } else {
        Ok((ata_temperature(&bytes)?, DiskTemperatureSource::AtaSmart))
    }
}

pub(super) fn read_health(
    volume: &ResourceVolume,
) -> PlatformResult<super::super::disk_health::DiskHealth> {
    use super::super::disk_health::{self, DiskHealthStatus};
    let (bytes, nvme, prediction, identity) = read_smart(volume, true)?;
    if nvme {
        return disk_health::nvme(volume, &bytes);
    }
    let status = match prediction {
        Some(true) => DiskHealthStatus::Warning,
        Some(false) => DiskHealthStatus::Healthy,
        None => DiskHealthStatus::Unknown,
    };
    disk_health::ata::read(volume, &bytes, status, identity)
}
