mod change_tracking;
mod directories;
mod directory_aggregate;
mod disk_cleanup;
mod file_layout;
mod inventory;
mod large_files;
mod native_io;
mod native_uninstall;
mod package_evidence;
mod package_locations;
mod package_reconciliation;
mod package_sources;
mod project_markers;
mod volumes;

use std::{
    ffi::OsStr,
    fs,
    os::windows::ffi::OsStrExt,
    os::windows::fs::MetadataExt,
    path::{Component, Path, PathBuf},
};

use crate::{
    ApplicationDirectories, ApplicationUninstallExecutionOutcome,
    ApplicationUninstallPlatformError, ApplicationUninstallRegistration,
    ApplicationUninstallRegistrationState, DirectPhysicalDirectoryEnumeration,
    DirectoryTreeAggregate, DirectoryTreeAggregateError, FastAnalysisQuery, FastAnalysisRecord,
    FastAnalysisScanError, FastAnalysisSummary, FilesystemChangeImpactError,
    FilesystemChangeImpactOutcome, FilesystemChangeMonitor, FilesystemChangeToken,
    LargeFileCandidateScanError, LargeFileCandidateSummary, Platform, PlatformCancellation,
    PlatformError, PlatformResult, ProjectMarkerCandidateProgress, ProjectMarkerCandidateQuery,
    ProjectMarkerCandidateScanError, ProjectMarkerCandidateSummary, ScanPurpose, SkipReason,
    SystemInventory, UserDirectories, VolumeInfo, WindowsDiskCleanupEstimate,
    WindowsDiskCleanupExecution, WindowsDiskCleanupKind,
};

pub struct WindowsPlatform;

const FILE_ATTRIBUTE_REPARSE_POINT_VALUE: u32 = 0x0000_0400;
const FILE_ATTRIBUTE_OFFLINE_VALUE: u32 = 0x0000_1000;
const FILE_ATTRIBUTE_RECALL_ON_OPEN_VALUE: u32 = 0x0004_0000;
const FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS_VALUE: u32 = 0x0040_0000;

/// Returns whether opening an entry can recall content from remote or offline storage.
///
/// Directory enumeration already returns these bits with the ordinary file attributes. Keeping
/// the check as a bit mask adds no filesystem call and protects providers that expose recall
/// semantics without a reparse-point bit.
pub(crate) fn is_remote_placeholder_attributes(attributes: u32) -> bool {
    attributes
        & (FILE_ATTRIBUTE_OFFLINE_VALUE
            | FILE_ATTRIBUTE_RECALL_ON_OPEN_VALUE
            | FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS_VALUE)
        != 0
}

pub(crate) fn application_directories(identifier: &str) -> PlatformResult<ApplicationDirectories> {
    directories::application_directories(identifier)
}

pub fn windows_disk_cleanup_estimates(
    kinds: &[WindowsDiskCleanupKind],
    cancellation: &PlatformCancellation,
) -> Vec<WindowsDiskCleanupEstimate> {
    disk_cleanup::estimates(kinds, cancellation, true)
}

/// Re-measures native handlers without using the short-lived preview cache.
///
/// Cleanup preflight calls this immediately before reporting a dry-run result,
/// so the expected byte count describes the current system state.
pub fn fresh_windows_disk_cleanup_estimates(
    kinds: &[WindowsDiskCleanupKind],
    cancellation: &PlatformCancellation,
) -> Vec<WindowsDiskCleanupEstimate> {
    disk_cleanup::estimates(kinds, cancellation, false)
}

pub fn execute_windows_disk_cleanup(
    kind: WindowsDiskCleanupKind,
    cancellation: &PlatformCancellation,
) -> WindowsDiskCleanupExecution {
    disk_cleanup::execute(kind, cancellation)
}

