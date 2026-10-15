use std::{
    io,
    mem::size_of,
    os::windows::ffi::OsStrExt,
    path::{Path, PathBuf},
    ptr,
};
use windows_sys::Win32::{
    Foundation::LocalFree,
    Security::{
        Authorization::ConvertStringSecurityDescriptorToSecurityDescriptorW,
        SetKernelObjectSecurity, DACL_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR,
        SECURITY_ATTRIBUTES,
    },
    Storage::FileSystem::{
        GetFileAttributesW, FILE_ATTRIBUTE_REPARSE_POINT, INVALID_FILE_ATTRIBUTES,
    },
    System::{Com::CoTaskMemFree, Threading::GetCurrentProcess},
    UI::Shell::{FOLDERID_LocalAppData, FOLDERID_ProgramFiles, SHGetKnownFolderPath},
};

pub(crate) fn wide(value: impl AsRef<std::ffi::OsStr>) -> Vec<u16> {
    value.as_ref().encode_wide().chain(Some(0)).collect()
}
pub(crate) fn allow_process_identity_query() -> io::Result<()> {
    // Client authentication needs only the image path. Grant interactive users
    // QUERY_LIMITED_INFORMATION, never memory access, termination, or duplication.
    let descriptor = Descriptor::new("D:P(A;;GA;;;SY)(A;;GA;;;BA)(A;;0x1000;;;IU)")?;
    if unsafe {
        SetKernelObjectSecurity(
            GetCurrentProcess(),
            DACL_SECURITY_INFORMATION,
            descriptor.raw(),
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}
pub(crate) fn program_files() -> io::Result<PathBuf> {
    known_folder(&FOLDERID_ProgramFiles, "Program Files")
}
pub(crate) fn local_app_data() -> io::Result<PathBuf> {
    known_folder(&FOLDERID_LocalAppData, "LocalAppData")
}
fn known_folder(id: &windows_sys::core::GUID, name: &str) -> io::Result<PathBuf> {
    let mut raw = ptr::null_mut();
    let hr = unsafe { SHGetKnownFolderPath(id, 0, ptr::null_mut(), &mut raw) };
    if hr < 0 {
        return Err(io::Error::other(format!(
            "{name} lookup failed HRESULT=0x{hr:08x}"
        )));
    }
    let mut length = 0;
    unsafe {
        while *raw.add(length) != 0 {
            length += 1;
        }
    }
    let result = PathBuf::from(String::from_utf16_lossy(unsafe {
        std::slice::from_raw_parts(raw, length)
    }));
    unsafe { CoTaskMemFree(raw.cast()) };
    reject_reparse(&result)?;
    Ok(result)
}
pub(crate) fn reject_reparse(path: &Path) -> io::Result<()> {
    let attributes = unsafe { GetFileAttributesW(wide(path).as_ptr()) };
    if attributes == INVALID_FILE_ATTRIBUTES {
        return Err(io::Error::last_os_error());
    }
    if attributes & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "privileged path is a reparse point",
        ));
    }
    Ok(())
}
pub(crate) struct Descriptor(PSECURITY_DESCRIPTOR);
impl Descriptor {
    pub(crate) fn new(sddl: &str) -> io::Result<Self> {
        let mut raw = ptr::null_mut();
        if unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                wide(sddl).as_ptr(),
                1,
                &mut raw,
                ptr::null_mut(),
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        Ok(Self(raw))
    }
    pub(crate) fn attributes(&self) -> SECURITY_ATTRIBUTES {
        SECURITY_ATTRIBUTES {
            nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: self.0,
            bInheritHandle: 0,
        }
    }
    pub(crate) fn raw(&self) -> PSECURITY_DESCRIPTOR {
        self.0
    }
}
impl Drop for Descriptor {
    fn drop(&mut self) {
        unsafe { LocalFree(self.0) };
    }
}
