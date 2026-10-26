use super::{privacy_io_error, privacy_path_io_error, NativePrivacyAction, NativePrivacyCandidate};
use crate::privacy::PrivacyCapabilityState;
use crate::{CoreError, CoreResult};
use mangodisk_platform::{current_platform, Platform, PlatformCancellation, PrivacyPlatform};
use std::{
    collections::BTreeMap, fs, io::Read, io::Seek, io::SeekFrom, path::Path, path::PathBuf,
    time::UNIX_EPOCH,
};

const MAX_DIRECTORY_SCAN_ENTRIES: u64 = 50_000;

pub(super) fn database_access_is_busy(error: &std::io::Error) -> bool {
    #[cfg(windows)]
    {
        // ERROR_SHARING_VIOLATION and ERROR_LOCK_VIOLATION indicate a live owner, not a missing
        // privacy permission. Chromium's Windows network service commonly uses the former.
        matches!(error.raw_os_error(), Some(32 | 33))
    }
    #[cfg(not(windows))]
    {
        error.kind() == std::io::ErrorKind::WouldBlock
    }
}

pub(super) fn deferred_database_fingerprint(path: &Path) -> CoreResult<String> {
    let metadata = fs::symlink_metadata(path).map_err(|error| {
        CoreError::operation_failed(format!(
            "privacy deferred database identity failed kind={:?}",
            error.kind()
        ))
    })?;
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err(CoreError::operation_failed(
            "privacy deferred database identity is unsafe",
        ));
    }
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"privacy-deferred-database-v1");
    hasher.update(path.to_string_lossy().as_bytes());
    hasher.update(&metadata.len().to_le_bytes());
    let modified_ns = metadata
        .modified()
        .ok()
        .and_then(|value| value.duration_since(UNIX_EPOCH).ok())
        .map(|value| value.as_nanos())
        .unwrap_or(0);
    hasher.update(&modified_ns.to_le_bytes());
    Ok(hasher.finalize().to_hex().to_string())
}

pub(super) fn current_fingerprint(candidate: &NativePrivacyCandidate) -> CoreResult<String> {
    match &candidate.action {
        NativePrivacyAction::Database { path, .. }
        | NativePrivacyAction::ReviewOnlyFile { path, .. } => file_fingerprint(path),
        NativePrivacyAction::Directories { roots } => {
            roots_summary_fingerprint(roots, &PlatformCancellation::new(|| false))
                .map(|value| value.0)
        }
        NativePrivacyAction::System {
            kind,
            roots,
            has_native_revision,
        } if !roots.is_empty() => {
            let root_revision =
                roots_summary_fingerprint(roots, &PlatformCancellation::new(|| false))?.0;
            let native_revision = if *has_native_revision {
                Some(
                    current_platform()
                        .system_privacy_trace_revision(*kind)?
                        .ok_or_else(|| {
                            CoreError::operation_failed(
                                "privacy system native revision is unavailable",
                            )
                        })?,
                )
            } else {
                None
            };
            Ok(system_trace_fingerprint(
                &root_revision,
                native_revision.as_deref(),
            ))
        }
        NativePrivacyAction::System { kind, .. } => {
            let cancellation = PlatformCancellation::new(|| false);
            let discovery = current_platform().discover_privacy_sources(&cancellation)?;
            let current = discovery
                .system_traces
                .into_iter()
                .find(|trace| trace.kind == *kind)
                .ok_or_else(|| {
                    CoreError::operation_failed("privacy system source is unavailable")
                })?;
            Ok(current.revision)
        }
        NativePrivacyAction::ApplicationNative { kind } => {
            let cancellation = PlatformCancellation::new(|| false);
            let discovery = current_platform().discover_privacy_sources(&cancellation)?;
            discovery
                .applications
                .into_iter()
                .flat_map(|application| application.traces)
                .find(|trace| trace.native_kind == Some(*kind))
                .map(|trace| trace.revision)
                .ok_or_else(|| {
                    CoreError::operation_failed("application privacy source is unavailable")
                })
        }
    }
}

