use std::path::{Component, Path};

/// Automatic cleanup must never own these top-level home directories. The
/// list is intentionally narrower than a user-configurable keep list: it
/// contains only identity, source, backup, synchronization, and virtual
/// machine locations whose contents cannot be inferred from a cache-like
/// descendant name.
const PROTECTED_HOME_ROOTS: &[&str] = &[
    ".ansible",
    ".aws",
    ".azure",
    ".config",
    ".docker",
    ".env",
    ".gcloud",
    ".git-credentials",
    ".gnupg",
    ".idea",
    ".kube",
    ".netrc",
    ".password-store",
    ".ssh",
    ".vscode",
    "backup",
    "backups",
    "box",
    "code",
    "creative cloud files",
    "dev",
    "dropbox",
    "google drive",
    "icloud drive",
    "my virtual machines",
    "nextcloud",
    "onedrive",
    "owncloud",
    "projects",
    "repos",
    "repositories",
    "saved games",
    "source",
    "src",
    "sync",
    "virtualbox vms",
    "vms",
    "workspace",
    "workspaces",
];

/// Repository metadata is protected at any depth. Build-output cleanup is
/// allowed inside a project, but repository state is never a build artifact.
const PROTECTED_REPOSITORY_COMPONENTS: &[&str] = &[".bzr", ".git", ".hg", ".svn"];

/// These Library subtrees contain user state even though they live below the
/// same Library directory as caches. The runtime check prevents a future
/// matcher from turning a narrowly authored rule into an unsafe root.
const PROTECTED_LIBRARY_ROOTS: &[&str] = &[
    "accounts",
    "addressbook",
    "calendars",
    "cloudstorage",
    "daemon containers",
    "keychains",
    "mail",
    "messages",
    "mobile documents",
    "photos",
    "safari",
];

/// File Provider and CloudKit keep synchronization databases, working-set
/// metadata, and provider coordination state in these otherwise cache-looking
/// locations. Automatic cleanup must preserve the complete subtree because a
/// partial removal can force a resync or detach locally materialized content.
const PROTECTED_LIBRARY_SUBTREES: &[&[&str]] = &[
    &["application support", "clouddocs"],
    &["application support", "fileprovider"],
    &["caches", "cloudkit"],
    &["caches", "com.apple.bird"],
    &["caches", "com.apple.cloudd"],
    &["caches", "com.apple.clouddocs"],
    &["caches", "com.apple.fileprovider"],
];

pub(crate) fn is_protected_home_root(value: &str) -> bool {
    let normalized = value.to_ascii_lowercase();
    if normalized == "onedrive" || normalized.starts_with("onedrive - ") {
        return true;
    }
    PROTECTED_HOME_ROOTS
        .iter()
        .any(|protected| value.eq_ignore_ascii_case(protected))
}

pub(crate) fn is_protected_repository_component(value: &str) -> bool {
    PROTECTED_REPOSITORY_COMPONENTS
        .iter()
        .any(|protected| value.eq_ignore_ascii_case(protected))
}

pub(crate) fn is_protected_library_root(value: &str) -> bool {
    PROTECTED_LIBRARY_ROOTS
        .iter()
        .any(|protected| value.eq_ignore_ascii_case(protected))
}

fn is_protected_library_subtree(components: &[String]) -> bool {
    PROTECTED_LIBRARY_SUBTREES.iter().any(|protected| {
        components.len() >= protected.len()
            && components
                .iter()
                .zip(*protected)
                .all(|(actual, expected)| actual.eq_ignore_ascii_case(expected))
    })
}

pub(crate) fn is_protected_home_relative_path(components: &[String]) -> bool {
    components
        .first()
        .is_some_and(|first| is_protected_home_root(first))
        || components
            .iter()
            .any(|component| is_protected_repository_component(component))
        || components.len() >= 2
            && ((components[0].eq_ignore_ascii_case("Library")
                && (is_protected_library_root(&components[1])
                    || is_protected_library_subtree(&components[1..])))
                || (components[0].eq_ignore_ascii_case(".local")
                    && components[1].eq_ignore_ascii_case("share")
                    && !is_verified_local_share_cache_root(components)))
}

/// Allows only platform-standard cache roots inside `~/.local/share`, which is
/// otherwise protected because applications commonly persist user state
/// there. Exact component matching prevents a cache-looking descendant from
/// weakening the boundary for an unrelated application directory.
fn is_verified_local_share_cache_root(components: &[String]) -> bool {
    components.len() == 4
        && components[0].eq_ignore_ascii_case(".local")
        && components[1].eq_ignore_ascii_case("share")
        && components[2].eq_ignore_ascii_case("NuGet")
        && matches!(
            components[3].to_ascii_lowercase().as_str(),
            "http-cache" | "v3-cache"
        )
}