impl Platform for WindowsPlatform {
    fn os_name(&self) -> &'static str {
        "windows"
    }

    fn system_volume_path(&self) -> PathBuf {
        volumes::system_drive_path()
    }

    fn system_volume(&self) -> PlatformResult<VolumeInfo> {
        volumes::system_volume().map_err(Into::into)
    }

    fn volumes(&self) -> PlatformResult<Vec<VolumeInfo>> {
        volumes::volumes().map_err(Into::into)
    }

    fn user_directories(&self) -> PlatformResult<UserDirectories> {
        directories::user_directories()
    }

    fn system_inventory(&self) -> PlatformResult<SystemInventory> {
        inventory::system_inventory(&PlatformCancellation::new(|| false)).map_err(Into::into)
    }

    fn system_inventory_with_cancellation(
        &self,
        cancellation: &PlatformCancellation,
    ) -> PlatformResult<SystemInventory> {
        inventory::system_inventory(cancellation).map_err(Into::into)
    }

    fn system_inventory_revision(&self) -> PlatformResult<String> {
        inventory::system_inventory_revision().map_err(Into::into)
    }

    fn system_inventory_revision_with_cancellation(
        &self,
        cancellation: &PlatformCancellation,
    ) -> PlatformResult<String> {
        inventory::system_inventory_revision_with_cancellation(cancellation).map_err(Into::into)
    }

    fn application_uninstall_registration_state(
        &self,
        registration: &ApplicationUninstallRegistration,
    ) -> Result<ApplicationUninstallRegistrationState, ApplicationUninstallPlatformError> {
        native_uninstall::registration_state(registration)
    }

    fn execute_application_uninstall_registration(
        &self,
        registration: &ApplicationUninstallRegistration,
    ) -> Result<ApplicationUninstallExecutionOutcome, ApplicationUninstallPlatformError> {
        native_uninstall::execute_registration(registration)
    }

    fn running_process_names(&self) -> PlatformResult<Vec<String>> {
        inventory::running_process_names(&PlatformCancellation::new(|| false)).map_err(Into::into)
    }

    fn running_process_names_with_cancellation(
        &self,
        cancellation: &PlatformCancellation,
    ) -> PlatformResult<Vec<String>> {
        inventory::running_process_names(cancellation).map_err(Into::into)
    }

    fn is_link_like(&self, metadata: &fs::Metadata) -> bool {
        let attributes = metadata.file_attributes();
        metadata.file_type().is_symlink()
            || attributes & FILE_ATTRIBUTE_REPARSE_POINT_VALUE != 0
            || is_remote_placeholder_attributes(attributes)
    }

    fn should_skip(
        &self,
        path: &Path,
        scan_root: &Path,
        purpose: ScanPurpose,
    ) -> Option<SkipReason> {
        if purpose == ScanPurpose::Cleanup {
            return None;
        }
        let mut components = path.components().filter_map(|component| match component {
            Component::Normal(value) => Some(value),
            _ => None,
        });
        let first = components.next();
        let second = components.next();

        // This runs in the full-volume traversal hot path. Comparing Windows
        // UTF-16 components directly avoids allocating a lowercase String and
        // formatting temporary values for every visited entry.
        let first_is = |value| first.is_some_and(|part| os_str_eq_ascii_case(part, value));
        let second_is = |value| second.is_some_and(|part| os_str_eq_ascii_case(part, value));
        if matches!(
            purpose,
            ScanPurpose::LargeFiles | ScanPurpose::DuplicateFiles
        ) && second.is_none()
            && [
                "hiberfil.sys",
                "pagefile.sys",
                "swapfile.sys",
                "dumpstack.log.tmp",
            ]
            .into_iter()
            .any(first_is)
        {
            // These files are owned by Windows memory and power management.
            // They can be changed only through the corresponding system
            // settings and must never be presented as ordinary deletable
            // files, even when they dominate a volume's free space.
            return Some(SkipReason::SystemCritical);
        }
        if purpose == ScanPurpose::DuplicateFiles
            && [
                "$windows.~bt",
                "$windows.~ws",
                "windows.old",
                "config.msi",
                "msocache",
                "perflogs",
            ]
            .into_iter()
            .any(first_is)
        {
            // Upgrade, installer, and diagnostic staging trees are managed as complete units by
            // Windows or Deep Cleanup. Presenting byte-equal members as independently removable
            // duplicates can leave an update or rollback image inconsistent.
            return Some(SkipReason::SystemCritical);
        }
        if first_is("system volume information")
            || first_is("$recycle.bin")
            || first_is("recovery")
            || first_is("$winreagent")
            || (first_is("windows")
                && (second_is("winsxs") || second_is("installer") || second_is("system32")))
        {
            return Some(SkipReason::SystemCritical);
        }
        if matches!(
            purpose,
            ScanPurpose::LargeFiles | ScanPurpose::DuplicateFiles
        ) && (first_is("windows")
            || first_is("program files")
            || first_is("program files (x86)")
            || first_is("programdata"))
        {
            return Some(SkipReason::SystemCritical);
        }
        if purpose == ScanPurpose::DuplicateFiles
            && is_transient_duplicate_scope(path)
            && !is_transient_duplicate_scope(scan_root)
        {
            return Some(SkipReason::SystemCritical);
        }
        None
    }

    fn validate_cleanup_root(&self, path: &Path) -> PlatformResult<()> {
        let canonical = fs::canonicalize(path)
            .map_err(|error| PlatformError::io("canonicalize path", &error))?;
        if canonical.parent().is_none()
            || normalize(&canonical) == normalize(&self.system_volume_path())
        {
            return Err(PlatformError::invalid_path("cleanup root is a volume root"));
        }
        let system_directory = directories::system_directory()?;
        let program_files_directories = directories::program_files_directories()?;
        if path_is_same_or_child(&canonical, &system_directory)
            || program_files_directories
                .iter()
                .any(|root| path_is_same_or_child(&canonical, root))
        {
            return Err(PlatformError::invalid_path(
                "cleanup root is system protected",
            ));
        }
        let user_directories = self.user_directories()?;
        for name in [
            "Desktop",
            "Documents",
            "Downloads",
            "Pictures",
            "Videos",
            "Music",
            "OneDrive",
            "Saved Games",
        ] {
            if path_is_same_or_child(&canonical, &user_directories.home_directory().join(name)) {
                return Err(PlatformError::invalid_path(
                    "cleanup root contains user content",
                ));
            }
        }
        Ok(())
    }

    fn fast_directory_tree_aggregate(
        &self,
        root: &Path,
        is_cancelled: &(dyn Fn() -> bool + Sync),
        report_progress: &(dyn Fn(&Path, u64, u64) + Sync),
    ) -> Result<Option<DirectoryTreeAggregate>, DirectoryTreeAggregateError> {
        directory_aggregate::measure(root, false, is_cancelled, report_progress).map(Some)
    }

    fn fast_project_artifact_tree_aggregate(
        &self,
        root: &Path,
        is_cancelled: &(dyn Fn() -> bool + Sync),
        report_progress: &(dyn Fn(&Path, u64, u64) + Sync),
    ) -> Result<Option<DirectoryTreeAggregate>, DirectoryTreeAggregateError> {
        directory_aggregate::measure(root, true, is_cancelled, report_progress).map(Some)
    }

    fn fast_direct_physical_directories(
        &self,
        root: &Path,
        maximum_entries: usize,
        is_cancelled: &(dyn Fn() -> bool + Sync),
    ) -> Result<Option<DirectPhysicalDirectoryEnumeration>, DirectoryTreeAggregateError> {
        directory_aggregate::direct_physical_directories(root, maximum_entries, is_cancelled)
            .map(Some)
    }

    fn capture_filesystem_change_token(
        &self,
        root: &Path,
    ) -> PlatformResult<Option<FilesystemChangeToken>> {
        change_tracking::capture_token(root).map_err(Into::into)
    }

    fn start_filesystem_change_monitor(
        &self,
        root: &Path,
        token: &FilesystemChangeToken,
        is_cancelled: &(dyn Fn() -> bool + Sync),
    ) -> PlatformResult<Option<FilesystemChangeMonitor>> {
        change_tracking::start_monitor(root, token, is_cancelled).map_err(Into::into)
    }

    fn filesystem_change_impact_plan(
        &self,
        root: &Path,
        token: &FilesystemChangeToken,
        is_cancelled: &(dyn Fn() -> bool + Sync),
    ) -> Result<Option<FilesystemChangeImpactOutcome>, FilesystemChangeImpactError> {
        change_tracking::impact_plan(self, root, token, is_cancelled).map(Some)
    }

    fn fast_large_file_candidates(
        &self,
        root: &Path,
        minimum_bytes: u64,
        is_cancelled: &(dyn Fn() -> bool + Sync),
        consumer: &mut dyn FnMut(PathBuf) -> Result<(), String>,
    ) -> Result<Option<LargeFileCandidateSummary>, LargeFileCandidateScanError> {
        large_files::find_candidates(self, root, minimum_bytes, is_cancelled, consumer).map(Some)
    }

    fn fast_project_marker_candidates(
        &self,
        query: ProjectMarkerCandidateQuery<'_>,
        is_cancelled: &(dyn Fn() -> bool + Sync),
        report_progress: &(dyn Fn(ProjectMarkerCandidateProgress) + Sync),
        consumer: &mut dyn FnMut(PathBuf) -> Result<(), String>,
    ) -> Result<Option<ProjectMarkerCandidateSummary>, ProjectMarkerCandidateScanError> {
        project_markers::scan(
            project_markers::ProjectMarkerScanRequest {
                root: query.root,
                file_names: query.file_names,
                file_suffixes: query.file_suffixes,
                pruned_directory_names: query.pruned_directory_names,
                maximum_depth: query.maximum_depth,
                is_cancelled,
                report_progress,
            },
            consumer,
        )
        .map(Some)
    }

    fn fast_analysis_records(
        &self,
        query: FastAnalysisQuery<'_>,
        is_cancelled: &(dyn Fn() -> bool + Sync),
        report_progress: &mut dyn FnMut(&Path, u64, u64),
        consumer: &mut dyn FnMut(FastAnalysisRecord) -> Result<(), String>,
    ) -> Result<Option<FastAnalysisSummary>, FastAnalysisScanError> {
        file_layout::analyze_records(
            self,
            file_layout::AnalysisScanRequest {
                root: query.root,
                purpose: query.purpose,
                large_file_minimum_bytes: query.large_file_minimum_bytes,
                is_cancelled,
                should_prune_directory: query.should_prune_directory,
                report_progress,
            },
            consumer,
        )
    }
}

