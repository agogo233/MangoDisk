use crate::privacy::{
    PrivacyCapabilityState, PrivacyCategory, PrivacyDataKind, PrivacyImpact, PrivacyItem,
    PrivacyRecommendation, PrivacySensitivity,
};
use mangodisk_platform::{
    PlatformPrivacyApplicationTraceAvailability, PlatformPrivacyApplicationTraceKind,
};

pub(super) struct PrivacyItemInput {
    pub(super) token: String,
    pub(super) source_id: String,
    pub(super) source_name: String,
    pub(super) profile_id: Option<String>,
    pub(super) profile_name: Option<String>,
    pub(super) kind: PrivacyDataKind,
    pub(super) capability: PrivacyCapabilityState,
    pub(super) item_count: u64,
    pub(super) estimated_bytes: u64,
    pub(super) requires_browser_close: bool,
}

pub(super) fn privacy_item_is_actionable(item: &PrivacyItem) -> bool {
    matches!(
        item.capability,
        PrivacyCapabilityState::Ready
            | PrivacyCapabilityState::BrowserRunning
            | PrivacyCapabilityState::ApplicationRunning
    ) && (item.item_count > 0
        || matches!(
            item.capability,
            PrivacyCapabilityState::BrowserRunning | PrivacyCapabilityState::ApplicationRunning
        ))
}

