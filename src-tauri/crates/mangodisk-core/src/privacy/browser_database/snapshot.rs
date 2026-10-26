use super::NEXT_SNAPSHOT;
use crate::{CoreError, CoreResult};
use std::{
    ffi::OsString,
    fs,
    path::{Path, PathBuf},
    sync::atomic::Ordering,
    time::UNIX_EPOCH,
};

const MAX_SNAPSHOT_DATABASE_BYTES: u64 = 1024 * 1024 * 1024;
const MAX_SNAPSHOT_COMPANION_BYTES: u64 = 512 * 1024 * 1024;
const SNAPSHOT_ATTEMPTS: usize = 3;

/// Owns a sensitive, short-lived copy of a browser database and its WAL.
///
/// The snapshot content never crosses the Core protocol or enters logs. Its directory is private
/// to MangoDisk and removed on every return path. Source metadata is compared before and after
/// copying so a concurrent checkpoint or write causes a bounded retry instead of producing an
/// internally inconsistent snapshot.
pub(super) struct DatabaseSnapshot {
    pub(super) directory: PathBuf,
    path: PathBuf,
}

impl DatabaseSnapshot {
    pub(super) fn capture(source: &Path) -> CoreResult<Self> {
        for attempt in 0..SNAPSHOT_ATTEMPTS {
            let directory = create_snapshot_directory()?;
            let path = directory.join("database.sqlite");
            match copy_stable_database(source, &path) {
                Ok(()) => return Ok(Self { directory, path }),
                Err(error) if attempt + 1 < SNAPSHOT_ATTEMPTS => {
                    remove_snapshot_directory(&directory)?;
                    log::debug!(
                        "privacy_database_snapshot_retry attempt={} error={}",
                        attempt + 1,
                        mangodisk_platform::diagnostics::text(&error)
                    );
                }
                Err(error) => {
                    remove_snapshot_directory(&directory)?;
                    return Err(error);
                }
            }
        }
        Err(CoreError::operation_failed(
            "privacy database snapshot attempts exhausted",
        ))
    }

    pub(super) fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for DatabaseSnapshot {
    fn drop(&mut self) {
        if let Err(error) = remove_snapshot_directory(&self.directory) {
            log::warn!("privacy_database_snapshot_cleanup_failed error={error}");
        }
    }
}

fn remove_snapshot_directory(directory: &Path) -> CoreResult<()> {
    match fs::remove_dir_all(directory) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(CoreError::operation_failed(format!(
            "privacy database snapshot cleanup failed kind={:?}",
            error.kind()
        ))),
    }
}

fn create_snapshot_directory() -> CoreResult<PathBuf> {
    // macOS commonly exposes its temporary root through `/var`, which is itself a symlink. SQLite
    // NOFOLLOW validates every path segment, so canonicalize the system-owned root before creating
    // the private child directory while still rejecting symlinks for database files themselves.
    let temporary_root = fs::canonicalize(std::env::temp_dir()).map_err(snapshot_io_error)?;
    for _ in 0..16 {
        let nonce = NEXT_SNAPSHOT.fetch_add(1, Ordering::Relaxed);
        let directory = temporary_root.join(format!(
            "mangodisk-privacy-scan-{}-{nonce}",
            std::process::id()
        ));
        match create_private_directory(&directory) {
            Ok(()) => return Ok(directory),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => {
                return Err(CoreError::operation_failed(format!(
                    "privacy database snapshot directory failed kind={:?}",
                    error.kind()
                )))
            }
        }
    }
    Err(CoreError::operation_failed(
        "privacy database snapshot directory collision",
    ))
}

#[cfg(unix)]
fn create_private_directory(path: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::DirBuilderExt;

    let mut builder = fs::DirBuilder::new();
    builder.mode(0o700).create(path)
}

#[cfg(not(unix))]
fn create_private_directory(path: &Path) -> std::io::Result<()> {
    fs::create_dir(path)
}

fn copy_stable_database(source: &Path, destination: &Path) -> CoreResult<()> {
    let wal_source = companion_path(source, "-wal");
    let before_database = snapshot_file_identity(source, MAX_SNAPSHOT_DATABASE_BYTES)?;
    let before_wal = optional_snapshot_file_identity(&wal_source, MAX_SNAPSHOT_COMPANION_BYTES)?;

    copy_snapshot_file(source, destination, MAX_SNAPSHOT_DATABASE_BYTES)?;
    if before_wal.is_some() {
        copy_snapshot_file(
            &wal_source,
            &companion_path(destination, "-wal"),
            MAX_SNAPSHOT_COMPANION_BYTES,
        )?;
    }

    let after_database = snapshot_file_identity(source, MAX_SNAPSHOT_DATABASE_BYTES)?;
    let after_wal = optional_snapshot_file_identity(&wal_source, MAX_SNAPSHOT_COMPANION_BYTES)?;
    if before_database != after_database || before_wal != after_wal {
        return Err(CoreError::operation_failed(
            "privacy database changed while snapshotting",
        ));
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct SnapshotFileIdentity {
    bytes: u64,
    modified_ns: u128,
}

fn snapshot_file_identity(path: &Path, max_bytes: u64) -> CoreResult<SnapshotFileIdentity> {
    let metadata = fs::symlink_metadata(path).map_err(snapshot_io_error)?;
    if !metadata.is_file() || metadata.file_type().is_symlink() || metadata.len() > max_bytes {
        return Err(CoreError::operation_failed(
            "privacy database snapshot source is unsafe",
        ));
    }
    let modified_ns = metadata
        .modified()
        .ok()
        .and_then(|value| value.duration_since(UNIX_EPOCH).ok())
        .map(|value| value.as_nanos())
        .unwrap_or(0);
    Ok(SnapshotFileIdentity {
        bytes: metadata.len(),
        modified_ns,
    })
}

fn optional_snapshot_file_identity(
    path: &Path,
    max_bytes: u64,
) -> CoreResult<Option<SnapshotFileIdentity>> {
    match fs::symlink_metadata(path) {
        Ok(_) => snapshot_file_identity(path, max_bytes).map(Some),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(snapshot_io_error(error)),
    }
}

fn copy_snapshot_file(source: &Path, destination: &Path, max_bytes: u64) -> CoreResult<()> {
    let expected = snapshot_file_identity(source, max_bytes)?.bytes;
    let copied = fs::copy(source, destination).map_err(snapshot_io_error)?;
    if copied != expected {
        return Err(CoreError::operation_failed(
            "privacy database snapshot copy was incomplete",
        ));
    }
    Ok(())
}

fn companion_path(path: &Path, suffix: &str) -> PathBuf {
    let mut value: OsString = path.as_os_str().to_owned();
    value.push(suffix);
    PathBuf::from(value)
}

pub(super) fn snapshot_io_error(error: std::io::Error) -> CoreError {
    CoreError::operation_failed(format!(
        "privacy database snapshot failed kind={:?}",
        error.kind()
    ))
}
