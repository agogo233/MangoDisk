mod policy;
use policy::PrivacyItemInput;
use policy::{
    application_availability_capability, application_data_kind, privacy_item,
    privacy_item_is_actionable, source_coverage_capability,
};

mod details;
#[cfg(test)]
use details::text_record_detail_entries;
use details::{platform_detail_entries, root_detail_entries, system_root_detail_entries};

mod fingerprint;
#[cfg(test)]
use fingerprint::fingerprint_matches_expectation;
use fingerprint::{
    current_fingerprint, database_access_is_busy, database_fingerprint_expectations,
    deferred_database_fingerprint, expected_fingerprint, file_fingerprint, fingerprint_failure,
    refresh_database_fingerprint_expectation, refresh_database_fingerprints,
    roots_summary_fingerprint, system_trace_fingerprint,
};

mod mutation;
use mutation::execute_candidate;
#[cfg(test)]
use mutation::{clear_directories, clear_recent_items};

mod execution_history;
use execution_history::append_history_record;
#[cfg(test)]
use execution_history::privacy_history_outcome;

use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
    sync::{Mutex, OnceLock},
    time::Instant,
};

use mangodisk_platform::{
    current_platform, Platform, PlatformCancellation, PlatformMutationState,
    PlatformPrivacyApplicationNativeTraceKind, PlatformPrivacyBrowserKind,
    PlatformPrivacySystemTraceKind, PrivacyPlatform,
};

use crate::{
    applications::{
        catalog::ProcessSnapshot,
        process_control::{close_resolved_applications, ResolvedApplicationCloseTarget},
    },
    shared::operation::{CoordinatedOperationKind, OperationCancellationToken, OperationGuard},
    ApplicationCloseBatchResult, CoreError, CoreErrorCode, CoreResult,
};

use super::{
    browser_database, PrivacyBrowserCloseRequest, PrivacyBrowserCloseRequirement,
    PrivacyBrowserStatusRequest, PrivacyBrowserStatusResult, PrivacyBrowserStatusTarget,
    PrivacyCapabilityState, PrivacyDataKind, PrivacyDetailsPage, PrivacyDetailsPresentation,
    PrivacyDetailsRequest, PrivacyExecutionItemResult, PrivacyExecutionItemStatus,
    PrivacyExecutionPlan, PrivacyExecutionPlanItem, PrivacyExecutionProgress,
    PrivacyExecutionProgressItem, PrivacyExecutionRequest, PrivacyExecutionResult,
    PrivacyExecutionRunRequest, PrivacyExecutionStage, PrivacyItem, PrivacyRecommendation,
    PrivacyScanProgress, PrivacyScanRequest, PrivacyScanResult, PrivacyScanStage,
    PrivacySourceCoverage, PrivacyTimeRange, PRIVACY_DETAILS_SCHEMA_VERSION,
    PRIVACY_PLAN_SCHEMA_VERSION, PRIVACY_SCAN_SCHEMA_VERSION,
};

const PLAN_TTL_MS: u64 = 5 * 60 * 1_000;
const MAX_SELECTIONS: usize = 256;
const MAX_DETAIL_PAGE_SIZE: u32 = 200;
const MAX_DETAIL_OFFSET: u64 = 100_000;

static SCAN_SESSION: OnceLock<Mutex<Option<PrivacyScanSession>>> = OnceLock::new();
static EXECUTION_PLAN: OnceLock<Mutex<Option<PendingPrivacyPlan>>> = OnceLock::new();

#[derive(Clone)]
struct PrivacyScanSession {
    #[cfg(windows)]
    browser_application_paths: BTreeMap<String, PathBuf>,
    public_result: PrivacyScanResult,
    candidates: BTreeMap<String, NativePrivacyCandidate>,
}

#[derive(Clone)]
struct PendingPrivacyPlan {
    #[cfg(windows)]
    browser_application_paths: BTreeMap<String, PathBuf>,
    public_plan: PrivacyExecutionPlan,
    candidates: Vec<NativePrivacyCandidate>,
    time_range: PrivacyTimeRange,
}

#[derive(Clone)]
struct NativePrivacyCandidate {
    token: String,
    item: PrivacyItem,
    fingerprint: String,
    action: NativePrivacyAction,
    browser_process_names: Vec<String>,
}

#[derive(Debug)]
struct PrivacyMutationFailure {
    error: CoreError,
    confirmed_affected_item_count: u64,
}

impl PrivacyMutationFailure {
    fn new(error: CoreError, confirmed_affected_item_count: u64) -> Self {
        Self {
            error,
            confirmed_affected_item_count,
        }
    }

    fn with_possible_side_effects(mut self) -> Self {
        self.error = self.error.with_possible_side_effects();
        self
    }
}

impl From<CoreError> for PrivacyMutationFailure {
    fn from(error: CoreError) -> Self {
        Self::new(error, 0)
    }
}

impl From<mangodisk_platform::PlatformError> for PrivacyMutationFailure {
    fn from(error: mangodisk_platform::PlatformError) -> Self {
        Self::from(CoreError::from(error))
    }
}

#[derive(Clone)]
enum NativePrivacyAction {
    Database {
        path: PathBuf,
        browser: PlatformPrivacyBrowserKind,
        kind: PrivacyDataKind,
        range: PrivacyTimeRange,
        scan_now_ms: u64,
    },
    Directories {
        roots: Vec<PathBuf>,
    },
    /// Aggregate evidence that is intentionally excluded from every execution plan.
    ReviewOnlyFile {
        path: PathBuf,
        browser: PlatformPrivacyBrowserKind,
        kind: PrivacyDataKind,
        range: PrivacyTimeRange,
        scan_now_ms: u64,
    },
    System {
        kind: PlatformPrivacySystemTraceKind,
        roots: Vec<PathBuf>,
        /// Whether this file-backed item also clears an operating-system owned mirror.
        has_native_revision: bool,
    },
    ApplicationNative {
        kind: PlatformPrivacyApplicationNativeTraceKind,
    },
}

pub struct PrivacyService;

impl PrivacyService {
    pub fn cancel_scan() {
        OperationCancellationToken::privacy_scan().cancel();
    }

    pub fn cancel_execution() {
        OperationCancellationToken::privacy_execution().cancel();
    }

    pub fn scan(request: PrivacyScanRequest) -> CoreResult<PrivacyScanResult> {
        Self::scan_with_progress(request, |_| {})
    }

