use super::super::{project_artifact_schema::ProjectArtifactRuleSource, project_root_index};
use super::candidate_selection::{
    has_excluded_name, has_hidden_file_name, path_component_count, path_identity_key,
    path_is_same_or_child, path_name_eq, path_or_ancestor_is_retained, paths_equal,
};
#[cfg(not(test))]
use super::project_discovery::{artifact_prune_names, project_matches_known_file_markers};
use super::project_discovery::{ALWAYS_SKIPPED_DIRECTORIES, DEEP_ALWAYS_SKIPPED_DIRECTORIES};
use super::{ProjectDiscoveryRoots, ProjectDiscoveryScan, ProjectMatch, ProjectRootMode};
use crate::{
    cleanup::{codex_worktrees, exclusions::CleanupExclusions},
    filesystem::metadata::{diagnostic_path, is_link_like, modified_ms},
};
#[cfg(not(test))]
use indexed::{deep_project_root_candidates, standard_project_root_candidates};
use mangodisk_platform::{current_platform, Platform};
#[cfg(not(test))]
use mangodisk_platform::{ScanDeviceClass, UserDirectories};
use std::{
    cmp::Reverse,
    collections::{HashMap, HashSet},
    fs,
    path::{Path, PathBuf},
    time::Instant,
};
const MAX_CONFIGURED_ROOTS: usize = 32;
pub(super) const MAX_DISCOVERY_DEPTH: usize = 64;
#[cfg(not(test))]
const MAX_STANDARD_ROOTS: usize = 10_000;
#[cfg(not(test))]
const MAX_DEEP_ROOTS: usize = 10_000;
pub(super) const MAX_STANDARD_PROJECT_ROOTS: usize = 96;
const DEEP_HOME_EXCLUSIONS: &[&str] = &["Applications", "Library"];
const DEEP_SYSTEM_VOLUME_EXCLUSIONS: &[&str] = &[
    "$Recycle.Bin",
    "Applications",
    "Library",
    "Program Files",
    "Program Files (x86)",
    "ProgramData",
    "Recovery",
    "System",
    "System Volume Information",
    "Users",
    "Windows",
    "bin",
    "cores",
    "dev",
    "private",
    "sbin",
    "usr",
];

pub(super) fn configured_codex_home() -> Option<PathBuf> {
    #[cfg(not(test))]
    {
        codex_worktrees::home()
            .map_err(|error| {
                log::warn!(
                    "codex_worktree_home_unavailable error={}",
                    mangodisk_platform::diagnostics::text(&error)
                );
            })
            .ok()
    }
    #[cfg(test)]
    {
        // Tests inject an isolated home at the plan boundary while retaining
        // the real discovery and protection logic inside the plan builder.
        None
    }
}

pub(super) fn automatic_codex_worktrees(
    home: Option<&Path>,
    is_cancelled: &(dyn Fn() -> bool + Sync),
) -> Vec<PathBuf> {
    let Some(home) = home else {
        return Vec::new();
    };
    let started = Instant::now();
    match codex_worktrees::discover(home, is_cancelled) {
        Ok(roots) => {
            log::info!(
                "codex_worktrees_discovered checkout_count={} elapsed_ms={}",
                roots.len(),
                started.elapsed().as_millis()
            );
            roots
        }
        Err(error) => {
            // An unavailable optional location must not suppress unrelated
            // cleanup results or broaden discovery to the whole data home.
            log::warn!(
                "codex_worktree_discovery_skipped error={}",
                mangodisk_platform::diagnostics::text(&error)
            );
            Vec::new()
        }
    }
}

pub(super) fn retain_recent_project_matches(
    projects: &mut Vec<ProjectMatch>,
    maximum_roots: usize,
) {
    let mut root_activity = HashMap::<String, (PathBuf, u64)>::new();
    for project in projects.iter() {
        let modified_at_ms = path_modified_ms(&project.project_root);
        root_activity
            .entry(path_identity_key(&project.project_root))
            .and_modify(|(_, current)| *current = (*current).max(modified_at_ms))
            .or_insert_with(|| (project.project_root.clone(), modified_at_ms));
    }
    let discovered_root_count = root_activity.len();
    if discovered_root_count <= maximum_roots {
        return;
    }
    let mut ranked_roots = root_activity.into_values().collect::<Vec<_>>();
    ranked_roots
        .sort_by_cached_key(|(path, modified)| (Reverse(*modified), path_identity_key(path)));
    ranked_roots.truncate(maximum_roots);
    let retained = ranked_roots
        .into_iter()
        .map(|(root, _)| path_identity_key(&root))
        .collect::<HashSet<_>>();
    projects.retain(|project| retained.contains(&path_identity_key(&project.project_root)));
    log::info!(
        "project_artifact_standard_projects_limited discovered_root_count={} retained_root_count={} retained_match_count={}",
        discovered_root_count,
        retained.len(),
        projects.len()
    );
}

