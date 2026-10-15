//! Diagnostic-only client: record the actual token and exercise production IPC.
#[cfg(windows)]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    use std::{fs::File, io::Write, ptr};
    use windows_sys::Win32::{
        Foundation::CloseHandle,
        Security::{GetTokenInformation, TokenElevation, TOKEN_ELEVATION, TOKEN_QUERY},
        System::Threading::{
            GetCurrentProcess, OpenProcess, OpenProcessToken, PROCESS_DUP_HANDLE,
            PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_TERMINATE, PROCESS_VM_READ,
        },
    };
    let path = std::env::args_os()
        .nth(1)
        .ok_or("diagnostic output path required")?;
    let mut output = File::create(path)?;
    let mut token = ptr::null_mut();
    if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) } == 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    let mut elevation = TOKEN_ELEVATION::default();
    let mut count = 0;
    let success = unsafe {
        GetTokenInformation(
            token,
            TokenElevation,
            (&mut elevation as *mut TOKEN_ELEVATION).cast(),
            std::mem::size_of::<TOKEN_ELEVATION>() as u32,
            &mut count,
        )
    };
    unsafe { CloseHandle(token) };
    if success == 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    writeln!(output, "token_elevation={}", elevation.TokenIsElevated)?;
    let manager = windows_service::service_manager::ServiceManager::local_computer(
        None::<&str>,
        windows_service::service_manager::ServiceManagerAccess::CONNECT,
    )?;
    let service = manager.open_service(
        mangodisk_privileged_protocol::SERVICE_NAME,
        windows_service::service::ServiceAccess::QUERY_STATUS,
    )?;
    let pid = service
        .query_status()?
        .process_id
        .ok_or("service is not running")?;
    for (name, rights) in [
        ("identity", PROCESS_QUERY_LIMITED_INFORMATION),
        ("terminate", PROCESS_TERMINATE),
        ("memory_read", PROCESS_VM_READ),
        ("duplicate_handles", PROCESS_DUP_HANDLE),
    ] {
        let handle = unsafe { OpenProcess(rights, 0, pid) };
        writeln!(output, "process_right={name} granted={}", !handle.is_null())?;
        if !handle.is_null() {
            unsafe { CloseHandle(handle) };
        }
    }
    let image = mangodisk_privileged::windows::deployment::install_directory()?
        .join(mangodisk_privileged::windows::deployment::EXECUTABLE_NAME);
    writeln!(
        output,
        "service_image_writable={}",
        std::fs::OpenOptions::new().write(true).open(image).is_ok()
    )?;
    for operation in [
        mangodisk_privileged_protocol::Operation::Health,
        mangodisk_privileged_protocol::Operation::CpuTemperature,
    ] {
        let before = std::time::Instant::now();
        writeln!(
            output,
            "operation={operation:?} response={:?} elapsed_us={}",
            mangodisk_privileged::windows::client::request(operation),
            before.elapsed().as_micros()
        )?;
    }
    if std::env::args().nth(2).as_deref() == Some("--measure") {
        for index in 0..32 {
            let before = std::time::Instant::now();
            let response = mangodisk_privileged::windows::client::request(
                mangodisk_privileged_protocol::Operation::CpuTemperature,
            );
            writeln!(
                output,
                "sample={index} elapsed_us={} response={response:?}",
                before.elapsed().as_micros()
            )?;
            std::thread::sleep(std::time::Duration::from_millis(250));
        }
    }
    Ok(())
}

#[cfg(not(windows))]
fn main() {}