    pub fn scan_with_progress(
        request: PrivacyScanRequest,
        progress: impl Fn(PrivacyScanProgress),
    ) -> CoreResult<PrivacyScanResult> {
        let operation = OperationGuard::start(CoordinatedOperationKind::PrivacyScan)?;
        // Starting a new scan invalidates every previous token immediately, including when the new
        // scan is cancelled or fails. This prevents adapters from executing a stale result after a
        // failed refresh.
        clear_scan_session()?;
        clear_pending_plan()?;
        let started = Instant::now();
        let observed_at_ms = crate::shared::clock::unix_time_ms();
        let cancellation = platform_cancellation(&operation);
        let platform = current_platform();
        progress(PrivacyScanProgress {
            stage: PrivacyScanStage::Discovering,
            source_name: None,
            completed_sources: 0,
            total_sources: 0,
        });
        let discovery = platform.discover_privacy_sources(&cancellation)?;
        let running_names = platform
            .running_process_names_with_cancellation(&cancellation)?
            .into_iter()
            .map(|name| process_leaf(&name).to_lowercase())
            .collect::<BTreeSet<_>>();
        operation.ensure_not_cancelled()?;

        let scan_nonce = format!("{}:{}", operation.id(), observed_at_ms);
        let mut candidates = BTreeMap::new();
        let mut items = Vec::new();
        let mut coverage = Vec::new();
        #[cfg(windows)]
        let mut browser_application_paths = BTreeMap::new();
        let total_sources = (discovery.browsers.len()
            + discovery.applications.len()
            + discovery.system_traces.len()) as u64;
        let mut completed_sources = 0_u64;

        for browser in discovery.browsers {
            progress(PrivacyScanProgress {
                stage: PrivacyScanStage::Browser,
                source_name: Some(browser.display_name.clone()),
                completed_sources,
                total_sources,
            });
            #[cfg(windows)]
            if let Some(path) = &browser.application_path {
                browser_application_paths.insert(browser.provider_key.clone(), path.clone());
            }
            let browser_icon_path = browser
                .application_path
                .as_ref()
                .map(|path| path.to_string_lossy().into_owned());
            let browser_running = browser
                .process_names
                .iter()
                .any(|name| running_names.contains(&name.to_lowercase()));
            let mut browser_item_count = 0_u64;
            for profile in browser.profiles {
                operation.ensure_not_cancelled()?;
                if let Some(path) = &profile.history_database {
                    add_database_candidate(
                        &scan_nonce,
                        &browser.provider_key,
                        &browser.display_name,
                        &profile.provider_key,
                        &profile.display_name,
                        path,
                        browser.kind,
                        PrivacyDataKind::BrowsingHistory,
                        request.time_range,
                        observed_at_ms,
                        browser_running,
                        &browser.process_names,
                        &mut candidates,
                        &mut items,
                    );
                    if browser.kind != PlatformPrivacyBrowserKind::Safari {
                        add_database_candidate(
                            &scan_nonce,
                            &browser.provider_key,
                            &browser.display_name,
                            &profile.provider_key,
                            &profile.display_name,
                            path,
                            browser.kind,
                            PrivacyDataKind::DownloadHistory,
                            request.time_range,
                            observed_at_ms,
                            browser_running,
                            &browser.process_names,
                            &mut candidates,
                            &mut items,
                        );
                    }
                    if browser.kind == PlatformPrivacyBrowserKind::Chromium {
                        add_database_candidate(
                            &scan_nonce,
                            &browser.provider_key,
                            &browser.display_name,
                            &profile.provider_key,
                            &profile.display_name,
                            path,
                            browser.kind,
                            PrivacyDataKind::SearchHistory,
                            request.time_range,
                            observed_at_ms,
                            browser_running,
                            &browser.process_names,
                            &mut candidates,
                            &mut items,
                        );
                    }
                }
                if let Some(path) = &profile.cookie_database {
                    add_database_candidate(
                        &scan_nonce,
                        &browser.provider_key,
                        &browser.display_name,
                        &profile.provider_key,
                        &profile.display_name,
                        path,
                        browser.kind,
                        PrivacyDataKind::Cookies,
                        request.time_range,
                        observed_at_ms,
                        browser_running,
                        &browser.process_names,
                        &mut candidates,
                        &mut items,
                    );
                }
                if let Some(path) = &profile.permission_database {
                    add_database_candidate(
                        &scan_nonce,
                        &browser.provider_key,
                        &browser.display_name,
                        &profile.provider_key,
                        &profile.display_name,
                        path,
                        browser.kind,
                        PrivacyDataKind::SitePermissions,
                        request.time_range,
                        observed_at_ms,
                        browser_running,
                        &browser.process_names,
                        &mut candidates,
                        &mut items,
                    );
                }
                if let Some(path) = &profile.saved_password_source {
                    add_database_candidate(
                        &scan_nonce,
                        &browser.provider_key,
                        &browser.display_name,
                        &profile.provider_key,
                        &profile.display_name,
                        path,
                        browser.kind,
                        PrivacyDataKind::SavedPasswords,
                        request.time_range,
                        observed_at_ms,
                        browser_running,
                        &browser.process_names,
                        &mut candidates,
                        &mut items,
                    );
                }
                if let Some(path) = &profile.autofill_database {
                    add_database_candidate(
                        &scan_nonce,
                        &browser.provider_key,
                        &browser.display_name,
                        &profile.provider_key,
                        &profile.display_name,
                        path,
                        browser.kind,
                        PrivacyDataKind::AutofillData,
                        request.time_range,
                        observed_at_ms,
                        browser_running,
                        &browser.process_names,
                        &mut candidates,
                        &mut items,
                    );
                }
                if let Some(path) = &profile.top_sites_database {
                    add_database_candidate(
                        &scan_nonce,
                        &browser.provider_key,
                        &browser.display_name,
                        &profile.provider_key,
                        &profile.display_name,
                        path,
                        browser.kind,
                        PrivacyDataKind::FrequentlyVisitedSites,
                        request.time_range,
                        observed_at_ms,
                        browser_running,
                        &browser.process_names,
                        &mut candidates,
                        &mut items,
                    );
                }
                if let Some(path) = &profile.shortcut_database {
                    add_database_candidate(
                        &scan_nonce,
                        &browser.provider_key,
                        &browser.display_name,
                        &profile.provider_key,
                        &profile.display_name,
                        path,
                        browser.kind,
                        PrivacyDataKind::AddressBarShortcuts,
                        request.time_range,
                        observed_at_ms,
                        browser_running,
                        &browser.process_names,
                        &mut candidates,
                        &mut items,
                    );
                }
                if let Some(path) = &profile.favicon_database {
                    add_database_candidate(
                        &scan_nonce,
                        &browser.provider_key,
                        &browser.display_name,
                        &profile.provider_key,
                        &profile.display_name,
                        path,
                        browser.kind,
                        PrivacyDataKind::WebsiteIcons,
                        request.time_range,
                        observed_at_ms,
                        browser_running,
                        &browser.process_names,
                        &mut candidates,
                        &mut items,
                    );
                }
                add_directory_candidate(
                    &scan_nonce,
                    &browser.provider_key,
                    &browser.display_name,
                    &profile.provider_key,
                    &profile.display_name,
                    PrivacyDataKind::Sessions,
                    profile.session_directories,
                    request.time_range,
                    browser_running,
                    &browser.process_names,
                    &cancellation,
                    &mut candidates,
                    &mut items,
                );
                add_directory_candidate(
                    &scan_nonce,
                    &browser.provider_key,
                    &browser.display_name,
                    &profile.provider_key,
                    &profile.display_name,
                    PrivacyDataKind::BrowserCache,
                    profile.cache_directories,
                    request.time_range,
                    browser_running,
                    &browser.process_names,
                    &cancellation,
                    &mut candidates,
                    &mut items,
                );
                add_directory_candidate(
                    &scan_nonce,
                    &browser.provider_key,
                    &browser.display_name,
                    &profile.provider_key,
                    &profile.display_name,
                    PrivacyDataKind::SiteStorage,
                    profile.site_storage_directories,
                    request.time_range,
                    browser_running,
                    &browser.process_names,
                    &cancellation,
                    &mut candidates,
                    &mut items,
                );
            }
            browser_item_count += items
                .iter()
                .filter(|item| item.source_id == browser.provider_key)
                .map(|item| item.item_count)
                .sum::<u64>();
            let browser_capabilities = items
                .iter()
                .filter(|item| item.source_id == browser.provider_key)
                .map(|item| item.capability)
                .collect::<Vec<_>>();
            coverage.push(PrivacySourceCoverage {
                source_id: browser.provider_key,
                source_name: browser.display_name,
                icon_path: browser_icon_path,
                capability: if browser_capabilities
                    .contains(&PrivacyCapabilityState::SchemaUnsupported)
                {
                    PrivacyCapabilityState::SchemaUnsupported
                } else if browser_capabilities.contains(&PrivacyCapabilityState::PermissionRequired)
                {
                    PrivacyCapabilityState::PermissionRequired
                } else if browser_capabilities.contains(&PrivacyCapabilityState::Unavailable) {
                    PrivacyCapabilityState::Unavailable
                } else if browser_capabilities.contains(&PrivacyCapabilityState::BrowserRunning) {
                    PrivacyCapabilityState::BrowserRunning
                } else if browser_item_count == 0 {
                    PrivacyCapabilityState::Empty
                } else {
                    PrivacyCapabilityState::Ready
                },
                item_count: browser_item_count,
            });
            completed_sources = completed_sources.saturating_add(1);
        }

        for application in discovery.applications {
            progress(PrivacyScanProgress {
                stage: PrivacyScanStage::Application,
                source_name: Some(application.display_name.clone()),
                completed_sources,
                total_sources,
            });
            let application_running = application
                .process_names
                .iter()
                .any(|name| running_names.contains(&process_leaf(name).to_lowercase()));
            let icon_path = application
                .application_path
                .as_ref()
                .map(|path| path.to_string_lossy().into_owned());
            let source_id = application.provider_key;
            let source_name = application.display_name;
            let process_names = application.process_names;
            let mut source_capabilities = Vec::new();
            let mut source_item_count = 0_u64;

            for trace in application.traces {
                operation.ensure_not_cancelled()?;
                let kind = application_data_kind(trace.kind);
                let token = candidate_token(&scan_nonce, &source_id, None, kind);
                let range_supported =
                    !trace.all_time_only || request.time_range == PrivacyTimeRange::AllTime;
                let platform_capability = application_availability_capability(trace.availability);
                let (base_capability, item_count, estimated_bytes, fingerprint) =
                    if !range_supported {
                        (PrivacyCapabilityState::Unsupported, 0, 0, None)
                    } else if let Some(capability) = platform_capability {
                        (capability, 0, 0, None)
                    } else if !trace.roots.is_empty() {
                        match roots_summary_fingerprint(&trace.roots, &cancellation) {
                            Ok((fingerprint, count, bytes)) => (
                                if count == 0 {
                                    PrivacyCapabilityState::Empty
                                } else {
                                    PrivacyCapabilityState::Ready
                                },
                                count,
                                bytes,
                                Some(fingerprint),
                            ),
                            Err(error) if error.code() == CoreErrorCode::PermissionDenied => {
                                (PrivacyCapabilityState::PermissionRequired, 0, 0, None)
                            }
                            Err(_) => (PrivacyCapabilityState::Unavailable, 0, 0, None),
                        }
                    } else if trace.native_kind.is_some() {
                        (
                            if trace.item_count == 0 {
                                PrivacyCapabilityState::Empty
                            } else {
                                PrivacyCapabilityState::Ready
                            },
                            trace.item_count,
                            0,
                            Some(trace.revision.clone()),
                        )
                    } else {
                        (PrivacyCapabilityState::Empty, 0, 0, Some("empty".into()))
                    };
                let capability = if application_running
                    && item_count > 0
                    && matches!(
                        base_capability,
                        PrivacyCapabilityState::Ready | PrivacyCapabilityState::Empty
                    ) {
                    PrivacyCapabilityState::ApplicationRunning
                } else {
                    base_capability
                };
                let item = privacy_item(PrivacyItemInput {
                    token: token.clone(),
                    source_id: source_id.clone(),
                    source_name: source_name.clone(),
                    profile_id: None,
                    profile_name: None,
                    kind,
                    capability,
                    item_count,
                    estimated_bytes,
                    requires_browser_close: !process_names.is_empty(),
                });
                if let Some(fingerprint) = fingerprint {
                    let action = match trace.native_kind {
                        Some(kind) => NativePrivacyAction::ApplicationNative { kind },
                        None => NativePrivacyAction::Directories { roots: trace.roots },
                    };
                    candidates.insert(
                        token.clone(),
                        NativePrivacyCandidate {
                            token,
                            item: item.clone(),
                            fingerprint,
                            action,
                            browser_process_names: process_names.clone(),
                        },
                    );
                }
                source_capabilities.push(capability);
                source_item_count = source_item_count.saturating_add(item_count);
                items.push(item);
            }
            coverage.push(PrivacySourceCoverage {
                source_id,
                source_name,
                icon_path,
                capability: source_coverage_capability(&source_capabilities, source_item_count),
                item_count: source_item_count,
            });
            completed_sources = completed_sources.saturating_add(1);
        }

        for trace in discovery.system_traces {
            progress(PrivacyScanProgress {
                stage: PrivacyScanStage::System,
                source_name: Some(trace.display_name.clone()),
                completed_sources,
                total_sources,
            });
            let capability = if !trace.available
                || (trace.all_time_only && request.time_range != PrivacyTimeRange::AllTime)
            {
                PrivacyCapabilityState::Unsupported
            } else if trace.item_count == 0 {
                PrivacyCapabilityState::Empty
            } else {
                PrivacyCapabilityState::Ready
            };
            coverage.push(PrivacySourceCoverage {
                source_id: trace.provider_key.clone(),
                source_name: trace.display_name.clone(),
                icon_path: None,
                capability,
                item_count: trace.item_count,
            });
            if trace.available {
                let kind = match trace.kind {
                    PlatformPrivacySystemTraceKind::CurrentClipboard => {
                        PrivacyDataKind::CurrentClipboard
                    }
                    PlatformPrivacySystemTraceKind::ClipboardHistory => {
                        PrivacyDataKind::ClipboardHistory
                    }
                    PlatformPrivacySystemTraceKind::RecentItems => PrivacyDataKind::RecentItems,
                    PlatformPrivacySystemTraceKind::RecentApplications => {
                        PrivacyDataKind::RecentApplications
                    }
                    PlatformPrivacySystemTraceKind::RecentDocumentHistory => {
                        PrivacyDataKind::RecentItems
                    }
                    PlatformPrivacySystemTraceKind::ApplicationUsageHistory => {
                        PrivacyDataKind::ApplicationUsageHistory
                    }
                    PlatformPrivacySystemTraceKind::NetworkConnectionHistory => {
                        PrivacyDataKind::NetworkConnectionHistory
                    }
                    PlatformPrivacySystemTraceKind::FolderViewHistory => {
                        PrivacyDataKind::FolderViewHistory
                    }
                    PlatformPrivacySystemTraceKind::PrinterHistory => {
                        PrivacyDataKind::PrinterHistory
                    }
                    PlatformPrivacySystemTraceKind::ShellHistory => PrivacyDataKind::ShellHistory,
                    PlatformPrivacySystemTraceKind::JumpLists => PrivacyDataKind::JumpLists,
                    PlatformPrivacySystemTraceKind::RunDialogHistory => {
                        PrivacyDataKind::RunDialogHistory
                    }
                    PlatformPrivacySystemTraceKind::FileDialogHistory => {
                        PrivacyDataKind::FileDialogHistory
                    }
                    PlatformPrivacySystemTraceKind::ExplorerSearchHistory => {
                        PrivacyDataKind::SystemSearchHistory
                    }
                    PlatformPrivacySystemTraceKind::ExplorerPathHistory => {
                        PrivacyDataKind::ExplorerPathHistory
                    }
                };
                let token = candidate_token(&scan_nonce, &trace.provider_key, None, kind);
                let has_native_revision = !trace.roots.is_empty() && !trace.revision.is_empty();
                let (fingerprint, _, bytes) = if trace.roots.is_empty() {
                    (trace.revision.clone(), 0, 0)
                } else {
                    let (root_revision, count, bytes) =
                        roots_summary_fingerprint(&trace.roots, &cancellation)?;
                    (
                        system_trace_fingerprint(
                            &root_revision,
                            has_native_revision.then_some(trace.revision.as_str()),
                        ),
                        count,
                        bytes,
                    )
                };
                let item = privacy_item(PrivacyItemInput {
                    token: token.clone(),
                    source_id: trace.provider_key,
                    source_name: trace.display_name,
                    profile_id: None,
                    profile_name: None,
                    kind,
                    capability,
                    item_count: trace.item_count,
                    estimated_bytes: bytes,
                    requires_browser_close: false,
                });
                candidates.insert(
                    token.clone(),
                    NativePrivacyCandidate {
                        token,
                        item: item.clone(),
                        fingerprint,
                        action: NativePrivacyAction::System {
                            kind: trace.kind,
                            roots: trace.roots,
                            has_native_revision,
                        },
                        browser_process_names: Vec::new(),
                    },
                );
                items.push(item);
            }
            completed_sources = completed_sources.saturating_add(1);
        }

        progress(PrivacyScanProgress {
            stage: PrivacyScanStage::Finalizing,
            source_name: None,
            completed_sources: total_sources,
            total_sources,
        });

        // Several privacy kinds can share one SQLite database. A later count against that database
        // can update its WAL/SHM metadata and invalidate a fingerprint captured for an earlier kind.
        // Refresh all database fingerprints only after every read-only query has completed.
        refresh_database_fingerprints(&mut candidates)?;
        items.sort_by(|left, right| {
            (
                left.category,
                &left.source_name,
                &left.profile_name,
                left.kind as u8,
            )
                .cmp(&(
                    right.category,
                    &right.source_name,
                    &right.profile_name,
                    right.kind as u8,
                ))
        });
        let revision = revision_for(&candidates);
        let public_result = PrivacyScanResult {
            schema_version: PRIVACY_SCAN_SCHEMA_VERSION,
            scan_id: blake3::hash(format!("privacy-scan:{scan_nonce}:{revision}").as_bytes())
                .to_hex()
                .to_string(),
            revision,
            time_range: request.time_range,
            scanned_at_ms: observed_at_ms,
            elapsed_ms: started.elapsed().as_millis() as u64,
            items,
            coverage,
        };
        replace_scan_session(PrivacyScanSession {
            #[cfg(windows)]
            browser_application_paths,
            public_result: public_result.clone(),
            candidates,
        })?;
        log::info!(
            "privacy_scan_completed operation_id={} source_count={} candidate_count={} elapsed_ms={}",
            operation.id(), public_result.coverage.len(), public_result.items.len(), public_result.elapsed_ms
        );
        operation.complete();
        Ok(public_result)
    }