/// Revalidates the resolved rule root immediately before automatic cleanup.
///
/// Manual large-file and duplicate-file deletion use their own explicit
/// candidate authorization and do not call this policy. Keeping the policies
/// separate prevents a global safety net from blocking intentional user file
/// management while making declarative cleanup fail closed.
pub(crate) fn validate_automatic_cleanup_root(root: &Path, home: &Path) -> Result<(), String> {
    let Some(relative) = relative_components(root, home)? else {
        return Ok(());
    };
    if relative.is_empty() {
        return Err("automatic cleanup cannot own the user home directory".to_string());
    }

    if is_protected_home_relative_path(&relative) {
        return Err("automatic cleanup cannot own protected user content".to_string());
    }
    Ok(())
}

fn relative_components(path: &Path, base: &Path) -> Result<Option<Vec<String>>, String> {
    let path = normalized_components(path)?;
    let base = normalized_components(base)?;
    if path.len() < base.len()
        || !path
            .iter()
            .zip(&base)
            .all(|(left, right)| left.eq_ignore_ascii_case(right))
    {
        return Ok(None);
    }
    Ok(Some(path.into_iter().skip(base.len()).collect()))
}

fn normalized_components(path: &Path) -> Result<Vec<String>, String> {
    let mut output = Vec::new();
    for component in path.components() {
        match component {
            Component::Prefix(prefix) => output.push(
                prefix
                    .as_os_str()
                    .to_str()
                    .ok_or_else(|| "automatic cleanup requires Unicode path roots".to_string())?
                    .to_string(),
            ),
            Component::RootDir => output.push(String::new()),
            Component::Normal(value) => output.push(
                value
                    .to_str()
                    .ok_or_else(|| "automatic cleanup requires Unicode path roots".to_string())?
                    .to_string(),
            ),
            Component::CurDir | Component::ParentDir => {
                return Err("automatic cleanup requires normalized path roots".to_string());
            }
        }
    }
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn automatic_cleanup_rejects_identity_sync_source_and_repository_roots() {
        let home = Path::new("/Users/example");
        for protected in [
            "/Users/example/.ssh/cache",
            "/Users/example/.aws/cache",
            "/Users/example/.config/tool/cache",
            "/Users/example/.local/share/tool/cache",
            "/Users/example/OneDrive/cache",
            "/Users/example/OneDrive - Example Organization/cache",
            "/Users/example/projects/generated",
            "/Users/example/workspace/app/.git/objects",
            "/Users/example/Library/Mobile Documents/com~apple~CloudDocs/cache",
            "/Users/example/Library/Application Support/CloudDocs/session/db",
            "/Users/example/Library/Application Support/FileProvider/state.db",
            "/Users/example/Library/Caches/CloudKit/CloudKitMetadata.db",
            "/Users/example/Library/Caches/com.apple.bird/session",
            "/Users/example/Library/Caches/com.apple.cloudd/session",
            "/Users/example/Library/Caches/com.apple.CloudDocs/session",
            "/Users/example/Library/Caches/com.apple.FileProvider/session",
            "/Users/example/Library/Daemon Containers/provider/state",
        ] {
            assert!(
                validate_automatic_cleanup_root(Path::new(protected), home).is_err(),
                "{protected} must remain outside automatic cleanup"
            );
        }
    }

    #[test]
    fn automatic_cleanup_allows_explicit_cache_and_package_roots() {
        let home = Path::new("/Users/example");
        for allowed in [
            "/Users/example/Library/Caches/com.example.app",
            "/Users/example/.cache/compiler",
            "/Users/example/.cargo/git/db",
            "/Users/example/.local/share/NuGet/http-cache",
            "/Users/example/.local/share/NuGet/v3-cache",
            "/Users/example/Library/Application Support/App/Code Cache",
        ] {
            assert!(
                validate_automatic_cleanup_root(Path::new(allowed), home).is_ok(),
                "{allowed} must remain eligible for a verified rule"
            );
        }
    }

    #[test]
    fn automatic_cleanup_keeps_other_local_share_state_protected() {
        let home = Path::new("/Users/example");
        for protected in [
            "/Users/example/.local/share/NuGet/config",
            "/Users/example/.local/share/Example/cache",
            "/Users/example/.local/share/NuGet/http-cache/nested",
        ] {
            assert!(
                validate_automatic_cleanup_root(Path::new(protected), home).is_err(),
                "{protected} must remain outside automatic cleanup"
            );
        }
    }

    #[test]
    fn paths_outside_the_home_are_deferred_to_platform_validation() {
        assert!(validate_automatic_cleanup_root(
            Path::new("/private/var/tmp/example"),
            Path::new("/Users/example")
        )
        .is_ok());
    }

    #[test]
    fn non_normalized_paths_fail_closed() {
        assert!(validate_automatic_cleanup_root(
            Path::new("/Users/example/projects/../Library/Caches"),
            Path::new("/Users/example")
        )
        .is_err());
    }
}
