use std::{
    collections::{HashMap, VecDeque},
    fs,
    path::{Path, PathBuf},
    sync::{Mutex, OnceLock},
};

use mangodisk_platform::{
    current_platform, FileSpaceUsage, FilesystemChangeMonitor, FilesystemChangeStatus,
    FilesystemChangeToken, Platform, ScanPurpose,
};

use crate::{
    filesystem::metadata::{display_fingerprint, display_path, is_link_like, modified_ms},
    shared::operation::OPERATION_CANCELLED_ERROR,
    storage::{
        analysis::{AnalysisResult, DirectoryEntryInfo},
        large_files::{LargeFileEntry, LargeFilesResult},
    },
};

const ANALYSIS_CACHE_ROOT_LIMIT: usize = 2;
const LARGE_FILE_RESULT_LIMIT: usize = 2_000;
const ANALYSIS_CACHE_UNAVAILABLE_ERROR: &str = "the analysis cache is unavailable";

/// Large-file scans retain every file above the smallest selectable threshold. Higher thresholds
/// can then be applied directly to the current in-memory scan without touching the filesystem.
pub(crate) const LARGE_FILE_INDEX_FLOOR_BYTES: u64 = 50 * 1024 * 1024;

static ANALYSIS_CACHE: OnceLock<Mutex<AnalysisCache>> = OnceLock::new();

#[derive(Clone, Copy, Default)]
pub(crate) struct DirectoryAggregate {
    /// Bytes charged to the containing volume and shown in storage views.
    pub(crate) bytes: u64,
    /// Content length retained for immutable snapshot and delete preflight checks.
    pub(crate) logical_bytes: u64,
    pub(crate) file_count: u64,
    pub(crate) skipped_count: u64,
    pub(crate) scanned_at_ms: u64,
    pub(crate) fingerprint: Option<[u8; 32]>,
}

#[derive(Clone, Copy)]
pub(crate) struct IndexedFile {
    pub(crate) bytes: u64,
    pub(crate) logical_bytes: u64,
    pub(crate) modified_at_ms: Option<u64>,
}

#[derive(Default)]
struct AnalysisCache {
    directories: HashMap<PathBuf, DirectoryAggregate>,
    files: HashMap<PathBuf, IndexedFile>,
    scan_roots: HashMap<PathBuf, ScanPurpose>,
    /// Monotonic operation identifiers prevent an older concurrent scan from replacing a newer
    /// snapshot of the same root after it finishes later.
    publish_generations: HashMap<PathBuf, u64>,
    /// Destructive cache updates increment this revision. A scan captures it before traversal and
    /// skips cache publication if a deletion happened while its private snapshot was being built.
    mutation_revision: u64,
    /// Orders cached roots from least recently used to most recently used.
    ///
    /// Directory and file maps are intentionally shared across roots to keep lookup inexpensive.
    /// The separate order only owns eviction policy and must be updated whenever a completed root
    /// is stored or reused.
    root_recency: VecDeque<PathBuf>,
    change_tokens: HashMap<PathBuf, Option<FilesystemChangeToken>>,
    change_monitors: HashMap<PathBuf, CachedChangeMonitor>,
}

struct CachedChangeMonitor {
    token: FilesystemChangeToken,
    monitor: FilesystemChangeMonitor,
}

enum ChangeValidation {
    Valid(Option<FilesystemChangeMonitor>),
    Stale,
}

pub(crate) enum CacheReuseDecision {
    Reusable,
    Miss,
}

/// Groups publication-only metadata so callers cannot confuse snapshot data with cache ordering
/// controls. The scan response is built from its private snapshot before this policy is applied.
pub(crate) struct SnapshotPublication {
    purpose: ScanPurpose,
    refresh: bool,
    change_token: Option<FilesystemChangeToken>,
    generation: u64,
    expected_mutation_revision: u64,
}

impl SnapshotPublication {
    pub(crate) const fn new(
        purpose: ScanPurpose,
        refresh: bool,
        change_token: Option<FilesystemChangeToken>,
        generation: u64,
        expected_mutation_revision: u64,
    ) -> Self {
        Self {
            purpose,
            refresh,
            change_token,
            generation,
            expected_mutation_revision,
        }
    }
}

impl ChangeValidation {
    fn into_valid_monitor(self) -> Option<Option<FilesystemChangeMonitor>> {
        match self {
            Self::Valid(monitor) => Some(monitor),
            Self::Stale => None,
        }
    }
}

