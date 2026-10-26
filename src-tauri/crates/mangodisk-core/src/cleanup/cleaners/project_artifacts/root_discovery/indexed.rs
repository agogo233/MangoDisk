use super::super::super::project_artifact_schema::ProjectArtifactRuleSource;
use super::super::project_discovery::artifact_prune_names;
use super::macos_root_requires_explicit_consent;
use super::{
    deep_discovery_roots, deep_roots_for_volume, standard_discovery_roots,
    validated_marker_project_root, ProjectDiscoveryRoots, ProjectDiscoveryScan,
    ALWAYS_SKIPPED_DIRECTORIES, DEEP_ALWAYS_SKIPPED_DIRECTORIES, MAX_DISCOVERY_DEPTH,
};
use crate::cleanup::exclusions::CleanupExclusions;
use crate::filesystem::metadata::diagnostic_path;
use mangodisk_platform::{
    current_platform, Platform, ProjectMarkerCandidateQuery, ProjectMarkerCandidateScanError,
};
use std::{
    collections::HashSet,
    path::{Path, PathBuf},
    time::Instant,
};

enum IndexedProjectRootOutcome {
    Indexed {
        exact_roots: Vec<PathBuf>,
        fallback_roots: Vec<PathBuf>,
    },
    Fallback,
    Cancelled,
}

struct IndexedProjectRootRequest<'a> {
    search_root: &'a Path,
    allowed_roots: &'a [PathBuf],
    file_names: &'a [String],
    file_suffixes: &'a [String],
    prune_names: &'a HashSet<String>,
    is_cancelled: &'a (dyn Fn() -> bool + Sync),
    report_path: &'a (dyn Fn(&Path) + Sync),
    report_files: &'a (dyn Fn(&Path, u64, u64) + Sync),
    exclusions: &'a CleanupExclusions,
}

pub(super) fn deep_project_root_candidates(
    home: &Path,
    system_volume: &Path,
    local_volume_roots: &[PathBuf],
    rules: &[ProjectArtifactRuleSource],
    scan: &ProjectDiscoveryScan<'_>,
) -> ProjectDiscoveryRoots {
    let ProjectDiscoveryScan {
        is_cancelled,
        report_path,
        report_files,
        exclusions,
    } = scan;
    let allowed_roots = deep_discovery_roots(home, system_volume, local_volume_roots);
    let file_names = project_marker_file_names(rules);
    let file_suffixes = project_marker_file_suffixes(rules);
    let prune_names = artifact_prune_names(rules);
    let mut exact_roots = Vec::new();
    let mut fallback_roots = Vec::new();

    for volume_root in local_volume_roots {
        if is_cancelled() {
            break;
        }
        let scoped_roots = deep_roots_for_volume(
            &allowed_roots,
            volume_root,
            system_volume,
            local_volume_roots,
        );
        if scoped_roots.is_empty() {
            continue;
        }
        match indexed_project_roots(IndexedProjectRootRequest {
            search_root: volume_root,
            allowed_roots: &scoped_roots,
            file_names: &file_names,
            file_suffixes: &file_suffixes,
            prune_names: &prune_names,
            is_cancelled,
            report_path,
            report_files,
            exclusions,
        }) {
            IndexedProjectRootOutcome::Indexed {
                exact_roots: indexed,
                fallback_roots: fallback,
            } => {
                exact_roots.extend(indexed);
                fallback_roots.extend(fallback);
            }
            IndexedProjectRootOutcome::Fallback => fallback_roots.extend(scoped_roots),
            IndexedProjectRootOutcome::Cancelled => break,
        }
    }

    ProjectDiscoveryRoots {
        exact_roots,
        recursive_roots: fallback_roots,
    }
}