pub(super) fn refresh_database_fingerprints(
    candidates: &mut BTreeMap<String, NativePrivacyCandidate>,
) -> CoreResult<()> {
    for candidate in candidates.values_mut() {
        match &candidate.action {
            NativePrivacyAction::Database { path, .. }
            | NativePrivacyAction::ReviewOnlyFile { path, .. } => {
                match file_fingerprint(path) {
                    Ok(fingerprint) => candidate.fingerprint = fingerprint,
                    Err(_)
                        if candidate.item.capability == PrivacyCapabilityState::BrowserRunning
                            && fs::File::open(path)
                                .err()
                                .as_ref()
                                .is_some_and(database_access_is_busy) =>
                    {
                        // Keep the metadata-only fingerprint created for an exclusively locked
                        // database. This candidate can only authorize the close prompt; Core
                        // replaces the deferred fingerprint after process closure and before
                        // destructive execution.
                    }
                    Err(error) => return Err(error),
                }
            }
            _ => {}
        }
    }
    Ok(())
}

pub(super) fn database_fingerprint_expectations(
    candidates: &[NativePrivacyCandidate],
) -> BTreeMap<PathBuf, String> {
    candidates
        .iter()
        .filter_map(|candidate| match &candidate.action {
            NativePrivacyAction::Database { path, .. } => {
                Some((path.clone(), candidate.fingerprint.clone()))
            }
            _ => None,
        })
        .collect()
}

pub(super) fn expected_fingerprint<'a>(
    candidate: &'a NativePrivacyCandidate,
    database_fingerprints: &'a BTreeMap<PathBuf, String>,
) -> &'a str {
    match &candidate.action {
        NativePrivacyAction::Database { path, .. } => database_fingerprints
            .get(path)
            .unwrap_or(&candidate.fingerprint),
        _ => &candidate.fingerprint,
    }
}

pub(super) fn fingerprint_failure(
    candidate: &NativePrivacyCandidate,
    expected: &str,
) -> Option<(&'static str, CoreError)> {
    match current_fingerprint(candidate) {
        Ok(current) if current == expected => None,
        Ok(current) => Some((
            "sourceChanged",
            CoreError::operation_failed(format!(
                "privacy source fingerprint changed: expected={expected} current={current}"
            )),
        )),
        Err(error) => Some(("sourceUnavailable", error)),
    }
}

#[cfg(test)]
pub(super) fn fingerprint_matches_expectation(
    candidate: &NativePrivacyCandidate,
    database_fingerprints: &BTreeMap<PathBuf, String>,
) -> bool {
    fingerprint_failure(
        candidate,
        expected_fingerprint(candidate, database_fingerprints),
    )
    .is_none()
}

pub(super) fn refresh_database_fingerprint_expectation(
    candidate: &NativePrivacyCandidate,
    database_fingerprints: &mut BTreeMap<PathBuf, String>,
) {
    if let NativePrivacyAction::Database { path, .. } = &candidate.action {
        if let Ok(fingerprint) = file_fingerprint(path) {
            database_fingerprints.insert(path.clone(), fingerprint);
        }
    }
}

pub(super) fn file_fingerprint(path: &Path) -> CoreResult<String> {
    let mut hasher = blake3::Hasher::new();
    hasher.update(path.to_string_lossy().as_bytes());
    update_file_fingerprint(&mut hasher, path)?;
    for suffix in ["-wal", "-shm"] {
        let sidecar = PathBuf::from(format!("{}{suffix}", path.to_string_lossy()));
        hasher.update(suffix.as_bytes());
        match fs::symlink_metadata(&sidecar) {
            Ok(_) => {
                hasher.update(&[1]);
                update_file_fingerprint(&mut hasher, &sidecar)?;
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                hasher.update(&[0]);
            }
            Err(error) => return Err(privacy_io_error("privacy metadata unavailable", error)),
        }
    }
    Ok(hasher.finalize().to_hex().to_string())
}