/// Reuse is intentionally limited to the current process. A completed scan has one authoritative
/// result in memory, avoiding duplicate storage and write backpressure on the traversal path.
pub(crate) fn reuse_decision(
    root: &Path,
    requested: ScanPurpose,
    is_cancelled: &(dyn Fn() -> bool + Sync),
) -> Result<CacheReuseDecision, String> {
    if is_cancelled() {
        return Err(OPERATION_CANCELLED_ERROR.to_string());
    }

    let candidate = {
        let cache = cache()
            .lock()
            .map_err(|_| ANALYSIS_CACHE_UNAVAILABLE_ERROR.to_string())?;
        cache
            .directories
            .contains_key(root)
            .then(|| {
                cache
                    .scan_roots
                    .iter()
                    .filter(|(scan_root, _)| root.starts_with(scan_root))
                    .max_by_key(|(scan_root, _)| scan_root.components().count())
                    .map(|(scan_root, purpose)| {
                        let token = cache.change_tokens.get(scan_root).copied().flatten();
                        let monitor = token.and_then(|token| {
                            cache
                                .change_monitors
                                .get(scan_root)
                                .filter(|cached| cached.token == token)
                                .map(|cached| cached.monitor.clone())
                        });
                        (
                            scan_root.clone(),
                            *purpose,
                            token,
                            monitor,
                            scan_root != root,
                        )
                    })
            })
            .flatten()
    };

    let Some((scan_root, cached_purpose, token, monitor, is_descendant_page)) = candidate else {
        return Ok(CacheReuseDecision::Miss);
    };
    let purpose_compatible = matches!(
        (cached_purpose, requested),
        (ScanPurpose::Analysis, _) | (ScanPurpose::LargeFiles, ScanPurpose::LargeFiles)
    );
    if !purpose_compatible {
        evict_memory_root(&scan_root)?;
        return Ok(CacheReuseDecision::Miss);
    }

    // Descendant navigation and a large-file view derived from an analysis are reads from the
    // immutable result of the active scan. Revalidating the pre-scan change token here makes a
    // busy home directory immediately stale even though the user only changed views. An explicit
    // refresh still bypasses this function, and destructive operations independently preflight
    // every selected path before execution.
    let derives_from_active_analysis = cached_purpose == ScanPurpose::Analysis
        && (is_descendant_page || requested == ScanPurpose::LargeFiles);
    if derives_from_active_analysis {
        mark_root_recent(&scan_root)?;
        return Ok(CacheReuseDecision::Reusable);
    }

    if let Some(new_monitor) =
        validate_change_token(&scan_root, token, monitor, is_cancelled)?.into_valid_monitor()
    {
        if let (Some(token), Some(new_monitor)) = (token, new_monitor) {
            install_change_monitor(&scan_root, token, new_monitor)?;
        }
        mark_root_recent(&scan_root)?;
        return Ok(CacheReuseDecision::Reusable);
    }

    if is_cancelled() {
        return Err(OPERATION_CANCELLED_ERROR.to_string());
    }
    evict_memory_root(&scan_root)?;
    Ok(CacheReuseDecision::Miss)
}

pub(crate) fn large_files_result(
    root: &Path,
    minimum_bytes: u64,
    cache_reused: bool,
) -> Result<LargeFilesResult, String> {
    let cache = cache()
        .lock()
        .map_err(|_| ANALYSIS_CACHE_UNAVAILABLE_ERROR.to_string())?;
    let root_aggregate = cache
        .directories
        .get(root)
        .copied()
        .ok_or_else(|| "the large-file scan result is no longer available".to_string())?;
    Ok(build_large_files_result(
        root,
        root_aggregate,
        &cache.files,
        minimum_bytes,
        cache_reused,
    ))
}

fn build_large_files_result(
    root: &Path,
    root_aggregate: DirectoryAggregate,
    files: &HashMap<PathBuf, IndexedFile>,
    minimum_bytes: u64,
    cache_reused: bool,
) -> LargeFilesResult {
    let mut entries = files
        .iter()
        .filter(|(path, file)| path.starts_with(root) && file.bytes >= minimum_bytes)
        .filter(|(path, _)| {
            current_platform()
                .should_skip(path, root, ScanPurpose::LargeFiles)
                .is_none()
        })
        .map(|(path, file)| large_file_entry(path, root, *file))
        .collect::<Vec<_>>();
    entries.sort_by(|left, right| {
        right
            .bytes
            .cmp(&left.bytes)
            .then_with(|| left.path.cmp(&right.path))
    });
    let total_count = entries.len() as u64;
    let total_bytes = entries.iter().map(|entry| entry.bytes).sum();
    entries.truncate(LARGE_FILE_RESULT_LIMIT);
    let returned_count = entries.len() as u64;

    LargeFilesResult {
        scan_id: 0,
        root: display_path(root),
        scanned_at_ms: root_aggregate.scanned_at_ms,
        minimum_bytes,
        total_bytes,
        total_count,
        returned_count,
        truncated: returned_count < total_count,
        skipped_count: root_aggregate.skipped_count,
        cache_reused,
        entries,
    }
}

pub(crate) fn large_files_result_from_snapshot(
    root: &Path,
    root_aggregate: DirectoryAggregate,
    files: &HashMap<PathBuf, IndexedFile>,
    minimum_bytes: u64,
) -> LargeFilesResult {
    build_large_files_result(root, root_aggregate, files, minimum_bytes, false)
}

fn large_file_entry(path: &Path, root: &Path, file: IndexedFile) -> LargeFileEntry {
    LargeFileEntry {
        name: path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default(),
        path: display_path(path),
        parent_path: display_path(path.parent().unwrap_or(root)),
        bytes: file.bytes,
        logical_bytes: file.logical_bytes,
        modified_at_ms: file.modified_at_ms,
    }
}