fn path_modified_ms(path: &Path) -> u64 {
    fs::symlink_metadata(path)
        .ok()
        .and_then(|metadata| modified_ms(&metadata))
        .unwrap_or(0)
}

pub(super) const fn root_mode_name(mode: ProjectRootMode) -> &'static str {
    match mode {
        ProjectRootMode::Explicit => "explicit",
        ProjectRootMode::Standard => "standard",
        ProjectRootMode::Deep => "deep",
        ProjectRootMode::SelectedVolumes => "selectedVolumes",
    }
}

pub(super) fn normalize_roots(configured_roots: &[String]) -> Result<Vec<PathBuf>, String> {
    if configured_roots.len() > MAX_CONFIGURED_ROOTS {
        return Err(format!(
            "project artifact scanning supports at most {MAX_CONFIGURED_ROOTS} roots"
        ));
    }
    normalize_root_paths(
        configured_roots.iter().map(PathBuf::from).collect(),
        "configured",
    )
}

#[cfg(not(test))]
pub(super) fn automatic_project_roots(
    rules: &[ProjectArtifactRuleSource],
    mode: ProjectRootMode,
    selected_volume_roots: &[PathBuf],
    scan: &ProjectDiscoveryScan<'_>,
) -> Result<ProjectDiscoveryRoots, String> {
    let exclusions = scan.exclusions;
    let user_directories = current_platform()
        .user_directories()
        .map_err(|error| error.to_string())?;
    let local_volume_roots = match mode {
        ProjectRootMode::Standard => Vec::new(),
        ProjectRootMode::Deep => current_platform()
            .volumes()
            .unwrap_or_else(|error| {
                log::warn!(
                    "project_artifact_volume_discovery_failed error={}",
                    mangodisk_platform::diagnostics::text(&error)
                );
                Vec::new()
            })
            .into_iter()
            .filter(|volume| {
                !matches!(
                    volume.scan_concurrency.class,
                    ScanDeviceClass::Network | ScanDeviceClass::Removable
                )
            })
            .map(|volume| PathBuf::from(volume.mount_point))
            .collect::<Vec<_>>(),
        ProjectRootMode::SelectedVolumes => selected_volume_roots.to_vec(),
        ProjectRootMode::Explicit => {
            return Err("explicit project roots cannot use automatic discovery".to_string());
        }
    };
    let candidates = match mode {
        ProjectRootMode::Standard => standard_project_root_candidates(
            user_directories.home_directory(),
            &standard_runtime_data_paths(&user_directories),
            rules,
            scan,
        ),
        ProjectRootMode::Deep => deep_project_root_candidates(
            user_directories.home_directory(),
            &current_platform().system_volume_path(),
            &local_volume_roots,
            rules,
            scan,
        ),
        ProjectRootMode::SelectedVolumes => {
            let mut standard = standard_project_root_candidates(
                user_directories.home_directory(),
                &standard_runtime_data_paths(&user_directories),
                rules,
                scan,
            );
            standard.extend(deep_project_root_candidates(
                user_directories.home_directory(),
                &current_platform().system_volume_path(),
                &local_volume_roots,
                rules,
                scan,
            ));
            standard
        }
        ProjectRootMode::Explicit => unreachable!("explicit roots bypass automatic discovery"),
    };
    let mut roots = ProjectDiscoveryRoots {
        exact_roots: normalize_exact_root_paths(candidates.exact_roots, "automaticExact")?,
        recursive_roots: normalize_root_paths(candidates.recursive_roots, "automaticFallback")?,
    };
    roots.exact_roots.retain(|root| !exclusions.matches(root));
    roots
        .recursive_roots
        .retain(|root| !exclusions.matches(root));
    let root_limit = match mode {
        ProjectRootMode::Standard => MAX_STANDARD_ROOTS,
        ProjectRootMode::Deep | ProjectRootMode::SelectedVolumes => MAX_DEEP_ROOTS,
        ProjectRootMode::Explicit => unreachable!("explicit roots bypass automatic discovery"),
    };
    if roots.root_count() > root_limit {
        let discovered_count = roots.root_count();
        roots.exact_roots.truncate(root_limit);
        let remaining = root_limit.saturating_sub(roots.exact_roots.len());
        roots.recursive_roots.truncate(remaining);
        log::warn!(
            "project_artifact_automatic_roots_truncated discovered_count={discovered_count} retained_count={root_limit}"
        );
    }
    log::info!(
        "project_artifact_roots_discovered mode={} exact_root_count={} recursive_root_count={} volume_count={}",
        root_mode_name(mode),
        roots.exact_roots.len(),
        roots.recursive_roots.len(),
        local_volume_roots.len()
    );
    Ok(roots)
}

