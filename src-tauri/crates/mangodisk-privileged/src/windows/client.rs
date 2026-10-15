//! Bounded ordinary-user client with SCM and native pipe-server authentication.
use super::{deployment, security};
use mangodisk_privileged_protocol::{self as protocol, Operation, Request, Response, SERVICE_NAME};
use std::{
    io,
    mem::size_of,
    path::{Path, PathBuf},
    ptr,
    time::{Duration, Instant},
};
use windows_service::{
    service::{ServiceAccess, ServiceState},
    service_manager::{ServiceManager, ServiceManagerAccess},
};
use windows_sys::Win32::{
    Foundation::{
        CloseHandle, GetLastError, ERROR_IO_PENDING, ERROR_PIPE_BUSY, HANDLE, INVALID_HANDLE_VALUE,
        WAIT_OBJECT_0,
    },
    Storage::FileSystem::{CreateFileW, ReadFile, WriteFile, FILE_FLAG_OVERLAPPED, OPEN_EXISTING},
    System::{
        Pipes::GetNamedPipeServerProcessId,
        Threading::{
            CreateEventW, GetExitCodeProcess, OpenProcess, QueryFullProcessImageNameW, ResetEvent,
            WaitForSingleObject, INFINITE, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
        },
        IO::{CancelIoEx, GetOverlappedResult, OVERLAPPED},
    },
    UI::{
        Shell::{ShellExecuteExW, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW},
        WindowsAndMessaging::SW_HIDE,
    },
};

struct Handle(HANDLE);
impl Drop for Handle {
    fn drop(&mut self) {
        unsafe { CloseHandle(self.0) };
    }
}

pub fn service_installed() -> io::Result<bool> {
    let manager = ServiceManager::local_computer(None::<&str>, ServiceManagerAccess::CONNECT)
        .map_err(io::Error::other)?;
    match manager.open_service(SERVICE_NAME, ServiceAccess::QUERY_STATUS) {
        Ok(_) => Ok(true),
        Err(windows_service::Error::Winapi(error)) if error.raw_os_error() == Some(1060) => {
            Ok(false)
        }
        Err(error) => Err(io::Error::other(error)),
    }
}
fn authenticate_server(pipe: HANDLE) -> io::Result<()> {
    let mut pid = 0;
    if unsafe { GetNamedPipeServerProcessId(pipe, &mut pid) } == 0 {
        return Err(io::Error::other(format!(
            "pipe_server_identity_failed error={}",
            io::Error::last_os_error()
        )));
    }
    let manager = ServiceManager::local_computer(None::<&str>, ServiceManagerAccess::CONNECT)
        .map_err(io::Error::other)?;
    let service = manager
        .open_service(SERVICE_NAME, ServiceAccess::QUERY_STATUS)
        .map_err(io::Error::other)?;
    let state = service.query_status().map_err(io::Error::other)?;
    if state.current_state != ServiceState::Running || pid == 0 || state.process_id != Some(pid) {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "named pipe peer is not the registered service",
        ));
    }
    let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
    if process.is_null() {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            format!(
                "service_process_query_failed error={}",
                io::Error::last_os_error()
            ),
        ));
    }
    let process = Handle(process);
    let mut path = vec![0u16; 32768];
    let mut count = path.len() as u32;
    if unsafe {
        QueryFullProcessImageNameW(process.0, PROCESS_NAME_WIN32, path.as_mut_ptr(), &mut count)
    } == 0
    {
        return Err(io::Error::other(format!(
            "service_image_query_failed error={}",
            io::Error::last_os_error()
        )));
    }
    let directory = deployment::install_directory()?;
    security::reject_reparse(&directory).map_err(|error| {
        io::Error::other(format!(
            "service_directory_verification_failed error={error}"
        ))
    })?;
    let expected = directory.join(deployment::EXECUTABLE_NAME);
    security::reject_reparse(&expected).map_err(|error| {
        io::Error::other(format!("service_file_verification_failed error={error}"))
    })?;
    let actual = PathBuf::from(String::from_utf16_lossy(&path[..count as usize]));
    if !actual
        .to_string_lossy()
        .eq_ignore_ascii_case(&expected.to_string_lossy())
    {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "service image is outside its protected installation",
        ));
    }
    Ok(())
}

