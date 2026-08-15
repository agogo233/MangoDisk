use std::{
    error::Error,
    fmt, fs,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

use mangodisk_platform::{current_platform, Platform, ScanPurpose};

use crate::{
    filesystem::{
        metadata::{diagnostic_path, modified_ms},
        PermanentDeleteCandidate,
    },
    shared::CoreErrorReason,
    storage::analysis::{AnalysisDeleteResult, AnalysisEntryCandidate},
};

pub(crate) struct AnalysisDeleteOutcome {
    pub(crate) target: PathBuf,
    pub(crate) result: AnalysisDeleteResult,
}

static NEXT_DELETE_STAGING_ID: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct PhysicalPathIdentity {
    volume: u64,
    index: u64,
}

/// Binds domain validation to one physical filesystem object.
///
/// Callers capture this value before their final ownership and snapshot checks.
/// The deletion boundary then moves the current path into private staging and
/// compares its physical identity with this capture before removing anything.
/// A same-path replacement therefore fails closed instead of deleting the new
/// object that appeared after validation.
pub(crate) struct PreparedPermanentDelete {
    path: PathBuf,
    metadata: fs::Metadata,
    identity: PhysicalPathIdentity,
    // Keep the identity handle open through the staging rename. This prevents
    // an unlinked object identity from being reused before the post-rename
    // comparison. Windows opens it with delete sharing so the handle does not
    // block MangoDisk's own rename operation.
    _identity_handle: fs::File,
}

/// Counts regular files that the permanent-delete boundary actually removed.
///
/// Whole-root cleanup cannot treat a pre-delete aggregate as the actual result:
/// tools may still write between aggregation and atomic staging. Accumulating
/// the live removal traversal keeps progress and history truthful.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct PermanentDeleteOutcome {
    released_bytes: u64,
    affected_item_count: u64,
}

impl PermanentDeleteOutcome {
    pub(crate) fn released_bytes(self) -> u64 {
        self.released_bytes
    }

    pub(crate) fn affected_item_count(self) -> u64 {
        self.affected_item_count
    }
}

impl PreparedPermanentDelete {
    pub(crate) fn metadata(&self) -> &fs::Metadata {
        &self.metadata
    }
}

#[derive(Debug)]
pub(crate) struct PermanentDeleteError {
    message: String,
    reason: Option<CoreErrorReason>,
    released_bytes: u64,
    affected_item_count: u64,
    partial: bool,
}

/// Captures a stable target before domain-specific validation begins.
///
/// The path is checked on both sides of the capture. Later replacement is
/// detected after the atomic staging rename, while an already present link or
/// reparse point is rejected before any mutation occurs.
pub(crate) fn prepare_path_for_permanent_delete(
    path: &Path,
) -> Result<PreparedPermanentDelete, PermanentDeleteError> {
    current_platform()
        .validate_path_no_links(path)
        .map_err(|error| error.to_string())?;

    #[cfg(unix)]
    let (metadata, identity, identity_handle) = {
        let identity_handle = fs::File::open(path).map_err(|error| {
            permanent_delete_io_error("failed to open the deletion target", error)
        })?;
        let metadata = identity_handle.metadata().map_err(|error| {
            permanent_delete_io_error("failed to read deletion target metadata", error)
        })?;
        if current_platform().is_link_like(&metadata) {
            return Err(
                "MangoDisk cannot permanently delete a link or reparse point"
                    .to_string()
                    .into(),
            );
        }
        let identity = metadata_identity(&metadata);
        current_platform()
            .validate_path_no_links(path)
            .map_err(|error| error.to_string())?;
        let verified = fs::symlink_metadata(path).map_err(|error| error.to_string())?;
        if current_platform().is_link_like(&verified) || metadata_identity(&verified) != identity {
            return Err("the item changed while its physical identity was captured"
                .to_string()
                .into());
        }
        (metadata, identity, identity_handle)
    };

    #[cfg(windows)]
    let (metadata, identity, identity_handle) = {
        let identity_handle = open_identity_handle(path)?;
        let metadata = identity_handle.metadata().map_err(|error| {
            permanent_delete_io_error("failed to read deletion target metadata", error)
        })?;
        if current_platform().is_link_like(&metadata) {
            return Err(
                "MangoDisk cannot permanently delete a link or reparse point"
                    .to_string()
                    .into(),
            );
        }
        let identity = handle_identity(&identity_handle)?;
        current_platform()
            .validate_path_no_links(path)
            .map_err(|error| error.to_string())?;
        (metadata, identity, identity_handle)
    };

    if !metadata.is_file() && !metadata.is_dir() {
        return Err(
            "only regular files and directories can be permanently deleted"
                .to_string()
                .into(),
        );
    }

    Ok(PreparedPermanentDelete {
        path: path.to_path_buf(),
        metadata,
        identity,
        _identity_handle: identity_handle,
    })
}

impl PermanentDeleteError {
    fn before_mutation(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            reason: None,
            released_bytes: 0,
            affected_item_count: 0,
            partial: false,
        }
    }

    fn before_mutation_with_reason(message: impl Into<String>, reason: CoreErrorReason) -> Self {
        Self {
            message: message.into(),
            reason: Some(reason),
            released_bytes: 0,
            affected_item_count: 0,
            partial: false,
        }
    }

    fn after_mutation(
        message: impl Into<String>,
        reason: Option<CoreErrorReason>,
        released_bytes: u64,
        affected_item_count: u64,
    ) -> Self {
        Self {
            message: message.into(),
            reason,
            released_bytes,
            affected_item_count,
            partial: true,
        }
    }

    pub(crate) fn released_bytes(&self) -> u64 {
        self.released_bytes
    }

    pub(crate) fn is_partial(&self) -> bool {
        self.partial
    }

    pub(crate) fn affected_item_count(&self) -> u64 {
        self.affected_item_count
    }

    pub(crate) fn reason(&self) -> Option<CoreErrorReason> {
        self.reason
    }
}