fn os_str_eq_ascii_case(value: &OsStr, expected: &str) -> bool {
    let mut value = value.encode_wide();
    let mut expected = expected.encode_utf16();
    loop {
        match (value.next(), expected.next()) {
            (Some(left), Some(right)) if ascii_lowercase(left) == ascii_lowercase(right) => {}
            (None, None) => return true,
            _ => return false,
        }
    }
}

fn ascii_lowercase(value: u16) -> u16 {
    if (u16::from(b'A')..=u16::from(b'Z')).contains(&value) {
        value + u16::from(b'a' - b'A')
    } else {
        value
    }
}

/// Returns whether a Windows path belongs to per-user application state.
///
/// Broad duplicate scans skip AppData because it contains app-managed caches, package state,
/// databases, and support files whose byte-identical members are not independent user copies.
/// The caller compares both the candidate and scan root so an explicit request targeting AppData
/// remains inspectable.
fn is_transient_duplicate_scope(path: &Path) -> bool {
    let mut components = path.components().filter_map(|component| match component {
        Component::Normal(value) => Some(value),
        _ => None,
    });
    let values = [
        components.next(),
        components.next(),
        components.next(),
        components.next(),
        components.next(),
        components.next(),
        components.next(),
    ];
    let is = |index: usize, expected: &str| {
        values[index].is_some_and(|value| os_str_eq_ascii_case(value, expected))
    };

    is(0, "temp") || (is(0, "users") && is(2, "appdata"))
}

