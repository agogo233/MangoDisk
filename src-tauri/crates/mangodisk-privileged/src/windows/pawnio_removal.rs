//! Explicit removal of the shared driver; no arbitrary paths or process termination.
use super::{deployment, diagnostics, security};
use std::{
    fs, io,
    mem::size_of,
    path::PathBuf,
    process::{Command, Stdio},
    ptr,
    time::Instant,
};
use windows_service::{
    service::{ServiceAccess, ServiceState},
    service_manager::{ServiceManager, ServiceManagerAccess},
};
use windows_sys::Win32::{
    Devices::DeviceAndDriverInstallation::{
        DiUninstallDriverW, SetupGetInfInformationW, SetupQueryInfOriginalFileInformationW,
        SetupQueryInfVersionInformationW, INFINFO_INF_NAME_IS_ABSOLUTE, SP_INF_INFORMATION,
        SP_ORIGINAL_FILE_INFO_W,
    },
    System::SystemInformation::GetSystemWindowsDirectoryW,
};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

pub(super) struct Removal {
    directory: PathBuf,
    uninstaller: Option<PathBuf>,
    packages: Vec<PathBuf>,
}
impl Removal {
    pub(super) fn prepare() -> Result<Self> {
        let directory = security::program_files()?.join("PawnIO");
        let uninstaller = directory.join("uninstall.exe");
        let uninstaller = if directory.try_exists()? {
            security::reject_reparse(&directory)?;
            if uninstaller.try_exists()? {
                security::reject_reparse(&uninstaller)?;
                deployment::verify_signature(&uninstaller)?;
                Some(uninstaller)
            } else {
                None
            }
        } else {
            None
        };
        Ok(Self {
            directory,
            uninstaller,
            packages: driver_packages()?,
        })
    }

    pub(super) fn execute(self) -> Result<()> {
        let started = Instant::now();
        diagnostics::info(format!(
            "pawnio_remove_started packages={} official_uninstaller={}",
            self.packages.len(),
            self.uninstaller.is_some()
        ));
        let mut restart = false;
        if let Some(path) = &self.uninstaller {
            // Only the signed uninstaller in the fixed machine directory is executed.
            security::reject_reparse(&self.directory)?;
            security::reject_reparse(path)?;
            deployment::verify_signature(path)?;
            let status = Command::new(path)
                .args(["-uninstall", "-silent"])
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()?;
            diagnostics::info(format!(
                "pawnio_remove_official path={} exit={:?}",
                path.display(),
                status.code()
            ));
            match status.code() {
                Some(0) => (),
                Some(3010 | 1641) => restart = true,
                code => {
                    return Err(io::Error::other(format!(
                        "PawnIO uninstaller failed exit={code:?}"
                    ))
                    .into())
                }
            }
        }
        // The official uninstaller can leave the driver-store package behind.
        for path in self.packages {
            if !path.try_exists()? {
                continue;
            }
            security::reject_reparse(&path)?;
            if !is_pawnio_package(&path)? {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "PawnIO package identity changed during removal",
                )
                .into());
            }
            let mut needs_reboot = 0;
            if unsafe {
                DiUninstallDriverW(
                    ptr::null_mut(),
                    security::wide(&path).as_ptr(),
                    0,
                    &mut needs_reboot,
                )
            } == 0
            {
                return Err(io::Error::other(format!(
                    "PawnIO package removal failed path={} error={}",
                    path.display(),
                    io::Error::last_os_error()
                ))
                .into());
            }
            restart |= needs_reboot != 0;
            diagnostics::info(format!(
                "pawnio_remove_package path={} restart_required={}",
                path.display(),
                needs_reboot != 0
            ));
        }
        let manager = ServiceManager::local_computer(None::<&str>, ServiceManagerAccess::CONNECT)?;
        match manager.open_service(
            "PawnIO",
            ServiceAccess::QUERY_STATUS | ServiceAccess::STOP | ServiceAccess::DELETE,
        ) {
            Ok(service) => {
                deployment::stop_service(&service)?;
                service.delete()?;
            }
            Err(windows_service::Error::Winapi(error)) if error.raw_os_error() == Some(1060) => (),
            Err(windows_service::Error::Winapi(error)) if error.raw_os_error() == Some(1072) => {
                restart = true
            }
            Err(error) => return Err(error.into()),
        }
        // A pending SCM record is not proof of removal; report the reboot requirement.
        match manager.open_service("PawnIO", ServiceAccess::QUERY_STATUS) {
            Ok(service) => {
                let state = service.query_status()?.current_state;
                diagnostics::info(format!("pawnio_remove_pending state={state:?}"));
                if state != ServiceState::Stopped {
                    return Err(io::Error::other("PawnIO is still running after removal").into());
                }
                restart = true;
            }
            Err(windows_service::Error::Winapi(error)) if error.raw_os_error() == Some(1060) => (),
            Err(windows_service::Error::Winapi(error)) if error.raw_os_error() == Some(1072) => {
                restart = true
            }
            Err(error) => return Err(error.into()),
        }
        if restart {
            diagnostics::info("pawnio_remove_completed outcome=restart_required");
            return Err(io::Error::from_raw_os_error(deployment::REBOOT_EXIT_CODE as i32).into());
        }
        let remaining = driver_packages()?;
        if !remaining.is_empty() || self.directory.try_exists()? {
            return Err(io::Error::other(format!(
                "PawnIO removal incomplete packages={} directory={}",
                remaining.len(),
                self.directory.display()
            ))
            .into());
        }
        diagnostics::info(format!(
            "pawnio_remove_completed outcome=removed elapsed_ms={}",
            started.elapsed().as_millis()
        ));
        Ok(())
    }
}