impl fmt::Display for PermanentDeleteError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl Error for PermanentDeleteError {}

impl From<String> for PermanentDeleteError {
    fn from(message: String) -> Self {
        Self::before_mutation(message)
    }
}

fn permanent_delete_io_error(context: &str, error: std::io::Error) -> PermanentDeleteError {
    let message = format!("{context}: {error}");
    match permanent_delete_io_reason(&error) {
        Some(reason) => PermanentDeleteError::before_mutation_with_reason(message, reason),
        None => PermanentDeleteError::before_mutation(message),
    }
}

fn permanent_delete_io_reason(error: &std::io::Error) -> Option<CoreErrorReason> {
    #[cfg(windows)]
    if matches!(error.raw_os_error(), Some(32 | 33)) {
        return Some(CoreErrorReason::ResourceBusy);
    }

    // Windows may report an open directory as access denied rather than a
    // sharing violation. Keep that ambiguity explicit so the UI recommends
    // both closing users of the item and checking permissions.
    if error.kind() == std::io::ErrorKind::PermissionDenied {
        return Some(CoreErrorReason::AccessDeniedOrBusy);
    }
    None
}

/// Validates and permanently deletes one analysis entry without updating a domain cache.
/// The analysis service owns cache synchronization because filesystem helpers
/// must not depend on storage-domain state.
pub(crate) fn delete_analysis_candidate_permanently(
    candidate: AnalysisEntryCandidate,
) -> Result<AnalysisDeleteOutcome, PermanentDeleteError> {
    let requested_root = PathBuf::from(&candidate.root);
    let root = current_platform()
        .canonicalize_no_links(&requested_root)
        .map_err(|error| format!("failed to access the current analysis directory: {error}"))?;
    let requested_target = PathBuf::from(&candidate.path);
    current_platform()
        .validate_path_no_links(&requested_target)
        .map_err(|error| error.to_string())?;
    let prepared = prepare_path_for_permanent_delete(&requested_target)?;
    let requested_metadata = prepared.metadata();
    // Canonicalization follows symbolic links. Reject links and reparse
    // points before resolving the path so the target cannot be moved instead
    // of the link itself.
    if current_platform().is_link_like(requested_metadata) {
        return Err("MangoDisk cannot process a link or reparse point"
            .to_string()
            .into());
    }
    let target = current_platform()
        .canonicalize_no_links(&requested_target)
        .map_err(|error| format!("failed to access the requested item: {error}"))?;

    // The UI displays only direct children of the current directory. Enforce
    // the same boundary in Core so a modified request cannot target the scan
    // root or an arbitrary system path.
    if target.parent() != Some(root.as_path()) {
        return Err(
            "only direct children of the current analysis directory can be processed"
                .to_string()
                .into(),
        );
    }

    let metadata = prepared.metadata();
    let target_type_matches = if candidate.is_directory {
        metadata.is_dir()
    } else {
        metadata.is_file()
    };
    if !target_type_matches {
        return Err("the item type changed after scanning".to_string().into());
    }
    // The explicit confirmation authorizes deletion of the current regular
    // file or directory at this direct-child path. Scan metadata is retained
    // only for result accounting: rejecting normal changes between analysis
    // and confirmation made a user-requested delete appear broken. Physical
    // identity remains pinned from this preparation through the staging rename,
    // so a concurrent replacement during execution still fails closed.
    if current_platform()
        .should_skip(
            &target,
            &current_platform().system_volume_path(),
            ScanPurpose::Analysis,
        )
        .is_some()
    {
        return Err(
            "MangoDisk cannot process a protected system or application item"
                .to_string()
                .into(),
        );
    }

    let verified_target = current_platform()
        .canonicalize_no_links(&requested_target)
        .map_err(|error| error.to_string())?;
    if verified_target != target {
        return Err("the item changed during safety validation"
            .to_string()
            .into());
    }
    delete_path_permanently(
        prepared,
        candidate.expected_bytes,
        candidate.expected_file_count,
    )?;
    Ok(AnalysisDeleteOutcome {
        target,
        result: AnalysisDeleteResult {
            removed_path: candidate.path,
            released_bytes: candidate.expected_bytes,
            removed_file_count: candidate.expected_file_count,
        },
    })
}

/// Deletes a preflighted regular file or directory without following a top-level link.
///
/// Domain services must complete their own ownership, snapshot, protected-path, and running
/// process checks before calling this irreversible boundary.
pub(crate) fn delete_path_permanently(
    target: PreparedPermanentDelete,
    expected_bytes: u64,
    expected_item_count: u64,
) -> Result<(), PermanentDeleteError> {
    if target.metadata.is_file() {
        delete_via_staging(
            target,
            expected_bytes,
            expected_item_count,
            StagedRemoval::File,
        )
        .map(|_| ())
    } else if target.metadata.is_dir() {
        delete_via_staging(
            target,
            expected_bytes,
            expected_item_count,
            StagedRemoval::DirectoryTree,
        )
        .map(|_| ())
    } else {
        Err(
            "only regular files and directories can be permanently deleted"
                .to_string()
                .into(),
        )
    }
}

/// Atomically stages a directory, then removes the tree in one cancellable pass.
///
/// Per-file cleanup checks cancellation between candidates; the whole-root fast
/// path must preserve that contract. It removes per-file identity and staging
/// transactions without sacrificing cancellation, link safety, or accounting.
pub(crate) fn delete_directory_tree_permanently_with_cancellation(
    target: PreparedPermanentDelete,
    expected_bytes: u64,
    expected_item_count: u64,
    is_cancelled: &dyn Fn() -> bool,
) -> Result<PermanentDeleteOutcome, PermanentDeleteError> {
    if !target.metadata.is_dir() {
        return Err(
            "only a regular directory can be removed as a directory tree"
                .to_string()
                .into(),
        );
    }
    delete_via_staging(
        target,
        expected_bytes,
        expected_item_count,
        StagedRemoval::CancellableDirectoryTree(is_cancelled),
    )
}