#[cfg(not(test))]
pub(super) fn cached_project_matches(
    rules: &[ProjectArtifactRuleSource],
    is_cancelled: &(dyn Fn() -> bool + Sync),
    report_path: &(dyn Fn(&Path) + Sync),
    exclusions: &CleanupExclusions,
) -> Result<Vec<ProjectMatch>, String> {
    let user_directories = current_platform()
        .user_directories()
        .map_err(|error| error.to_string())?;
    let allowed_roots = standard_discovery_roots(
        user_directories.home_directory(),
        &standard_runtime_data_paths(&user_directories),
    );
    let prune_names = artifact_prune_names(rules);
    let cached = project_root_index::load().unwrap_or_else(|error| {
        log::warn!(
            "project_root_index_load_failed error={}",
            mangodisk_platform::diagnostics::text(&error)
        );
        Vec::new()
    });
    let indexed_count = cached.len();
    let maximum_cached_roots = MAX_STANDARD_PROJECT_ROOTS.saturating_mul(2);
    let mut eligible_roots = cached
        .into_iter()
        .filter(|root| {
            !exclusions.matches(root)
                && automatic_project_root_allowed(root, &allowed_roots, &prune_names)
                && is_real_directory(root)
        })
        .map(|root| {
            let modified_at_ms = path_modified_ms(&root);
            (root, modified_at_ms)
        })
        .collect::<Vec<_>>();
    eligible_roots.sort_by(|(left_path, left_modified), (right_path, right_modified)| {
        right_modified
            .cmp(left_modified)
            .then_with(|| left_path.cmp(right_path))
    });
    eligible_roots.truncate(maximum_cached_roots);
    let mut projects = Vec::new();
    let mut visited = HashSet::new();
    for (root, _) in eligible_roots {
        if is_cancelled() {
            break;
        }
        report_path(&root);
        // The index stores paths that were canonicalized when first discovered.
        // Re-canonicalizing every historical project made routine scans scale
        // with the complete index. Every artifact path is still validated
        // without links before it can enter a cleanup plan.
        if !visited.insert(path_identity_key(&root)) {
            continue;
        }
        for (rule_index, rule) in rules.iter().enumerate() {
            if project_matches_known_file_markers(&root, &rule.project_match, exclusions) {
                projects.push(ProjectMatch {
                    rule_index,
                    project_root: root.clone(),
                    allow_descendant_scan: false,
                });
            }
        }
    }
    log::info!(
        "project_artifact_cached_projects_checked indexed_count={} checked_count={} match_count={}",
        indexed_count,
        visited.len(),
        projects.len()
    );
    Ok(projects)
}

#[cfg(test)]
pub(super) fn cached_project_matches(
    _rules: &[ProjectArtifactRuleSource],
    _is_cancelled: &(dyn Fn() -> bool + Sync),
    _report_path: &(dyn Fn(&Path) + Sync),
    _exclusions: &CleanupExclusions,
) -> Result<Vec<ProjectMatch>, String> {
    Ok(Vec::new())
}

#[cfg(test)]
pub(super) fn automatic_project_roots(
    _rules: &[ProjectArtifactRuleSource],
    _mode: ProjectRootMode,
    selected_volume_roots: &[PathBuf],
    scan: &ProjectDiscoveryScan<'_>,
) -> Result<ProjectDiscoveryRoots, String> {
    let _ = (scan.is_cancelled, scan.report_path, scan.report_files);
    // Unit tests must not scan the contributor's real workspaces. Candidate
    // discovery is covered with isolated fixtures below.
    Ok(ProjectDiscoveryRoots {
        exact_roots: Vec::new(),
        recursive_roots: selected_volume_roots
            .iter()
            .filter(|root| !scan.exclusions.matches(root))
            .cloned()
            .collect(),
    })
}

