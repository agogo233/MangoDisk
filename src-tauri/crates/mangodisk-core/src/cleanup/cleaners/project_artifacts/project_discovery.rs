use super::super::project_artifact_schema::{
    ProjectArtifactRuleSource, ProjectArtifactSource, ProjectMatchSource,
};
use super::candidate_selection::{
    path_identity_key, path_name_ends_with, path_name_eq, paths_equal,
};
use super::{ArtifactDraft, DirectoryEntries, ProjectMatch, ProjectTraversalPolicy};
use crate::{
    cleanup::exclusions::CleanupExclusions,
    filesystem::metadata::{diagnostic_path, is_link_like},
};
use mangodisk_platform::{current_platform, Platform};
use std::{
    cmp::Reverse,
    collections::HashSet,
    fs,
    path::{Path, PathBuf},
};
const MAX_DISCOVERED_DIRECTORIES: usize = 500_000;
const MAX_DISCOVERED_PROJECTS: usize = 50_000;
pub(super) const ALWAYS_SKIPPED_DIRECTORIES: &[&str] = &[".git", ".hg", ".svn"];
pub(super) const DEEP_ALWAYS_SKIPPED_DIRECTORIES: &[&str] = &[
    "$Recycle.Bin",
    "Program Files",
    "Program Files (x86)",
    "ProgramData",
    "Recovery",
    "System Volume Information",
    "Windows",
];

pub(super) fn discover_projects(
    roots: &[PathBuf],
    rules: &[ProjectArtifactRuleSource],
    policy: ProjectTraversalPolicy<'_>,
    is_cancelled: &(dyn Fn() -> bool + Sync),
    report_path: &(dyn Fn(&Path) + Sync),
) -> Result<(Vec<ProjectMatch>, bool), String> {
    let ProjectTraversalPolicy {
        skip_hidden_directories,
        deep,
        maximum_depth,
        exclusions,
    } = policy;
    let prune_names = artifact_prune_names(rules);
    let mut stack = roots
        .iter()
        .cloned()
        .map(|path| (path, 0_usize))
        .collect::<Vec<_>>();
    let mut projects = Vec::new();
    let mut visited = 0_usize;
    let mut limited = false;
    while let Some((directory, depth)) = stack.pop() {
        if is_cancelled() {
            return Ok((projects, true));
        }
        if exclusions.matches(&directory) {
            log::debug!(
                "project_artifact_directory_skipped path={} reason=excludedFolder",
                diagnostic_path(&directory)
            );
            continue;
        }
        report_path(&directory);
        visited += 1;
        if visited > MAX_DISCOVERED_DIRECTORIES || projects.len() >= MAX_DISCOVERED_PROJECTS {
            limited = true;
            break;
        }
        let entries = match read_directory_entries(&directory) {
            Ok(entries) => entries,
            Err(error) => {
                log::debug!(
                    "project_artifact_directory_skipped path={} error_kind={:?}",
                    diagnostic_path(&directory),
                    error.kind()
                );
                continue;
            }
        };
        for (rule_index, rule) in rules.iter().enumerate() {
            if project_matches_entries(
                &directory,
                &entries.file_names,
                &rule.project_match,
                exclusions,
            ) {
                projects.push(ProjectMatch {
                    rule_index,
                    project_root: directory.clone(),
                    allow_descendant_scan: true,
                });
            }
        }
        if depth >= maximum_depth {
            continue;
        }
        for child in entries.directories.into_iter().rev() {
            let name = child
                .file_name()
                .map(|value| value.to_string_lossy())
                .unwrap_or_default();
            let excluded = exclusions.matches(&child);
            if ALWAYS_SKIPPED_DIRECTORIES
                .iter()
                .any(|candidate| path_name_eq(&name, candidate))
                || (skip_hidden_directories && name.starts_with('.'))
                || (deep
                    && DEEP_ALWAYS_SKIPPED_DIRECTORIES
                        .iter()
                        .any(|candidate| path_name_eq(&name, candidate)))
                || prune_names
                    .iter()
                    .any(|candidate| path_name_eq(&name, candidate))
                || excluded
            {
                if excluded {
                    log::debug!(
                        "project_artifact_directory_skipped path={} reason=excludedFolder",
                        diagnostic_path(&child)
                    );
                }
                continue;
            }
            stack.push((child, depth + 1));
        }
    }
    sort_and_deduplicate_project_matches(&mut projects);
    Ok((projects, limited))
}

