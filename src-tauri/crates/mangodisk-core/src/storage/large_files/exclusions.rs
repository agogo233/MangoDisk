use std::path::{Path, PathBuf};

use mangodisk_platform::{current_platform, Platform};

use crate::shared::{CoreError, CoreResult};

pub(crate) const MAX_LARGE_FILE_EXCLUDED_PATHS: usize = 50;

/// Validates user exclusions once at the Core boundary and exposes one matcher to every scan
/// strategy. Missing folders remain valid saved preferences but are inactive for the current scan,
/// which keeps removable-volume preferences useful without weakening path validation.
#[derive(Debug)]
pub(crate) struct LargeFileExclusions {
    roots: Vec<PathBuf>,
    requested_count: usize,
    unavailable_count: usize,
    out_of_scope_count: usize,
}

impl LargeFileExclusions {
    pub(crate) fn resolve(scan_root: &Path, requested: Vec<String>) -> CoreResult<Self> {
        if requested.len() > MAX_LARGE_FILE_EXCLUDED_PATHS {
            return Err(CoreError::invalid_input(
                "too many large-file exclusion paths were requested",
            ));
        }

        let requested_count = requested.len();
        let mut unavailable_count = 0;
        let mut out_of_scope_count = 0;
        let mut roots = Vec::<PathBuf>::new();
        for value in requested {
            let value = value.trim();
            if value.is_empty() {
                return Err(CoreError::invalid_input(
                    "large-file exclusion paths must not be empty",
                ));
            }
            let requested_path = PathBuf::from(value);
            if !requested_path.is_absolute() {
                return Err(CoreError::invalid_input(
                    "large-file exclusion paths must be absolute",
                ));
            }
            if !requested_path.try_exists().map_err(|error| {
                CoreError::operation_failed(format!(
                    "failed to inspect a large-file exclusion path: {error}"
                ))
            })? {
                unavailable_count += 1;
                continue;
            }
            let path = current_platform()
                .canonicalize_no_links(&requested_path)
                .map_err(|error| CoreError::invalid_input(error.to_string()))?;
            if !path.is_dir() {
                return Err(CoreError::invalid_input(
                    "large-file exclusion paths must be directories",
                ));
            }
            if current_platform().paths_equal(&path, scan_root)
                || !current_platform().path_is_same_or_child(&path, scan_root)
            {
                // The explicit scan scope always wins. Exclusions outside it are harmless saved
                // preferences for another disk, while excluding the scope itself would otherwise
                // produce a surprising empty scan.
                out_of_scope_count += 1;
                continue;
            }
            if roots
                .iter()
                .any(|root| current_platform().path_is_same_or_child(&path, root))
            {
                continue;
            }
            roots.retain(|root| !current_platform().path_is_same_or_child(root, &path));
            roots.push(path);
        }

        Ok(Self {
            roots,
            requested_count,
            unavailable_count,
            out_of_scope_count,
        })
    }

    pub(crate) fn matches(&self, path: &Path) -> bool {
        self.roots
            .iter()
            .any(|root| current_platform().path_is_same_or_child(path, root))
    }

    pub(crate) fn active_count(&self) -> usize {
        self.roots.len()
    }

    pub(crate) fn requested_count(&self) -> usize {
        self.requested_count
    }

    pub(crate) fn unavailable_count(&self) -> usize {
        self.unavailable_count
    }

    pub(crate) fn out_of_scope_count(&self) -> usize {
        self.out_of_scope_count
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    #[test]
    fn collapses_nested_exclusions_and_ignores_other_scopes() {
        let root = std::env::temp_dir().join(format!(
            "mangodisk-large-file-exclusions-{}",
            std::process::id()
        ));
        let excluded = root.join("excluded");
        let nested = excluded.join("nested");
        let other = std::env::temp_dir().join(format!(
            "mangodisk-large-file-exclusions-other-{}",
            std::process::id()
        ));
        fs::create_dir_all(&nested).expect("the nested exclusion fixture should be created");
        fs::create_dir_all(&other).expect("the other-scope fixture should be created");
        let canonical_root = current_platform()
            .canonicalize_no_links(&root)
            .expect("the fixture root should resolve");
        let canonical_nested = current_platform()
            .canonicalize_no_links(&nested)
            .expect("the nested fixture should resolve");

        let exclusions = LargeFileExclusions::resolve(
            &canonical_root,
            vec![
                nested.to_string_lossy().into_owned(),
                excluded.to_string_lossy().into_owned(),
                other.to_string_lossy().into_owned(),
            ],
        )
        .expect("valid exclusions should resolve");

        assert_eq!(exclusions.active_count(), 1);
        assert_eq!(exclusions.out_of_scope_count(), 1);
        assert!(exclusions.matches(&canonical_nested.join("candidate.bin")));
        assert!(!exclusions.matches(&canonical_root.join("included.bin")));

        fs::remove_dir_all(&root).expect("the fixture root should be removed");
        fs::remove_dir_all(&other).expect("the other-scope fixture should be removed");
    }

    #[test]
    fn keeps_unavailable_absolute_paths_inactive() {
        let root = std::env::temp_dir();
        let unavailable = root.join(format!(
            "mangodisk-large-file-missing-{}",
            std::process::id()
        ));
        let exclusions =
            LargeFileExclusions::resolve(&root, vec![unavailable.to_string_lossy().into_owned()])
                .expect("an unavailable saved folder should not invalidate the scan");

        assert_eq!(exclusions.active_count(), 0);
        assert_eq!(exclusions.unavailable_count(), 1);
    }

    #[test]
    fn rejects_relative_paths_at_the_core_boundary() {
        let error = LargeFileExclusions::resolve(
            &std::env::temp_dir(),
            vec!["relative/excluded-folder".to_string()],
        )
        .expect_err("relative exclusions must be rejected");

        assert!(error.to_string().contains("must be absolute"));
    }

    #[test]
    fn rejects_requests_above_the_supported_limit() {
        let requested = (0..=MAX_LARGE_FILE_EXCLUDED_PATHS)
            .map(|index| {
                std::env::temp_dir()
                    .join(format!("mangodisk-missing-exclusion-{index}"))
                    .to_string_lossy()
                    .into_owned()
            })
            .collect();

        let error = LargeFileExclusions::resolve(&std::env::temp_dir(), requested)
            .expect_err("oversized exclusion requests must be rejected");

        assert!(error.to_string().contains("too many"));
    }
}