    pub fn details(request: PrivacyDetailsRequest) -> CoreResult<PrivacyDetailsPage> {
        if request.limit == 0
            || request.limit > MAX_DETAIL_PAGE_SIZE
            || request.offset > MAX_DETAIL_OFFSET
        {
            return Err(CoreError::invalid_input(
                "privacy detail page is outside the supported range",
            ));
        }
        let session = current_scan_session()?;
        if request.scan_id != session.public_result.scan_id {
            return Err(CoreError::invalid_input("privacy scan session has expired"));
        }
        let candidate = session
            .candidates
            .get(&request.token)
            .ok_or_else(|| CoreError::invalid_input("privacy detail token is unknown"))?;
        // Detail loading is read-only and the opaque token remains bound to one allowlisted source
        // in the active scan session. Windows recent-item registries and application databases can
        // legitimately change immediately after a scan, so enforcing the destructive-operation
        // fingerprint here made an ordinary detail click fail intermittently. Execution still
        // performs its complete fingerprint preflight before the first mutation.
        log::info!(
            "privacy_details_requested source_id={} kind={:?} offset={} limit={}",
            candidate.item.source_id,
            candidate.item.kind,
            request.offset,
            request.limit
        );
        let fetch_limit = request.limit.saturating_add(1);
        let (presentation, mut entries) = match &candidate.action {
            NativePrivacyAction::Database {
                path,
                browser,
                kind,
                range,
                scan_now_ms,
            }
            | NativePrivacyAction::ReviewOnlyFile {
                path,
                browser,
                kind,
                range,
                scan_now_ms,
            } => (
                PrivacyDetailsPresentation::List,
                browser_database::details(
                    path,
                    *browser,
                    *kind,
                    *range,
                    *scan_now_ms,
                    request.offset,
                    fetch_limit,
                )?,
            ),
            NativePrivacyAction::Directories { roots } => (
                PrivacyDetailsPresentation::List,
                root_detail_entries(roots, request.offset, fetch_limit)?,
            ),
            NativePrivacyAction::System { kind, roots, .. } if !roots.is_empty() => {
                let entries =
                    system_root_detail_entries(*kind, roots, request.offset, fetch_limit)?;
                (PrivacyDetailsPresentation::List, entries)
            }
            NativePrivacyAction::System { kind, .. } => {
                platform_detail_entries(current_platform().system_privacy_trace_details(
                    *kind,
                    request.offset,
                    fetch_limit,
                )?)
            }
            NativePrivacyAction::ApplicationNative { kind } => {
                platform_detail_entries(current_platform().application_privacy_trace_details(
                    *kind,
                    request.offset,
                    fetch_limit,
                )?)
            }
        };
        let has_more = entries.len() > request.limit as usize;
        if has_more {
            entries.truncate(request.limit as usize);
        }
        let next_offset = has_more.then(|| request.offset.saturating_add(entries.len() as u64));
        log::info!(
            "privacy_details_loaded source_id={} kind={:?} presentation={:?} entry_count={} has_more={}",
            candidate.item.source_id,
            candidate.item.kind,
            presentation,
            entries.len(),
            has_more
        );
        Ok(PrivacyDetailsPage {
            schema_version: PRIVACY_DETAILS_SCHEMA_VERSION,
            scan_id: request.scan_id,
            token: request.token,
            total_item_count: candidate.item.item_count,
            presentation,
            entries,
            next_offset,
        })
    }