#[cfg(not(test))]
fn deep_roots_for_volume(
    allowed_roots: &[PathBuf],
    volume_root: &Path,
    system_volume: &Path,
    local_volume_roots: &[PathBuf],
) -> Vec<PathBuf> {
    if paths_equal(volume_root, system_volume) {
        let other_volumes = local_volume_roots
            .iter()
            .filter(|other| !paths_equal(other, system_volume))
            .collect::<Vec<_>>();
        allowed_roots
            .iter()
            .filter(|root| {
                !other_volumes
                    .iter()
                    .any(|other| path_is_same_or_child(root, other))
            })
            .cloned()
            .collect()
    } else {
        allowed_roots
            .iter()
            .filter(|root| path_is_same_or_child(root, volume_root))
            .cloned()
            .collect()
    }
}

pub(super) fn validated_marker_project_root(
    marker_path: &Path,
    allowed_roots: &[PathBuf],
    prune_names: &HashSet<String>,
    maximum_depth: usize,
) -> Option<PathBuf> {
    let metadata = fs::symlink_metadata(marker_path).ok()?;
    if !metadata.is_file() || is_link_like(&metadata) {
        return None;
    }
    let project_root = marker_path.parent()?;
    if !is_real_directory(project_root) {
        return None;
    }
    let allowed_root = allowed_roots
        .iter()
        .find(|allowed| path_is_same_or_child(project_root, allowed))?;
    let relative_depth = current_platform()
        .relative_path(project_root, allowed_root)?
        .components()
        .count();
    (relative_depth <= maximum_depth
        && automatic_project_root_allowed(project_root, allowed_roots, prune_names))
    .then(|| project_root.to_path_buf())
}

pub(super) fn automatic_project_root_allowed(
    project_root: &Path,
    allowed_roots: &[PathBuf],
    prune_names: &HashSet<String>,
) -> bool {
    let Some(allowed_root) = allowed_roots
        .iter()
        .find(|allowed| path_is_same_or_child(project_root, allowed))
    else {
        return false;
    };
    let Some(relative) = current_platform().relative_path(project_root, allowed_root) else {
        return false;
    };
    !relative.components().any(|component| {
        let name = component.as_os_str().to_string_lossy();
        name.starts_with('.')
            || ALWAYS_SKIPPED_DIRECTORIES
                .iter()
                .any(|candidate| path_name_eq(&name, candidate))
            || DEEP_ALWAYS_SKIPPED_DIRECTORIES
                .iter()
                .any(|candidate| path_name_eq(&name, candidate))
            || prune_names
                .iter()
                .any(|candidate| path_name_eq(&name, candidate))
    })
}

pub(super) fn deep_discovery_roots(
    home: &Path,
    system_volume: &Path,
    local_volume_roots: &[PathBuf],
) -> Vec<PathBuf> {
    let mut roots = immediate_real_directories(home)
        .into_iter()
        .filter(|path| {
            !has_excluded_name(path, DEEP_HOME_EXCLUSIONS) && !has_hidden_file_name(path)
        })
        .collect::<Vec<_>>();
    for volume_root in local_volume_roots {
        if paths_equal(volume_root, system_volume) {
            roots.extend(
                immediate_real_directories(volume_root)
                    .into_iter()
                    .filter(|path| {
                        !has_excluded_name(path, DEEP_SYSTEM_VOLUME_EXCLUSIONS)
                            && !has_hidden_file_name(path)
                    }),
            );
        } else {
            roots.push(volume_root.clone());
        }
    }
    roots
}

pub(super) fn standard_discovery_roots(
    home: &Path,
    runtime_data_paths: &[PathBuf],
) -> Vec<PathBuf> {
    immediate_real_directories(home)
        .into_iter()
        .filter(|path| !has_hidden_file_name(path))
        // Platform APIs provide cache and application-data paths. Excluding
        // their first ancestor under the profile avoids traversing runtime
        // data without assuming that users name development folders in a
        // particular language or convention.
        .filter(|path| {
            !runtime_data_paths
                .iter()
                .any(|runtime_path| path_is_same_or_child(runtime_path, path))
        })
        .collect()
}