/// Removes a directory only when it is still the captured object and remains
/// empty. Declarative cleanup uses this boundary after removing individually
/// authorized children, so concurrent new files are preserved rather than
/// being swept up by a recursive delete.
pub(crate) fn delete_empty_directory_permanently(
    target: PreparedPermanentDelete,
) -> Result<(), PermanentDeleteError> {
    if !target.metadata.is_dir() {
        return Err("only a regular directory can be removed as empty"
            .to_string()
            .into());
    }
    delete_via_staging(target, 0, 0, StagedRemoval::EmptyDirectory).map(|_| ())
}

#[derive(Clone, Copy)]
enum StagedRemoval<'a> {
    File,
    EmptyDirectory,
    DirectoryTree,
    CancellableDirectoryTree(&'a dyn Fn() -> bool),
}

struct StagedRemovalFailure {
    error: std::io::Error,
    verified_outcome: Option<PermanentDeleteOutcome>,
}

/// Moves a directory to a private same-volume staging location before recursively deleting it.
///
/// Recursive deletion is not atomic on either supported platform. The rename first removes the
/// original path atomically, while a failed recursive deletion can restore whatever remains. The
/// structured error records bytes that were already released so callers do not report a false
/// zero-byte outcome.
fn delete_via_staging(
    target: PreparedPermanentDelete,
    expected_bytes: u64,
    expected_item_count: u64,
    removal: StagedRemoval<'_>,
) -> Result<PermanentDeleteOutcome, PermanentDeleteError> {
    let path = target.path.as_path();
    let parent = path
        .parent()
        .ok_or_else(|| PermanentDeleteError::before_mutation("the directory has no parent"))?;
    let staging_root = create_staging_directory(parent)?;
    let staged_target = staging_root.join("target");
    if let Err(error) = fs::rename(path, &staged_target) {
        let _ = fs::remove_dir(&staging_root);
        return Err(permanent_delete_io_error(
            "failed to prepare the item for permanent deletion",
            error,
        ));
    }

    let staged_identity_matches =
        physical_path_identity(&staged_target).is_ok_and(|identity| identity == target.identity);
    if !staged_identity_matches {
        return rollback_staged_target(
            path,
            &staging_root,
            &staged_target,
            "the item was replaced before permanent deletion",
            Some(CoreErrorReason::ItemChanged),
            0,
            0,
        )
        .map(|_| PermanentDeleteOutcome::default());
    }

    let expected_outcome = PermanentDeleteOutcome {
        released_bytes: expected_bytes,
        affected_item_count: expected_item_count,
    };
    let removal_result = match removal {
        StagedRemoval::File => fs::remove_file(&staged_target)
            .map(|_| expected_outcome)
            .map_err(|error| StagedRemovalFailure {
                error,
                verified_outcome: None,
            }),
        StagedRemoval::EmptyDirectory => fs::remove_dir(&staged_target)
            .map(|_| PermanentDeleteOutcome::default())
            .map_err(|error| StagedRemovalFailure {
                error,
                verified_outcome: None,
            }),
        StagedRemoval::DirectoryTree => fs::remove_dir_all(&staged_target)
            .map(|_| expected_outcome)
            .map_err(|error| StagedRemovalFailure {
                error,
                verified_outcome: None,
            }),
        StagedRemoval::CancellableDirectoryTree(is_cancelled) => {
            remove_directory_tree_cancellable(&staged_target, is_cancelled)
        }
    };
    match removal_result {
        Ok(outcome) => {
            if let Err(error) = fs::remove_dir(&staging_root) {
                log::warn!(
                    "permanent_delete_staging_cleanup_failed staging={} error_digest={}",
                    diagnostic_path(&staging_root),
                    blake3::hash(error.to_string().as_bytes()).to_hex()
                );
            }
            Ok(outcome)
        }
        Err(delete_failure) => {
            if !staged_target.exists() {
                let _ = fs::remove_dir(&staging_root);
                return Ok(delete_failure.verified_outcome.unwrap_or(expected_outcome));
            }
            // The cancellable traversal supplies verified live totals, avoiding
            // another full scan after cancellation. Legacy standard removals do
            // not collect per-entry totals and still infer them from the remainder.
            let verified_outcome = delete_failure.verified_outcome.or_else(|| {
                measure_remaining(&staged_target)
                    .ok()
                    .map(|remaining| PermanentDeleteOutcome {
                        released_bytes: expected_bytes.saturating_sub(remaining.bytes),
                        affected_item_count: expected_item_count
                            .saturating_sub(remaining.item_count),
                    })
            });
            let released_bytes = verified_outcome.map_or(0, |outcome| outcome.released_bytes);
            let affected_item_count =
                verified_outcome.map_or(0, |outcome| outcome.affected_item_count);
            rollback_staged_target(
                path,
                &staging_root,
                &staged_target,
                &format!(
                    "failed to permanently delete the item: {}",
                    delete_failure.error
                ),
                permanent_delete_io_reason(&delete_failure.error),
                released_bytes,
                affected_item_count,
            )
            .map(|_| PermanentDeleteOutcome::default())
        }
    }
}

/// Removes a staged tree while observing cancellation between directory entries.
///
/// `remove_dir_all` has no cancellation hook, which is unacceptable for the
/// large small-file trees most likely to be cancelled. The traversal uses
/// `symlink_metadata` and the platform link-like policy; a link or Windows
/// reparse point appearing after aggregation stops deletion and rolls back the
/// remainder without following the target.
fn remove_directory_tree_cancellable(
    root: &Path,
    is_cancelled: &dyn Fn() -> bool,
) -> Result<PermanentDeleteOutcome, StagedRemovalFailure> {
    let mut outcome = PermanentDeleteOutcome::default();
    remove_directory_tree_entry(root, is_cancelled, &mut outcome).map_err(|error| {
        StagedRemovalFailure {
            error,
            verified_outcome: Some(outcome),
        }
    })?;
    Ok(outcome)
}