fn driver_packages() -> io::Result<Vec<PathBuf>> {
    let mut windows = [0u16; 32768];
    let length =
        unsafe { GetSystemWindowsDirectoryW(windows.as_mut_ptr(), windows.len() as u32) } as usize;
    if length == 0 || length >= windows.len() {
        return Err(io::Error::last_os_error());
    }
    let directory = PathBuf::from(String::from_utf16_lossy(&windows[..length])).join("INF");
    security::reject_reparse(&directory)?;
    let mut packages = Vec::new();
    for entry in fs::read_dir(directory)? {
        let path = entry?.path();
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default();
        if !published_inf(name) {
            continue;
        }
        security::reject_reparse(&path)?;
        if is_pawnio_package(&path)? {
            packages.push(path);
        }
    }
    Ok(packages)
}

fn published_inf(name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    name.strip_prefix("oem")
        .and_then(|name| name.strip_suffix(".inf"))
        .is_some_and(|digits| {
            !digits.is_empty() && digits.bytes().all(|digit| digit.is_ascii_digit())
        })
}
fn utf16_text(value: &[u16]) -> String {
    String::from_utf16_lossy(&value[..value.iter().position(|&c| c == 0).unwrap_or(value.len())])
}
fn is_pawnio_package(path: &std::path::Path) -> io::Result<bool> {
    let wide = security::wide(path);
    let mut required = 0;
    if unsafe {
        SetupGetInfInformationW(
            wide.as_ptr().cast(),
            INFINFO_INF_NAME_IS_ABSOLUTE,
            ptr::null_mut(),
            0,
            &mut required,
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    if required < size_of::<SP_INF_INFORMATION>() as u32 || required > 1024 * 1024 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "driver INF metadata exceeds bounds",
        ));
    }
    // Use an aligned backing allocation for the variable-length Windows structure.
    let mut buffer = vec![0u64; (required as usize).div_ceil(size_of::<u64>())];
    let info = buffer.as_mut_ptr().cast::<SP_INF_INFORMATION>();
    if unsafe {
        SetupGetInfInformationW(
            wide.as_ptr().cast(),
            INFINFO_INF_NAME_IS_ABSOLUTE,
            info,
            required,
            ptr::null_mut(),
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    let mut original = SP_ORIGINAL_FILE_INFO_W {
        cbSize: size_of::<SP_ORIGINAL_FILE_INFO_W>() as u32,
        ..Default::default()
    };
    if unsafe { SetupQueryInfOriginalFileInformationW(info, 0, ptr::null(), &mut original) } == 0 {
        return Err(io::Error::last_os_error());
    }
    let original_name = original.OriginalInfName;
    if !utf16_text(&original_name).eq_ignore_ascii_case("pawnio.inf") {
        return Ok(false);
    }
    let mut provider = [0u16; 1024];
    if unsafe {
        SetupQueryInfVersionInformationW(
            info,
            0,
            security::wide("Provider").as_ptr(),
            provider.as_mut_ptr(),
            provider.len() as u32,
            ptr::null_mut(),
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    if !utf16_text(&provider).eq_ignore_ascii_case("namazso") {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "unexpected PawnIO driver provider",
        ));
    }
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn published_names_reject_paths_and_other_packages() {
        for name in ["oem1.inf", "OEM26.INF", "oem0.inf"] {
            assert!(published_inf(name));
        }
        for name in [
            "oem.inf",
            "pawnio.inf",
            "oem26.inf.bak",
            "oem-1.inf",
            r"..\oem26.inf",
            "oem1/2.inf",
        ] {
            assert!(!published_inf(name));
        }
    }
}