pub(crate) fn analysis_result(root: &Path) -> Result<Option<AnalysisResult>, String> {
    let root_aggregate = {
        let cache = cache()
            .lock()
            .map_err(|_| ANALYSIS_CACHE_UNAVAILABLE_ERROR.to_string())?;
        cache.directories.get(root).copied()
    };
    let Some(root_aggregate) = root_aggregate else {
        return Ok(None);
    };

    let children = read_analysis_children(root)?;
    let cache = cache()
        .lock()
        .map_err(|_| ANALYSIS_CACHE_UNAVAILABLE_ERROR.to_string())?;
    Ok(Some(build_analysis_result(
        root,
        root_aggregate,
        children,
        |path| cache.directories.get(path).copied(),
        |path| cache.files.get(path).copied(),
    )))
}

pub(crate) fn analysis_result_from_snapshot(
    root: &Path,
    root_aggregate: DirectoryAggregate,
    directories: &HashMap<PathBuf, DirectoryAggregate>,
    files: &HashMap<PathBuf, IndexedFile>,
) -> Result<AnalysisResult, String> {
    let children = read_analysis_children(root)?;
    Ok(build_analysis_result(
        root,
        root_aggregate,
        children,
        |path| directories.get(path).copied(),
        |path| files.get(path).copied(),
    ))
}

fn read_analysis_children(
    root: &Path,
) -> Result<Vec<(fs::DirEntry, PathBuf, fs::Metadata)>, String> {
    Ok(fs::read_dir(root)
        .map_err(|error| format!("failed to read the analysis root: {error}"))?
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let path = entry.path();
            let metadata = fs::symlink_metadata(&path).ok()?;
            (!is_link_like(&metadata)).then_some((entry, path, metadata))
        })
        .collect())
}

fn build_analysis_result(
    root: &Path,
    root_aggregate: DirectoryAggregate,
    children: Vec<(fs::DirEntry, PathBuf, fs::Metadata)>,
    directory_aggregate: impl FnMut(&Path) -> Option<DirectoryAggregate>,
    indexed_file: impl FnMut(&Path) -> Option<IndexedFile>,
) -> AnalysisResult {
    let mut entries = build_analysis_entries(children, directory_aggregate, indexed_file);
    entries.sort_by(|left, right| {
        right
            .bytes
            .cmp(&left.bytes)
            .then_with(|| left.path.cmp(&right.path))
    });
    entries.truncate(80);

    AnalysisResult {
        scan_id: 0,
        root: display_path(root),
        scanned_at_ms: root_aggregate.scanned_at_ms,
        total_bytes: root_aggregate.bytes,
        skipped_count: root_aggregate.skipped_count,
        entries,
    }
}

fn build_analysis_entries(
    children: Vec<(fs::DirEntry, PathBuf, fs::Metadata)>,
    mut directory_aggregate: impl FnMut(&Path) -> Option<DirectoryAggregate>,
    mut indexed_file: impl FnMut(&Path) -> Option<IndexedFile>,
) -> Vec<DirectoryEntryInfo> {
    children
        .into_iter()
        .map(|(entry, path, metadata)| {
            let aggregate = if metadata.is_dir() {
                directory_aggregate(&path).unwrap_or_default()
            } else {
                let usage = indexed_file(&path)
                    .map(|file| mangodisk_platform::FileSpaceUsage {
                        logical_bytes: file.logical_bytes,
                        allocated_bytes: file.bytes,
                    })
                    .unwrap_or_else(|| current_platform().file_space_usage(&path, &metadata));
                DirectoryAggregate {
                    bytes: usage.allocated_bytes,
                    logical_bytes: usage.logical_bytes,
                    file_count: u64::from(metadata.is_file()),
                    ..DirectoryAggregate::default()
                }
            };
            DirectoryEntryInfo {
                name: entry.file_name().to_string_lossy().into_owned(),
                path: display_path(&path),
                bytes: aggregate.bytes,
                logical_bytes: aggregate.logical_bytes,
                file_count: aggregate.file_count,
                is_directory: metadata.is_dir(),
                modified_at_ms: modified_ms(&metadata),
                content_fingerprint: metadata
                    .is_dir()
                    .then(|| aggregate.fingerprint.map(display_fingerprint))
                    .flatten(),
            }
        })
        .collect()
}

