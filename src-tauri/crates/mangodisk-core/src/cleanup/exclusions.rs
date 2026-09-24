use std::path::{Path, PathBuf};

use mangodisk_platform::{current_platform, Platform};

use crate::{
    filesystem::metadata::diagnostic_path,
    shared::{CoreError, CoreResult},
};

const MAX_EXCLUDED_PATHS: usize = 50;

/// Resolved roots used only by project-artifact discovery and execution.
/// Declarative rules and other specialized cleaners ignore them.
#[derive(Debug, Clone, Default)]
pub(super) struct CleanupExclusions {
    roots: Vec<PathBuf>,
    aliases: Vec<PathBuf>,
}

impl CleanupExclusions {
    pub(super) fn resolve(requested: &[String]) -> CoreResult<Self> {
        if requested.len() > MAX_EXCLUDED_PATHS {
            log::warn!(
                "cleanup_exclusion_rejected requested_count={} limit={} reason=tooManyPaths outcome=blocked",
                requested.len(),
                MAX_EXCLUDED_PATHS
            );
            return Err(CoreError::invalid_input("too many cleanup exclusion paths"));
        }
        let mut roots = Vec::<PathBuf>::new();
        let mut unavailable = 0;
        for value in requested {
            let path = PathBuf::from(value.trim());
            if value.trim().is_empty() || !path.is_absolute() {
                log::warn!(
                    "cleanup_exclusion_rejected path={} reason=emptyOrNotAbsolute outcome=blocked",
                    diagnostic_path(&path)
                );
                return Err(CoreError::invalid_input(
                    "cleanup exclusion paths must be absolute directories",
                ));
            }
            let exists = path.try_exists().map_err(|error| {
                log::warn!(
                    "cleanup_exclusion_rejected path={} reason=inspectFailed error={} outcome=blocked",
                    diagnostic_path(&path),
                    mangodisk_platform::diagnostics::text(&error)
                );
                CoreError::operation_failed(format!(
                    "failed to inspect cleanup exclusion {}: {error}",
                    diagnostic_path(&path)
                ))
            })?;
            let protected_root = if exists {
                let canonical = current_platform()
                    .canonicalize_no_links(&path)
                    .map_err(|error| {
                        log::warn!(
                            "cleanup_exclusion_rejected path={} reason=canonicalizeFailed error={} outcome=blocked",
                            diagnostic_path(&path),
                            mangodisk_platform::diagnostics::text(&error)
                        );
                        CoreError::invalid_input(error.to_string())
                    })?;
                if !canonical.is_dir() {
                    log::warn!(
                        "cleanup_exclusion_rejected path={} reason=notDirectory outcome=blocked",
                        diagnostic_path(&path)
                    );
                    return Err(CoreError::invalid_input(
                        "cleanup exclusion paths must be directories",
                    ));
                }
                canonical
            } else {
                unavailable += 1;
                log::info!(
                    "cleanup_exclusion_unavailable path={} outcome=retained_if_restored",
                    diagnostic_path(&path)
                );
                // A missing folder can reappear during a long scan or cleanup.
                // Retain its absolute path so the exclusion remains a safety boundary.
                path
            };
            if roots
                .iter()
                .any(|root| current_platform().path_is_same_or_child(&protected_root, root))
            {
                continue;
            }
            roots.retain(|root| !current_platform().path_is_same_or_child(root, &protected_root));
            roots.push(protected_root);
        }
        for root in &roots {
            log::info!(
                "cleanup_exclusion_root_resolved path={}",
                diagnostic_path(root)
            );
        }
        log::info!(
            "cleanup_exclusions_resolved requested_count={} protected_root_count={} unavailable_count={}",
            requested.len(),
            roots.len(),
            unavailable
        );
        let aliases = roots.iter().flat_map(|root| system_aliases(root)).collect();
        Ok(Self { roots, aliases })
    }

    pub(super) fn matches(&self, path: &Path) -> bool {
        self.roots
            .iter()
            .chain(&self.aliases)
            .any(|root| current_platform().path_is_same_or_child(path, root))
    }

    pub(super) fn intersects(&self, path: &Path) -> bool {
        self.roots.iter().chain(&self.aliases).any(|root| {
            current_platform().path_is_same_or_child(path, root)
                || current_platform().path_is_same_or_child(root, path)
        })
    }
}

#[cfg(target_os = "macos")]
fn system_aliases(path: &Path) -> Vec<PathBuf> {
    // macOS allows these fixed system symlinks in cleanup roots. Finder's
    // folder picker resolves them to /private, while declarative rules can
    // retain /var, /tmp, or /etc from environment and catalog templates.
    [
        ("/private/var", "/var"),
        ("/private/tmp", "/tmp"),
        ("/private/etc", "/etc"),
    ]
    .into_iter()
    .filter_map(|(canonical, alias)| {
        path.strip_prefix(canonical)
            .ok()
            .map(|suffix| PathBuf::from(alias).join(suffix))
    })
    .collect()
}

#[cfg(not(target_os = "macos"))]
fn system_aliases(_: &Path) -> Vec<PathBuf> {
    Vec::new()
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_temp_alias_matches_the_same_canonical_exclusion() {
        let canonical = PathBuf::from("/private/var");
        let alias = PathBuf::from("/var");
        let exclusions = CleanupExclusions::resolve(&[canonical.to_string_lossy().into_owned()])
            .expect("resolve the canonical exclusion");

        assert!(exclusions.matches(&alias.join("candidate.tmp")));
        assert!(exclusions.intersects(&alias));
    }

    #[test]
    fn temporarily_unavailable_exclusion_still_prunes_when_it_reappears() {
        let fixture = tempfile::tempdir().expect("create exclusion fixture");
        let excluded = fixture.path().join("reappearing");
        let exclusions = CleanupExclusions::resolve(&[excluded.to_string_lossy().into_owned()])
            .expect("resolve a temporarily unavailable exclusion");

        fs::create_dir(&excluded).expect("restore the excluded directory");
        assert!(exclusions.matches(&excluded.join("candidate.tmp")));
    }
}