pub(super) fn sort_and_deduplicate_project_matches(projects: &mut Vec<ProjectMatch>) {
    projects.sort_by_cached_key(|project| {
        (
            path_identity_key(&project.project_root),
            project.rule_index,
            Reverse(project.allow_descendant_scan),
        )
    });
    projects.dedup_by(|left, right| {
        left.rule_index == right.rule_index && paths_equal(&left.project_root, &right.project_root)
    });
}

fn read_directory_entries(path: &Path) -> std::io::Result<DirectoryEntries> {
    let mut file_names = Vec::new();
    let mut directories = Vec::new();
    for entry in fs::read_dir(path)? {
        let entry = entry?;
        let metadata = fs::symlink_metadata(entry.path())?;
        if is_link_like(&metadata) {
            continue;
        }
        if metadata.is_dir() {
            directories.push(entry.path());
        } else if metadata.is_file() {
            file_names.push(entry.file_name().to_string_lossy().into_owned());
        }
    }
    directories.sort();
    Ok(DirectoryEntries {
        file_names,
        directories,
    })
}

pub(super) fn project_matches(
    project_root: &Path,
    project_match: &ProjectMatchSource,
    exclusions: &CleanupExclusions,
) -> bool {
    read_directory_entries(project_root).is_ok_and(|entries| {
        project_matches_entries(project_root, &entries.file_names, project_match, exclusions)
    })
}

#[cfg(not(test))]
pub(super) fn project_matches_known_file_markers(
    project_root: &Path,
    project_match: &ProjectMatchSource,
    exclusions: &CleanupExclusions,
) -> bool {
    !project_match.file_names_any.is_empty()
        && project_match.file_names_any.iter().any(|file_name| {
            !exclusions.matches(&project_root.join(file_name))
                && safe_marker_file_exists(project_root, file_name)
        })
        && project_match.relative_paths_all.iter().all(|relative| {
            !exclusions.matches(&join_rule_path(project_root, relative))
                && safe_required_path_exists(project_root, relative)
        })
        && (project_match.relative_paths_any.is_empty()
            || project_match.relative_paths_any.iter().any(|relative| {
                !exclusions.matches(&join_rule_path(project_root, relative))
                    && safe_required_path_exists(project_root, relative)
            }))
}

#[cfg(not(test))]
fn safe_marker_file_exists(project_root: &Path, file_name: &str) -> bool {
    let path = project_root.join(file_name);
    fs::symlink_metadata(&path).is_ok_and(|metadata| metadata.is_file() && !is_link_like(&metadata))
        && current_platform().validate_path_no_links(&path).is_ok()
}

fn project_matches_entries(
    project_root: &Path,
    file_names: &[String],
    project_match: &ProjectMatchSource,
    exclusions: &CleanupExclusions,
) -> bool {
    let marker_matches = file_names.iter().any(|file_name| {
        project_match
            .file_names_any
            .iter()
            .any(|candidate| path_name_eq(file_name, candidate))
            || project_match
                .file_suffixes_any
                .iter()
                .any(|suffix| path_name_ends_with(file_name, suffix))
    });
    marker_matches
        && project_match.relative_paths_all.iter().all(|relative| {
            !exclusions.matches(&join_rule_path(project_root, relative))
                && safe_required_path_exists(project_root, relative)
        })
        && (project_match.relative_paths_any.is_empty()
            || project_match.relative_paths_any.iter().any(|relative| {
                !exclusions.matches(&join_rule_path(project_root, relative))
                    && safe_required_path_exists(project_root, relative)
            }))
}

fn safe_required_path_exists(project_root: &Path, relative: &str) -> bool {
    let path = join_rule_path(project_root, relative);
    fs::symlink_metadata(&path).is_ok_and(|metadata| !is_link_like(&metadata))
        && current_platform().validate_path_no_links(&path).is_ok()
}