fn transfer(pipe: HANDLE, bytes: &mut [u8], writing: bool, deadline: Instant) -> io::Result<()> {
    let event = unsafe { CreateEventW(ptr::null(), 1, 0, ptr::null()) };
    if event.is_null() {
        return Err(io::Error::last_os_error());
    }
    let event = Handle(event);
    let mut cursor = 0;
    while cursor < bytes.len() {
        unsafe { ResetEvent(event.0) };
        let mut overlapped = OVERLAPPED {
            hEvent: event.0,
            ..Default::default()
        };
        let mut count = 0;
        let remaining = &mut bytes[cursor..];
        let success = unsafe {
            if writing {
                WriteFile(
                    pipe,
                    remaining.as_ptr(),
                    remaining.len() as u32,
                    &mut count,
                    &mut overlapped,
                )
            } else {
                ReadFile(
                    pipe,
                    remaining.as_mut_ptr(),
                    remaining.len() as u32,
                    &mut count,
                    &mut overlapped,
                )
            }
        };
        if success == 0 {
            let code = unsafe { GetLastError() };
            if code != ERROR_IO_PENDING {
                return Err(io::Error::from_raw_os_error(code as i32));
            }
            let timeout = deadline
                .saturating_duration_since(Instant::now())
                .as_millis()
                .min(u32::MAX as u128) as u32;
            if unsafe { WaitForSingleObject(event.0, timeout) } != WAIT_OBJECT_0 {
                // The buffer and OVERLAPPED must remain alive until cancellation
                // completes, including a race with normal completion.
                unsafe {
                    CancelIoEx(pipe, &overlapped);
                    GetOverlappedResult(pipe, &overlapped, &mut count, 1);
                }
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "privileged IPC deadline exceeded",
                ));
            }
            if unsafe { GetOverlappedResult(pipe, &overlapped, &mut count, 0) } == 0 {
                return Err(io::Error::last_os_error());
            }
        }
        if count == 0 {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "privileged IPC peer closed",
            ));
        }
        cursor += count as usize;
    }
    Ok(())
}
pub fn request(operation: Operation) -> io::Result<Response> {
    let deadline = Instant::now() + Duration::from_secs(2);
    let handle = loop {
        // Specific data rights avoid granting clients CREATE_PIPE_INSTANCE.
        let handle = unsafe {
            CreateFileW(
                security::wide(protocol::PIPE_NAME).as_ptr(),
                0x120183,
                0,
                ptr::null(),
                OPEN_EXISTING,
                FILE_FLAG_OVERLAPPED,
                ptr::null_mut(),
            )
        };
        if handle != INVALID_HANDLE_VALUE {
            break Handle(handle);
        }
        let code = unsafe { GetLastError() };
        if code != ERROR_PIPE_BUSY || Instant::now() >= deadline {
            let error = io::Error::from_raw_os_error(code as i32);
            return Err(io::Error::new(
                error.kind(),
                format!("pipe_open_failed error={error}"),
            ));
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    authenticate_server(handle.0).map_err(|error| {
        io::Error::new(
            error.kind(),
            format!("ipc_authentication_failed error={error}"),
        )
    })?;
    let request = Request::new(operation);
    let mut frame = protocol::encode(&request)?;
    transfer(handle.0, &mut frame, true, deadline).map_err(|error| {
        io::Error::new(
            error.kind(),
            format!("ipc_request_write_failed error={error}"),
        )
    })?;
    let mut header = [0; 4];
    transfer(handle.0, &mut header, false, deadline).map_err(|error| {
        io::Error::new(
            error.kind(),
            format!("ipc_header_read_failed error={error}"),
        )
    })?;
    let length = u32::from_le_bytes(header) as usize;
    protocol::frame_length(length)?;
    let mut body = vec![0; length];
    transfer(handle.0, &mut body, false, deadline)?;
    let response: Response = serde_json::from_slice(&body)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    response.validate(&request)?;
    Ok(response)
}

/// Fixed setup operations; callers cannot supply a command or staging path.
pub enum SetupAction {
    InstallCpuTemperature,
    Uninstall { remove_driver: bool },
}

/// Stage only the desktop's embedded image and keep it immutable through elevation.
pub fn elevate_embedded(bytes: &[u8], action: SetupAction) -> io::Result<u32> {
    let bootstrap = super::bootstrap::prepare(bytes)?;
    elevate(bootstrap.path(), action)
}

fn elevate(bootstrap: &Path, action: SetupAction) -> io::Result<u32> {
    let file = security::wide(bootstrap);
    let parameters = match action {
        SetupAction::InstallCpuTemperature => "--install-cpu-temperature",
        SetupAction::Uninstall {
            remove_driver: false,
        } => "--uninstall",
        SetupAction::Uninstall {
            remove_driver: true,
        } => "--uninstall-with-pawnio",
    };
    let parameters = security::wide(parameters);
    let verb = security::wide("runas");
    let mut execution = SHELLEXECUTEINFOW {
        cbSize: size_of::<SHELLEXECUTEINFOW>() as u32,
        fMask: SEE_MASK_NOCLOSEPROCESS,
        lpVerb: verb.as_ptr(),
        lpFile: file.as_ptr(),
        lpParameters: parameters.as_ptr(),
        nShow: SW_HIDE,
        ..Default::default()
    };
    if unsafe { ShellExecuteExW(&mut execution) } == 0 {
        return Err(io::Error::last_os_error());
    }
    if execution.hProcess.is_null() {
        return Err(io::Error::other("Windows returned no installation process"));
    }
    let process = Handle(execution.hProcess);
    if unsafe { WaitForSingleObject(process.0, 180_000) } != WAIT_OBJECT_0 {
        // Do not release the app operation gate while the installer is running.
        // A blocking worker owns this wait; the frontend remains responsive and reports the busy state.
        if unsafe { WaitForSingleObject(process.0, INFINITE) } != WAIT_OBJECT_0 {
            return Err(io::Error::last_os_error());
        }
    }
    let mut code = 0;
    if unsafe { GetExitCodeProcess(process.0, &mut code) } == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(code)
}