fn normalize(path: &Path) -> String {
    let value = path.to_string_lossy().replace('/', "\\");
    let value = value
        .strip_prefix(r"\\?\UNC\")
        .map(|path| format!(r"\\{path}"))
        .or_else(|| value.strip_prefix(r"\\?\").map(str::to_string))
        .unwrap_or(value);
    value.trim_end_matches('\\').to_ascii_lowercase()
}

fn path_is_same_or_child(path: &Path, root: &Path) -> bool {
    let path = normalize(path);
    let root = normalize(root);
    path == root
        || path
            .strip_prefix(&root)
            .is_some_and(|suffix| suffix.starts_with('\\'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remote_storage_attributes_fail_closed_without_reparse_points() {
        for attributes in [
            FILE_ATTRIBUTE_OFFLINE_VALUE,
            FILE_ATTRIBUTE_RECALL_ON_OPEN_VALUE,
            FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS_VALUE,
        ] {
            assert!(is_remote_placeholder_attributes(attributes));
        }
        assert!(!is_remote_placeholder_attributes(0));
        assert!(!is_remote_placeholder_attributes(
            FILE_ATTRIBUTE_REPARSE_POINT_VALUE
        ));
    }

    #[test]
    fn windows_path_identity_ignores_verbatim_prefix_and_casing() {
        assert!(path_is_same_or_child(
            Path::new(r"\\?\C:\Windows\System32"),
            Path::new(r"c:\WINDOWS")
        ));
        assert!(!path_is_same_or_child(
            Path::new(r"C:\Windows.old"),
            Path::new(r"C:\Windows")
        ));
    }

    #[test]
    fn actual_windows_system_root_is_never_a_cleanup_root() {
        let platform = WindowsPlatform;
        let system_root =
            directories::system_directory().expect("Windows system directory should be available");

        assert!(platform.validate_cleanup_root(&system_root).is_err());
    }

    #[test]
    fn removable_file_scans_exclude_windows_managed_volume_files() {
        let platform = WindowsPlatform;
        for path in [
            r"C:\hiberfil.sys",
            r"C:\pagefile.sys",
            r"D:\swapfile.sys",
            r"D:\DumpStack.log.tmp",
        ] {
            for purpose in [ScanPurpose::LargeFiles, ScanPurpose::DuplicateFiles] {
                assert_eq!(
                    platform.should_skip(Path::new(path), Path::new(r"C:\"), purpose),
                    Some(SkipReason::SystemCritical),
                    "{path} must not be exposed as an ordinary removable file"
                );
            }
        }
        assert_eq!(
            platform.should_skip(
                Path::new(r"C:\archives\pagefile.sys"),
                Path::new(r"C:\"),
                ScanPurpose::LargeFiles
            ),
            None,
            "a matching user file outside the volume root must not be hidden"
        );
    }

    #[test]
    fn broad_duplicate_scans_skip_windows_staging_and_temporary_data() {
        let platform = WindowsPlatform;
        for path in [
            r"C:\Windows.old\Windows\System32\example.dll",
            r"C:\$Windows.~BT\Sources\install.esd",
            r"C:\Config.Msi\rollback.rbf",
            r"C:\Users\MangoDiskUser\AppData\Local\Temp\archive.bin",
            r"C:\Users\MangoDiskUser\AppData\Local\Microsoft\Windows\INetCache\cache.dat",
            r"C:\Users\MangoDiskUser\AppData\Roaming\Example\managed-copy.bin",
        ] {
            assert_eq!(
                platform.should_skip(
                    Path::new(path),
                    Path::new(r"C:\"),
                    ScanPurpose::DuplicateFiles
                ),
                Some(SkipReason::SystemCritical),
                "{path} must not enter a broad duplicate scan"
            );
        }
    }

    #[test]
    fn explicit_duplicate_scans_allow_temporary_directories() {
        let platform = WindowsPlatform;
        assert_eq!(
            platform.should_skip(
                Path::new(r"C:\Users\MangoDiskUser\AppData\Local\Temp\archive.bin"),
                Path::new(r"C:\Users\MangoDiskUser\AppData\Local\Temp"),
                ScanPurpose::DuplicateFiles
            ),
            None
        );
        assert_eq!(
            platform.should_skip(
                Path::new(r"C:\Users\MangoDiskUser\AppData\Roaming\Example\managed-copy.bin"),
                Path::new(r"C:\Users\MangoDiskUser\AppData"),
                ScanPurpose::DuplicateFiles
            ),
            None
        );
        assert_eq!(
            platform.should_skip(
                Path::new(r"C:\Users\MangoDiskUser\Documents\Temp\report.pdf"),
                Path::new(r"C:\Users\MangoDiskUser"),
                ScanPurpose::DuplicateFiles
            ),
            None
        );
    }
}
