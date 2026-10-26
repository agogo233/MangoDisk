use super::super::project_artifact_protection;
use super::{ArtifactDraft, ArtifactMeasurement};
use crate::{
    cleanup::measurement::MeasureResult,
    filesystem::metadata::{diagnostic_path, is_link_like, latest_timestamp, modified_ms},
};
use mangodisk_platform::{current_platform, DirectoryTreeAggregateError, Platform};
use std::{
    fs,
    path::Path,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Mutex,
    },
    thread,
};
const MEASUREMENT_WORKER_LIMIT: usize = 4;
pub(super) const PROGRESS_FILE_BATCH_SIZE: u64 = 128;

pub(super) fn measure_artifacts(
    drafts: Vec<ArtifactDraft>,
    is_cancelled: &(dyn Fn() -> bool + Sync),
    report_path: &(dyn Fn(&Path) + Sync),
    report_files: &(dyn Fn(&Path, u64, u64) + Sync),
) -> Vec<(ArtifactDraft, ArtifactMeasurement)> {
    if drafts.is_empty() {
        return Vec::new();
    }
    let next = AtomicUsize::new(0);
    let results = Mutex::new(
        (0..drafts.len())
            .map(|_| None)
            .collect::<Vec<Option<ArtifactMeasurement>>>(),
    );
    let worker_count = thread::available_parallelism()
        .map(|value| value.get())
        .unwrap_or(2)
        .min(MEASUREMENT_WORKER_LIMIT)
        .min(drafts.len());
    thread::scope(|scope| {
        for _ in 0..worker_count {
            scope.spawn(|| loop {
                let index = next.fetch_add(1, Ordering::Relaxed);
                if index >= drafts.len() || is_cancelled() {
                    break;
                }
                let mut measured = measure_directory_with_progress(
                    &drafts[index].path,
                    is_cancelled,
                    report_path,
                    report_files,
                );
                if let Err(error) =
                    validate_artifact_protection(&drafts[index].path, &measured, is_cancelled)
                {
                    measured.measured.skipped_count =
                        measured.measured.skipped_count.saturating_add(1);
                    log::warn!(
                        "project_artifact_preview_limited path={} reason=authoredContentOrUnknown error={}",
                        diagnostic_path(&drafts[index].path),
                        mangodisk_platform::diagnostics::text(&error)
                    );
                }
                if let Ok(mut values) = results.lock() {
                    values[index] = Some(measured);
                } else {
                    break;
                }
            });
        }
    });
    let measured = results
        .into_inner()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    drafts
        .into_iter()
        .zip(measured)
        .map(|(draft, result)| {
            (
                draft,
                result.unwrap_or(ArtifactMeasurement {
                    measured: MeasureResult {
                        bytes: 0,
                        file_count: 0,
                        skipped_count: 1,
                        ..MeasureResult::default()
                    },
                    modified_at_ms: None,
                    authored_entry: None,
                }),
            )
        })
        .collect()
}

pub(super) fn measure_directory(
    root: &Path,
    is_cancelled: &(dyn Fn() -> bool + Sync),
) -> ArtifactMeasurement {
    measure_directory_with_progress(root, is_cancelled, &|_| {}, &|_, _, _| {})
}

pub(super) fn measure_directory_with_progress(
    root: &Path,
    is_cancelled: &(dyn Fn() -> bool + Sync),
    report_path: &(dyn Fn(&Path) + Sync),
    report_files: &(dyn Fn(&Path, u64, u64) + Sync),
) -> ArtifactMeasurement {
    // Project-artifact measurement still publishes its authoritative totals
    // through `report_files` after success. Ignore provisional native counts
    // here so a failed native path can use the portable fallback without
    // duplicating UI counters.
    let native_progress = |path: &Path, _: u64, _: u64| report_path(path);
    match current_platform().fast_project_artifact_tree_aggregate(
        root,
        is_cancelled,
        &native_progress,
        project_artifact_protection::is_authored_entry_name,
    ) {
        Ok(Some(aggregate)) => {
            report_files(root, aggregate.file_count, aggregate.bytes);
            let modified_at_ms = aggregate
                .sources
                .iter()
                .filter_map(|source| source.modified_at_ms)
                .max();
            log::debug!(
                "project_artifact_directory_aggregate_finished strategy={} file_count={} bytes={} skipped_count={}",
                aggregate.strategy,
                aggregate.file_count,
                aggregate.bytes,
                aggregate.skipped_count
            );
            return ArtifactMeasurement {
                measured: MeasureResult {
                    bytes: aggregate.bytes,
                    file_count: aggregate.file_count,
                    skipped_count: aggregate.skipped_count,
                    read_failures: aggregate.read_failures,
                },
                modified_at_ms,
                authored_entry: aggregate.flagged_entry,
            };
        }
        Ok(None) => {}
        Err(DirectoryTreeAggregateError::Cancelled) => {
            return ArtifactMeasurement {
                measured: MeasureResult {
                    bytes: 0,
                    file_count: 0,
                    skipped_count: 1,
                    ..MeasureResult::default()
                },
                modified_at_ms: None,
                authored_entry: None,
            };
        }
        Err(DirectoryTreeAggregateError::Platform(error)) => {
            log::warn!(
                "project_artifact_directory_aggregate_fallback error={}",
                mangodisk_platform::diagnostics::text(&error)
            );
        }
    }

    portable_measure_directory_with_progress(root, is_cancelled, report_path, report_files)
}

