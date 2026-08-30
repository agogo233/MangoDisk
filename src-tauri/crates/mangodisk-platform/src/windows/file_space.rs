use std::{fs, os::windows::ffi::OsStrExt, path::Path};

use windows_sys::Win32::{
    Foundation::{GetLastError, SetLastError, ERROR_SUCCESS},
    Storage::FileSystem::GetCompressedFileSizeW,
};

use crate::FileSpaceUsage;

pub(super) fn usage(path: &Path, metadata: &fs::Metadata) -> FileSpaceUsage {
    let logical_bytes = metadata.len();
    let wide = path
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    let mut high = 0_u32;
    // A valid allocation can have a low word of `u32::MAX`, so clear and inspect
    // the thread-local error code instead of treating that return value alone as
    // failure.
    unsafe { SetLastError(ERROR_SUCCESS) };
    let low = unsafe { GetCompressedFileSizeW(wide.as_ptr(), &mut high) };
    if low == u32::MAX && unsafe { GetLastError() } != ERROR_SUCCESS {
        return FileSpaceUsage::logical_only(logical_bytes);
    }
    FileSpaceUsage {
        logical_bytes,
        allocated_bytes: (u64::from(high) << 32) | u64::from(low),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::{
        Foundation::HANDLE,
        System::{Ioctl::FSCTL_SET_SPARSE, IO::DeviceIoControl},
    };

    #[test]
    fn sparse_file_reports_less_allocated_space_than_logical_space() {
        let root =
            std::env::temp_dir().join(format!("mangodisk-file-space-{}", std::process::id()));
        fs::create_dir_all(&root).expect("create file-space fixture");
        let path = root.join("sparse.bin");
        let file = fs::File::create(&path).expect("create sparse fixture");
        let mut returned = 0_u32;
        let marked_sparse = unsafe {
            DeviceIoControl(
                file.as_raw_handle() as HANDLE,
                FSCTL_SET_SPARSE,
                std::ptr::null(),
                0,
                std::ptr::null_mut(),
                0,
                &mut returned,
                std::ptr::null_mut(),
            )
        };
        assert_ne!(marked_sparse, 0, "mark fixture as an NTFS sparse file");
        file.set_len(64 * 1024 * 1024)
            .expect("extend sparse fixture");
        let metadata = file.metadata().expect("read sparse fixture metadata");
        let usage = usage(&path, &metadata);

        assert_eq!(usage.logical_bytes, 64 * 1024 * 1024);
        assert!(usage.allocated_bytes < usage.logical_bytes);
        fs::remove_dir_all(root).expect("remove file-space fixture");
    }
}