    pub fn prepare(request: PrivacyExecutionRequest) -> CoreResult<PrivacyExecutionPlan> {
        let started_at_ms = crate::shared::clock::unix_time_ms();
        if request.tokens.is_empty() || request.tokens.len() > MAX_SELECTIONS {
            return Err(CoreError::invalid_input(
                "privacy selection count is invalid",
            ));
        }
        let unique = request.tokens.iter().collect::<BTreeSet<_>>();
        if unique.len() != request.tokens.len() {
            return Err(CoreError::invalid_input(
                "privacy selection contains duplicate tokens",
            ));
        }
        let operation = OperationGuard::start(CoordinatedOperationKind::PrivacyExecution)?;
        let session = current_scan_session()?;
        if request.scan_id != session.public_result.scan_id {
            return Err(CoreError::invalid_input("privacy scan session has expired"));
        }
        let mut selected = Vec::new();
        for token in &request.tokens {
            let candidate = session.candidates.get(token).ok_or_else(|| {
                CoreError::invalid_input("privacy selection contains an unknown token")
            })?;
            if !privacy_item_is_actionable(&candidate.item)
                || matches!(
                    candidate.item.recommendation,
                    PrivacyRecommendation::ReviewOnly | PrivacyRecommendation::Unsupported
                )
            {
                return Err(CoreError::invalid_input(
                    "privacy selection is not actionable",
                ));
            }
            // Preparation validates the opaque selection against the current scan session but does
            // not traverse every source again. Execution performs a complete fingerprint preflight
            // before its first mutation, so repeating it here only delays confirmation and can race
            // with live browser databases without adding a safety boundary.
            selected.push(candidate.clone());
        }
        selected.sort_by(|left, right| left.token.cmp(&right.token));
        let created_at_ms = crate::shared::clock::unix_time_ms();
        let expires_at_ms = created_at_ms.saturating_add(PLAN_TTL_MS);
        let plan_id = plan_id(&session.public_result, &selected, expires_at_ms);
        let browser_close_requirements = browser_close_requirements(&selected);
        let public_plan = PrivacyExecutionPlan {
            schema_version: PRIVACY_PLAN_SCHEMA_VERSION,
            plan_id,
            scan_id: session.public_result.scan_id,
            created_at_ms,
            expires_at_ms,
            browser_close_requirements: browser_close_requirements.clone(),
            requires_confirmation: true,
            requires_browser_close: !browser_close_requirements.is_empty(),
            items: selected
                .iter()
                .map(|candidate| PrivacyExecutionPlanItem {
                    token: candidate.token.clone(),
                    source_id: candidate.item.source_id.clone(),
                    source_name: candidate.item.source_name.clone(),
                    profile_name: candidate.item.profile_name.clone(),
                    kind: candidate.item.kind,
                    impact: candidate.item.impact,
                    item_count: candidate.item.item_count,
                    estimated_bytes: candidate.item.estimated_bytes,
                    requires_browser_close: candidate.item.requires_browser_close,
                    synchronization_may_propagate: candidate.item.synchronization_may_propagate,
                })
                .collect(),
        };
        replace_pending_plan(PendingPrivacyPlan {
            #[cfg(windows)]
            browser_application_paths: session.browser_application_paths,
            public_plan: public_plan.clone(),
            candidates: selected,
            time_range: session.public_result.time_range,
        })?;
        log::info!(
            "privacy_plan_prepared operation_id={} item_count={} expires_in_ms={} elapsed_ms={}",
            operation.id(),
            public_plan.items.len(),
            PLAN_TTL_MS,
            crate::shared::clock::unix_time_ms().saturating_sub(started_at_ms)
        );
        operation.complete();
        Ok(public_plan)
    }

    /// Closes only browser identities resolved from the still-pending privacy
    /// plan. The adapter supplies stable source IDs, never process names, so
    /// this command cannot become an arbitrary process-termination primitive.
    ///
    /// Closing a browser can flush structured privacy data. The pending plan remains available for
    /// a bounded force-close retry and for execution's candidate-scoped refresh. The adapter must
    /// not rebuild the complete privacy catalog between process close and execution.
    pub fn close_browsers(
        request: PrivacyBrowserCloseRequest,
    ) -> CoreResult<ApplicationCloseBatchResult> {
        validate_browser_process_request(&request.plan_id, &request.source_ids)?;
        let operation = OperationGuard::start(CoordinatedOperationKind::ApplicationClose)?;
        let targets = resolve_browser_process_targets(&request.plan_id, &request.source_ids)?;
        let result = close_resolved_applications(targets, request.mode)?;
        log::info!(
            "privacy_browser_close_finished operation_id={} source_count={} remaining_process_count={} failed_target_count={}",
            operation.id(),
            request.source_ids.len(),
            result.remaining_process_count,
            result.failed_target_count
        );
        operation.complete();
        Ok(result)
    }

    /// Refreshes only the process state authorized by the pending plan. It intentionally performs
    /// no close request and no privacy rescan, allowing the confirmation dialog to remove
    /// applications that finish shutting down after the bounded graceful-close wait.
    pub fn refresh_browser_status(
        request: PrivacyBrowserStatusRequest,
    ) -> CoreResult<PrivacyBrowserStatusResult> {
        validate_browser_process_request(&request.plan_id, &request.source_ids)?;
        let started = Instant::now();
        let targets = resolve_browser_process_targets(&request.plan_id, &request.source_ids)?;
        let snapshot = ProcessSnapshot::capture().map_err(CoreError::operation_failed)?;
        let targets = targets
            .into_iter()
            .map(|target| {
                let running_processes = snapshot.matching_processes(&target.executable_names);
                PrivacyBrowserStatusTarget {
                    source_id: target.target_id,
                    running_processes,
                }
            })
            .collect::<Vec<_>>();
        let running_process_count = targets
            .iter()
            .map(|target| target.running_processes.len() as u64)
            .sum();
        let elapsed_ms = started.elapsed().as_millis() as u64;
        if running_process_count == 0 {
            log::info!(
                "privacy_browser_status_cleared source_count={} elapsed_ms={elapsed_ms}",
                request.source_ids.len()
            );
        } else {
            log::debug!(
                "privacy_browser_status_refreshed source_count={} running_process_count={running_process_count} elapsed_ms={elapsed_ms}",
                request.source_ids.len()
            );
        }
        Ok(PrivacyBrowserStatusResult {
            running_process_count,
            targets,
            elapsed_ms,
        })
    }

    pub fn execute(request: PrivacyExecutionRunRequest) -> CoreResult<PrivacyExecutionResult> {
        Self::execute_with_progress(request, |_| {})
    }

