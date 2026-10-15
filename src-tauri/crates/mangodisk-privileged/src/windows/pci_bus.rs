//! Serialize SMN index/data transactions with other hardware monitoring tools.
use super::{diagnostics, security};
use std::io;
use windows_sys::Win32::{
    Foundation::{CloseHandle, HANDLE, WAIT_ABANDONED, WAIT_FAILED, WAIT_OBJECT_0, WAIT_TIMEOUT},
    System::Threading::{CreateMutexExW, ReleaseMutex, WaitForSingleObject, MUTEX_MODIFY_STATE},
};

pub(super) struct PciBus(HANDLE);
impl PciBus {
    pub fn new() -> io::Result<Self> {
        Self::named("Global\\Access_PCI")
    }
    fn named(name: &str) -> io::Result<Self> {
        // This public coordination object conveys no driver privileges. Match the
        // monitoring ecosystem's ACL so an ordinary client can open it first or later.
        let descriptor = security::Descriptor::new("D:(A;;GA;;;WD)")?;
        let attributes = descriptor.attributes();
        let handle = unsafe {
            CreateMutexExW(
                &attributes,
                security::wide(name).as_ptr(),
                0,
                0x0010_0000 | MUTEX_MODIFY_STATE,
            )
        };
        if handle.is_null() {
            return Err(io::Error::last_os_error());
        }
        Ok(Self(handle))
    }

    pub fn acquire(&self) -> io::Result<PciGuard<'_>> {
        match unsafe { WaitForSingleObject(self.0, 100) } {
            WAIT_OBJECT_0 => Ok(PciGuard(self)),
            WAIT_ABANDONED => {
                diagnostics::info("cpu_temperature_pci_mutex outcome=abandoned_acquired");
                Ok(PciGuard(self))
            }
            WAIT_TIMEOUT => Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "AMD temperature PCI mutex timeout wait_ms=100",
            )),
            WAIT_FAILED => Err(io::Error::last_os_error()),
            code => Err(io::Error::other(format!(
                "AMD temperature PCI mutex failed native_code={code:#x}"
            ))),
        }
    }
}

impl Drop for PciBus {
    fn drop(&mut self) {
        unsafe { CloseHandle(self.0) };
    }
}
pub(super) struct PciGuard<'a>(&'a PciBus);
impl Drop for PciGuard<'_> {
    fn drop(&mut self) {
        if unsafe { ReleaseMutex(self.0 .0) } == 0 {
            diagnostics::error(format!(
                "cpu_temperature_pci_mutex_release_failed error={}",
                io::Error::last_os_error()
            ));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pci_transactions_wait_for_other_threads_and_release_on_scope_exit() {
        let name = format!("Global\\MangoDisk.PciBus.Test.{}", std::process::id());
        let bus = PciBus::named(&name).unwrap();
        let guard = bus.acquire().unwrap();
        let other_name = name.clone();
        let outcome = std::thread::spawn(move || {
            let other = PciBus::named(&other_name).unwrap();
            other.acquire().err().unwrap().kind()
        })
        .join()
        .unwrap();
        assert_eq!(outcome, io::ErrorKind::TimedOut);
        drop(guard);
        std::thread::spawn(move || {
            let other = PciBus::named(&name).unwrap();
            let _guard = other.acquire().unwrap();
        })
        .join()
        .unwrap();
    }
}
