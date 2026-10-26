use super::fingerprint::tree_fingerprint_with_cancellation;
use super::{
    privacy_io_error, privacy_path_io_error, NativePrivacyAction, NativePrivacyCandidate,
    PrivacyMutationFailure,
};
use crate::privacy::browser_database;
use crate::{CoreError, CoreResult};
use mangodisk_platform::{
    current_platform, Platform, PlatformCancellation, PlatformPrivacySystemTraceKind,
    PrivacyPlatform,
};
use std::{fs, path::Path, path::PathBuf};

pub(super) fn execute_candidate(
    candidate: &NativePrivacyCandidate,
) -> Result<u64, PrivacyMutationFailure> {
    match &candidate.action {
        NativePrivacyAction::Database {
            path,
            browser,
            kind,
            range,
            scan_now_ms,
        } => {
            browser_database::clear(path, *browser, *kind, *range, *scan_now_ms).map_err(Into::into)
        }
        NativePrivacyAction::Directories { roots } => clear_directories(roots),
        NativePrivacyAction::ReviewOnlyFile { .. } => Err(CoreError::operation_failed(
            "review-only privacy evidence cannot be executed",
        )
        .into()),
        NativePrivacyAction::System {
            kind:
                kind @ (PlatformPrivacySystemTraceKind::RecentItems
                | PlatformPrivacySystemTraceKind::RecentDocumentHistory),
            roots,
            has_native_revision,
        } => {
            let mut removed = 0_u64;
            for root in roots {
                match clear_recent_items(root) {
                    Ok(root_removed) => removed = removed.saturating_add(root_removed),
                    Err(mut failure) => {
                        failure.confirmed_affected_item_count = failure
                            .confirmed_affected_item_count
                            .saturating_add(removed);
                        if removed > 0 {
                            failure.error = failure.error.with_possible_side_effects();
                        }
                        return Err(failure);
                    }
                }
            }
            let native_verified = if *kind == PlatformPrivacySystemTraceKind::RecentDocumentHistory
                && *has_native_revision
            {
                Some(
                    current_platform()
                        .clear_system_privacy_trace(
                            PlatformPrivacySystemTraceKind::RecentDocumentHistory,
                        )
                        .map_err(|error| {
                            let mut failure = PrivacyMutationFailure::from(error);
                            failure.confirmed_affected_item_count = removed;
                            if removed > 0 {
                                failure = failure.with_possible_side_effects();
                            }
                            failure
                        })?,
                )
            } else {
                None
            };
            if native_verified == Some(false) {
                return Err(PrivacyMutationFailure::new(
                    CoreError::operation_failed("recent document registry verification failed")
                        .with_possible_side_effects(),
                    removed,
                ));
            }
            Ok(removed)
        }
        NativePrivacyAction::System { roots, .. } if !roots.is_empty() => {
            let removed = clear_directories(roots)?;
            Ok(if removed == 0 {
                0
            } else {
                candidate.item.item_count
            })
        }
        NativePrivacyAction::System { kind, .. } => {
            let verified = current_platform().clear_system_privacy_trace(*kind)?;
            if !verified {
                return Err(CoreError::operation_failed(
                    "privacy native action verification failed",
                )
                .with_possible_side_effects()
                .into());
            }
            if *kind != PlatformPrivacySystemTraceKind::ClipboardHistory {
                return Ok(candidate.item.item_count);
            }

            // Windows preserves pinned clipboard-history entries by contract. Recounting after the
            // native call reports only entries actually removed instead of claiming pinned data was
            // cleared. The platform adapter hashes IDs and never reads clipboard content.
            let cancellation = PlatformCancellation::new(|| false);
            let discovery = current_platform()
                .discover_privacy_sources(&cancellation)
                .map_err(|error| {
                    PrivacyMutationFailure::from(error).with_possible_side_effects()
                })?;
            let remaining = discovery
                .system_traces
                .into_iter()
                .find(|trace| trace.kind == *kind && trace.available)
                .ok_or_else(|| {
                    PrivacyMutationFailure::from(CoreError::operation_failed(
                        "privacy native action post-verification is unavailable",
                    ))
                    .with_possible_side_effects()
                })?
                .item_count;
            Ok(candidate.item.item_count.saturating_sub(remaining))
        }
        NativePrivacyAction::ApplicationNative { kind } => {
            let verified = current_platform().clear_application_privacy_trace(*kind)?;
            if verified {
                Ok(candidate.item.item_count)
            } else {
                Err(CoreError::operation_failed(
                    "application privacy native action verification failed",
                )
                .with_possible_side_effects()
                .into())
            }
        }
    }
}