fn update_file_fingerprint(hasher: &mut blake3::Hasher, path: &Path) -> CoreResult<()> {
    const SAMPLE_BYTES: usize = 4096;

    let mut file = open_fingerprint_file(path)
        .map_err(|error| privacy_io_error("privacy fingerprint open failed", error))?;
    let before = file
        .metadata()
        .map_err(|error| privacy_io_error("privacy fingerprint metadata failed", error))?;
    if !before.is_file() || current_platform().is_link_like(&before) {
        return Err(CoreError::operation_failed(
            "privacy database identity is unsafe",
        ));
    }
    let modified = before
        .modified()
        .ok()
        .and_then(|value| value.duration_since(UNIX_EPOCH).ok())
        .map_or(0, |value| value.as_nanos());
    hasher.update(&before.len().to_le_bytes());
    hasher.update(&modified.to_le_bytes());

    // SQLite updates its file-change counter in the first page on every main-database commit,
    // while WAL commits append frames at the end. Hashing both bounded regions detects those
    // changes even on filesystems whose timestamp resolution is too coarse for rapid writes.
    let head_bytes = before.len().min(SAMPLE_BYTES as u64) as usize;
    let mut sample = vec![0_u8; head_bytes];
    file.read_exact(&mut sample)
        .map_err(|error| privacy_io_error("privacy fingerprint read failed", error))?;
    hasher.update(&sample);
    if before.len() > SAMPLE_BYTES as u64 {
        let tail_offset = before.len().saturating_sub(SAMPLE_BYTES as u64);
        file.seek(SeekFrom::Start(tail_offset))
            .map_err(|error| privacy_io_error("privacy fingerprint seek failed", error))?;
        sample.resize(SAMPLE_BYTES, 0);
        file.read_exact(&mut sample)
            .map_err(|error| privacy_io_error("privacy fingerprint read failed", error))?;
        hasher.update(&sample);
    }

    let after = file
        .metadata()
        .map_err(|error| privacy_io_error("privacy fingerprint metadata failed", error))?;
    if before.len() != after.len() || before.modified().ok() != after.modified().ok() {
        return Err(CoreError::operation_failed(
            "privacy database changed while fingerprinting",
        ));
    }
    Ok(())
}

#[cfg(unix)]
fn open_fingerprint_file(path: &Path) -> std::io::Result<fs::File> {
    use std::os::unix::fs::OpenOptionsExt;

    fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(path)
}

#[cfg(windows)]
fn open_fingerprint_file(path: &Path) -> std::io::Result<fs::File> {
    use std::os::windows::fs::OpenOptionsExt;
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_FLAG_OPEN_REPARSE_POINT, FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE,
    };

    fs::OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
        .open(path)
}

/// Captures only root and direct-child metadata during an ordinary scan.
///
/// Browser storage can contain hundreds of thousands of cache entries. Recursive safety
/// validation is still mandatory immediately before deletion, but doing it during every read-only
/// scan would make the page unusable and cancellation sluggish. This bounded summary is sufficient
/// for scan-to-plan drift detection; execution performs the complete link-aware traversal.
pub(super) fn roots_summary_fingerprint(
    roots: &[PathBuf],
    cancellation: &PlatformCancellation,
) -> CoreResult<(String, u64, u64)> {
    let mut facts = Vec::new();
    let mut count = 0;
    let mut bytes = 0;
    for root in roots {
        if cancellation.is_cancelled() {
            return Err(CoreError::operation_cancelled());
        }
        if facts.len() as u64 >= MAX_DIRECTORY_SCAN_ENTRIES {
            return Err(CoreError::operation_failed(
                "privacy directory exceeds the bounded scan limit",
            ));
        }
        let metadata = fs::symlink_metadata(root).map_err(|error| {
            privacy_path_io_error("privacy directory metadata unavailable", root, error)
        })?;
        if current_platform().is_link_like(&metadata) {
            return Err(CoreError::operation_failed(format!(
                "privacy directory contains an unsafe root link: {}",
                current_platform().display_path(root)
            )));
        }
        facts.push((
            root.to_string_lossy().into_owned(),
            metadata.len(),
            metadata.is_dir(),
            modified_at_nanos(&metadata),
        ));
        if metadata.is_dir() {
            for entry in fs::read_dir(root).map_err(|error| {
                privacy_path_io_error("privacy directory is unreadable", root, error)
            })? {
                let entry = entry.map_err(|error| {
                    privacy_path_io_error("privacy directory entry is unreadable", root, error)
                })?;
                if facts.len() as u64 >= MAX_DIRECTORY_SCAN_ENTRIES {
                    return Err(CoreError::operation_failed(
                        "privacy directory exceeds the bounded scan limit",
                    ));
                }
                let metadata = fs::symlink_metadata(entry.path()).map_err(|error| {
                    privacy_path_io_error(
                        "privacy entry metadata unavailable",
                        &entry.path(),
                        error,
                    )
                })?;
                if current_platform().is_link_like(&metadata) {
                    return Err(CoreError::operation_failed(format!(
                        "privacy directory contains a link-like entry: {}",
                        current_platform().display_path(&entry.path())
                    )));
                }
                facts.push((
                    entry.file_name().to_string_lossy().into_owned(),
                    metadata.len(),
                    metadata.is_dir(),
                    modified_at_nanos(&metadata),
                ));
                count += 1;
                if metadata.is_file() {
                    bytes += metadata.len();
                }
            }
        } else {
            count += 1;
            bytes += metadata.len();
        }
    }
    facts.sort();
    let mut hasher = blake3::Hasher::new();
    for (name, len, directory, modified) in facts {
        hasher.update(name.as_bytes());
        hasher.update(&len.to_le_bytes());
        hasher.update(&[u8::from(directory)]);
        hasher.update(&modified.to_le_bytes());
    }
    Ok((hasher.finalize().to_hex().to_string(), count, bytes))
}