pub(crate) fn store_memory_only(
    root: &Path,
    root_aggregate: DirectoryAggregate,
    scanned_directories: HashMap<PathBuf, DirectoryAggregate>,
    scanned_files: HashMap<PathBuf, IndexedFile>,
    publication: SnapshotPublication,
) -> Result<bool, String> {
    let removed_monitors = {
        let mut cache = cache()
            .lock()
            .map_err(|_| ANALYSIS_CACHE_UNAVAILABLE_ERROR.to_string())?;
        if cache.mutation_revision != publication.expected_mutation_revision {
            log::info!(
                "analysis_cache_publish_skipped generation={} reason=concurrent_mutation expected_revision={} actual_revision={}",
                publication.generation,
                publication.expected_mutation_revision,
                cache.mutation_revision
            );
            return Ok(false);
        }
        // Refreshing an ancestor removes descendant snapshots, and refreshing a descendant
        // rewrites entries that also belong to an ancestor snapshot. Compare every overlapping
        // root so a slower older scan cannot erase or partially mix a newer concurrent result.
        if cache
            .publish_generations
            .iter()
            .any(|(cached_root, generation)| {
                (cached_root.starts_with(root) || root.starts_with(cached_root))
                    && *generation > publication.generation
            })
        {
            log::info!(
                "analysis_cache_publish_skipped generation={} reason=newer_snapshot",
                publication.generation
            );
            return Ok(false);
        }
        let mut removed_monitors = Vec::new();
        // The directory and file maps are flattened across cached roots. Any overlapping
        // publication must therefore replace its subtree atomically, even when the caller did not
        // request an explicit refresh. This occurs when two compatible scan kinds finish out of
        // order on an ancestor and descendant. Leaving the old nested root metadata behind would
        // let its older change token evict records now owned by the newer ancestor snapshot.
        let replaces_overlapping_snapshot = publication.refresh
            || cache
                .scan_roots
                .keys()
                .any(|cached_root| cached_root.starts_with(root) || root.starts_with(cached_root));
        if replaces_overlapping_snapshot {
            if let Some(previous) = cache.directories.get(root).copied() {
                for (path, aggregate) in &mut cache.directories {
                    if path != root && root.starts_with(path) {
                        aggregate.bytes = aggregate
                            .bytes
                            .saturating_sub(previous.bytes)
                            .saturating_add(root_aggregate.bytes);
                        aggregate.logical_bytes = aggregate
                            .logical_bytes
                            .saturating_sub(previous.logical_bytes)
                            .saturating_add(root_aggregate.logical_bytes);
                        aggregate.file_count = aggregate
                            .file_count
                            .saturating_sub(previous.file_count)
                            .saturating_add(root_aggregate.file_count);
                        aggregate.skipped_count = aggregate
                            .skipped_count
                            .saturating_sub(previous.skipped_count)
                            .saturating_add(root_aggregate.skipped_count);
                        aggregate.scanned_at_ms = root_aggregate.scanned_at_ms;
                    }
                }
            }
            cache.directories.retain(|path, _| !path.starts_with(root));
            cache.files.retain(|path, _| !path.starts_with(root));
            cache.scan_roots.retain(|path, _| !path.starts_with(root));
            cache
                .publish_generations
                .retain(|path, _| !path.starts_with(root));
            cache.root_recency.retain(|path| !path.starts_with(root));
            cache
                .change_tokens
                .retain(|path, _| !path.starts_with(root));
            removed_monitors.extend(take_monitors(&mut cache, |path| path.starts_with(root)));
        } else if cache
            .change_monitors
            .get(root)
            .is_some_and(|cached| Some(cached.token) != publication.change_token)
        {
            if let Some(cached) = cache.change_monitors.remove(root) {
                removed_monitors.push(cached.monitor);
            }
        }
        // Overlapping roots are removed before applying the capacity limit so replacing a cached
        // descendant with its ancestor reuses that slot instead of evicting an unrelated root.
        while !cache.scan_roots.contains_key(root)
            && cache.scan_roots.len() >= ANALYSIS_CACHE_ROOT_LIMIT
        {
            let least_recent_root = cache
                .root_recency
                .front()
                .cloned()
                .ok_or_else(|| "the analysis cache root recency is inconsistent".to_string())?;
            let roots_before = cache.scan_roots.len();
            removed_monitors.extend(evict_cached_root(&mut cache, &least_recent_root));
            log::info!(
                "analysis_cache_root_evicted roots_before={} roots_after={} root_limit={}",
                roots_before,
                cache.scan_roots.len(),
                ANALYSIS_CACHE_ROOT_LIMIT
            );
        }
        cache.directories.extend(scanned_directories);
        cache.files.extend(scanned_files);
        cache
            .scan_roots
            .insert(root.to_path_buf(), publication.purpose);
        cache
            .publish_generations
            .insert(root.to_path_buf(), publication.generation);
        touch_root(&mut cache, root);
        cache
            .change_tokens
            .insert(root.to_path_buf(), publication.change_token);
        removed_monitors
    };
    drop(removed_monitors);
    Ok(true)
}

pub(crate) fn mutation_revision() -> Result<u64, String> {
    cache()
        .lock()
        .map(|cache| cache.mutation_revision)
        .map_err(|_| ANALYSIS_CACHE_UNAVAILABLE_ERROR.to_string())
}