    pub fn execute_with_progress<F>(
        request: PrivacyExecutionRunRequest,
        mut update: F,
    ) -> CoreResult<PrivacyExecutionResult>
    where
        F: FnMut(PrivacyExecutionProgress),
    {
        if request.plan_id.len() < 16 || request.plan_id.len() > 128 {
            return Err(CoreError::invalid_input(
                "privacy plan identifier is invalid",
            ));
        }
        let excluded_source_ids = request.excluded_source_ids.iter().collect::<BTreeSet<_>>();
        if excluded_source_ids.len() != request.excluded_source_ids.len()
            || excluded_source_ids.len() > MAX_SELECTIONS
        {
            return Err(CoreError::invalid_input(
                "privacy execution source exclusions are invalid",
            ));
        }
        let operation = OperationGuard::start(CoordinatedOperationKind::PrivacyExecution)?;
        let started_at_ms = crate::shared::clock::unix_time_ms();
        let mut pending = take_pending_plan(&request.plan_id)?;
        if crate::shared::clock::unix_time_ms() > pending.public_plan.expires_at_ms {
            return Err(CoreError::invalid_input(
                "privacy execution plan has expired",
            ));
        }
        let excludable_source_ids = pending
            .public_plan
            .browser_close_requirements
            .iter()
            .map(|requirement| requirement.source_id.as_str())
            .collect::<BTreeSet<_>>();
        if excluded_source_ids
            .iter()
            .any(|source_id| !excludable_source_ids.contains(source_id.as_str()))
        {
            return Err(CoreError::invalid_input(
                "privacy execution excludes an unauthorized source",
            ));
        }
        let selected_item_count = pending.candidates.len();
        pending
            .candidates
            .retain(|candidate| !excluded_source_ids.contains(&candidate.item.source_id));
        let total_item_count = pending.candidates.len() as u64;
        if pending.candidates.is_empty() {
            // Source exclusions may remove the complete authorized selection. This is a skipped
            // confirmation, not a cleanup operation or a valid empty history record.
            operation.ensure_not_cancelled()?;
            update(PrivacyExecutionProgress {
                stage: PrivacyExecutionStage::Finalizing,
                current_token: None,
                current_source_name: None,
                current_kind: None,
                completed_item_count: 0,
                total_item_count: 0,
                affected_item_count: 0,
                elapsed_ms: crate::shared::clock::unix_time_ms().saturating_sub(started_at_ms),
                completed_items: Vec::new(),
            });
            let scan = current_scan_session().ok().and_then(|session| {
                (session.public_result.scan_id == pending.public_plan.scan_id)
                    .then_some(session.public_result)
            });
            log::info!(
                "privacy_execution_skipped operation_id={} reason=allSourcesExcluded selected_item_count={} excluded_source_count={} retained_item_count=0 history_saved=false outcome=unchanged",
                operation.id(), selected_item_count, excluded_source_ids.len()
            );
            operation.complete();
            return Ok(PrivacyExecutionResult {
                plan_id: request.plan_id,
                affected_item_count: 0,
                failed_item_count: 0,
                items: Vec::new(),
                scan,
            });
        }
        update(PrivacyExecutionProgress {
            stage: PrivacyExecutionStage::Validating,
            current_token: None,
            current_source_name: None,
            current_kind: None,
            completed_item_count: 0,
            total_item_count,
            affected_item_count: 0,
            elapsed_ms: crate::shared::clock::unix_time_ms().saturating_sub(started_at_ms),
            completed_items: Vec::new(),
        });
        let running = current_platform()
            .running_process_names()?
            .into_iter()
            .map(|name| process_leaf(&name).to_lowercase())
            .collect::<BTreeSet<_>>();

        // Closing a browser can flush its final WAL or session state. Refresh only the already
        // authorized candidates whose owning process is now closed. This keeps destructive
        // preflight fail-closed without running another catalog scan before the result dialog.
        let mut refreshed_candidate_count = 0_u64;
        for candidate in &mut pending.candidates {
            let was_running = matches!(
                candidate.item.capability,
                PrivacyCapabilityState::BrowserRunning | PrivacyCapabilityState::ApplicationRunning
            );
            let is_running = candidate
                .browser_process_names
                .iter()
                .any(|name| running.contains(&name.to_lowercase()));
            if was_running && !is_running {
                candidate.fingerprint = current_fingerprint(candidate).inspect_err(|error| {
                    log_privacy_item_failure(
                        operation.id(),
                        candidate,
                        "refresh_candidate",
                        "sourceUnavailable",
                        Some(error),
                        0,
                    );
                })?;
                candidate.item.capability = PrivacyCapabilityState::Ready;
                refreshed_candidate_count = refreshed_candidate_count.saturating_add(1);
            }
        }
        if refreshed_candidate_count > 0 || !excluded_source_ids.is_empty() {
            log::info!(
                "privacy_execution_selection_refreshed operation_id={} refreshed_candidate_count={} excluded_source_count={} selected_item_count={} retained_item_count={}",
                operation.id(),
                refreshed_candidate_count,
                excluded_source_ids.len(),
                selected_item_count,
                total_item_count
            );
        }

        // Validate the complete plan before the first mutation. Without this pass, an early item
        // could be cleared before a later source is found to be running or changed since scan.
        // The second per-item fingerprint below remains necessary to cover drift during execution.
        let preflight_failures = pending
            .candidates
            .iter()
            .map(|candidate| {
                if operation.ensure_not_cancelled().is_err() {
                    Some(("operationCancelled", None))
                } else if candidate
                    .browser_process_names
                    .iter()
                    .any(|name| running.contains(&name.to_lowercase()))
                {
                    Some(("browserRunning", None))
                } else {
                    fingerprint_failure(candidate, &candidate.fingerprint)
                        .map(|(reason, error)| (reason, Some(error)))
                }
            })
            .collect::<Vec<_>>();
        if preflight_failures.iter().any(Option::is_some) {
            for (candidate, failure) in pending.candidates.iter().zip(&preflight_failures) {
                if let Some((reason, error)) = failure {
                    log_privacy_item_failure(
                        operation.id(),
                        candidate,
                        "preflight",
                        reason,
                        error.as_ref(),
                        0,
                    );
                }
            }
            let results = pending
                .candidates
                .iter()
                .zip(preflight_failures)
                .map(|(candidate, failure)| {
                    if let Some((reason, _)) = failure {
                        execution_result(
                            candidate,
                            if reason == "operationCancelled" {
                                PrivacyExecutionItemStatus::Cancelled
                            } else {
                                PrivacyExecutionItemStatus::Failed
                            },
                            0,
                            false,
                            Some(reason),
                        )
                    } else {
                        execution_result(
                            candidate,
                            PrivacyExecutionItemStatus::Cancelled,
                            0,
                            false,
                            Some("preflightBlocked"),
                        )
                    }
                })
                .collect::<Vec<_>>();
            let failed_item_count = results
                .iter()
                .filter(|item| item.status == PrivacyExecutionItemStatus::Failed)
                .count() as u64;
            let cancelled_item_count = results
                .iter()
                .filter(|item| item.status == PrivacyExecutionItemStatus::Cancelled)
                .count() as u64;
            update(PrivacyExecutionProgress {
                stage: PrivacyExecutionStage::Finalizing,
                current_token: None,
                current_source_name: None,
                current_kind: None,
                completed_item_count: total_item_count,
                total_item_count,
                affected_item_count: 0,
                elapsed_ms: crate::shared::clock::unix_time_ms().saturating_sub(started_at_ms),
                completed_items: execution_progress_items(&results),
            });
            let history_saved = append_history_record(
                operation.id(),
                request.plan_id.clone(),
                pending.time_range,
                &pending.candidates,
                &results,
                started_at_ms,
                crate::shared::clock::unix_time_ms(),
            );
            log::warn!(
                "privacy_execution_preflight_blocked operation_id={} selected_item_count={} retained_item_count={} failed_count={} cancelled_count={} history_saved={} outcome=unchanged",
                operation.id(),
                selected_item_count,
                total_item_count,
                failed_item_count,
                cancelled_item_count,
                history_saved
            );
            operation.complete();
            let scan = current_scan_session().ok().and_then(|session| {
                (session.public_result.scan_id == pending.public_plan.scan_id)
                    .then_some(session.public_result)
            });
            return Ok(PrivacyExecutionResult {
                plan_id: request.plan_id,
                affected_item_count: 0,
                failed_item_count,
                items: results,
                scan,
            });
        }
        let mut results: Vec<PrivacyExecutionItemResult> = Vec::new();
        let mut database_fingerprints = database_fingerprint_expectations(&pending.candidates);
        for candidate in &pending.candidates {
            update(PrivacyExecutionProgress {
                stage: PrivacyExecutionStage::Cleaning,
                current_token: Some(candidate.token.clone()),
                current_source_name: Some(candidate.item.source_name.clone()),
                current_kind: Some(candidate.item.kind),
                completed_item_count: results.len() as u64,
                total_item_count,
                affected_item_count: results.iter().map(|item| item.affected_item_count).sum(),
                elapsed_ms: crate::shared::clock::unix_time_ms().saturating_sub(started_at_ms),
                completed_items: execution_progress_items(&results),
            });
            if operation.ensure_not_cancelled().is_err() {
                results.push(execution_result(
                    candidate,
                    PrivacyExecutionItemStatus::Cancelled,
                    0,
                    false,
                    Some("operationCancelled"),
                ));
                continue;
            }
            // A browser can start after the plan-wide preflight. Refresh process state before each
            // browser mutation so a newly launched process cannot share the database transaction.
            match browser_is_running(candidate) {
                Ok(false) => {}
                Ok(true) => {
                    log_privacy_item_failure(
                        operation.id(),
                        candidate,
                        "execution",
                        "browser_running",
                        None,
                        0,
                    );
                    results.push(execution_result(
                        candidate,
                        PrivacyExecutionItemStatus::Failed,
                        0,
                        false,
                        Some("browserRunning"),
                    ));
                    continue;
                }
                Err(error) => {
                    log_privacy_item_failure(
                        operation.id(),
                        candidate,
                        "process_check",
                        "operation_failed",
                        Some(&error),
                        0,
                    );
                    results.push(execution_result(
                        candidate,
                        PrivacyExecutionItemStatus::Failed,
                        0,
                        false,
                        Some("operationFailed"),
                    ));
                    continue;
                }
            }
            let expected = expected_fingerprint(candidate, &database_fingerprints);
            if let Some((reason, error)) = fingerprint_failure(candidate, expected) {
                log_privacy_item_failure(
                    operation.id(),
                    candidate,
                    "execution",
                    reason,
                    Some(&error),
                    0,
                );
                results.push(execution_result(
                    candidate,
                    PrivacyExecutionItemStatus::Failed,
                    0,
                    false,
                    Some(reason),
                ));
                continue;
            }
            match execute_candidate(candidate) {
                Ok(affected) => {
                    // Multiple selected kinds can share one browser database. The first successful
                    // transaction legitimately changes that file, so later items must compare
                    // against the post-transaction fingerprint rather than the original scan. This
                    // refresh is scoped to the exact database path; unrelated sources retain their
                    // scan fingerprint and still fail closed on drift.
                    refresh_database_fingerprint_expectation(candidate, &mut database_fingerprints);
                    results.push(execution_result(
                        candidate,
                        if affected == 0 {
                            PrivacyExecutionItemStatus::Unchanged
                        } else {
                            PrivacyExecutionItemStatus::Cleared
                        },
                        affected,
                        true,
                        None,
                    ));
                }
                Err(failure) => {
                    let mutation_uncertain =
                        failure.error.mutation_state() == PlatformMutationState::MayHaveChanged;
                    let reason = if failure.confirmed_affected_item_count > 0 {
                        "partiallyCleared"
                    } else if mutation_uncertain {
                        "outcomeUncertain"
                    } else {
                        "operationFailed"
                    };
                    log_privacy_item_failure(
                        operation.id(),
                        candidate,
                        "mutation_or_verification",
                        reason,
                        Some(&failure.error),
                        failure.confirmed_affected_item_count,
                    );
                    results.push(execution_result(
                        candidate,
                        PrivacyExecutionItemStatus::Failed,
                        failure.confirmed_affected_item_count,
                        false,
                        Some(reason),
                    ));
                }
            }
        }
        let failed_item_count = results
            .iter()
            .filter(|item| item.status == PrivacyExecutionItemStatus::Failed)
            .count() as u64;
        let cancelled_item_count = results
            .iter()
            .filter(|item| item.status == PrivacyExecutionItemStatus::Cancelled)
            .count() as u64;
        let affected_item_count = results.iter().map(|item| item.affected_item_count).sum();
        let partial_failure_count = results
            .iter()
            .filter(|item| item.failure_reason.as_deref() == Some("partiallyCleared"))
            .count();
        let uncertain_failure_count = results
            .iter()
            .filter(|item| item.failure_reason.as_deref() == Some("outcomeUncertain"))
            .count();
        update(PrivacyExecutionProgress {
            stage: PrivacyExecutionStage::Finalizing,
            current_token: None,
            current_source_name: None,
            current_kind: None,
            completed_item_count: total_item_count,
            total_item_count,
            affected_item_count,
            elapsed_ms: crate::shared::clock::unix_time_ms().saturating_sub(started_at_ms),
            completed_items: execution_progress_items(&results),
        });
        let history_saved = append_history_record(
            operation.id(),
            request.plan_id.clone(),
            pending.time_range,
            &pending.candidates,
            &results,
            started_at_ms,
            crate::shared::clock::unix_time_ms(),
        );
        // A verified cleanup already gives Core enough evidence to update the visible snapshot.
        // Rebuilding the entire privacy catalog here makes a successful action feel like another
        // scan and needlessly rereads every application. Reconcile only verified executed rows;
        // untouched rows retain their original authority and therefore require a rescan if their
        // underlying source changed.
        let scan =
            reconcile_scan_after_execution(&pending.public_plan.scan_id, &results, operation.id());
        let operation_id = operation.id();
        let scan_reconciled = scan.is_some();
        log::info!(
            "privacy_execution_completed operation_id={operation_id} affected_count={affected_item_count} failed_count={failed_item_count} cancelled_count={cancelled_item_count} partial_failure_count={partial_failure_count} uncertain_failure_count={uncertain_failure_count} scan_reconciled={scan_reconciled} selected_item_count={selected_item_count} retained_item_count={total_item_count} result_item_count={} history_saved={history_saved}",
            results.len()
        );
        operation.complete();
        Ok(PrivacyExecutionResult {
            plan_id: request.plan_id,
            affected_item_count,
            failed_item_count,
            items: results,
            scan,
        })
    }
}

