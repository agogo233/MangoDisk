//! Fixed Intel DTS and AMD Zen temperature reads; register identifiers never cross IPC.
use super::{deployment, pci_bus::PciBus, security};
use crate::cpu_temperature::{amd_zen_celsius, CpuTemperatureSensor};
use mangodisk_privileged_protocol::{intel_package_celsius, CapabilityStatus, TemperatureReading};
use std::{
    ffi::c_void,
    io, ptr,
    time::{SystemTime, UNIX_EPOCH},
};
use windows_sys::Win32::{
    Foundation::{FreeLibrary, HMODULE},
    System::{
        LibraryLoader::{
            GetProcAddress, LoadLibraryExW, LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR,
            LOAD_LIBRARY_SEARCH_SYSTEM32,
        },
        SystemInformation::GROUP_AFFINITY,
        Threading::{GetCurrentThread, SetThreadGroupAffinity},
    },
};

#[cfg(target_arch = "x86_64")]
use windows_sys::Win32::System::SystemInformation::{
    GetLogicalProcessorInformationEx, RelationProcessorPackage,
};

type Open = unsafe extern "system" fn(*mut *mut c_void) -> i32;
type Load = unsafe extern "system" fn(*mut c_void, *const u8, usize) -> i32;
type Execute = unsafe extern "system" fn(
    *mut c_void,
    *const u8,
    *const u64,
    usize,
    *mut u64,
    usize,
    *mut usize,
) -> i32;
type Close = unsafe extern "system" fn(*mut c_void) -> i32;

pub(crate) fn sensor() -> Option<CpuTemperatureSensor> {
    #[cfg(target_arch = "x86_64")]
    {
        let vendor = std::arch::x86_64::__cpuid(0);
        let identity = std::arch::x86_64::__cpuid(1);
        let thermal = std::arch::x86_64::__cpuid(6);
        let mut brand_bytes = Vec::new();
        if std::arch::x86_64::__cpuid(0x8000_0000).eax >= 0x8000_0004 {
            for leaf in 0x8000_0002..=0x8000_0004 {
                let data = std::arch::x86_64::__cpuid(leaf);
                for word in [data.eax, data.ebx, data.ecx, data.edx] {
                    brand_bytes.extend_from_slice(&word.to_le_bytes());
                }
            }
        }
        let brand = String::from_utf8_lossy(&brand_bytes);
        // VBS may set the hypervisor bit on physical hardware. A valid native
        // sensor read remains mandatory, rather than rejecting that bit alone.
        let sensor = crate::cpu_temperature::identify(
            [vendor.ebx, vendor.edx, vendor.ecx],
            identity.eax,
            thermal.eax,
            brand.trim_matches(['\0', ' ']),
        )?;
        single_package().then_some(sensor)
    }
    #[cfg(not(target_arch = "x86_64"))]
    {
        None
    }
}
pub(crate) fn supported() -> bool {
    sensor().is_some()
}
pub(crate) fn module_artifact() -> Option<&'static crate::artifacts::Artifact> {
    sensor().map(|sensor| match sensor {
        CpuTemperatureSensor::IntelPackageDts => &crate::artifacts::INTEL_MODULE,
        CpuTemperatureSensor::AmdZen { .. } => &crate::artifacts::AMD_ZEN_MODULE,
    })
}
#[cfg(target_arch = "x86_64")]
fn single_package() -> bool {
    let mut length = 0;
    unsafe {
        GetLogicalProcessorInformationEx(RelationProcessorPackage, ptr::null_mut(), &mut length)
    };
    if !(8..=1024 * 1024).contains(&length) {
        return false;
    }
    let mut buffer = vec![0u64; (length as usize).div_ceil(8)];
    if unsafe {
        GetLogicalProcessorInformationEx(
            RelationProcessorPackage,
            buffer.as_mut_ptr().cast(),
            &mut length,
        )
    } == 0
    {
        return false;
    }
    let bytes =
        unsafe { std::slice::from_raw_parts(buffer.as_ptr().cast::<u8>(), length as usize) };
    let mut cursor = 0;
    let mut packages = 0;
    while cursor + 8 <= bytes.len() {
        let relation = u32::from_le_bytes(
            bytes[cursor..cursor + 4]
                .try_into()
                .expect("header bounded"),
        );
        let size = u32::from_le_bytes(
            bytes[cursor + 4..cursor + 8]
                .try_into()
                .expect("header bounded"),
        ) as usize;
        if size < 8 || size > bytes.len() - cursor || relation != RelationProcessorPackage as u32 {
            return false;
        }
        packages += 1;
        cursor += size;
    }
    cursor == bytes.len() && packages == 1
}
pub fn hardware_supported() -> bool {
    supported()
}

