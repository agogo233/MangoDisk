#[cfg(not(windows))]
use std::fs;
use std::path::Path;

// Callers own staging, syncing, permissions, and cleanup. This primitive only preserves the
// platform-specific replacement contract shared by persisted records and privacy sources.

#[cfg(not(windows))]
pub(crate) fn replace_file(source: &Path, target: &Path) -> std::io::Result<()> {
    fs::rename(source, target)
}

#[cfg(windows)]
pub(crate) fn replace_file(source: &Path, target: &Path) -> std::io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{
        MoveFileExW, MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH,
    };

    let source = source
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect::<Vec<_>>();
    let target = target
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect::<Vec<_>>();
    let succeeded = unsafe {
        MoveFileExW(
            source.as_ptr(),
            target.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    };
    if succeeded == 0 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::replace_file;
    use std::fs;

    #[test]
    fn replaces_existing_content_and_consumes_the_staged_file() {
        let directory = tempfile::tempdir().unwrap();
        let staged = directory.path().join("staged.json");
        let target = directory.path().join("record.json");
        fs::write(&staged, b"replacement").unwrap();
        fs::write(&target, b"original").unwrap();
        replace_file(&staged, &target).unwrap();
        assert_eq!(fs::read(&target).unwrap(), b"replacement");
        assert!(!staged.exists());
    }

    #[test]
    fn failed_replacement_preserves_existing_target_content() {
        let directory = tempfile::tempdir().unwrap();
        let target = directory.path().join("record.json");
        fs::write(&target, b"original").unwrap();
        assert!(replace_file(&directory.path().join("missing.json"), &target).is_err());
        assert_eq!(fs::read(&target).unwrap(), b"original");
    }
}
