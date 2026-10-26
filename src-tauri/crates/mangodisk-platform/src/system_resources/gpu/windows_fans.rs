//! Read-only NVAPI fallback when a driver omits the optional WDDM tachometer.
//! Public ABI: NVIDIA/nvapi, nvapi_interface.h. Optional client fan-status ABI:
//! LibreHardwareMonitor v0.9.6, Interop/NvApi.cs (read-only, fixed V1 layout).
use super::windows_fan_values::CoolerStatus;
use crate::{PlatformError, PlatformErrorCode, PlatformResult};
use std::{
    collections::HashMap,
    ffi::c_void,
    ptr,
    time::{Duration, Instant},
};
use windows_sys::{
    core::w,
    Win32::{
        Foundation::{FreeLibrary, GetLastError, HMODULE},
        System::LibraryLoader::{GetProcAddress, LoadLibraryExW, LOAD_LIBRARY_SEARCH_SYSTEM32},
    },
};

type Initialize = unsafe extern "C" fn() -> i32;
type Enumerate = unsafe extern "C" fn(*mut *mut c_void, *mut u32) -> i32;
type ReadValue = unsafe extern "C" fn(*mut c_void, *mut u32) -> i32;
type ReadCoolers = unsafe extern "C" fn(*mut c_void, *mut CoolerStatus) -> i32;
type Query = unsafe extern "C" fn(u32) -> *const c_void;
const RETRY: Duration = Duration::from_secs(30);

fn library_error(code: u32) -> PlatformError {
    PlatformError::new(
        if matches!(code, 2 | 126 | 1157) {
            PlatformErrorCode::Unsupported
        } else {
            PlatformErrorCode::Io
        },
        format!("gpu_fan source=nvapi stage=system_driver_library native_code={code}"),
    )
}