fn remove_directory_tree_entry(
    path: &Path,
    is_cancelled: &dyn Fn() -> bool,
    outcome: &mut PermanentDeleteOutcome,
) -> Result<(), std::io::Error> {
    if is_cancelled() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::Interrupted,
            "directory tree deletion cancelled",
        ));
    }
    let metadata = fs::symlink_metadata(path)?;
    if current_platform().is_link_like(&metadata) {
        return Err(std::io::Error::other(
            "a link or reparse point appeared during directory tree deletion",
        ));
    }
    if metadata.is_file() {
        let bytes = metadata.len();
        fs::remove_file(path)?;
        outcome.released_bytes = outcome.released_bytes.saturating_add(bytes);
        outcome.affected_item_count = outcome.affected_item_count.saturating_add(1);
        return Ok(());
    }
    if !metadata.is_dir() {
        return Err(std::io::Error::other(
            "an unsupported filesystem entry appeared during directory tree deletion",
        ));
    }

    for entry in fs::read_dir(path)? {
        if is_cancelled() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::Interrupted,
                "directory tree deletion cancelled",
            ));
        }
        remove_directory_tree_entry(&entry?.path(), is_cancelled, outcome)?;
    }
    if is_cancelled() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::Interrupted,
            "directory tree deletion cancelled",
        ));
    }
    fs::remove_dir(path)
}

fn rollback_staged_target(
    original_path: &Path,
    staging_root: &Path,
    staged_target: &Path,
    reason: &str,
    failure_reason: Option<CoreErrorReason>,
    released_bytes: u64,
    affected_item_count: u64,
) -> Result<(), PermanentDeleteError> {
    match fs::rename(staged_target, original_path) {
        Ok(()) => {
            let _ = fs::remove_dir(staging_root);
            let partially_deleted = released_bytes > 0 || affected_item_count > 0;
            let message = if partially_deleted {
                format!(
                    "the item was partially deleted; remaining contents were restored: {reason}"
                )
            } else {
                format!("the item was not deleted and was restored: {reason}")
            };
            Err(if partially_deleted {
                PermanentDeleteError::after_mutation(
                    message,
                    failure_reason,
                    released_bytes,
                    affected_item_count,
                )
            } else {
                let mut error = PermanentDeleteError::before_mutation(message);
                error.reason = failure_reason;
                error
            })
        }
        Err(rollback_error) => {
            log::error!(
                "permanent_delete_rollback_failed target={} staging={} reason_digest={} rollback_error_digest={}",
                diagnostic_path(original_path),
                diagnostic_path(staged_target),
                blake3::hash(reason.as_bytes()).to_hex(),
                blake3::hash(rollback_error.to_string().as_bytes()).to_hex()
            );
            Err(PermanentDeleteError::after_mutation(
                format!(
                    "the item could not be restored automatically after permanent deletion stopped: {reason}"
                ),
                failure_reason,
                released_bytes,
                affected_item_count,
            ))
        }
    }
}

fn create_staging_directory(parent: &Path) -> Result<PathBuf, PermanentDeleteError> {
    for _ in 0..32 {
        let id = NEXT_DELETE_STAGING_ID.fetch_add(1, Ordering::Relaxed);
        let path = parent.join(format!(".mangodisk-delete-{}-{id}", std::process::id()));
        match fs::create_dir(&path) {
            Ok(()) => return Ok(path),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => {
                return Err(permanent_delete_io_error(
                    "failed to create a private deletion staging directory",
                    error,
                ));
            }
        }
    }
    Err(PermanentDeleteError::before_mutation(
        "failed to reserve a unique deletion staging directory",
    ))
}

#[derive(Clone, Copy)]
struct RemainingMeasurement {
    bytes: u64,
    item_count: u64,
}

fn measure_remaining(path: &Path) -> Result<RemainingMeasurement, std::io::Error> {
    let metadata = fs::symlink_metadata(path)?;
    if metadata.is_file() {
        return Ok(RemainingMeasurement {
            bytes: metadata.len(),
            item_count: 1,
        });
    }
    if !metadata.is_dir() || current_platform().is_link_like(&metadata) {
        return Ok(RemainingMeasurement {
            bytes: metadata.len(),
            item_count: 1,
        });
    }
    let mut measurement = RemainingMeasurement {
        bytes: 0,
        item_count: 0,
    };
    for entry in fs::read_dir(path)? {
        let child = measure_remaining(&entry?.path())?;
        measurement.bytes = measurement.bytes.saturating_add(child.bytes);
        measurement.item_count = measurement.item_count.saturating_add(child.item_count);
    }
    Ok(measurement)
}

#[cfg(unix)]
fn metadata_identity(metadata: &fs::Metadata) -> PhysicalPathIdentity {
    use std::os::unix::fs::MetadataExt;

    PhysicalPathIdentity {
        volume: metadata.dev(),
        index: metadata.ino(),
    }
}

#[cfg(unix)]
fn physical_path_identity(path: &Path) -> Result<PhysicalPathIdentity, PermanentDeleteError> {
    let metadata = fs::symlink_metadata(path).map_err(|error| error.to_string())?;
    if current_platform().is_link_like(&metadata) {
        return Err("the staged item became a link or reparse point"
            .to_string()
            .into());
    }
    Ok(metadata_identity(&metadata))
}

#[cfg(windows)]
fn open_identity_handle(path: &Path) -> Result<fs::File, PermanentDeleteError> {
    use std::os::windows::fs::OpenOptionsExt;
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, FILE_READ_ATTRIBUTES,
        FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE,
    };

    fs::OpenOptions::new()
        .access_mode(FILE_READ_ATTRIBUTES)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
        .open(path)
        .map_err(|error| permanent_delete_io_error("failed to open the deletion target", error))
}

