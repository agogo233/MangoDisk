//! Resident configuration has explicit commits and must not participate in plugin exit saves.
use serde_json::{Map, Value};
use std::{
    fs::{self, File},
    io::{self, Write},
    path::Path,
};

pub(super) fn read(path: &Path) -> io::Result<Map<String, Value>> {
    match fs::read(path) {
        Ok(bytes) => serde_json::from_slice(&bytes)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(Map::new()),
        Err(error) => Err(error),
    }
}

pub(super) fn write(path: &Path, content: &[u8]) -> io::Result<()> {
    write_with(path, |file| file.write_all(content))
}

fn write_with(path: &Path, write: impl FnOnce(&mut File) -> io::Result<()>) -> io::Result<()> {
    let parent = path.parent().ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidInput, "preference path has no parent")
    })?;
    let permissions = match fs::metadata(path) {
        Ok(metadata) => {
            let permissions = metadata.permissions();
            if permissions.readonly() {
                return Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "resident preference file is read-only",
                ));
            }
            Some(permissions)
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => None,
        Err(error) => return Err(error),
    };
    fs::create_dir_all(parent)?;
    let mut staged = tempfile::Builder::new()
        .prefix(".resident-")
        .tempfile_in(parent)?;
    if let Some(permissions) = permissions {
        staged.as_file().set_permissions(permissions)?;
    }
    write(staged.as_file_mut())?;
    staged.as_file().sync_all()?;
    // A failed write or replacement drops only the staging file. The existing
    // document remains authoritative until the atomic rename succeeds.
    staged.persist(path).map_err(|error| error.error)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explicit_replacements_preserve_other_document_keys() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("resident.json");
        let mut document: Map<String, Value> = serde_json::from_value(serde_json::json!({
            "preferences": { "schemaVersion": 25 }, "other": { "keep": true }
        }))
        .unwrap();
        fs::write(&path, serde_json::to_vec(&document).unwrap()).unwrap();
        document.insert(
            "preferences".into(),
            serde_json::json!({ "schemaVersion": 28 }),
        );
        write(&path, &serde_json::to_vec_pretty(&document).unwrap()).unwrap();
        assert_eq!(read(&path).unwrap(), document);
        assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 1);
    }

    #[test]
    fn partial_write_failure_preserves_original_and_removes_staging_file() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("resident.json");
        let original = b"original preference document";
        fs::write(&path, original).unwrap();
        let result = write_with(&path, |file| {
            file.write_all(b"partial replacement")?;
            Err(io::Error::other("injected storage failure"))
        });
        assert!(result.is_err());
        assert_eq!(fs::read(&path).unwrap(), original);
        assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 1);
    }

    #[test]
    fn failed_replacement_does_not_remove_an_existing_directory() {
        let directory = tempfile::tempdir().unwrap();
        let target = directory.path().join("resident.json");
        fs::create_dir(&target).unwrap();
        fs::write(target.join("keep"), b"original").unwrap();
        assert!(write(&target, b"replacement").is_err());
        assert_eq!(fs::read(target.join("keep")).unwrap(), b"original");
        assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 1);
    }

    #[test]
    fn missing_records_start_empty_and_malformed_records_remain_untouched() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("resident.json");
        assert!(read(&path).unwrap().is_empty());
        assert!(!path.exists());
        for malformed in [b"{".as_slice(), b"[]", b"null"] {
            fs::write(&path, malformed).unwrap();
            assert!(read(&path).is_err());
            assert_eq!(fs::read(&path).unwrap(), malformed);
        }
    }

    #[test]
    fn read_only_records_are_not_replaced() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("resident.json");
        fs::write(&path, b"original").unwrap();
        let original_permissions = fs::metadata(&path).unwrap().permissions();
        let mut read_only = original_permissions.clone();
        read_only.set_readonly(true);
        fs::set_permissions(&path, read_only).unwrap();
        let result = write(&path, b"replacement");
        fs::set_permissions(&path, original_permissions).unwrap();
        assert_eq!(result.unwrap_err().kind(), io::ErrorKind::PermissionDenied);
        assert_eq!(fs::read(&path).unwrap(), b"original");
    }
}