fn validate_browser_process_request(plan_id: &str, source_ids: &[String]) -> CoreResult<()> {
    if plan_id.len() < 16 || plan_id.len() > 128 {
        return Err(CoreError::invalid_input(
            "privacy plan identifier is invalid",
        ));
    }
    let selected_source_ids = source_ids.iter().collect::<BTreeSet<_>>();
    if selected_source_ids.is_empty() || selected_source_ids.len() != source_ids.len() {
        return Err(CoreError::invalid_input(
            "privacy browser process selection is invalid",
        ));
    }
    Ok(())
}

/// Resolves process names exclusively from the pending plan. Both close and status-refresh
/// requests share this boundary so a renderer-provided source ID can never become an arbitrary
/// process-control or process-inspection primitive.
fn resolve_browser_process_targets(
    plan_id: &str,
    source_ids: &[String],
) -> CoreResult<Vec<ResolvedApplicationCloseTarget>> {
    let pending = current_pending_plan(plan_id)?;
    if crate::shared::clock::unix_time_ms() > pending.public_plan.expires_at_ms {
        return Err(CoreError::invalid_input(
            "privacy execution plan has expired",
        ));
    }
    let allowed = pending
        .public_plan
        .browser_close_requirements
        .iter()
        .map(|requirement| {
            (
                requirement.source_id.as_str(),
                requirement.processes.as_slice(),
            )
        })
        .collect::<BTreeMap<_, _>>();
    source_ids
        .iter()
        .map(|source_id| {
            let executable_names = allowed.get(source_id.as_str()).ok_or_else(|| {
                CoreError::invalid_input(
                    "privacy browser process request contains an unknown source",
                )
            })?;
            #[cfg(windows)]
            let executable_paths =
                if crate::applications::process_control::requires_exact_close_identity(
                    executable_names,
                ) {
                    pending
                        .browser_application_paths
                        .get(source_id)
                        .cloned()
                        .into_iter()
                        .collect()
                } else {
                    Vec::new()
                };
            #[cfg(not(windows))]
            let executable_paths = Vec::new();
            Ok(ResolvedApplicationCloseTarget {
                target_id: source_id.clone(),
                executable_names: executable_names.to_vec(),
                executable_paths,
            })
        })
        .collect()
}