pub(crate) fn remove_entry(
    target: &Path,
    removed_usage: FileSpaceUsage,
    file_count: u64,
    is_directory: bool,
) {
    let removed_monitors = {
        let Ok(mut cache) = cache().lock() else {
            log::warn!("analysis_cache_update_failed reason=poisoned_lock");
            return;
        };
        cache.mutation_revision = cache.mutation_revision.saturating_add(1);
        let removed_usage = if is_directory {
            cache
                .directories
                .get(target)
                .map(|aggregate| FileSpaceUsage {
                    logical_bytes: aggregate.logical_bytes,
                    allocated_bytes: aggregate.bytes,
                })
        } else {
            cache.files.get(target).map(|file| FileSpaceUsage {
                logical_bytes: file.logical_bytes,
                allocated_bytes: file.bytes,
            })
        }
        .unwrap_or(removed_usage);
        let removed_monitors = if is_directory {
            cache.files.retain(|path, _| !path.starts_with(target));
            cache
                .directories
                .retain(|path, _| !path.starts_with(target));
            cache.scan_roots.retain(|path, _| !path.starts_with(target));
            cache
                .publish_generations
                .retain(|path, _| !path.starts_with(target));
            cache.root_recency.retain(|path| !path.starts_with(target));
            cache
                .change_tokens
                .retain(|path, _| !path.starts_with(target));
            take_monitors(&mut cache, |path| path.starts_with(target))
        } else {
            cache.files.remove(target);
            Vec::new()
        };
        for (directory, aggregate) in &mut cache.directories {
            if target.starts_with(directory) {
                aggregate.bytes = aggregate
                    .bytes
                    .saturating_sub(removed_usage.allocated_bytes);
                aggregate.logical_bytes = aggregate
                    .logical_bytes
                    .saturating_sub(removed_usage.logical_bytes);
                aggregate.file_count = aggregate.file_count.saturating_sub(file_count);
                aggregate.fingerprint = None;
            }
        }
        removed_monitors
    };
    drop(removed_monitors);
}

pub(crate) fn clear_all() -> Result<(), String> {
    let previous = {
        let mut cache = cache()
            .lock()
            .map_err(|_| ANALYSIS_CACHE_UNAVAILABLE_ERROR.to_string())?;
        let next_revision = cache.mutation_revision.saturating_add(1);
        let mut previous = std::mem::take(&mut *cache);
        cache.mutation_revision = next_revision;
        previous.mutation_revision = 0;
        previous
    };
    drop(previous);
    Ok(())
}

#[cfg(test)]
pub(crate) fn memory_entry_counts() -> Result<(usize, usize, usize), String> {
    let cache = cache()
        .lock()
        .map_err(|_| ANALYSIS_CACHE_UNAVAILABLE_ERROR.to_string())?;
    Ok((
        cache.scan_roots.len(),
        cache.directories.len(),
        cache.files.len(),
    ))
}

fn cache() -> &'static Mutex<AnalysisCache> {
    ANALYSIS_CACHE.get_or_init(|| Mutex::new(AnalysisCache::default()))
}

fn validate_change_token(
    root: &Path,
    token: Option<FilesystemChangeToken>,
    monitor: Option<FilesystemChangeMonitor>,
    is_cancelled: &(dyn Fn() -> bool + Sync),
) -> Result<ChangeValidation, String> {
    if is_cancelled() {
        return Err(OPERATION_CANCELLED_ERROR.to_string());
    }
    let Some(token) = token else {
        return Ok(ChangeValidation::Stale);
    };
    if let Some(monitor) = monitor {
        return Ok(match monitor.status() {
            FilesystemChangeStatus::Clean => ChangeValidation::Valid(None),
            FilesystemChangeStatus::Pending
            | FilesystemChangeStatus::Changed
            | FilesystemChangeStatus::HistoryUnavailable => ChangeValidation::Stale,
        });
    }

    let started = current_platform().start_filesystem_change_monitor(root, &token, is_cancelled);
    if is_cancelled() {
        return Err(OPERATION_CANCELLED_ERROR.to_string());
    }
    match started {
        Ok(Some(monitor)) if monitor.status() == FilesystemChangeStatus::Clean => {
            let reusable_monitor = cfg!(target_os = "macos").then_some(monitor);
            Ok(ChangeValidation::Valid(reusable_monitor))
        }
        Ok(Some(_) | None) => Ok(ChangeValidation::Stale),
        Err(error) => {
            log::warn!(
                "analysis_cache_change_validation_failed diagnostic={}",
                error.diagnostic()
            );
            Ok(ChangeValidation::Stale)
        }
    }
}

fn install_change_monitor(
    root: &Path,
    token: FilesystemChangeToken,
    monitor: FilesystemChangeMonitor,
) -> Result<(), String> {
    let removed_monitors = {
        let mut cache = cache()
            .lock()
            .map_err(|_| ANALYSIS_CACHE_UNAVAILABLE_ERROR.to_string())?;
        let mut removed_monitors = Vec::new();
        if let Some(previous) = cache
            .change_monitors
            .insert(root.to_path_buf(), CachedChangeMonitor { token, monitor })
        {
            removed_monitors.push(previous.monitor);
        }
        removed_monitors
    };
    drop(removed_monitors);
    Ok(())
}

fn take_monitors(
    cache: &mut AnalysisCache,
    mut matches: impl FnMut(&Path) -> bool,
) -> Vec<FilesystemChangeMonitor> {
    let roots = cache
        .change_monitors
        .keys()
        .filter(|root| matches(root))
        .cloned()
        .collect::<Vec<_>>();
    roots
        .into_iter()
        .filter_map(|root| cache.change_monitors.remove(&root))
        .map(|cached| cached.monitor)
        .collect()
}

fn evict_memory_root(root: &Path) -> Result<(), String> {
    let removed_monitors = {
        let mut cache = cache()
            .lock()
            .map_err(|_| ANALYSIS_CACHE_UNAVAILABLE_ERROR.to_string())?;
        evict_cached_root(&mut cache, root)
    };
    drop(removed_monitors);
    Ok(())
}