pub(super) fn collect_artifact_drafts(
    projects: &[ProjectMatch],
    rules: &[ProjectArtifactRuleSource],
    is_cancelled: &(dyn Fn() -> bool + Sync),
    report_path: &(dyn Fn(&Path) + Sync),
    exclusions: &CleanupExclusions,
) -> Vec<ArtifactDraft> {
    let mut drafts = Vec::new();
    let prune_names = artifact_prune_names(rules);
    for project in projects {
        if is_cancelled() {
            break;
        }
        if exclusions.matches(&project.project_root) {
            continue;
        }
        report_path(&project.project_root);
        for artifact in &rules[project.rule_index].artifacts {
            match artifact {
                ProjectArtifactSource::RelativeDirectory { path } => {
                    let candidate = join_rule_path(&project.project_root, path);
                    if exclusions.protects_tree(&candidate, is_cancelled) {
                        log::debug!(
                            "project_artifact_candidate_skipped rule_id={} path={} reason=excludedFolderOverlap",
                            rules[project.rule_index].id,
                            diagnostic_path(&candidate)
                        );
                        continue;
                    }
                    if let Ok(candidate) = validate_candidate(&project.project_root, &candidate) {
                        drafts.push(ArtifactDraft {
                            rule_index: project.rule_index,
                            project_root: project.project_root.clone(),
                            path: candidate,
                        });
                    }
                }
                ProjectArtifactSource::DescendantDirectory { name, max_depth } => {
                    if !project.allow_descendant_scan {
                        continue;
                    }
                    drafts.extend(discover_descendant_artifacts(
                        project,
                        name,
                        *max_depth,
                        &prune_names,
                        is_cancelled,
                        report_path,
                        exclusions,
                    ));
                }
            }
        }
    }
    drafts
}

fn discover_descendant_artifacts(
    project: &ProjectMatch,
    target_name: &str,
    max_depth: usize,
    prune_names: &HashSet<String>,
    is_cancelled: &(dyn Fn() -> bool + Sync),
    report_path: &(dyn Fn(&Path) + Sync),
    exclusions: &CleanupExclusions,
) -> Vec<ArtifactDraft> {
    let mut stack = vec![(project.project_root.clone(), 0_usize)];
    let mut drafts = Vec::new();
    while let Some((directory, depth)) = stack.pop() {
        if is_cancelled() || depth >= max_depth {
            continue;
        }
        if exclusions.matches(&directory) {
            continue;
        }
        report_path(&directory);
        let Ok(entries) = read_directory_entries(&directory) else {
            continue;
        };
        for child in entries.directories {
            if exclusions.matches(&child) {
                continue;
            }
            let name = child
                .file_name()
                .map(|value| value.to_string_lossy())
                .unwrap_or_default();
            if path_name_eq(&name, target_name) {
                if exclusions.protects_tree(&child, is_cancelled) {
                    log::debug!(
                        "project_artifact_candidate_skipped project_root={} path={} reason=excludedFolderOverlap",
                        diagnostic_path(&project.project_root),
                        diagnostic_path(&child)
                    );
                    continue;
                }
                if let Ok(child) = validate_candidate(&project.project_root, &child) {
                    drafts.push(ArtifactDraft {
                        rule_index: project.rule_index,
                        project_root: project.project_root.clone(),
                        path: child,
                    });
                }
                continue;
            }
            if ALWAYS_SKIPPED_DIRECTORIES
                .iter()
                .any(|candidate| path_name_eq(&name, candidate))
                || prune_names
                    .iter()
                    .any(|candidate| path_name_eq(&name, candidate))
            {
                continue;
            }
            stack.push((child, depth + 1));
        }
    }
    drafts
}

pub(super) fn validate_candidate(project_root: &Path, candidate: &Path) -> Result<PathBuf, String> {
    if paths_equal(candidate, project_root) {
        return Err("an artifact directory cannot equal its project root".to_string());
    }
    let metadata = fs::symlink_metadata(candidate).map_err(|error| error.to_string())?;
    if !metadata.is_dir() || is_link_like(&metadata) {
        return Err("an artifact candidate must be a real directory".to_string());
    }
    current_platform()
        .validate_path_no_links(candidate)
        .map_err(|error| error.to_string())?;
    let canonical = current_platform()
        .canonicalize_no_links(candidate)
        .map_err(|error| error.to_string())?;
    if current_platform().paths_equal(&canonical, project_root)
        || !current_platform().path_is_same_or_child(&canonical, project_root)
    {
        return Err("an artifact candidate escaped its project root".to_string());
    }
    Ok(canonical)
}

pub(super) fn artifact_prune_names(rules: &[ProjectArtifactRuleSource]) -> HashSet<String> {
    rules
        .iter()
        .flat_map(|rule| &rule.artifacts)
        .filter_map(|artifact| match artifact {
            ProjectArtifactSource::RelativeDirectory { path } => Path::new(path)
                .components()
                .next()
                .map(|component| component.as_os_str().to_string_lossy().into_owned()),
            ProjectArtifactSource::DescendantDirectory { name, .. } => Some(name.clone()),
        })
        .collect()
}

fn join_rule_path(root: &Path, relative: &str) -> PathBuf {
    relative
        .split(['/', '\\'])
        .fold(root.to_path_buf(), |path, component| path.join(component))
}