/// Preserves the platform-independent reference implementation used when a native aggregate is
/// unavailable or fails. Keeping it callable in isolation also lets regression tests compare the
/// optimized path against the original project-artifact semantics on the same fixture.
pub(super) fn portable_measure_directory_with_progress(
    root: &Path,
    is_cancelled: &(dyn Fn() -> bool + Sync),
    report_path: &(dyn Fn(&Path) + Sync),
    report_files: &(dyn Fn(&Path, u64, u64) + Sync),
) -> ArtifactMeasurement {
    let mut result = ArtifactMeasurement::default();
    let mut stack = vec![root.to_path_buf()];
    let mut pending_file_count = 0_u64;
    let mut pending_bytes = 0_u64;
    let mut latest_file_path = None;
    while let Some(path) = stack.pop() {
        if is_cancelled() {
            result.measured.skipped_count = result.measured.skipped_count.saturating_add(1);
            break;
        }
        let metadata = match fs::symlink_metadata(&path) {
            Ok(metadata) => metadata,
            Err(error) => {
                result.measured.read_failures.record(
                    &path,
                    &error,
                    mangodisk_platform::FileReadStage::ReadMetadata,
                );
                result.measured.skipped_count = result.measured.skipped_count.saturating_add(1);
                continue;
            }
        };
        if is_link_like(&metadata) || metadata.is_file() {
            // Generated dependency trees commonly contain symbolic links.
            // Count link metadata without following its target so a safe link
            // does not degrade the entire artifact rule to Limited. Candidate
            // root links are still rejected by validation before measurement.
            result.measured.bytes = result.measured.bytes.saturating_add(metadata.len());
            result.measured.file_count = result.measured.file_count.saturating_add(1);
            result.modified_at_ms = latest_timestamp(result.modified_at_ms, modified_ms(&metadata));
            pending_file_count = pending_file_count.saturating_add(1);
            pending_bytes = pending_bytes.saturating_add(metadata.len());
            latest_file_path = Some(path);
            // Project artifact measurement can outlive the declarative rule
            // scan by several seconds. Batching keeps the UI counter live
            // without adding one atomic callback for every generated file.
            if pending_file_count >= PROGRESS_FILE_BATCH_SIZE {
                report_files(
                    latest_file_path
                        .as_deref()
                        .expect("a progress batch must contain a file path"),
                    pending_file_count,
                    pending_bytes,
                );
                pending_file_count = 0;
                pending_bytes = 0;
            }
        } else if metadata.is_dir() {
            report_path(&path);
            match fs::read_dir(&path) {
                Ok(entries) => {
                    for entry in entries {
                        match entry {
                            Ok(entry) => {
                                let name = entry.file_name();
                                if result.authored_entry.is_none()
                                    && project_artifact_protection::is_authored_entry_name(&name)
                                {
                                    result.authored_entry =
                                        Some(name.to_string_lossy().into_owned());
                                }
                                // Continue measuring the protected entry and its descendants.
                                // Preview keeps an accurate estimate even though the candidate is
                                // fail-closed and cannot enter execution.
                                stack.push(entry.path());
                            }
                            Err(error) => {
                                result.measured.read_failures.record(
                                    &path,
                                    &error,
                                    mangodisk_platform::FileReadStage::ReadDirectory,
                                );
                                result.measured.skipped_count =
                                    result.measured.skipped_count.saturating_add(1)
                            }
                        }
                    }
                }
                Err(error) => {
                    result.measured.read_failures.record(
                        &path,
                        &error,
                        mangodisk_platform::FileReadStage::OpenDirectory,
                    );
                    result.measured.skipped_count = result.measured.skipped_count.saturating_add(1)
                }
            }
        } else {
            result.measured.skipped_count = result.measured.skipped_count.saturating_add(1);
        }
    }
    if pending_file_count > 0 {
        report_files(
            latest_file_path
                .as_deref()
                .expect("a progress batch must contain a file path"),
            pending_file_count,
            pending_bytes,
        );
    }
    result
}

pub(super) fn validate_artifact_protection(
    root: &Path,
    measurement: &ArtifactMeasurement,
    is_cancelled: &(dyn Fn() -> bool + Sync),
) -> Result<(), String> {
    if let Some(entry) = &measurement.authored_entry {
        return Err(format!(
            "artifact contains protected authored entry: {entry}"
        ));
    }
    // An incomplete traversal is already fail-closed by `measurement_limited`. Avoid launching
    // Git when the filesystem result cannot authorize deletion regardless of repository state.
    if measurement.measured.skipped_count > 0 {
        return Ok(());
    }
    project_artifact_protection::validate_ownership(root, is_cancelled)
}