#[allow(clippy::too_many_arguments)]
fn add_database_candidate(
    scan_nonce: &str,
    source_id: &str,
    source_name: &str,
    profile_id: &str,
    profile_name: &str,
    path: &Path,
    browser: PlatformPrivacyBrowserKind,
    kind: PrivacyDataKind,
    range: PrivacyTimeRange,
    observed_at_ms: u64,
    browser_running: bool,
    process_names: &[String],
    candidates: &mut BTreeMap<String, NativePrivacyCandidate>,
    items: &mut Vec<PrivacyItem>,
) {
    let token = candidate_token(scan_nonce, source_id, Some(profile_id), kind);
    let supports_time_range = !matches!(
        kind,
        PrivacyDataKind::SitePermissions
            | PrivacyDataKind::SavedPasswords
            | PrivacyDataKind::AutofillData
            | PrivacyDataKind::FrequentlyVisitedSites
            | PrivacyDataKind::WebsiteIcons
    ) || range == PrivacyTimeRange::AllTime;
    let file_access_error = fs::File::open(path).err();
    let source_busy = file_access_error
        .as_ref()
        .is_some_and(database_access_is_busy);
    let permission_denied = file_access_error.as_ref().is_some_and(|error| {
        error.kind() == std::io::ErrorKind::PermissionDenied && !database_access_is_busy(error)
    });
    let file_unavailable = file_access_error.is_some() && !permission_denied && !source_busy;
    // SQLite's read-only connection observes committed WAL records without copying browser data.
    // A short busy timeout bounds races with an active writer. Positive live results remain gated
    // by BrowserRunning so execution still requires process closure and a candidate-scoped refresh
    // before any mutation.
    let count = if !supports_time_range || permission_denied || file_unavailable || source_busy {
        Ok(0)
    } else {
        browser_database::count(path, browser, kind, range, observed_at_ms)
    };
    // A read-only SQLite query can still create or update WAL/SHM coordination files. Capture
    // the fingerprint after the query so the scan cannot make its own result look stale. A running
    // browser result is used only for review until execution refreshes that selected candidate.
    // Windows Chromium can deny every shared read while its network service owns the Cookie
    // database. Bind the close-browser plan to safe file metadata without reading contents; this
    // fingerprint can authorize only the close prompt and is replaced before mutation.
    let fingerprint = if source_busy && browser_running {
        deferred_database_fingerprint(path)
    } else {
        file_fingerprint(path)
    };
    let capability = match (&fingerprint, &count) {
        (Ok(_), _) if !supports_time_range => PrivacyCapabilityState::Unsupported,
        (_, _) if permission_denied => PrivacyCapabilityState::PermissionRequired,
        (Ok(_), _) if source_busy && browser_running => PrivacyCapabilityState::BrowserRunning,
        (_, _) if source_busy => PrivacyCapabilityState::Unavailable,
        (_, _) if file_unavailable => PrivacyCapabilityState::Unavailable,
        (Ok(_), Ok(0)) => PrivacyCapabilityState::Empty,
        (Ok(_), Ok(_)) if browser_running => PrivacyCapabilityState::BrowserRunning,
        (Ok(_), Ok(_)) => PrivacyCapabilityState::Ready,
        _ => PrivacyCapabilityState::SchemaUnsupported,
    };
    let item_count = count.unwrap_or(0);
    let estimated_bytes = fs::metadata(path)
        .map(|metadata| metadata.len())
        .unwrap_or(0);
    let item = privacy_item(PrivacyItemInput {
        token: token.clone(),
        source_id: source_id.into(),
        source_name: source_name.into(),
        profile_id: Some(profile_id.into()),
        profile_name: Some(profile_name.into()),
        kind,
        capability,
        item_count,
        estimated_bytes,
        requires_browser_close: true,
    });
    if let Ok(fingerprint) = fingerprint {
        candidates.insert(
            token.clone(),
            NativePrivacyCandidate {
                token,
                item: item.clone(),
                fingerprint,
                action: if item.recommendation == PrivacyRecommendation::ReviewOnly {
                    NativePrivacyAction::ReviewOnlyFile {
                        path: path.to_path_buf(),
                        browser,
                        kind,
                        range,
                        scan_now_ms: observed_at_ms,
                    }
                } else {
                    NativePrivacyAction::Database {
                        path: path.to_path_buf(),
                        browser,
                        kind,
                        range,
                        scan_now_ms: observed_at_ms,
                    }
                },
                browser_process_names: process_names.to_vec(),
            },
        );
    }
    items.push(item);
}

#[allow(clippy::too_many_arguments)]
fn add_directory_candidate(
    scan_nonce: &str,
    source_id: &str,
    source_name: &str,
    profile_id: &str,
    profile_name: &str,
    kind: PrivacyDataKind,
    roots: Vec<PathBuf>,
    range: PrivacyTimeRange,
    browser_running: bool,
    process_names: &[String],
    cancellation: &PlatformCancellation,
    candidates: &mut BTreeMap<String, NativePrivacyCandidate>,
    items: &mut Vec<PrivacyItem>,
) {
    if roots.is_empty() {
        return;
    }
    let token = candidate_token(scan_nonce, source_id, Some(profile_id), kind);
    let snapshot = roots_summary_fingerprint(&roots, cancellation);
    let (capability, count, bytes, fingerprint) = match snapshot {
        Ok((fingerprint, count, bytes)) if range != PrivacyTimeRange::AllTime => (
            PrivacyCapabilityState::Unsupported,
            count,
            bytes,
            Some(fingerprint),
        ),
        Ok((fingerprint, 0, bytes)) => (PrivacyCapabilityState::Empty, 0, bytes, Some(fingerprint)),
        Ok((fingerprint, count, bytes)) if browser_running => (
            PrivacyCapabilityState::BrowserRunning,
            count,
            bytes,
            Some(fingerprint),
        ),
        Ok((fingerprint, count, bytes)) => (
            PrivacyCapabilityState::Ready,
            count,
            bytes,
            Some(fingerprint),
        ),
        Err(error) if error.code() == CoreErrorCode::PermissionDenied => {
            (PrivacyCapabilityState::PermissionRequired, 0, 0, None)
        }
        Err(_) => (PrivacyCapabilityState::Unavailable, 0, 0, None),
    };
    let item = privacy_item(PrivacyItemInput {
        token: token.clone(),
        source_id: source_id.into(),
        source_name: source_name.into(),
        profile_id: Some(profile_id.into()),
        profile_name: Some(profile_name.into()),
        kind,
        capability,
        item_count: count,
        estimated_bytes: bytes,
        requires_browser_close: true,
    });
    if let Some(fingerprint) = fingerprint {
        candidates.insert(
            token.clone(),
            NativePrivacyCandidate {
                token,
                item: item.clone(),
                fingerprint,
                action: NativePrivacyAction::Directories { roots },
                browser_process_names: process_names.to_vec(),
            },
        );
    }
    items.push(item);
}

fn browser_close_requirements(
    candidates: &[NativePrivacyCandidate],
) -> Vec<PrivacyBrowserCloseRequirement> {
    let mut groups = BTreeMap::<String, (String, BTreeMap<String, String>)>::new();
    for candidate in candidates.iter().filter(|candidate| {
        matches!(
            candidate.item.capability,
            PrivacyCapabilityState::BrowserRunning | PrivacyCapabilityState::ApplicationRunning
        )
    }) {
        let entry = groups
            .entry(candidate.item.source_id.clone())
            .or_insert_with(|| (candidate.item.source_name.clone(), BTreeMap::new()));
        for process in &candidate.browser_process_names {
            entry
                .1
                .entry(process_leaf(process).to_lowercase())
                .or_insert_with(|| process.clone());
        }
    }
    groups
        .into_iter()
        .map(
            |(source_id, (source_name, processes))| PrivacyBrowserCloseRequirement {
                source_id,
                source_name,
                processes: processes.into_values().collect(),
            },
        )
        .collect()
}

fn browser_is_running(candidate: &NativePrivacyCandidate) -> CoreResult<bool> {
    if candidate.browser_process_names.is_empty() {
        return Ok(false);
    }
    let running = current_platform()
        .running_process_names()?
        .into_iter()
        .map(|name| process_leaf(&name).to_lowercase())
        .collect::<BTreeSet<_>>();
    Ok(candidate
        .browser_process_names
        .iter()
        .any(|name| running.contains(&name.to_lowercase())))
}

fn privacy_io_error(context: &str, error: std::io::Error) -> CoreError {
    if error.kind() == std::io::ErrorKind::PermissionDenied {
        CoreError::new(
            CoreErrorCode::PermissionDenied,
            format!("{context}: {error}"),
        )
    } else {
        CoreError::operation_failed(format!("{context}: {error}"))
    }
}

// A source may span several roots; retain the failing child rather than only its source list.
fn privacy_path_io_error(context: &str, path: &Path, error: std::io::Error) -> CoreError {
    privacy_io_error(
        &format!(
            "{context} path={} error_kind={:?} os_error={:?}",
            current_platform().display_path(path),
            error.kind(),
            error.raw_os_error()
        ),
        error,
    )
}

fn candidate_token(
    scan_nonce: &str,
    source_id: &str,
    profile_name: Option<&str>,
    kind: PrivacyDataKind,
) -> String {
    blake3::hash(
        format!(
            "{scan_nonce}:{source_id}:{}:{kind:?}",
            profile_name.unwrap_or_default()
        )
        .as_bytes(),
    )
    .to_hex()
    .to_string()
}
fn revision_for(candidates: &BTreeMap<String, NativePrivacyCandidate>) -> String {
    let mut hasher = blake3::Hasher::new();
    for candidate in candidates.values() {
        hasher.update(candidate.token.as_bytes());
        hasher.update(candidate.fingerprint.as_bytes());
    }
    hasher.finalize().to_hex().to_string()
}
fn plan_id(
    scan: &PrivacyScanResult,
    candidates: &[NativePrivacyCandidate],
    expires_at_ms: u64,
) -> String {
    let mut hasher = blake3::Hasher::new();
    hasher.update(scan.scan_id.as_bytes());
    hasher.update(scan.revision.as_bytes());
    hasher.update(&expires_at_ms.to_le_bytes());
    for candidate in candidates {
        hasher.update(candidate.token.as_bytes());
        hasher.update(candidate.fingerprint.as_bytes());
    }
    hasher.finalize().to_hex().to_string()
}
fn process_leaf(value: &str) -> String {
    Path::new(value)
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| value.into())
}