fn mark_root_recent(root: &Path) -> Result<(), String> {
    let mut cache = cache()
        .lock()
        .map_err(|_| ANALYSIS_CACHE_UNAVAILABLE_ERROR.to_string())?;
    if cache.scan_roots.contains_key(root) {
        touch_root(&mut cache, root);
    }
    Ok(())
}

fn touch_root(cache: &mut AnalysisCache, root: &Path) {
    cache.root_recency.retain(|cached| cached != root);
    cache.root_recency.push_back(root.to_path_buf());
}

/// Removes one independently cached root and any nested refresh roots that share its records.
/// Nested roots cannot outlive their owner because the flattened directory and file maps contain
/// overlapping keys. Distinct roots remain available and preserve their relative recency.
fn evict_cached_root(cache: &mut AnalysisCache, root: &Path) -> Vec<FilesystemChangeMonitor> {
    cache.directories.retain(|path, _| !path.starts_with(root));
    cache.files.retain(|path, _| !path.starts_with(root));
    cache.scan_roots.retain(|path, _| !path.starts_with(root));
    cache
        .publish_generations
        .retain(|path, _| !path.starts_with(root));
    cache.root_recency.retain(|path| !path.starts_with(root));
    cache
        .change_tokens
        .retain(|path, _| !path.starts_with(root));
    take_monitors(cache, |path| path.starts_with(root))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store_test_analysis_root(root: &Path, scanned_at_ms: u64) {
        let aggregate = DirectoryAggregate {
            scanned_at_ms,
            ..DirectoryAggregate::default()
        };
        store_memory_only(
            root,
            aggregate,
            HashMap::from([(root.to_path_buf(), aggregate)]),
            HashMap::new(),
            SnapshotPublication::new(
                ScanPurpose::Analysis,
                true,
                None,
                scanned_at_ms,
                mutation_revision().expect("test cache revision should load"),
            ),
        )
        .expect("test analysis result should store");
    }

    #[test]
    fn missing_change_token_is_stale() {
        let validation =
            validate_change_token(Path::new("/missing-change-token"), None, None, &|| false)
                .expect("validation should succeed");
        assert!(matches!(validation, ChangeValidation::Stale));
    }

    #[test]
    fn large_file_results_are_derived_from_memory() {
        let _operation_lock = crate::shared::operation::test_operation_lock();
        clear_all().expect("cache should clear");
        let root = PathBuf::from("/memory-large-files");
        let file = root.join("large.bin");
        let aggregate = DirectoryAggregate {
            bytes: LARGE_FILE_INDEX_FLOOR_BYTES,
            file_count: 1,
            scanned_at_ms: 7,
            ..DirectoryAggregate::default()
        };
        store_memory_only(
            &root,
            aggregate,
            HashMap::from([(root.clone(), aggregate)]),
            HashMap::from([(
                file,
                IndexedFile {
                    bytes: LARGE_FILE_INDEX_FLOOR_BYTES,
                    logical_bytes: LARGE_FILE_INDEX_FLOOR_BYTES,
                    modified_at_ms: Some(5),
                },
            )]),
            SnapshotPublication::new(
                ScanPurpose::LargeFiles,
                true,
                None,
                7,
                mutation_revision().expect("test cache revision should load"),
            ),
        )
        .expect("memory result should store");

        let result = large_files_result(&root, LARGE_FILE_INDEX_FLOOR_BYTES, false)
            .expect("memory result should load");
        assert_eq!(result.total_count, 1);
        assert_eq!(result.total_bytes, LARGE_FILE_INDEX_FLOOR_BYTES);
        clear_all().expect("cache should clear");
    }

    #[test]
    fn uncached_file_removal_subtracts_allocated_and_logical_sizes_independently() {
        let _operation_lock = crate::shared::operation::test_operation_lock();
        clear_all().expect("cache should clear");
        let root = PathBuf::from("/memory-analysis-removal");
        let file = root.join("small.bin");
        let aggregate = DirectoryAggregate {
            bytes: 4_096,
            logical_bytes: 1,
            file_count: 1,
            scanned_at_ms: 10,
            ..DirectoryAggregate::default()
        };
        store_memory_only(
            &root,
            aggregate,
            HashMap::from([(root.clone(), aggregate)]),
            HashMap::new(),
            SnapshotPublication::new(
                ScanPurpose::Analysis,
                true,
                None,
                10,
                mutation_revision().expect("test cache revision should load"),
            ),
        )
        .expect("analysis result should store");

        remove_entry(
            &file,
            FileSpaceUsage {
                logical_bytes: 1,
                allocated_bytes: 4_096,
            },
            1,
            false,
        );

        let cache = cache().lock().expect("cache should remain available");
        let updated = cache
            .directories
            .get(&root)
            .expect("the containing aggregate should remain cached");
        assert_eq!(updated.bytes, 0);
        assert_eq!(updated.logical_bytes, 0);
        assert_eq!(updated.file_count, 0);
        drop(cache);
        clear_all().expect("cache should clear");
    }

    #[test]
    fn large_file_view_reuses_active_analysis_without_change_history() {
        let _operation_lock = crate::shared::operation::test_operation_lock();
        clear_all().expect("cache should clear");
        let root = PathBuf::from("/memory-analysis-for-large-files");
        let aggregate = DirectoryAggregate {
            bytes: LARGE_FILE_INDEX_FLOOR_BYTES,
            file_count: 1,
            scanned_at_ms: 8,
            ..DirectoryAggregate::default()
        };
        store_memory_only(
            &root,
            aggregate,
            HashMap::from([(root.clone(), aggregate)]),
            HashMap::new(),
            SnapshotPublication::new(
                ScanPurpose::Analysis,
                true,
                None,
                8,
                mutation_revision().expect("test cache revision should load"),
            ),
        )
        .expect("analysis result should store");

        let decision = reuse_decision(&root, ScanPurpose::LargeFiles, &|| false)
            .expect("cache reuse should succeed");
        assert!(matches!(decision, CacheReuseDecision::Reusable));
        clear_all().expect("cache should clear");
    }

    #[test]
    fn clearing_memory_removes_the_only_scan_result() {
        let _operation_lock = crate::shared::operation::test_operation_lock();
        clear_all().expect("cache should clear");
        let root = PathBuf::from("/memory-only-result");
        let aggregate = DirectoryAggregate {
            scanned_at_ms: 9,
            ..DirectoryAggregate::default()
        };
        store_memory_only(
            &root,
            aggregate,
            HashMap::from([(root.clone(), aggregate)]),
            HashMap::new(),
            SnapshotPublication::new(
                ScanPurpose::Analysis,
                true,
                None,
                9,
                mutation_revision().expect("test cache revision should load"),
            ),
        )
        .expect("memory result should store");
        clear_all().expect("cache should clear");
        assert!(analysis_result(&root)
            .expect("cache lookup should succeed")
            .is_none());
    }

    #[test]
    fn older_concurrent_snapshot_cannot_replace_a_newer_generation() {
        let _operation_lock = crate::shared::operation::test_operation_lock();
        clear_all().expect("cache should clear");
        let root = PathBuf::from("/memory-generation-order");
        let newer = DirectoryAggregate {
            bytes: 2,
            logical_bytes: 2,
            file_count: 1,
            scanned_at_ms: 2,
            ..DirectoryAggregate::default()
        };
        let revision = mutation_revision().expect("cache revision should load");
        assert!(store_memory_only(
            &root,
            newer,
            HashMap::from([(root.clone(), newer)]),
            HashMap::new(),
            SnapshotPublication::new(ScanPurpose::Analysis, true, None, 20, revision),
        )
        .expect("newer snapshot should publish"));

        let older = DirectoryAggregate {
            bytes: 1,
            logical_bytes: 1,
            file_count: 1,
            scanned_at_ms: 1,
            ..DirectoryAggregate::default()
        };
        assert!(!store_memory_only(
            &root,
            older,
            HashMap::from([(root.clone(), older)]),
            HashMap::new(),
            SnapshotPublication::new(ScanPurpose::Analysis, true, None, 10, revision),
        )
        .expect("older snapshot should be skipped safely"));
        assert_eq!(
            cache()
                .lock()
                .expect("cache should remain available")
                .directories
                .get(&root)
                .expect("newer root should remain")
                .scanned_at_ms,
            2
        );
        clear_all().expect("cache should clear");
    }

    #[test]
    fn older_ancestor_snapshot_cannot_erase_a_newer_descendant() {
        let _operation_lock = crate::shared::operation::test_operation_lock();
        clear_all().expect("cache should clear");
        let root = PathBuf::from("/memory-overlapping-generation");
        let descendant = root.join("nested");
        let newer = DirectoryAggregate {
            bytes: 2,
            logical_bytes: 2,
            file_count: 1,
            scanned_at_ms: 2,
            ..DirectoryAggregate::default()
        };
        let revision = mutation_revision().expect("cache revision should load");
        assert!(store_memory_only(
            &descendant,
            newer,
            HashMap::from([(descendant.clone(), newer)]),
            HashMap::new(),
            SnapshotPublication::new(ScanPurpose::Analysis, true, None, 20, revision),
        )
        .expect("newer descendant should publish"));

        let older = DirectoryAggregate {
            bytes: 1,
            logical_bytes: 1,
            file_count: 1,
            scanned_at_ms: 1,
            ..DirectoryAggregate::default()
        };
        assert!(!store_memory_only(
            &root,
            older,
            HashMap::from([(root.clone(), older)]),
            HashMap::new(),
            SnapshotPublication::new(ScanPurpose::Analysis, true, None, 10, revision),
        )
        .expect("older ancestor should be skipped safely"));
        let cache = cache().lock().expect("cache should remain available");
        assert!(cache.directories.contains_key(&descendant));
        assert!(!cache.directories.contains_key(&root));
        drop(cache);
        clear_all().expect("cache should clear");
    }

    #[test]
    fn newer_ancestor_replaces_cached_descendant_without_an_explicit_refresh() {
        let _operation_lock = crate::shared::operation::test_operation_lock();
        clear_all().expect("cache should clear");
        let root = PathBuf::from("/memory-overlapping-replacement");
        let descendant = root.join("nested");
        let unrelated = PathBuf::from("/memory-overlapping-unrelated");
        let stale_file = descendant.join("stale.bin");
        store_test_analysis_root(&unrelated, 5);
        let old = DirectoryAggregate {
            bytes: 1,
            logical_bytes: 1,
            file_count: 1,
            scanned_at_ms: 1,
            ..DirectoryAggregate::default()
        };
        let revision = mutation_revision().expect("cache revision should load");
        assert!(store_memory_only(
            &descendant,
            old,
            HashMap::from([(descendant.clone(), old)]),
            HashMap::from([(
                stale_file.clone(),
                IndexedFile {
                    bytes: 1,
                    logical_bytes: 1,
                    modified_at_ms: None,
                },
            )]),
            SnapshotPublication::new(ScanPurpose::Analysis, true, None, 10, revision),
        )
        .expect("descendant snapshot should publish"));

        let replacement = DirectoryAggregate {
            bytes: 2,
            logical_bytes: 2,
            file_count: 1,
            scanned_at_ms: 2,
            ..DirectoryAggregate::default()
        };
        assert!(store_memory_only(
            &root,
            replacement,
            HashMap::from([
                (root.clone(), replacement),
                (descendant.clone(), replacement),
            ]),
            HashMap::new(),
            SnapshotPublication::new(ScanPurpose::Analysis, false, None, 20, revision),
        )
        .expect("newer ancestor snapshot should publish"));

        let cache = cache().lock().expect("cache should remain available");
        assert_eq!(cache.scan_roots.len(), 2);
        assert!(cache.scan_roots.contains_key(&root));
        assert!(cache.scan_roots.contains_key(&unrelated));
        assert!(!cache.scan_roots.contains_key(&descendant));
        assert!(!cache.files.contains_key(&stale_file));
        assert_eq!(
            cache
                .directories
                .get(&descendant)
                .expect("replacement descendant should remain")
                .scanned_at_ms,
            2
        );
        drop(cache);
        clear_all().expect("cache should clear");
    }

    #[test]
    fn concurrent_mutation_prevents_stale_snapshot_publication() {
        let _operation_lock = crate::shared::operation::test_operation_lock();
        clear_all().expect("cache should clear");
        let root = PathBuf::from("/memory-concurrent-mutation");
        let aggregate = DirectoryAggregate {
            bytes: 1,
            logical_bytes: 1,
            file_count: 1,
            scanned_at_ms: 1,
            ..DirectoryAggregate::default()
        };
        let revision = mutation_revision().expect("cache revision should load");
        remove_entry(
            &root.join("removed.bin"),
            FileSpaceUsage {
                logical_bytes: 1,
                allocated_bytes: 1,
            },
            1,
            false,
        );

        assert!(!store_memory_only(
            &root,
            aggregate,
            HashMap::from([(root.clone(), aggregate)]),
            HashMap::new(),
            SnapshotPublication::new(ScanPurpose::Analysis, true, None, 1, revision),
        )
        .expect("stale publication should be skipped safely"));
        assert!(!cache()
            .lock()
            .expect("cache should remain available")
            .directories
            .contains_key(&root));
        clear_all().expect("cache should clear");
    }

    #[test]
    fn storing_a_third_root_evicts_only_the_least_recent_root() {
        let _operation_lock = crate::shared::operation::test_operation_lock();
        clear_all().expect("cache should clear");
        let root_a = PathBuf::from("/memory-lru-a");
        let root_b = PathBuf::from("/memory-lru-b");
        let root_c = PathBuf::from("/memory-lru-c");

        store_test_analysis_root(&root_a, 1);
        store_test_analysis_root(&root_b, 2);
        store_test_analysis_root(&root_c, 3);

        let cache = cache().lock().expect("cache should be readable");
        assert!(!cache.scan_roots.contains_key(&root_a));
        assert!(!cache.directories.contains_key(&root_a));
        assert!(cache.scan_roots.contains_key(&root_b));
        assert!(cache.directories.contains_key(&root_b));
        assert!(cache.scan_roots.contains_key(&root_c));
        assert!(cache.directories.contains_key(&root_c));
        assert_eq!(
            cache.root_recency,
            VecDeque::from([root_b.clone(), root_c.clone()])
        );
        drop(cache);
        clear_all().expect("cache should clear");
    }

    #[test]
    fn reusing_a_root_updates_its_eviction_recency() {
        let _operation_lock = crate::shared::operation::test_operation_lock();
        clear_all().expect("cache should clear");
        let root_a = PathBuf::from("/memory-lru-reused-a");
        let root_b = PathBuf::from("/memory-lru-reused-b");
        let root_c = PathBuf::from("/memory-lru-reused-c");

        store_test_analysis_root(&root_a, 1);
        store_test_analysis_root(&root_b, 2);
        let decision = reuse_decision(&root_a, ScanPurpose::LargeFiles, &|| false)
            .expect("analysis result should be reusable for large files");
        assert!(matches!(decision, CacheReuseDecision::Reusable));
        store_test_analysis_root(&root_c, 3);

        let cache = cache().lock().expect("cache should be readable");
        assert!(cache.scan_roots.contains_key(&root_a));
        assert!(!cache.scan_roots.contains_key(&root_b));
        assert!(cache.scan_roots.contains_key(&root_c));
        assert_eq!(
            cache.root_recency,
            VecDeque::from([root_a.clone(), root_c.clone()])
        );
        drop(cache);
        clear_all().expect("cache should clear");
    }
}