fn native_error(stage: &str, code: i32) -> PlatformError {
    PlatformError::new(
        if matches!(code, -3 | -6 | -104) || (stage == "client_fan_status" && code == -9) {
            PlatformErrorCode::Unsupported
        } else {
            PlatformErrorCode::OperationFailed
        },
        format!("gpu_fan source=nvapi stage={stage} native_code={code}"),
    )
}
struct Library(HMODULE);
impl Drop for Library {
    fn drop(&mut self) {
        unsafe {
            FreeLibrary(self.0);
        }
    }
}
struct Api {
    _library: Library,
    unload: Initialize,
    enumerate: Enumerate,
    bus: ReadValue,
    tachometer: Option<ReadValue>,
    coolers: Option<ReadCoolers>,
    devices: Vec<(u32, *mut c_void)>,
    discovered: Option<Instant>,
    discovery_retry: Option<(Instant, PlatformError)>,
    retries: HashMap<u32, (Instant, PlatformError)>,
}
impl Drop for Api {
    fn drop(&mut self) {
        unsafe {
            (self.unload)();
        }
    }
}
impl Api {
    fn open() -> PlatformResult<Self> {
        #[cfg(target_pointer_width = "64")]
        let name = w!("nvapi64.dll");
        #[cfg(target_pointer_width = "32")]
        let name = w!("nvapi.dll");
        let raw = unsafe { LoadLibraryExW(name, ptr::null_mut(), LOAD_LIBRARY_SEARCH_SYSTEM32) };
        if raw.is_null() {
            return Err(library_error(unsafe { GetLastError() }));
        }
        let library = Library(raw);
        let query = unsafe { GetProcAddress(raw, c"nvapi_QueryInterface".as_ptr().cast()) }
            .ok_or_else(|| native_error("query_interface", -3))?;
        let query =
            unsafe { std::mem::transmute::<unsafe extern "system" fn() -> isize, Query>(query) };
        macro_rules! interface {
            ($id:expr, $kind:ty) => {{
                let id: u32 = $id;
                let pointer = unsafe { query(id) };
                if pointer.is_null() {
                    return Err(native_error(&format!("interface_{id:#x}"), -3));
                }
                // Required entry points use the public fixed-signature query ABI.
                unsafe { std::mem::transmute::<*const c_void, $kind>(pointer) }
            }};
        }
        let initialize = interface!(0x0150_e828, Initialize);
        let unload = interface!(0xd22b_dd7e, Initialize);
        let enumerate = interface!(0xe5ac_921f, Enumerate);
        let bus = interface!(0x1be0_b8e5, ReadValue);
        let tachometer = unsafe { query(0x5f60_8315) };
        let tachometer = (!tachometer.is_null())
            .then(|| unsafe { std::mem::transmute::<*const c_void, ReadValue>(tachometer) });
        let coolers = unsafe { query(0x35ae_d5e8) };
        let coolers = (!coolers.is_null())
            .then(|| unsafe { std::mem::transmute::<*const c_void, ReadCoolers>(coolers) });
        if tachometer.is_none() && coolers.is_none() {
            return Err(native_error("fan_interfaces", -3));
        }
        let code = unsafe { initialize() };
        if code != 0 {
            return Err(native_error("initialize", code));
        }
        Ok(Self {
            _library: library,
            unload,
            enumerate,
            bus,
            tachometer,
            coolers,
            devices: Vec::new(),
            discovered: None,
            discovery_retry: None,
            retries: HashMap::new(),
        })
    }
    fn discover(&mut self) -> PlatformResult<()> {
        if self.discovered.is_some_and(|at| at.elapsed() < RETRY) {
            return Ok(());
        }
        if let Some((_, error)) = self
            .discovery_retry
            .as_ref()
            .filter(|(until, _)| Instant::now() < *until)
        {
            return Err(error.clone());
        }
        let result = self.discover_current();
        match &result {
            Ok(()) => self.discovery_retry = None,
            Err(error) => {
                // Enumeration failures affect the whole API, not just one PCI bus.
                // Discard handles that may have been invalidated by a driver reset.
                self.devices.clear();
                self.discovered = None;
                self.discovery_retry = Some((Instant::now() + RETRY, error.clone()));
            }
        }
        result
    }
    fn discover_current(&mut self) -> PlatformResult<()> {
        let mut handles = [ptr::null_mut(); 64];
        let mut count = 0;
        let code = unsafe { (self.enumerate)(handles.as_mut_ptr(), &mut count) };
        if code != 0 {
            return Err(native_error("enumerate", code));
        }
        if count as usize > handles.len() {
            return Err(native_error("enumeration_count", -1));
        }
        let mut devices = Vec::new();
        for handle in handles
            .into_iter()
            .take(count as usize)
            .filter(|handle| !handle.is_null())
        {
            let mut bus = 0;
            if unsafe { (self.bus)(handle, &mut bus) } == 0 {
                devices.push((bus, handle));
            }
        }
        self.devices = devices;
        self.retries.clear();
        self.discovered = Some(Instant::now());
        Ok(())
    }
    fn read_tachometers(&self, handle: *mut c_void) -> PlatformResult<Vec<f64>> {
        // Newer boards can reject GetTachReading while exposing every physical fan
        // through the client status query. Never resolve any control/write entry point.
        let cooler_error = if let Some(read) = self.coolers {
            let mut status = CoolerStatus::default();
            let code = unsafe { read(handle, &mut status) };
            let result = if code == 0 {
                status.speeds()
            } else {
                Err(native_error("client_fan_status", code))
            };
            match result {
                Ok(speeds) => return Ok(speeds),
                Err(error) if error.code() == PlatformErrorCode::Unsupported => Some(error),
                Err(error) => return Err(error),
            }
        } else {
            None
        };
        let result = if let Some(read) = self.tachometer {
            let mut rpm = 0;
            let code = unsafe { read(handle, &mut rpm) };
            if code != 0 {
                Err(native_error("tachometer", code))
            } else if f64::from(rpm) > super::super::fans::MAX_FAN_RPM {
                Err(PlatformError::new(
                    PlatformErrorCode::InvalidData,
                    format!(
                        "gpu_fan source=nvapi stage=tachometer native_rpm={rpm} outcome=invalid"
                    ),
                ))
            } else {
                Ok(vec![f64::from(rpm)])
            }
        } else {
            Err(native_error("tachometer_interface", -3))
        };
        result.map_err(|error| match cooler_error {
            Some(cooler) => PlatformError::new(
                error.code(),
                format!("{}; fallback={}", cooler.diagnostic(), error.diagnostic()),
            ),
            None => error,
        })
    }

    fn read(&mut self, bus: u32) -> PlatformResult<Vec<f64>> {
        self.discover()?;
        if let Some((_, error)) = self
            .retries
            .get(&bus)
            .filter(|(at, _)| Instant::now() < *at)
        {
            return Err(error.clone());
        }
        let mut matches = self.devices.iter().filter(|(value, _)| *value == bus);
        let handle = matches.next().map(|(_, handle)| *handle);
        // A PCI bus match is accepted only when it identifies exactly one physical GPU.
        let result = match (handle, matches.next()) {
            (Some(handle), None) => self.read_tachometers(handle),
            _ => Err(PlatformError::new(PlatformErrorCode::Unsupported, format!("gpu_fan source=nvapi stage=identity pci_bus={bus} outcome=missing_or_ambiguous"))),
        };
        if let Err(error) = &result {
            self.retries
                .insert(bus, (Instant::now() + RETRY, error.clone()));
        }
        result
    }
}