#[cfg(any(not(test), target_os = "macos"))]
pub(super) fn macos_root_requires_explicit_consent(path: &Path) -> bool {
    #[cfg(target_os = "macos")]
    {
        let Ok(user_directories) = current_platform().user_directories() else {
            return false;
        };
        path.parent().is_some_and(|parent| {
            paths_equal(parent, user_directories.home_directory())
                && has_excluded_name(
                    path,
                    &[
                        "Desktop",
                        "Documents",
                        "Downloads",
                        "Movies",
                        "Music",
                        "Pictures",
                    ],
                )
        })
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = path;
        false
    }
}

#[cfg(not(test))]
fn standard_runtime_data_paths(user_directories: &UserDirectories) -> Vec<PathBuf> {
    user_directories
        .application_storage_directories()
        .iter()
        .cloned()
        .chain(std::iter::once(
            user_directories.temporary_directory().to_path_buf(),
        ))
        .collect()
}

pub(super) fn update_project_root_index(projects: &[ProjectMatch]) {
    let mut roots = project_root_index::load().unwrap_or_else(|error| {
        log::warn!(
            "project_root_index_merge_load_failed error={}",
            mangodisk_platform::diagnostics::text(&error)
        );
        Vec::new()
    });
    roots.extend(projects.iter().map(|project| project.project_root.clone()));
    if let Err(error) = project_root_index::save(&roots) {
        log::warn!(
            "project_root_index_save_failed error={}",
            mangodisk_platform::diagnostics::text(&error)
        );
    } else {
        log::info!("project_root_index_updated root_count={}", roots.len());
    }
}

fn immediate_real_directories(parent: &Path) -> Vec<PathBuf> {
    let mut directories = fs::read_dir(parent)
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| is_real_directory(path))
        .collect::<Vec<_>>();
    directories.sort();
    directories
}

fn is_real_directory(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok_and(|metadata| metadata.is_dir() && !is_link_like(&metadata))
}

pub(super) fn normalize_root_paths(
    paths: Vec<PathBuf>,
    source: &str,
) -> Result<Vec<PathBuf>, String> {
    normalize_root_paths_with_policy(paths, source, true)
}

pub(super) fn normalize_exact_root_paths(
    paths: Vec<PathBuf>,
    source: &str,
) -> Result<Vec<PathBuf>, String> {
    normalize_root_paths_with_policy(paths, source, false)
}

fn normalize_root_paths_with_policy(
    paths: Vec<PathBuf>,
    source: &str,
    remove_descendants: bool,
) -> Result<Vec<PathBuf>, String> {
    let mut roots = Vec::<PathBuf>::new();
    let mut root_identities = HashSet::<String>::new();
    for path in paths {
        let metadata = match fs::symlink_metadata(&path) {
            Ok(metadata) if metadata.is_dir() && !is_link_like(&metadata) => metadata,
            Ok(_) => {
                log::warn!(
                    "project_artifact_root_skipped source={} path={} reason=notDirectoryOrLink",
                    source,
                    diagnostic_path(&path)
                );
                continue;
            }
            Err(error) => {
                if source == "configured" {
                    log::warn!(
                        "project_artifact_root_skipped source={} path={} reason=metadata error_kind={:?}",
                        source,
                        diagnostic_path(&path),
                        error.kind()
                    );
                }
                continue;
            }
        };
        let _ = metadata;
        let canonical = match current_platform().canonicalize_no_links(&path) {
            Ok(path) => path,
            Err(error) => {
                log::warn!(
                    "project_artifact_root_skipped source={} path={} reason=canonicalize error={}",
                    source,
                    diagnostic_path(&path),
                    mangodisk_platform::diagnostics::text(&error)
                );
                continue;
            }
        };
        if root_identities.insert(path_identity_key(&canonical)) {
            roots.push(canonical);
        }
    }
    roots.sort_by_cached_key(|root| (path_component_count(root), path_identity_key(root)));
    if !remove_descendants {
        return Ok(roots);
    }
    let mut deduplicated = Vec::<PathBuf>::new();
    let mut retained_identities = HashSet::<String>::new();
    for root in roots {
        if path_or_ancestor_is_retained(&root, &retained_identities) {
            continue;
        }
        retained_identities.insert(path_identity_key(&root));
        deduplicated.push(root);
    }
    Ok(deduplicated)
}

#[cfg(not(test))]
mod indexed;