#[cfg(windows)]
fn handle_identity(file: &fs::File) -> Result<PhysicalPathIdentity, PermanentDeleteError> {
    use std::{mem::MaybeUninit, os::windows::io::AsRawHandle};
    use windows_sys::Win32::Storage::FileSystem::{
        GetFileInformationByHandle, BY_HANDLE_FILE_INFORMATION,
    };

    let mut information = MaybeUninit::<BY_HANDLE_FILE_INFORMATION>::zeroed();
    // SAFETY: `file` owns a valid handle for the duration of the call. A
    // nonzero result guarantees that Windows initialized every output field.
    let succeeded =
        unsafe { GetFileInformationByHandle(file.as_raw_handle() as _, information.as_mut_ptr()) };
    if succeeded == 0 {
        return Err(
            "failed to read the physical identity of the deletion target"
                .to_string()
                .into(),
        );
    }
    // SAFETY: API success was checked immediately above.
    let information = unsafe { information.assume_init() };
    Ok(PhysicalPathIdentity {
        volume: u64::from(information.dwVolumeSerialNumber),
        index: (u64::from(information.nFileIndexHigh) << 32) | u64::from(information.nFileIndexLow),
    })
}

#[cfg(windows)]
fn physical_path_identity(path: &Path) -> Result<PhysicalPathIdentity, PermanentDeleteError> {
    let handle = open_identity_handle(path)?;
    let metadata = handle.metadata().map_err(|error| {
        permanent_delete_io_error("failed to read staged target metadata", error)
    })?;
    if current_platform().is_link_like(&metadata) {
        return Err("the staged item became a link or reparse point"
            .to_string()
            .into());
    }
    handle_identity(&handle)
}

/// Validates and permanently deletes one file result without coordinating the
/// batch. Batch logging, operation locking, and cache updates belong to the
/// large-file service that produced the candidate.
pub(crate) fn delete_file_candidate_permanently(
    candidate: &PermanentDeleteCandidate,
) -> Result<(PathBuf, u64), PermanentDeleteError> {
    let requested_target = PathBuf::from(&candidate.path);
    let prepared = prepare_path_for_permanent_delete(&requested_target)?;
    validate_permanent_delete_candidate(prepared.metadata(), candidate)?;
    let target = current_platform()
        .canonicalize_no_links(&requested_target)
        .map_err(|error| format!("failed to access the requested file: {error}"))?;
    if current_platform()
        .should_skip(
            &target,
            &current_platform().system_volume_path(),
            ScanPurpose::LargeFiles,
        )
        .is_some()
    {
        return Err(
            "MangoDisk cannot process a protected system or application file"
                .to_string()
                .into(),
        );
    }
    let verified_target = current_platform()
        .canonicalize_no_links(&requested_target)
        .map_err(|error| error.to_string())?;
    if verified_target != target {
        return Err("the file changed during safety validation"
            .to_string()
            .into());
    }
    validate_permanent_delete_candidate(prepared.metadata(), candidate)?;
    let released_bytes = prepared.metadata().len();
    delete_path_permanently(prepared, candidate.expected_bytes, 1)?;
    Ok((target, released_bytes))
}

fn validate_permanent_delete_candidate(
    metadata: &fs::Metadata,
    candidate: &PermanentDeleteCandidate,
) -> Result<(), String> {
    if !metadata.is_file() || current_platform().is_link_like(metadata) {
        return Err("only regular files can be permanently deleted".to_string());
    }
    if candidate.expected_modified_at_ms.is_none()
        || metadata.len() != candidate.expected_bytes
        || modified_ms(metadata) != candidate.expected_modified_at_ms
    {
        return Err("the file changed after scanning".to_string());
    }
    Ok(())
}

#[cfg(test)]
mod permanent_delete_tests {
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::*;

    static NEXT_DELETE_SANDBOX_ID: AtomicU64 = AtomicU64::new(1);

    struct DeleteSandbox(PathBuf);

