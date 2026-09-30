use std::{io, path::Path};

/// The filesystem operation that failed, independent of product/UI guidance.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileReadStage {
    OpenDirectory,
    ReadDirectory,
    ReadMetadata,
}

impl FileReadStage {
    fn as_str(self) -> &'static str {
        match self {
            Self::OpenDirectory => "open_directory",
            Self::ReadDirectory => "read_directory",
            Self::ReadMetadata => "read_metadata",
        }
    }
}

/// Read failures are separate from intentional link, mount, and placeholder skips.
/// Counts describe failed read operations, not the number of files hidden below them.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FileReadFailures {
    pub count: u64,
    /// Native permission denials, including possible privacy restrictions.
    pub permission_denied_count: u64,
    /// macOS protected app-data reads for which privacy settings may help.
    /// This is an observation, not proof that Full Disk Access is disabled.
    pub privacy_restricted_count: u64,
}

impl FileReadFailures {
    pub fn record(&mut self, path: &Path, error: &io::Error, stage: FileReadStage) {
        self.record_with_sequence(path, error, stage, |count| count);
    }

    /// Parallel directory readers share a sequence so each child cannot reset the warning budget.
    #[cfg(target_os = "macos")]
    pub(crate) fn record_in_scope(
        &mut self,
        path: &Path,
        error: &io::Error,
        stage: FileReadStage,
        sequence: &std::sync::atomic::AtomicU64,
    ) {
        self.record_with_sequence(path, error, stage, |_| {
            sequence
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
                .saturating_add(1)
        });
    }

    fn record_with_sequence(
        &mut self,
        path: &Path,
        error: &io::Error,
        stage: FileReadStage,
        next_sequence: impl FnOnce(u64) -> u64,
    ) {
        // Applications routinely remove cache entries during a scan.
        if error.kind() == io::ErrorKind::NotFound {
            return;
        }
        self.count = self.count.saturating_add(1);
        let permission_denied = error.kind() == io::ErrorKind::PermissionDenied;
        if permission_denied {
            self.permission_denied_count = self.permission_denied_count.saturating_add(1);
        }
        #[cfg(target_os = "macos")]
        let privacy_restriction_possible =
            crate::macos::file_read::is_privacy_restricted(path, error);
        #[cfg(not(target_os = "macos"))]
        let privacy_restriction_possible = false;
        if privacy_restriction_possible {
            self.privacy_restricted_count = self.privacy_restricted_count.saturating_add(1);
        }
        // Keep representative failures in default logs. Remaining failures stay
        // available at Debug; merged counters retain every failure in this scope.
        let scope_failure_count = next_sequence(self.count);
        let level = if scope_failure_count <= 3 {
            log::Level::Warn
        } else {
            log::Level::Debug
        };
        log::log!(
            level,
            "filesystem_read_failed stage={} path={} error_kind={:?} os_error={:?} privacy_restriction_possible={} error={} outcome=skipped scope_failure_count={}",
            stage.as_str(),
            crate::diagnostics::text(&path.display()),
            error.kind(),
            error.raw_os_error(),
            privacy_restriction_possible,
            crate::diagnostics::text(error),
            scope_failure_count
        );
    }

    pub fn merge(&mut self, other: Self) {
        self.count = self.count.saturating_add(other.count);
        self.permission_denied_count = self
            .permission_denied_count
            .saturating_add(other.permission_denied_count);
        self.privacy_restricted_count = self
            .privacy_restricted_count
            .saturating_add(other.privacy_restricted_count);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_failures_ignore_disappeared_entries_and_keep_other_errors() {
        let mut failures = FileReadFailures::default();
        let path = Path::new("fixture/cache");
        failures.record(
            path,
            &io::Error::from(io::ErrorKind::NotFound),
            FileReadStage::ReadMetadata,
        );
        assert_eq!(failures.count, 0);
        failures.record(
            path,
            &io::Error::from(io::ErrorKind::PermissionDenied),
            FileReadStage::OpenDirectory,
        );
        failures.record(
            path,
            &io::Error::from(io::ErrorKind::Other),
            FileReadStage::ReadDirectory,
        );
        assert_eq!(failures.count, 2);
        assert_eq!(failures.permission_denied_count, 1);
        assert_eq!(failures.privacy_restricted_count, 0);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn merged_failures_distinguish_native_denials_from_possible_privacy_restrictions() {
        let protected = dirs::home_dir()
            .expect("home must be available")
            .join("Library/Containers/fixture/Data/Library/Caches");
        let ordinary = Path::new("fixture/cache");
        let mut discovery = FileReadFailures::default();
        discovery.record(
            &protected,
            &io::Error::from_raw_os_error(libc::EPERM),
            FileReadStage::OpenDirectory,
        );
        discovery.record(
            &protected,
            &io::Error::from_raw_os_error(libc::EACCES),
            FileReadStage::ReadMetadata,
        );
        let mut traversal = FileReadFailures::default();
        traversal.record(
            ordinary,
            &io::Error::from_raw_os_error(libc::EPERM),
            FileReadStage::OpenDirectory,
        );
        traversal.record(
            ordinary,
            &io::Error::from_raw_os_error(libc::EIO),
            FileReadStage::ReadDirectory,
        );
        traversal.record(
            ordinary,
            &io::Error::from_raw_os_error(libc::ENOENT),
            FileReadStage::ReadMetadata,
        );
        discovery.merge(traversal);
        assert_eq!(discovery.count, 4);
        assert_eq!(discovery.permission_denied_count, 3);
        assert_eq!(discovery.privacy_restricted_count, 1);
    }
}