pub(super) fn privacy_item(input: PrivacyItemInput) -> PrivacyItem {
    let PrivacyItemInput {
        token,
        source_id,
        source_name,
        profile_id,
        profile_name,
        kind,
        capability,
        item_count,
        estimated_bytes,
        requires_browser_close,
    } = input;
    let (category, sensitivity, impact, recommendation, synchronization_may_propagate) = match kind
    {
        PrivacyDataKind::BrowsingHistory => (
            PrivacyCategory::BrowserActivity,
            PrivacySensitivity::Activity,
            // Removing activity history does not sign the user out or delete downloaded content.
            // Synchronization impact remains visible in the row and confirmation, but this low-
            // impact trace follows the same recommendation policy as other browser history.
            PrivacyImpact::Low,
            PrivacyRecommendation::Recommended,
            true,
        ),
        PrivacyDataKind::DownloadHistory => (
            PrivacyCategory::BrowserActivity,
            PrivacySensitivity::Activity,
            PrivacyImpact::Low,
            PrivacyRecommendation::Recommended,
            false,
        ),
        PrivacyDataKind::Cookies => (
            PrivacyCategory::BrowserAccountState,
            PrivacySensitivity::AccountState,
            PrivacyImpact::SignOut,
            PrivacyRecommendation::Manual,
            true,
        ),
        PrivacyDataKind::SiteStorage => (
            PrivacyCategory::BrowserAccountState,
            PrivacySensitivity::AccountState,
            PrivacyImpact::SignOut,
            PrivacyRecommendation::Manual,
            true,
        ),
        PrivacyDataKind::SitePermissions => (
            PrivacyCategory::BrowserAccountState,
            PrivacySensitivity::AccountState,
            PrivacyImpact::SignOut,
            PrivacyRecommendation::Manual,
            true,
        ),
        PrivacyDataKind::Sessions => (
            PrivacyCategory::BrowserActivity,
            PrivacySensitivity::ContentDerived,
            // Session traces do not remove bookmarks, downloads, or account data. The browser-
            // close confirmation still gives users a final chance to preserve open work.
            PrivacyImpact::Low,
            PrivacyRecommendation::Recommended,
            false,
        ),
        PrivacyDataKind::BrowserCache => (
            PrivacyCategory::BrowserActivity,
            PrivacySensitivity::ContentDerived,
            PrivacyImpact::Low,
            // Browser caches are regenerated and contain copies derived from visited pages. They
            // are safe to recommend while still requiring the owning browser to be closed.
            PrivacyRecommendation::Recommended,
            false,
        ),
        PrivacyDataKind::SearchHistory => (
            PrivacyCategory::BrowserActivity,
            PrivacySensitivity::Activity,
            PrivacyImpact::Low,
            // Search terms may participate in browser synchronization, which remains explicit in
            // the item description and confirmation without changing their low-impact selection.
            PrivacyRecommendation::Recommended,
            true,
        ),
        PrivacyDataKind::WebsiteIcons => (
            PrivacyCategory::BrowserActivity,
            PrivacySensitivity::ContentDerived,
            PrivacyImpact::Low,
            PrivacyRecommendation::Recommended,
            false,
        ),
        PrivacyDataKind::FrequentlyVisitedSites => (
            PrivacyCategory::BrowserActivity,
            PrivacySensitivity::Activity,
            // Removing the new-tab ranking changes convenience rather than account state. Keep it
            // manual because the visible shortcut layout is still part of the user's workflow.
            PrivacyImpact::Workflow,
            PrivacyRecommendation::Manual,
            false,
        ),
        PrivacyDataKind::AddressBarShortcuts => (
            PrivacyCategory::BrowserActivity,
            PrivacySensitivity::Activity,
            PrivacyImpact::Low,
            PrivacyRecommendation::Recommended,
            false,
        ),
        PrivacyDataKind::SavedPasswords | PrivacyDataKind::AutofillData => (
            PrivacyCategory::BrowserAccountState,
            PrivacySensitivity::PersonalContent,
            PrivacyImpact::DataLoss,
            // Personal browser data is never selected automatically. It remains available as an
            // explicit user choice and requires high-impact confirmation before Core execution.
            PrivacyRecommendation::Manual,
            true,
        ),
        PrivacyDataKind::CurrentClipboard => (
            PrivacyCategory::SystemActivity,
            PrivacySensitivity::ContentDerived,
            PrivacyImpact::Workflow,
            PrivacyRecommendation::Manual,
            false,
        ),
        PrivacyDataKind::ClipboardHistory => (
            PrivacyCategory::SystemActivity,
            PrivacySensitivity::ContentDerived,
            PrivacyImpact::Workflow,
            PrivacyRecommendation::Manual,
            false,
        ),
        PrivacyDataKind::RecentItems => (
            PrivacyCategory::SystemActivity,
            PrivacySensitivity::Activity,
            PrivacyImpact::Low,
            PrivacyRecommendation::Recommended,
            false,
        ),
        PrivacyDataKind::RecentApplications
        | PrivacyDataKind::ApplicationUsageHistory
        | PrivacyDataKind::NetworkConnectionHistory
        | PrivacyDataKind::PrinterHistory => (
            PrivacyCategory::SystemActivity,
            PrivacySensitivity::Activity,
            PrivacyImpact::Low,
            PrivacyRecommendation::Recommended,
            false,
        ),
        PrivacyDataKind::FolderViewHistory => (
            PrivacyCategory::SystemActivity,
            PrivacySensitivity::Activity,
            // Shell folder-view history also owns layout preferences. Clearing it improves privacy
            // but can reset view modes or desktop icon placement, so it requires explicit choice.
            PrivacyImpact::Workflow,
            PrivacyRecommendation::Manual,
            false,
        ),
        PrivacyDataKind::ShellHistory => (
            PrivacyCategory::SystemActivity,
            PrivacySensitivity::PersonalContent,
            // Command histories can contain paths, host names, and arguments. They are valuable
            // privacy traces but clearing them removes a user's searchable workflow history.
            PrivacyImpact::Workflow,
            PrivacyRecommendation::Manual,
            false,
        ),
        PrivacyDataKind::JumpLists => (
            PrivacyCategory::SystemActivity,
            PrivacySensitivity::Activity,
            // Windows updates jump lists independently of browser activity. Keep this volatile
            // source opt-in so it cannot block the default browser cleanup plan on drift.
            PrivacyImpact::Workflow,
            PrivacyRecommendation::Manual,
            false,
        ),
        PrivacyDataKind::RunDialogHistory
        | PrivacyDataKind::FileDialogHistory
        | PrivacyDataKind::SystemSearchHistory
        | PrivacyDataKind::ExplorerPathHistory => (
            PrivacyCategory::SystemActivity,
            PrivacySensitivity::Activity,
            PrivacyImpact::Low,
            PrivacyRecommendation::Recommended,
            false,
        ),
        PrivacyDataKind::ApplicationCache | PrivacyDataKind::ApplicationLogs => (
            PrivacyCategory::ApplicationActivity,
            PrivacySensitivity::ContentDerived,
            PrivacyImpact::Low,
            PrivacyRecommendation::Recommended,
            false,
        ),
        PrivacyDataKind::ApplicationSessions => (
            PrivacyCategory::ApplicationActivity,
            PrivacySensitivity::AccountState,
            // Session storage can restore an application's workflow and may also hold transient
            // sign-in state. It remains user-selected even though the source is fully clearable.
            PrivacyImpact::Workflow,
            PrivacyRecommendation::Manual,
            false,
        ),
        PrivacyDataKind::EditorLocalHistory => (
            PrivacyCategory::ApplicationActivity,
            PrivacySensitivity::PersonalContent,
            // Editor local history may be the only remaining copy of unsaved work. Exposing it as
            // a clearable item preserves user choice, but it must never be selected by default.
            PrivacyImpact::DataLoss,
            PrivacyRecommendation::Manual,
            false,
        ),
        PrivacyDataKind::RecentProjects => (
            PrivacyCategory::ApplicationActivity,
            PrivacySensitivity::Activity,
            PrivacyImpact::Workflow,
            PrivacyRecommendation::Manual,
            false,
        ),
        PrivacyDataKind::RecentDocuments
        | PrivacyDataKind::RecentConnections
        | PrivacyDataKind::PlaybackHistory
        | PrivacyDataKind::RecentPaths
        | PrivacyDataKind::RecentSearches => (
            PrivacyCategory::ApplicationActivity,
            PrivacySensitivity::Activity,
            PrivacyImpact::Low,
            PrivacyRecommendation::Recommended,
            false,
        ),
    };
    let selected_by_default = recommendation == PrivacyRecommendation::Recommended
        && matches!(
            capability,
            PrivacyCapabilityState::Ready
                | PrivacyCapabilityState::BrowserRunning
                | PrivacyCapabilityState::ApplicationRunning
        )
        && item_count > 0;
    PrivacyItem {
        token,
        source_id,
        source_name,
        profile_id,
        profile_name,
        category,
        kind,
        sensitivity,
        impact,
        recommendation,
        capability,
        item_count,
        estimated_bytes,
        selected_by_default,
        requires_browser_close,
        synchronization_may_propagate,
    }
}