pub(super) fn standard_project_root_candidates(
    home: &Path,
    runtime_data_paths: &[PathBuf],
    rules: &[ProjectArtifactRuleSource],
    scan: &ProjectDiscoveryScan<'_>,
) -> ProjectDiscoveryRoots {
    let ProjectDiscoveryScan {
        is_cancelled,
        report_path,
        report_files,
        exclusions,
    } = scan;
    let allowed_roots = standard_discovery_roots(home, runtime_data_paths);
    if allowed_roots.is_empty() {
        return ProjectDiscoveryRoots::default();
    }
    let file_names = project_marker_file_names(rules);
    let file_suffixes = project_marker_file_suffixes(rules);
    let prune_names = artifact_prune_names(rules);
    match indexed_project_roots(IndexedProjectRootRequest {
        search_root: home,
        allowed_roots: &allowed_roots,
        file_names: &file_names,
        file_suffixes: &file_suffixes,
        prune_names: &prune_names,
        is_cancelled,
        report_path,
        report_files,
        exclusions,
    }) {
        IndexedProjectRootOutcome::Indexed {
            exact_roots,
            fallback_roots,
        } => ProjectDiscoveryRoots {
            exact_roots,
            recursive_roots: fallback_roots,
        },
        IndexedProjectRootOutcome::Fallback => ProjectDiscoveryRoots {
            exact_roots: Vec::new(),
            recursive_roots: allowed_roots,
        },
        IndexedProjectRootOutcome::Cancelled => ProjectDiscoveryRoots::default(),
    }
}