fn privacy_mutation_failure(
    error: CoreError,
    confirmed_affected_item_count: u64,
    may_have_changed: bool,
) -> PrivacyMutationFailure {
    let error = if may_have_changed {
        error.with_possible_side_effects()
    } else {
        error
    };
    PrivacyMutationFailure::new(error, confirmed_affected_item_count)
}

pub(super) fn clear_directories(roots: &[PathBuf]) -> Result<u64, PrivacyMutationFailure> {
    // Capture each root's count before the first mutation. When a later root fails, the execution
    // result can still report the minimum number of entries whose removal already succeeded.
    let root_counts = roots
        .iter()
        .map(|root| {
            tree_fingerprint_with_cancellation(root, &PlatformCancellation::new(|| false))
                .map(|(_, count, _)| count)
        })
        .collect::<CoreResult<Vec<_>>>()
        .map_err(PrivacyMutationFailure::from)?;
    let count = root_counts.iter().copied().sum();
    let mut root_kinds = Vec::with_capacity(roots.len());
    let mut confirmed_affected_item_count = 0_u64;
    for (root, root_count) in roots.iter().zip(&root_counts) {
        let mut root_changed = false;
        // Re-read root metadata immediately before mutation. `Path::is_dir` follows links and
        // would allow a root swapped after preflight to redirect deletion outside the discovered
        // browser profile. Keeping the original root itself also matches browser expectations:
        // directory contents are cleared, while a session file is removed as one atomic item.
        let metadata = fs::symlink_metadata(root).map_err(|error| {
            privacy_mutation_failure(
                privacy_path_io_error("privacy root metadata unavailable", root, error),
                confirmed_affected_item_count,
                confirmed_affected_item_count > 0,
            )
        })?;
        if current_platform().is_link_like(&metadata) {
            return Err(privacy_mutation_failure(
                CoreError::operation_failed("privacy root changed to a link-like entry"),
                confirmed_affected_item_count,
                confirmed_affected_item_count > 0,
            ));
        }
        if metadata.is_dir() {
            root_kinds.push(true);
            let entries = fs::read_dir(root).map_err(|error| {
                privacy_mutation_failure(
                    privacy_path_io_error("privacy directory is unreadable", root, error),
                    confirmed_affected_item_count,
                    confirmed_affected_item_count > 0,
                )
            })?;
            for entry in entries {
                let path = entry
                    .map_err(|error| {
                        privacy_mutation_failure(
                            privacy_path_io_error("privacy entry is unreadable", root, error),
                            confirmed_affected_item_count,
                            root_changed || confirmed_affected_item_count > 0,
                        )
                    })?
                    .path();
                let metadata = fs::symlink_metadata(&path).map_err(|error| {
                    privacy_mutation_failure(
                        privacy_path_io_error("privacy entry metadata unavailable", &path, error),
                        confirmed_affected_item_count,
                        root_changed || confirmed_affected_item_count > 0,
                    )
                })?;
                if current_platform().is_link_like(&metadata) {
                    return Err(privacy_mutation_failure(
                        CoreError::operation_failed(
                            "privacy directory changed to a link-like entry",
                        ),
                        confirmed_affected_item_count,
                        root_changed || confirmed_affected_item_count > 0,
                    ));
                }
                let removal = if metadata.is_dir() {
                    fs::remove_dir_all(&path)
                } else {
                    fs::remove_file(&path)
                };
                removal.map_err(|error| {
                    privacy_mutation_failure(
                        privacy_path_io_error("privacy entry removal failed", &path, error),
                        confirmed_affected_item_count,
                        true,
                    )
                })?;
                root_changed = true;
            }
        } else if metadata.is_file() {
            root_kinds.push(false);
            fs::remove_file(root).map_err(|error| {
                privacy_mutation_failure(
                    privacy_path_io_error("privacy entry removal failed", root, error),
                    confirmed_affected_item_count,
                    true,
                )
            })?;
            root_changed = true;
        } else {
            return Err(privacy_mutation_failure(
                CoreError::operation_failed("privacy root changed to an unsupported file type"),
                confirmed_affected_item_count,
                confirmed_affected_item_count > 0,
            ));
        }
        if root_changed {
            confirmed_affected_item_count =
                confirmed_affected_item_count.saturating_add(*root_count);
        }
    }

    for (root, was_directory) in roots.iter().zip(root_kinds) {
        if was_directory {
            let metadata = fs::symlink_metadata(root).map_err(|error| {
                privacy_mutation_failure(
                    privacy_path_io_error("privacy directory verification failed", root, error),
                    confirmed_affected_item_count,
                    true,
                )
            })?;
            if !metadata.is_dir() || current_platform().is_link_like(&metadata) {
                return Err(privacy_mutation_failure(
                    CoreError::operation_failed("privacy directory verification failed"),
                    confirmed_affected_item_count,
                    true,
                ));
            }
            let mut entries = fs::read_dir(root).map_err(|error| {
                privacy_mutation_failure(
                    privacy_path_io_error("privacy directory verification failed", root, error),
                    confirmed_affected_item_count,
                    true,
                )
            })?;
            if entries.next().is_some() {
                return Err(privacy_mutation_failure(
                    CoreError::operation_failed("privacy directory verification failed"),
                    confirmed_affected_item_count,
                    true,
                ));
            }
        } else {
            match fs::symlink_metadata(root) {
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => {
                    return Err(privacy_mutation_failure(
                        privacy_path_io_error("privacy file verification failed", root, error),
                        confirmed_affected_item_count,
                        true,
                    ))
                }
                Ok(_) => {
                    return Err(privacy_mutation_failure(
                        CoreError::operation_failed("privacy file verification failed"),
                        confirmed_affected_item_count,
                        true,
                    ))
                }
            }
        }
    }
    Ok(count)
}