pub(super) fn application_data_kind(kind: PlatformPrivacyApplicationTraceKind) -> PrivacyDataKind {
    match kind {
        PlatformPrivacyApplicationTraceKind::Cache => PrivacyDataKind::ApplicationCache,
        PlatformPrivacyApplicationTraceKind::Logs => PrivacyDataKind::ApplicationLogs,
        PlatformPrivacyApplicationTraceKind::Sessions => PrivacyDataKind::ApplicationSessions,
        PlatformPrivacyApplicationTraceKind::EditorLocalHistory => {
            PrivacyDataKind::EditorLocalHistory
        }
        PlatformPrivacyApplicationTraceKind::RecentDocuments => PrivacyDataKind::RecentDocuments,
        PlatformPrivacyApplicationTraceKind::RecentProjects => PrivacyDataKind::RecentProjects,
        PlatformPrivacyApplicationTraceKind::RecentConnections => {
            PrivacyDataKind::RecentConnections
        }
        PlatformPrivacyApplicationTraceKind::PlaybackHistory => PrivacyDataKind::PlaybackHistory,
        PlatformPrivacyApplicationTraceKind::RecentPaths => PrivacyDataKind::RecentPaths,
        PlatformPrivacyApplicationTraceKind::RecentSearches => PrivacyDataKind::RecentSearches,
    }
}

pub(super) fn application_availability_capability(
    availability: PlatformPrivacyApplicationTraceAvailability,
) -> Option<PrivacyCapabilityState> {
    match availability {
        PlatformPrivacyApplicationTraceAvailability::Available => None,
        PlatformPrivacyApplicationTraceAvailability::PermissionRequired => {
            Some(PrivacyCapabilityState::PermissionRequired)
        }
        PlatformPrivacyApplicationTraceAvailability::Unavailable => {
            Some(PrivacyCapabilityState::Unavailable)
        }
    }
}

pub(super) fn source_coverage_capability(
    capabilities: &[PrivacyCapabilityState],
    item_count: u64,
) -> PrivacyCapabilityState {
    for capability in [
        PrivacyCapabilityState::SchemaUnsupported,
        PrivacyCapabilityState::PermissionRequired,
        PrivacyCapabilityState::Unavailable,
        PrivacyCapabilityState::Unsupported,
        PrivacyCapabilityState::ApplicationRunning,
        PrivacyCapabilityState::BrowserRunning,
    ] {
        if capabilities.contains(&capability) {
            return capability;
        }
    }
    if item_count == 0 {
        PrivacyCapabilityState::Empty
    } else {
        PrivacyCapabilityState::Ready
    }
}