fn indexed_project_roots(request: IndexedProjectRootRequest<'_>) -> IndexedProjectRootOutcome {
    let IndexedProjectRootRequest {
        search_root,
        allowed_roots,
        file_names,
        file_suffixes,
        prune_names,
        is_cancelled,
        report_path,
        report_files,
        exclusions,
    } = request;
    let mut indexed_candidates = Vec::new();
    let mut fallback_roots = Vec::new();
    let prune_names_for_native_scan = native_project_scan_prune_names(prune_names);
    let mut candidate_count = 0_u64;
    let mut fallback_root_count = 0_usize;
    let mut strategies = HashSet::new();
    let mut consent_protected_roots_unavailable = false;

    // The native scanner must walk the exact same roots as the portable fallback. Scanning the
    // common ancestor first and rejecting candidates afterwards still performs all filesystem I/O
    // under excluded runtime-data trees, which made standard scans slower without changing their
    // results. A failed or unsupported root is appended as a fallback discovery root, so one
    // inaccessible volume cannot discard candidates already obtained from another volume.
    for allowed_root in allowed_roots {
        if is_cancelled() {
            return IndexedProjectRootOutcome::Cancelled;
        }
        if exclusions.matches(allowed_root) {
            log::info!(
                "project_marker_scan_root_skipped scope={} reason=excludedFolder",
                diagnostic_path(allowed_root)
            );
            continue;
        }
        if exclusions.intersects(allowed_root) {
            // The native marker query prunes by directory name, not by path.
            // Only this affected root needs the portable path-aware traversal.
            fallback_root_count = fallback_root_count.saturating_add(1);
            fallback_roots.push(allowed_root.clone());
            log::info!(
                "project_marker_scan_root_fallback scope={} reason=excludedDescendant",
                diagnostic_path(allowed_root)
            );
            continue;
        }
        if consent_protected_roots_unavailable && macos_root_requires_explicit_consent(allowed_root)
        {
            log::info!(
                "project_marker_fast_scan_root_skipped scope={} reason=consent_protected_access_unavailable",
                diagnostic_path(allowed_root)
            );
            continue;
        }
        let root_started = Instant::now();
        let result = current_platform().fast_project_marker_candidates(
            ProjectMarkerCandidateQuery {
                root: allowed_root,
                file_names,
                file_suffixes,
                pruned_directory_names: &prune_names_for_native_scan,
                maximum_depth: MAX_DISCOVERY_DEPTH,
            },
            is_cancelled,
            &|progress| report_path(&progress.current_directory),
            &mut |marker_path| {
                report_path(&marker_path);
                if let Some(project_root) = validated_marker_project_root(
                    &marker_path,
                    allowed_roots,
                    prune_names,
                    MAX_DISCOVERY_DEPTH,
                ) {
                    indexed_candidates.push(project_root);
                }
                Ok(())
            },
        );
        match result {
            Ok(Some(summary)) => {
                candidate_count = candidate_count.saturating_add(summary.candidate_count);
                strategies.insert(summary.strategy);
                // Publish counters only after the native root completes. Paths remain live during
                // enumeration, while delayed counter commit prevents a failed fast path and its
                // portable fallback from double-counting the same filesystem work.
                report_files(allowed_root, summary.file_count, 0);
                log::info!(
                    "project_marker_fast_scan_root_completed scope={} strategy={} candidate_count={} file_count={} directory_count={} elapsed_ms={}",
                    diagnostic_path(allowed_root),
                    summary.strategy,
                    summary.candidate_count,
                    summary.file_count,
                    summary.directory_count,
                    root_started.elapsed().as_millis()
                );
            }
            Ok(None) => {
                fallback_root_count = fallback_root_count.saturating_add(1);
                fallback_roots.push(allowed_root.clone());
                log::info!(
                    "project_marker_fast_scan_root_unavailable scope={} elapsed_ms={}",
                    diagnostic_path(allowed_root),
                    root_started.elapsed().as_millis()
                );
            }
            Err(ProjectMarkerCandidateScanError::Cancelled) => {
                return IndexedProjectRootOutcome::Cancelled;
            }
            Err(ProjectMarkerCandidateScanError::Unavailable(error)) => {
                if macos_root_requires_explicit_consent(allowed_root) {
                    // Access to Desktop, Documents, and the other protected
                    // home folders is granted to the process as one macOS
                    // privacy capability. A timeout on one of them predicts
                    // the same block for its peers, so skip only that known
                    // protected set while continuing custom project roots.
                    consent_protected_roots_unavailable = true;
                }
                log::warn!(
                    "project_marker_fast_scan_root_skipped scope={} elapsed_ms={} reason=unavailable error={}",
                    diagnostic_path(allowed_root),
                    root_started.elapsed().as_millis(),
                    mangodisk_platform::diagnostics::text(&error)
                );
            }
            Err(ProjectMarkerCandidateScanError::Platform(error))
            | Err(ProjectMarkerCandidateScanError::Consumer(error)) => {
                fallback_root_count = fallback_root_count.saturating_add(1);
                fallback_roots.push(allowed_root.clone());
                log::warn!(
                    "project_marker_fast_scan_root_failed scope={} elapsed_ms={} error={}",
                    diagnostic_path(allowed_root),
                    root_started.elapsed().as_millis(),
                    mangodisk_platform::diagnostics::text(&error)
                );
            }
        }
    }

    if strategies.is_empty() && fallback_root_count == allowed_roots.len() {
        log::info!(
            "project_marker_fast_scan_unavailable scope={} fallback_root_count={}",
            diagnostic_path(search_root),
            fallback_root_count
        );
        return IndexedProjectRootOutcome::Fallback;
    }

    log::info!(
        "project_marker_fast_scan_completed strategy_count={} scope={} candidate_count={} accepted_or_fallback_root_count={} fallback_root_count={}",
        strategies.len(),
        diagnostic_path(search_root),
        candidate_count,
        indexed_candidates.len(),
        fallback_root_count
    );
    IndexedProjectRootOutcome::Indexed {
        exact_roots: indexed_candidates,
        fallback_roots,
    }
}

fn native_project_scan_prune_names(prune_names: &HashSet<String>) -> Vec<String> {
    let mut names = prune_names.iter().cloned().collect::<Vec<_>>();
    names.extend(
        ALWAYS_SKIPPED_DIRECTORIES
            .iter()
            .chain(DEEP_ALWAYS_SKIPPED_DIRECTORIES.iter())
            .map(|name| (*name).to_string()),
    );
    names.sort();
    names.dedup();
    names
}

fn project_marker_file_names(rules: &[ProjectArtifactRuleSource]) -> Vec<String> {
    let mut names = rules
        .iter()
        .flat_map(|rule| rule.project_match.file_names_any.iter().cloned())
        .collect::<Vec<_>>();
    names.sort();
    names.dedup();
    names
}

fn project_marker_file_suffixes(rules: &[ProjectArtifactRuleSource]) -> Vec<String> {
    let mut suffixes = rules
        .iter()
        .flat_map(|rule| rule.project_match.file_suffixes_any.iter().cloned())
        .collect::<Vec<_>>();
    suffixes.sort();
    suffixes.dedup();
    suffixes
}