pub(super) fn clear_recent_items(root: &Path) -> Result<u64, PrivacyMutationFailure> {
    let metadata = fs::symlink_metadata(root).map_err(|error| {
        PrivacyMutationFailure::from(privacy_io_error("recent items metadata unavailable", error))
    })?;
    if !metadata.is_dir() || current_platform().is_link_like(&metadata) {
        return Err(
            CoreError::operation_failed("recent items directory identity is unsafe").into(),
        );
    }
    let mut removed = 0;
    for entry in fs::read_dir(root).map_err(|error| {
        PrivacyMutationFailure::from(CoreError::operation_failed(format!(
            "recent items are unreadable: {error}"
        )))
    })? {
        let path = entry
            .map_err(|error| {
                privacy_mutation_failure(
                    CoreError::operation_failed(format!("recent item is unreadable: {error}")),
                    removed,
                    removed > 0,
                )
            })?
            .path();
        if !path
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("lnk"))
        {
            continue;
        }
        let metadata = fs::symlink_metadata(&path).map_err(|error| {
            privacy_mutation_failure(
                CoreError::operation_failed(format!("recent item metadata unavailable: {error}")),
                removed,
                removed > 0,
            )
        })?;
        if !metadata.is_file() || current_platform().is_link_like(&metadata) {
            return Err(privacy_mutation_failure(
                CoreError::operation_failed("recent item identity is unsafe"),
                removed,
                removed > 0,
            ));
        }
        fs::remove_file(&path).map_err(|error| {
            privacy_mutation_failure(
                CoreError::operation_failed(format!("recent item removal failed: {error}")),
                removed,
                true,
            )
        })?;
        removed += 1;
    }
    let remaining = fs::read_dir(root)
        .map_err(|error| {
            privacy_mutation_failure(
                CoreError::operation_failed(format!("recent items are unreadable: {error}")),
                removed,
                true,
            )
        })?
        .filter_map(Result::ok)
        .filter(|entry| {
            entry
                .path()
                .extension()
                .is_some_and(|extension| extension.eq_ignore_ascii_case("lnk"))
        })
        .count();
    if remaining != 0 {
        return Err(privacy_mutation_failure(
            CoreError::operation_failed("recent items verification failed"),
            removed,
            true,
        ));
    }
    Ok(removed)
}
