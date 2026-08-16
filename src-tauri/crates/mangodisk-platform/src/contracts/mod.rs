mod applications;
mod directory_aggregate;
mod disk_cleanup;
mod error;
mod platform;
mod scan;
mod volumes;

pub use applications::{
    ApplicationComponentAggregate, ApplicationComponentAggregateError, ApplicationInstallScope,
    ApplicationInventorySource, ApplicationSourceIdentity, ApplicationUninstallExecutionOutcome,
    ApplicationUninstallPlatformError, ApplicationUninstallRegistration,
    ApplicationUninstallRegistrationState, DetectedTool, InstalledApplication,
    MacosPrivilegedApplicationRemovalOutcome, SystemInventory, WindowsRegisteredUninstallKind,
    WindowsRegistryView,
};
#[cfg(test)]
pub(crate) use directory_aggregate::reference_directory_tree_aggregate;
pub(crate) use directory_aggregate::DirectoryAggregateProgress;
pub use directory_aggregate::{
    DirectPhysicalDirectoryEnumeration, DirectoryTreeAggregate, DirectoryTreeAggregateError,
    DirectoryTreeSourceAggregate,
};
pub use disk_cleanup::{
    PlatformCancellation, WindowsDiskCleanupAvailability, WindowsDiskCleanupEstimate,
    WindowsDiskCleanupExecution, WindowsDiskCleanupExecutionStatus, WindowsDiskCleanupKind,
};
pub use error::{PlatformError, PlatformErrorCode, PlatformResult};
pub use platform::Platform;
pub(crate) use scan::FilesystemChangeMonitorBackend;
pub use scan::{
    FastAnalysisQuery, FastAnalysisRecord, FastAnalysisScanError, FastAnalysisSummary,
    FilesystemChangeImpactError, FilesystemChangeImpactOutcome, FilesystemChangeImpactPlan,
    FilesystemChangeImpactSummary, FilesystemChangeImpactUnavailable, FilesystemChangeMonitor,
    FilesystemChangeStatus, FilesystemChangeToken, LargeFileCandidateScanError,
    LargeFileCandidateSummary, ProjectMarkerCandidateProgress, ProjectMarkerCandidateQuery,
    ProjectMarkerCandidateScanError, ProjectMarkerCandidateSummary, ScanPurpose, SkipReason,
};
pub use volumes::{
    ApplicationDirectories, ScanConcurrency, ScanDeviceClass, UserDirectories, VolumeInfo,
};