fn modified_at_nanos(metadata: &fs::Metadata) -> u128 {
    metadata
        .modified()
        .ok()
        .and_then(|value| value.duration_since(UNIX_EPOCH).ok())
        .map_or(0, |value| value.as_nanos())
}

pub(super) fn system_trace_fingerprint(
    root_revision: &str,
    native_revision: Option<&str>,
) -> String {
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"mangodisk-system-privacy-trace-v1\0");
    hasher.update(root_revision.as_bytes());
    if let Some(native_revision) = native_revision {
        hasher.update(b"\0native\0");
        hasher.update(native_revision.as_bytes());
    }
    hasher.finalize().to_hex().to_string()
}

pub(super) fn tree_fingerprint_with_cancellation(
    root: &Path,
    cancellation: &PlatformCancellation,
) -> CoreResult<(String, u64, u64)> {
    let metadata = fs::symlink_metadata(root).map_err(|error| {
        privacy_path_io_error("privacy directory metadata unavailable", root, error)
    })?;
    if current_platform().is_link_like(&metadata) {
        return Err(CoreError::operation_failed(format!(
            "privacy directory contains an unsafe root link: {}",
            current_platform().display_path(root)
        )));
    }
    let mut entries = vec![root.to_path_buf()];
    let mut facts = Vec::new();
    let mut count = 0;
    let mut bytes = 0;
    while let Some(path) = entries.pop() {
        if cancellation.is_cancelled() {
            return Err(CoreError::operation_cancelled());
        }
        if facts.len() as u64 >= MAX_DIRECTORY_SCAN_ENTRIES {
            return Err(CoreError::operation_failed(
                "privacy directory exceeds the bounded scan limit",
            ));
        }
        let metadata = fs::symlink_metadata(&path).map_err(|error| {
            privacy_path_io_error("privacy entry metadata unavailable", &path, error)
        })?;
        if current_platform().is_link_like(&metadata) {
            return Err(CoreError::operation_failed(format!(
                "privacy directory contains a link-like entry: {}",
                current_platform().display_path(&path)
            )));
        }
        let relative = path
            .strip_prefix(root)
            .unwrap_or(&path)
            .to_string_lossy()
            .into_owned();
        facts.push((relative, metadata.len(), metadata.is_dir()));
        if metadata.is_dir() {
            for entry in fs::read_dir(&path).map_err(|error| {
                privacy_path_io_error("privacy directory is unreadable", &path, error)
            })? {
                entries.push(
                    entry
                        .map_err(|error| {
                            privacy_path_io_error(
                                "privacy directory entry is unreadable",
                                &path,
                                error,
                            )
                        })?
                        .path(),
                );
            }
        } else {
            count += 1;
            bytes += metadata.len();
        }
    }
    facts.sort();
    let mut hasher = blake3::Hasher::new();
    for (relative, len, directory) in facts {
        hasher.update(relative.as_bytes());
        hasher.update(&len.to_le_bytes());
        hasher.update(&[u8::from(directory)]);
    }
    Ok((hasher.finalize().to_hex().to_string(), count, bytes))
}