pub(crate) struct Reader {
    library: HMODULE,
    handle: *mut c_void,
    execute: Execute,
    close: Close,
    sensor: CpuTemperatureSensor,
    pci_bus: Option<PciBus>,
}
impl Reader {
    pub(crate) fn new() -> Result<Self, CapabilityStatus> {
        let sensor = sensor().ok_or(CapabilityStatus::UnsupportedHardware)?;
        let artifact = match sensor {
            CpuTemperatureSensor::IntelPackageDts => &crate::artifacts::INTEL_MODULE,
            CpuTemperatureSensor::AmdZen { .. } => &crate::artifacts::AMD_ZEN_MODULE,
        };
        if !deployment::driver_available() {
            return Err(CapabilityStatus::DriverMissing);
        }
        let directory = security::program_files()
            .map_err(|_| CapabilityStatus::DriverMissing)?
            .join("PawnIO");
        security::reject_reparse(&directory).map_err(|_| CapabilityStatus::DriverMissing)?;
        let dll = directory.join("PawnIOLib.dll");
        security::reject_reparse(&dll).map_err(|_| CapabilityStatus::DriverMissing)?;
        let module = deployment::read_verified(
            &deployment::install_directory()
                .map_err(|_| CapabilityStatus::SensorUnavailable)?
                .join(artifact.file_name),
            artifact,
        )
        .map_err(|error| {
            super::diagnostics::error(format!(
                "cpu_temperature_module_verification_failed module={} error={error}",
                artifact.file_name
            ));
            CapabilityStatus::SensorUnavailable
        })?;
        let pci_bus = if matches!(sensor, CpuTemperatureSensor::AmdZen { .. }) {
            Some(PciBus::new().map_err(|error| {
                super::diagnostics::error(format!(
                    "cpu_temperature_pci_mutex_open_failed error={error}"
                ));
                CapabilityStatus::SensorUnavailable
            })?)
        } else {
            None
        };
        let library = unsafe {
            LoadLibraryExW(
                security::wide(&dll).as_ptr(),
                ptr::null_mut(),
                LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR | LOAD_LIBRARY_SEARCH_SYSTEM32,
            )
        };
        if library.is_null() {
            super::diagnostics::error(format!(
                "pawnio_library_load_failed error={}",
                io::Error::last_os_error()
            ));
            return Err(CapabilityStatus::DriverMissing);
        }
        let mut handle = ptr::null_mut();
        let result = (|| {
            let resolve = |name: &std::ffi::CStr| {
                unsafe { GetProcAddress(library, name.as_ptr().cast()) }
                    .ok_or(CapabilityStatus::DriverMissing)
            };
            let open: Open = unsafe { std::mem::transmute(resolve(c"pawnio_open")?) };
            let load: Load = unsafe { std::mem::transmute(resolve(c"pawnio_load")?) };
            let execute: Execute = unsafe { std::mem::transmute(resolve(c"pawnio_execute")?) };
            let close: Close = unsafe { std::mem::transmute(resolve(c"pawnio_close")?) };
            let hr = unsafe { open(&mut handle) };
            if hr < 0 {
                super::diagnostics::error(format!("pawnio_open_failed HRESULT=0x{hr:08x}"));
                return Err(if hr as u32 == 0x80070005 {
                    CapabilityStatus::DriverAccessDenied
                } else {
                    CapabilityStatus::DriverMissing
                });
            }
            let hr = unsafe { load(handle, module.as_ptr(), module.len()) };
            if hr < 0 {
                super::diagnostics::error(format!("pawnio_module_load_failed HRESULT=0x{hr:08x}"));
                unsafe { close(handle) };
                handle = ptr::null_mut();
                return Err(CapabilityStatus::SensorUnavailable);
            }
            super::diagnostics::info(format!("cpu_temperature_collector_ready sensor={sensor:?} module={} sample_interval_ms=4000", artifact.file_name));
            Ok(Self {
                library,
                handle,
                execute,
                close,
                sensor,
                pci_bus,
            })
        })();
        if result.is_err() {
            unsafe { FreeLibrary(library) };
        }
        result
    }
    fn read_register(&self, entry: &std::ffi::CStr, register: u64) -> io::Result<u64> {
        let mut value = 0;
        let mut count = 0;
        let hr = unsafe {
            (self.execute)(
                self.handle,
                entry.as_ptr().cast(),
                &register,
                1,
                &mut value,
                1,
                &mut count,
            )
        };
        if hr < 0 || count != 1 {
            return Err(io::Error::other(format!("CPU temperature register read failed entry={} register=0x{register:x} HRESULT=0x{hr:08x} count={count}", entry.to_string_lossy())));
        }
        Ok(value)
    }
    pub(crate) fn read(&mut self) -> Result<TemperatureReading, CapabilityStatus> {
        let result = (|| {
            let celsius = match self.sensor {
                CpuTemperatureSensor::IntelPackageDts => {
                    // Pin both register reads to the same package, then restore the service thread.
                    let _affinity = PackageAffinity::bind()?;
                    let target = self.read_register(c"ioctl_read_msr", 0x1a2)?;
                    let thermal = self.read_register(c"ioctl_read_msr", 0x1b1)?;
                    intel_package_celsius(target, thermal)
                        .ok_or_else(|| io::Error::other("Intel package DTS is invalid"))?
                }
                CpuTemperatureSensor::AmdZen { tdie_offset } => {
                    // PawnIO selects the SMN index before reading its data. Serialize
                    // that transaction across processes; no control register is exposed.
                    let _guard = self
                        .pci_bus
                        .as_ref()
                        .expect("AMD collector owns PCI mutex")
                        .acquire()?;
                    let raw = self.read_register(c"ioctl_read_smn", 0x59800)?;
                    amd_zen_celsius(raw, tdie_offset).ok_or_else(|| io::Error::other(format!("AMD Zen temperature is invalid register=0x59800 raw={raw:#x} tdie_offset={tdie_offset}")))?
                }
            };
            Ok::<_, io::Error>(TemperatureReading {
                celsius,
                sampled_at_ms: now_ms(),
            })
        })();
        result.map_err(|error| {
            super::diagnostics::error(format!(
                "cpu_temperature_read_failed sensor={:?} error={error}",
                self.sensor
            ));
            CapabilityStatus::SensorUnavailable
        })
    }
}

struct PackageAffinity(GROUP_AFFINITY);
impl PackageAffinity {
    fn bind() -> io::Result<Self> {
        let affinity = GROUP_AFFINITY {
            Mask: 1,
            Group: 0,
            ..Default::default()
        };
        let mut previous = GROUP_AFFINITY::default();
        if unsafe { SetThreadGroupAffinity(GetCurrentThread(), &affinity, &mut previous) } == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(Self(previous))
    }
}
impl Drop for PackageAffinity {
    fn drop(&mut self) {
        unsafe { SetThreadGroupAffinity(GetCurrentThread(), &self.0, ptr::null_mut()) };
    }
}
impl Drop for Reader {
    fn drop(&mut self) {
        unsafe {
            (self.close)(self.handle);
            FreeLibrary(self.library);
        }
    }
}
pub(crate) fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}