#[derive(Default)]
pub(super) struct Reader {
    api: Option<Api>,
    retry: Option<(Instant, PlatformError)>,
}

fn nvidia_bus(identity: &str, physical_count: u32) -> Option<u32> {
    if physical_count != 1 {
        return None;
    }
    let parts = identity
        .strip_prefix("pci:10de:")?
        .split(':')
        .collect::<Vec<_>>();
    if parts.len() != 4 {
        return None;
    }
    u16::from_str_radix(parts[0], 16).ok()?;
    let bus = parts[1].parse().ok()?;
    if parts[2].parse::<u32>().ok()? > 31 || parts[3].parse::<u32>().ok()? > 7 {
        return None;
    }
    Some(bus)
}

impl Reader {
    pub(super) fn read(&mut self, identity: &str, physical_count: u32) -> PlatformResult<Vec<f64>> {
        let Some(bus) = nvidia_bus(identity, physical_count) else {
            return Err(PlatformError::new(
                PlatformErrorCode::Unsupported,
                "gpu_fan source=nvapi stage=identity reason=no_unambiguous_nvidia_pci_device",
            ));
        };
        if let Some((_, error)) = self.retry.as_ref().filter(|(at, _)| Instant::now() < *at) {
            return Err(error.clone());
        }
        if self.api.is_none() {
            match Api::open() {
                Ok(api) => {
                    self.api = Some(api);
                    self.retry = None;
                }
                Err(error) => {
                    self.retry = Some((Instant::now() + RETRY, error.clone()));
                    return Err(error);
                }
            }
        }
        self.api.as_mut().expect("opened NVAPI reader").read(bus)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn failed_enumeration_is_backed_off_across_adapters() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        static CALLS: AtomicUsize = AtomicUsize::new(0);
        unsafe extern "C" fn enumerate(_: *mut *mut c_void, _: *mut u32) -> i32 {
            CALLS.fetch_add(1, Ordering::SeqCst);
            -1
        }
        unsafe extern "C" fn unload() -> i32 {
            0
        }
        unsafe extern "C" fn bus(_: *mut c_void, _: *mut u32) -> i32 {
            0
        }
        let mut api = Api {
            _library: Library(ptr::null_mut()),
            unload,
            enumerate,
            bus,
            tachometer: None,
            coolers: None,
            devices: vec![(1, std::ptr::dangling_mut::<c_void>())],
            discovered: None,
            discovery_retry: None,
            retries: HashMap::new(),
        };
        for bus in [1, 2, 1] {
            assert!(api.read(bus).is_err());
        }
        assert_eq!(CALLS.load(Ordering::SeqCst), 1);
        assert!(api.devices.is_empty());
        api.discovery_retry = Some((Instant::now(), native_error("enumerate", -1)));
        assert!(api.read(1).is_err());
        assert_eq!(CALLS.load(Ordering::SeqCst), 2);
    }
    #[test]
    fn driver_library_access_failures_are_distinct_from_missing_capabilities() {
        assert_eq!(library_error(126).code(), PlatformErrorCode::Unsupported);
        assert_eq!(library_error(5).code(), PlatformErrorCode::Io);
        assert_eq!(
            native_error("tachometer", -104).code(),
            PlatformErrorCode::Unsupported
        );
        assert_eq!(
            native_error("tachometer", -1).code(),
            PlatformErrorCode::OperationFailed
        );
        assert_eq!(
            native_error("client_fan_status", -9).code(),
            PlatformErrorCode::Unsupported
        );
    }
    #[test]
    fn fallback_requires_a_complete_nvidia_identity_without_linked_device_ambiguity() {
        assert_eq!(nvidia_bus("pci:10de:2206:1:0:0", 1), Some(1));
        for (id, count) in [
            ("pci:1002:2206:1:0:0", 1),
            ("pci:10de:2206:1:0:0", 2),
            ("pci:10de:2206:1", 1),
            ("pci:10de:2206:1:32:0", 1),
            ("device:unmapped", 1),
        ] {
            assert_eq!(nvidia_bus(id, count), None);
        }
    }
}