    impl DeleteSandbox {
        fn new() -> Self {
            let unique = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos();
            let id = NEXT_DELETE_SANDBOX_ID.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "mangodisk-permanent-delete-{}-{unique}-{id}",
                std::process::id()
            ));
            fs::create_dir_all(&path).expect("create the permanent-delete fixture");
            Self(path)
        }
    }

    impl Drop for DeleteSandbox {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn permanent_delete_removes_a_matching_regular_file() {
        let sandbox = DeleteSandbox::new();
        let path = sandbox.0.join("large.bin");
        fs::write(&path, b"large-file-fixture").expect("write the deletion fixture");
        let metadata = fs::metadata(&path).expect("read the deletion fixture");
        let candidate = PermanentDeleteCandidate {
            path: path.to_string_lossy().into_owned(),
            expected_bytes: metadata.len(),
            expected_modified_at_ms: modified_ms(&metadata),
        };

        let (_, released_bytes) = delete_file_candidate_permanently(&candidate)
            .expect("a matching regular file should be permanently deleted");

        assert_eq!(released_bytes, metadata.len());
        assert!(!path.exists());
    }

    #[test]
    fn permanent_delete_preserves_a_file_that_changed_after_scanning() {
        let sandbox = DeleteSandbox::new();
        let path = sandbox.0.join("changed.bin");
        fs::write(&path, b"original").expect("write the original deletion fixture");
        let metadata = fs::metadata(&path).expect("read the original deletion fixture");
        let candidate = PermanentDeleteCandidate {
            path: path.to_string_lossy().into_owned(),
            expected_bytes: metadata.len(),
            expected_modified_at_ms: modified_ms(&metadata),
        };
        fs::write(&path, b"changed-content").expect("change the deletion fixture");

        let error = delete_file_candidate_permanently(&candidate)
            .expect_err("a changed file must not be permanently deleted");

        assert!(error.to_string().contains("changed after scanning"));
        assert!(path.exists());
    }

    #[test]
    fn permanent_delete_path_removes_a_preflighted_directory() {
        let sandbox = DeleteSandbox::new();
        let path = sandbox.0.join("directory");
        fs::create_dir_all(&path).expect("create the directory deletion fixture");
        fs::write(path.join("payload.bin"), b"payload")
            .expect("write the directory deletion fixture");

        let prepared = prepare_path_for_permanent_delete(&path)
            .expect("the directory target should be prepared");
        delete_path_permanently(prepared, b"payload".len() as u64, 1)
            .expect("a preflighted directory should be permanently deleted");

        assert!(!path.exists());
        assert!(
            fs::read_dir(&sandbox.0)
                .expect("read the deletion sandbox")
                .all(|entry| !entry
                    .expect("read the staging entry")
                    .file_name()
                    .to_string_lossy()
                    .starts_with(".mangodisk-delete-")),
            "successful deletion must not leave a staging directory"
        );
    }

    #[test]
    fn cancellable_directory_delete_reports_live_removed_content() {
        let sandbox = DeleteSandbox::new();
        let path = sandbox.0.join("cancellable-directory");
        fs::create_dir_all(path.join("nested")).expect("create the cancellable fixture");
        fs::write(path.join("first.bin"), b"first").expect("write the first fixture");
        fs::write(path.join("nested/second.bin"), b"second").expect("write the second fixture");
        let prepared = prepare_path_for_permanent_delete(&path)
            .expect("the cancellable directory should be prepared");

        let outcome =
            delete_directory_tree_permanently_with_cancellation(prepared, 1, 1, &|| false)
                .expect("the cancellable directory should be removed");

        assert_eq!(outcome.released_bytes(), 11);
        assert_eq!(outcome.affected_item_count(), 2);
        assert!(!path.exists());
    }

    #[test]
    fn cancellable_directory_delete_restores_remaining_content() {
        use std::cell::Cell;

        let sandbox = DeleteSandbox::new();
        let path = sandbox.0.join("cancelled-directory");
        fs::create_dir_all(&path).expect("create the cancellation fixture");
        for index in 0..32 {
            fs::write(path.join(format!("{index:02}.cache")), b"cache")
                .expect("write the cancellation fixture");
        }
        let prepared = prepare_path_for_permanent_delete(&path)
            .expect("the cancelled directory should be prepared");
        let checks = Cell::new(0_u64);

        let error =
            delete_directory_tree_permanently_with_cancellation(prepared, 32 * 5, 32, &|| {
                let current = checks.get().saturating_add(1);
                checks.set(current);
                current >= 6
            })
            .expect_err("cancellation should stop the staged directory traversal");

        assert!(error.is_partial());
        assert!(error.released_bytes() > 0);
        assert!(error.released_bytes() < 32 * 5);
        assert!(error.affected_item_count() > 0);
        assert!(error.affected_item_count() < 32);
        assert!(path.exists(), "remaining content must be restored");
        assert_eq!(
            fs::read_dir(&path)
                .expect("read the restored directory")
                .count() as u64
                + error.affected_item_count(),
            32
        );
    }

    /// Measures return latency after cancellation during a large small-file delete.
    ///
    /// The diagnostic creates a large temporary dataset and is ignored by
    /// default. Its output contains only counts and timings, never fixture paths.
    #[test]
    #[ignore = "filesystem cancellation latency benchmark"]
    fn benchmark_cancellable_directory_delete_latency() {
        use std::{
            sync::{
                atomic::{AtomicBool, Ordering},
                Arc,
            },
            thread,
            time::{Duration, Instant},
        };

        let file_count = std::env::var("MANGODISK_CLEANUP_BENCHMARK_FILE_COUNT")
            .ok()
            .and_then(|value| value.parse::<u64>().ok())
            .filter(|count| *count > 0)
            .unwrap_or(20_000);
        let sandbox = DeleteSandbox::new();
        let path = sandbox.0.join("cancellation-latency");
        fs::create_dir_all(&path).expect("create the cancellation benchmark root");
        for index in 0..file_count {
            fs::write(path.join(format!("{index:08}.cache")), b"cache")
                .expect("write the cancellation benchmark file");
        }
        let prepared = prepare_path_for_permanent_delete(&path)
            .expect("prepare the cancellation benchmark directory");
        let cancelled = Arc::new(AtomicBool::new(false));
        let cancellation_signal = Arc::clone(&cancelled);
        let signal_delay = Duration::from_millis(50);
        let signal_thread = thread::spawn(move || {
            thread::sleep(signal_delay);
            cancellation_signal.store(true, Ordering::Release);
        });

        let started = Instant::now();
        let error = delete_directory_tree_permanently_with_cancellation(
            prepared,
            file_count * 5,
            file_count,
            &|| cancelled.load(Ordering::Acquire),
        )
        .expect_err("the asynchronous cancellation signal should stop deletion");
        let total_elapsed = started.elapsed();
        signal_thread
            .join()
            .expect("join the cancellation signal thread");
        let response_elapsed = total_elapsed.saturating_sub(signal_delay);

        assert!(
            path.exists(),
            "remaining files must be restored after cancellation"
        );
        assert!(error.affected_item_count() < file_count);
        assert!(
            response_elapsed < Duration::from_millis(500),
            "cancellation should be observed within 500 ms"
        );
        println!(
            "cleanup_whole_root_cancellation file_count={file_count} total_ms={:.2} response_ms={:.2} affected_item_count={}",
            total_elapsed.as_secs_f64() * 1_000.0,
            response_elapsed.as_secs_f64() * 1_000.0,
            error.affected_item_count()
        );
    }

    #[test]
    fn confirmed_analysis_directory_deletes_when_scan_metadata_is_stale() {
        let sandbox = DeleteSandbox::new();
        let path = sandbox.0.join("confirmed-analysis-directory");
        fs::create_dir_all(&path).expect("create the confirmed analysis fixture");
        fs::write(path.join("payload.bin"), b"payload changed after analysis")
            .expect("write the changed analysis fixture");
        fs::write(path.join("new-after-analysis.bin"), b"new")
            .expect("write the new analysis fixture");
        let candidate = AnalysisEntryCandidate {
            root: sandbox.0.to_string_lossy().into_owned(),
            path: path.to_string_lossy().into_owned(),
            expected_bytes: b"payload".len() as u64,
            expected_file_count: 1,
            is_directory: true,
        };

        let outcome = delete_analysis_candidate_permanently(candidate)
            .expect("explicit confirmation should authorize the current directory contents");

        // The result updates the displayed scan snapshot, so it keeps the
        // original aggregate rather than claiming an unmeasured live value.
        assert_eq!(outcome.result.released_bytes, b"payload".len() as u64);
        assert_eq!(outcome.result.removed_file_count, 1);
        assert!(!path.exists());
    }

    #[cfg(windows)]
    #[test]
    fn windows_delete_errors_preserve_actionable_failure_reasons() {
        assert_eq!(
            permanent_delete_io_reason(&std::io::Error::from_raw_os_error(32)),
            Some(CoreErrorReason::ResourceBusy)
        );
        assert_eq!(
            permanent_delete_io_reason(&std::io::Error::from_raw_os_error(5)),
            Some(CoreErrorReason::AccessDeniedOrBusy)
        );
    }

    #[test]
    fn permanent_delete_preserves_a_same_path_replacement() {
        let sandbox = DeleteSandbox::new();
        let path = sandbox.0.join("directory");
        let moved_original = sandbox.0.join("moved-original");
        fs::create_dir_all(&path).expect("create the original deletion target");
        fs::write(path.join("original.bin"), b"original")
            .expect("write the original deletion target");

        let prepared = prepare_path_for_permanent_delete(&path)
            .expect("the original target should be prepared");
        fs::rename(&path, &moved_original).expect("move the original target aside");
        fs::create_dir_all(&path).expect("create the same-path replacement");
        fs::write(path.join("replacement.bin"), b"replacement")
            .expect("write the same-path replacement");

        let error = delete_path_permanently(prepared, b"original".len() as u64, 1)
            .expect_err("a same-path replacement must not be deleted");

        assert!(error.to_string().contains("replaced"));
        assert!(
            path.join("replacement.bin").exists(),
            "the same-path replacement must be restored"
        );
        assert!(
            moved_original.join("original.bin").exists(),
            "the original object must remain untouched after being moved"
        );
    }

    #[test]
    fn empty_directory_delete_preserves_a_concurrent_new_file() {
        let sandbox = DeleteSandbox::new();
        let path = sandbox.0.join("directory");
        fs::create_dir_all(&path).expect("create the empty directory fixture");
        let prepared = prepare_path_for_permanent_delete(&path)
            .expect("the empty directory should be prepared");
        fs::write(path.join("concurrent.bin"), b"concurrent")
            .expect("write a file after the directory was prepared");

        let error = delete_empty_directory_permanently(prepared)
            .expect_err("a directory that became non-empty must not be deleted");

        assert!(error.to_string().contains("restored"));
        assert!(
            path.join("concurrent.bin").exists(),
            "a concurrently created file must remain visible"
        );
    }

    #[test]
    fn rollback_reports_deleted_zero_byte_items_as_partial() {
        let sandbox = DeleteSandbox::new();
        let staging_root = sandbox.0.join("staging");
        let staged_target = staging_root.join("target");
        let original_path = sandbox.0.join("restored");
        fs::create_dir_all(&staged_target).expect("create the rollback fixture");

        let error = rollback_staged_target(
            &original_path,
            &staging_root,
            &staged_target,
            "simulated zero-byte partial deletion",
            None,
            0,
            1,
        )
        .expect_err("a partial rollback should retain its structured outcome");

        assert!(error.is_partial());
        assert_eq!(error.released_bytes(), 0);
        assert_eq!(error.affected_item_count(), 1);
        assert!(original_path.exists());
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn permanent_delete_restores_remaining_directory_after_recursive_failure() {
        use std::os::unix::fs::PermissionsExt;

        let sandbox = DeleteSandbox::new();
        let path = sandbox.0.join("directory");
        let blocked = path.join("blocked");
        let payload = blocked.join("payload.bin");
        fs::create_dir_all(&blocked).expect("create the rollback fixture");
        fs::write(&payload, b"payload").expect("write the rollback fixture");
        fs::set_permissions(&blocked, fs::Permissions::from_mode(0o000))
            .expect("block traversal of the rollback fixture");

        let prepared = prepare_path_for_permanent_delete(&path)
            .expect("the rollback target should be prepared");
        let error = delete_path_permanently(prepared, b"payload".len() as u64, 1)
            .expect_err("an unreadable descendant should fail recursive deletion");

        assert!(
            error.to_string().contains("restored"),
            "the failure must explain that remaining contents were restored"
        );
        assert!(
            path.exists(),
            "the remaining directory must return to its path"
        );
        fs::set_permissions(&blocked, fs::Permissions::from_mode(0o700))
            .expect("restore fixture permissions");
        assert!(
            payload.exists(),
            "the blocked payload must remain available"
        );
    }
}

#[cfg(all(test, windows))]
mod windows_permanent_delete_tests {
    use std::{
        fs::OpenOptions,
        os::windows::fs::OpenOptionsExt,
        process::Command,
        time::{SystemTime, UNIX_EPOCH},
    };

    use super::*;

    struct JunctionSandbox {
        root: PathBuf,
        junction: PathBuf,
    }

    impl Drop for JunctionSandbox {
        fn drop(&mut self) {
            let junction_removed =
                !self.junction.exists() || fs::remove_dir(&self.junction).is_ok();
            if junction_removed {
                let _ = fs::remove_dir_all(&self.root);
            }
        }
    }

    #[test]
    fn permanent_delete_rejects_file_reached_through_junction() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let sandbox = std::env::temp_dir().join(format!(
            "MangoDisk-Junction-Permanent-Delete-{}-{unique}",
            std::process::id()
        ));
        let real_directory = sandbox.join("real");
        let junction = sandbox.join("junction");
        let _sandbox_cleanup = JunctionSandbox {
            root: sandbox.clone(),
            junction: junction.clone(),
        };
        let protected_file = real_directory.join("must-remain.bin");
        fs::create_dir_all(&real_directory).expect("the junction fixture directory should exist");
        fs::write(&protected_file, b"MangoDisk junction safety fixture")
            .expect("the junction fixture should be written");
        let output = Command::new("cmd.exe")
            .args(["/d", "/c", "mklink", "/J"])
            .arg(&junction)
            .arg(&real_directory)
            .output()
            .expect("mklink should create the test junction");
        assert!(
            output.status.success(),
            "failed to create test junction: {}",
            String::from_utf8_lossy(&output.stderr)
        );

        let linked_file = junction.join("must-remain.bin");
        let metadata =
            fs::metadata(&protected_file).expect("the fixture metadata should be readable");
        let candidate = PermanentDeleteCandidate {
            path: linked_file.to_string_lossy().into_owned(),
            expected_bytes: metadata.len(),
            expected_modified_at_ms: modified_ms(&metadata),
        };
        let error = delete_file_candidate_permanently(&candidate)
            .expect_err("a file reached through a junction must be rejected");

        assert!(error.to_string().contains("link or reparse point"));
        assert!(
            protected_file.exists(),
            "the junction target must remain untouched"
        );
    }

    #[test]
    fn permanent_delete_does_not_hide_a_directory_with_a_locked_child() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let sandbox = std::env::temp_dir().join(format!(
            "MangoDisk-Locked-Permanent-Delete-{}-{unique}",
            std::process::id()
        ));
        let directory = sandbox.join("directory");
        let payload = directory.join("payload.bin");
        fs::create_dir_all(&directory).expect("create the locked deletion fixture");
        fs::write(&payload, b"locked payload").expect("write the locked deletion fixture");
        let locked = OpenOptions::new()
            .read(true)
            .share_mode(0)
            .open(&payload)
            .expect("lock the deletion fixture without delete sharing");

        let prepared = prepare_path_for_permanent_delete(&directory)
            .expect("the locked target should be prepared");
        let error = delete_path_permanently(prepared, b"locked payload".len() as u64, 1)
            .expect_err("a locked descendant must stop permanent deletion");

        assert!(
            directory.exists(),
            "a failed delete must leave or restore the visible directory"
        );
        assert!(payload.exists(), "the locked file must remain available");
        drop(locked);
        fs::remove_dir_all(sandbox).expect("remove the locked deletion fixture");
        assert!(
            !error.is_partial() || error.released_bytes() == 0,
            "the untouched locked fixture must not report released bytes"
        );
    }

    #[test]
    fn cancellable_directory_delete_restores_a_locked_child() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let sandbox = std::env::temp_dir().join(format!(
            "MangoDisk-Locked-Cancellable-Delete-{}-{unique}",
            std::process::id()
        ));
        let directory = sandbox.join("directory");
        let payload = directory.join("payload.bin");
        fs::create_dir_all(&directory).expect("create the locked cancellable fixture");
        fs::write(&payload, b"locked payload").expect("write the locked cancellable fixture");
        let locked = OpenOptions::new()
            .read(true)
            .share_mode(0)
            .open(&payload)
            .expect("lock the cancellable fixture without delete sharing");
        let prepared = prepare_path_for_permanent_delete(&directory)
            .expect("prepare the locked cancellable directory");

        let error = delete_directory_tree_permanently_with_cancellation(
            prepared,
            b"locked payload".len() as u64,
            1,
            &|| false,
        )
        .expect_err("a locked descendant must stop cancellable directory deletion");

        assert!(directory.exists(), "the directory must be restored");
        assert!(payload.exists(), "the locked payload must remain available");
        assert_eq!(error.released_bytes(), 0);
        assert_eq!(error.affected_item_count(), 0);
        drop(locked);
        fs::remove_dir_all(sandbox).expect("remove the locked cancellable fixture");
    }

    #[test]
    fn cancellable_directory_delete_rejects_a_new_junction() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let sandbox = std::env::temp_dir().join(format!(
            "MangoDisk-Cancellable-Junction-{}-{unique}",
            std::process::id()
        ));
        let directory = sandbox.join("directory");
        let protected = sandbox.join("protected");
        let junction = directory.join("late-junction");
        let protected_file = protected.join("must-remain.bin");
        let _sandbox_cleanup = JunctionSandbox {
            root: sandbox.clone(),
            junction: junction.clone(),
        };
        fs::create_dir_all(&directory).expect("create the cancellable junction root");
        fs::create_dir_all(&protected).expect("create the protected junction target");
        fs::write(&protected_file, b"protected").expect("write the protected junction fixture");
        let prepared = prepare_path_for_permanent_delete(&directory)
            .expect("prepare the directory before the junction appears");
        let output = Command::new("cmd.exe")
            .args(["/d", "/c", "mklink", "/J"])
            .arg(&junction)
            .arg(&protected)
            .output()
            .expect("mklink should create the late junction");
        assert!(output.status.success(), "create the late junction fixture");

        let error = delete_directory_tree_permanently_with_cancellation(prepared, 0, 0, &|| false)
            .expect_err("a new junction must stop cancellable directory deletion");

        assert!(
            error.to_string().contains("link or reparse point"),
            "the failure must retain the safety reason"
        );
        assert!(directory.exists(), "the cleanup root must be restored");
        assert!(
            protected_file.exists(),
            "the junction target must never be traversed or deleted"
        );
    }
}