fn reconcile_scan_after_execution(
    executed_scan_id: &str,
    results: &[PrivacyExecutionItemResult],
    operation_id: u64,
) -> Option<PrivacyScanResult> {
    let mut session = match current_scan_session() {
        Ok(session) if session.public_result.scan_id == executed_scan_id => session,
        Ok(_) => {
            log::warn!(
                "privacy_scan_reconcile_skipped operation_id={operation_id} reason=scan_id_mismatch"
            );
            return None;
        }
        Err(error) => {
            let error_detail = mangodisk_platform::diagnostics::text(&error);
            log::warn!(
                "privacy_scan_reconcile_skipped operation_id={operation_id} reason=session_unavailable error_detail={error_detail}"
            );
            return None;
        }
    };
    let completed_tokens = results
        .iter()
        .filter(|result| {
            result.verified
                && matches!(
                    result.status,
                    PrivacyExecutionItemStatus::Cleared | PrivacyExecutionItemStatus::Unchanged
                )
        })
        .map(|result| result.token.as_str())
        .collect::<BTreeSet<_>>();

    // Completed tokens remain as empty presentation rows so users can see that the category was
    // scanned. They are removed only from the private candidate map, which makes them impossible
    // to select for another destructive plan without a later real scan finding new data.
    session
        .candidates
        .retain(|token, _| !completed_tokens.contains(token.as_str()));
    for item in &mut session.public_result.items {
        if completed_tokens.contains(item.token.as_str()) {
            item.capability = PrivacyCapabilityState::Empty;
            item.item_count = 0;
            item.estimated_bytes = 0;
            item.selected_by_default = false;
            item.requires_browser_close = false;
        }
    }
    for coverage in &mut session.public_result.coverage {
        let source_items = session
            .public_result
            .items
            .iter()
            .filter(|item| item.source_id == coverage.source_id)
            .collect::<Vec<_>>();
        coverage.item_count = source_items.iter().map(|item| item.item_count).sum();
        coverage.capability = source_coverage_capability(
            &source_items
                .iter()
                .map(|item| item.capability)
                .collect::<Vec<_>>(),
            coverage.item_count,
        );
    }
    session.public_result.revision = revision_for(&session.candidates);
    session.public_result.scan_id = blake3::hash(
        format!(
            "privacy-reconciled-scan:{}:{}:{}",
            executed_scan_id,
            session.public_result.revision,
            crate::shared::clock::unix_time_ms()
        )
        .as_bytes(),
    )
    .to_hex()
    .to_string();
    let public_result = session.public_result.clone();
    if let Err(error) = replace_scan_session(session) {
        let error_detail = mangodisk_platform::diagnostics::text(&error);
        log::warn!(
            "privacy_scan_reconcile_publish_failed operation_id={operation_id} error_detail={error_detail}"
        );
        return None;
    }
    Some(public_result)
}

fn execution_result(
    candidate: &NativePrivacyCandidate,
    status: PrivacyExecutionItemStatus,
    affected_item_count: u64,
    verified: bool,
    failure_reason: Option<&str>,
) -> PrivacyExecutionItemResult {
    PrivacyExecutionItemResult {
        token: candidate.token.clone(),
        status,
        affected_item_count,
        verified,
        failure_reason: failure_reason.map(Into::into),
    }
}

fn execution_progress_items(
    results: &[PrivacyExecutionItemResult],
) -> Vec<PrivacyExecutionProgressItem> {
    results
        .iter()
        .map(|result| PrivacyExecutionProgressItem {
            token: result.token.clone(),
            status: result.status,
        })
        .collect()
}

/// Keep the source, mutation state and native cause together for post-failure diagnosis.
fn log_privacy_item_failure(
    operation_id: u64,
    candidate: &NativePrivacyCandidate,
    stage: &'static str,
    reason: &'static str,
    error: Option<&CoreError>,
    confirmed_affected_item_count: u64,
) {
    let error_detail = error
        .map(|error| mangodisk_platform::diagnostics::text(&error))
        .unwrap_or_else(|| "none".into());
    let mutation_state = error
        .map(|error| match error.mutation_state() {
            PlatformMutationState::NotAttempted => "not_attempted",
            PlatformMutationState::MayHaveChanged => "may_have_changed",
        })
        .unwrap_or("not_attempted");
    let paths = match &candidate.action {
        NativePrivacyAction::Database { path, .. }
        | NativePrivacyAction::ReviewOnlyFile { path, .. } => vec![path],
        NativePrivacyAction::Directories { roots } | NativePrivacyAction::System { roots, .. } => {
            roots.iter().collect()
        }
        NativePrivacyAction::ApplicationNative { .. } => Vec::new(),
    };
    let source_paths = paths
        .iter()
        .map(|path| current_platform().display_path(path))
        .collect::<Vec<_>>()
        .join(" | ");
    let source_paths = mangodisk_platform::diagnostics::text(&source_paths);
    log::warn!(
        "privacy_item_failed operation_id={operation_id} source_id={} kind={:?} stage={stage} reason={reason} mutation_state={mutation_state} confirmed_affected_count={confirmed_affected_item_count} source_paths={source_paths} error_detail={error_detail}",
        candidate.item.source_id,
        candidate.item.kind
    );
}
fn platform_cancellation(operation: &OperationGuard) -> PlatformCancellation {
    let flag = operation.cancellation_flag();
    PlatformCancellation::new(move || flag.load(std::sync::atomic::Ordering::Relaxed))
}
fn scan_session_lock() -> &'static Mutex<Option<PrivacyScanSession>> {
    SCAN_SESSION.get_or_init(|| Mutex::new(None))
}
fn plan_lock() -> &'static Mutex<Option<PendingPrivacyPlan>> {
    EXECUTION_PLAN.get_or_init(|| Mutex::new(None))
}
fn replace_scan_session(session: PrivacyScanSession) -> CoreResult<()> {
    *scan_session_lock()
        .lock()
        .map_err(|_| CoreError::operation_failed("privacy scan session is unavailable"))? =
        Some(session);
    Ok(())
}
fn current_scan_session() -> CoreResult<PrivacyScanSession> {
    scan_session_lock()
        .lock()
        .map_err(|_| CoreError::operation_failed("privacy scan session is unavailable"))?
        .clone()
        .ok_or_else(|| CoreError::invalid_input("privacy scan session has expired"))
}
fn clear_scan_session() -> CoreResult<()> {
    *scan_session_lock()
        .lock()
        .map_err(|_| CoreError::operation_failed("privacy scan session is unavailable"))? = None;
    Ok(())
}
fn replace_pending_plan(plan: PendingPrivacyPlan) -> CoreResult<()> {
    *plan_lock()
        .lock()
        .map_err(|_| CoreError::operation_failed("privacy execution plan is unavailable"))? =
        Some(plan);
    Ok(())
}
fn clear_pending_plan() -> CoreResult<()> {
    *plan_lock()
        .lock()
        .map_err(|_| CoreError::operation_failed("privacy execution plan is unavailable"))? = None;
    Ok(())
}
fn current_pending_plan(plan_id: &str) -> CoreResult<PendingPrivacyPlan> {
    let pending = plan_lock()
        .lock()
        .map_err(|_| CoreError::operation_failed("privacy execution plan is unavailable"))?
        .clone()
        .ok_or_else(|| CoreError::invalid_input("privacy execution plan has expired"))?;
    if pending.public_plan.plan_id != plan_id {
        return Err(CoreError::invalid_input(
            "privacy execution plan does not match",
        ));
    }
    Ok(pending)
}
fn take_pending_plan(plan_id: &str) -> CoreResult<PendingPrivacyPlan> {
    let mut guard = plan_lock()
        .lock()
        .map_err(|_| CoreError::operation_failed("privacy execution plan is unavailable"))?;
    let pending = guard
        .take()
        .ok_or_else(|| CoreError::invalid_input("privacy execution plan has expired"))?;
    if pending.public_plan.plan_id != plan_id {
        return Err(CoreError::invalid_input(
            "privacy execution plan does not match",
        ));
    }
    Ok(pending)
}

#[cfg(all(test, windows))]
#[path = "brave_origin_process_tests.rs"]
mod brave_origin_process_tests;

#[cfg(all(test, any(target_os = "macos", windows)))]
#[path = "brave_origin_tests.rs"]
mod brave_origin_tests;

#[cfg(test)]
#[path = "service_tests.rs"]
mod tests;
