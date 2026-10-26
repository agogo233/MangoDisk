use super::ArtifactDraft;
use mangodisk_platform::{current_platform, Platform};
use std::{
    collections::HashSet,
    path::{Path, PathBuf},
};

pub(super) fn deduplicate_artifacts(mut drafts: Vec<ArtifactDraft>) -> Vec<ArtifactDraft> {
    drafts.sort_by_cached_key(|draft| {
        (
            path_component_count(&draft.path),
            path_identity_key(&draft.path),
            draft.rule_index,
        )
    });
    let mut retained = Vec::<ArtifactDraft>::new();
    let mut retained_identities = HashSet::<String>::new();
    for draft in drafts {
        if path_or_ancestor_is_retained(&draft.path, &retained_identities) {
            continue;
        }
        retained_identities.insert(path_identity_key(&draft.path));
        retained.push(draft);
    }
    retained
}

pub(super) fn has_hidden_file_name(path: &Path) -> bool {
    path.file_name()
        .is_some_and(|name| name.to_string_lossy().starts_with('.'))
}

pub(super) fn has_excluded_name(path: &Path, excluded_names: &[&str]) -> bool {
    let name = path
        .file_name()
        .map(|value| value.to_string_lossy())
        .unwrap_or_default();
    excluded_names
        .iter()
        .any(|excluded| path_name_eq(&name, excluded))
}

pub(super) fn paths_equal(left: &Path, right: &Path) -> bool {
    current_platform().paths_equal(left, right)
}

pub(super) fn path_identity_key(path: &Path) -> String {
    current_platform().path_identity_key(path)
}

pub(super) fn path_is_same_or_child(path: &Path, root: &Path) -> bool {
    current_platform().path_is_same_or_child(path, root)
}

pub(super) fn path_component_count(path: &Path) -> usize {
    PathBuf::from(current_platform().display_path(path))
        .components()
        .count()
}

pub(super) fn path_or_ancestor_is_retained(
    path: &Path,
    retained_identities: &HashSet<String>,
) -> bool {
    // Candidate paths are canonicalized before deduplication. Walking their
    // lexical ancestors keeps containment checks bounded by path depth while
    // still delegating case folding and verbatim-prefix handling to the
    // platform-owned identity contract.
    path.ancestors()
        .any(|ancestor| retained_identities.contains(&path_identity_key(ancestor)))
}

#[cfg(windows)]
pub(super) fn path_name_eq(left: &str, right: &str) -> bool {
    left.eq_ignore_ascii_case(right)
}

#[cfg(not(windows))]
pub(super) fn path_name_eq(left: &str, right: &str) -> bool {
    left == right
}

#[cfg(windows)]
pub(super) fn path_name_ends_with(value: &str, suffix: &str) -> bool {
    value
        .to_ascii_lowercase()
        .ends_with(&suffix.to_ascii_lowercase())
}

#[cfg(not(windows))]
pub(super) fn path_name_ends_with(value: &str, suffix: &str) -> bool {
    value.ends_with(suffix)
}
