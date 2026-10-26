use super::*;
use crate::history::{
    HistoryService, OperationDetails, OperationOutcome, PrivacyCleanupHistoryItem,
    PrivacyCleanupHistoryItemStatus,
};
use crate::privacy::{PrivacyCategory, PrivacyImpact, PrivacyRecommendation};
use crate::shared::operation::test_operation_lock;
use mangodisk_platform::PlatformPrivacyApplicationTraceAvailability;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::UNIX_EPOCH;

static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(1);

fn execution_request(plan_id: String) -> PrivacyExecutionRunRequest {
    PrivacyExecutionRunRequest {
        plan_id,
        excluded_source_ids: Vec::new(),
    }
}

fn fixture_directory(name: &str) -> PathBuf {
    let directory = std::env::temp_dir().join(format!(
        "mangodisk-privacy-service-{name}-{}-{}",
        std::process::id(),
        NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir_all(&directory).unwrap();
    directory
}

fn install_directory_scan_session(root: &Path) -> (String, String) {
    let token = "a".repeat(64);
    let item = privacy_item(PrivacyItemInput {
        token: token.clone(),
        source_id: "synthetic".into(),
        source_name: "Synthetic browser".into(),
        profile_id: Some("synthetic:profile".into()),
        profile_name: Some("Test profile".into()),
        kind: PrivacyDataKind::SiteStorage,
        capability: PrivacyCapabilityState::Ready,
        item_count: 1,
        estimated_bytes: 1,
        requires_browser_close: false,
    });
    let roots = vec![root.to_path_buf()];
    let fingerprint = roots_summary_fingerprint(&roots, &PlatformCancellation::new(|| false))
        .unwrap()
        .0;
    let candidate = NativePrivacyCandidate {
        token: token.clone(),
        item: item.clone(),
        fingerprint: fingerprint.clone(),
        action: NativePrivacyAction::Directories { roots },
        browser_process_names: Vec::new(),
    };
    let scan_id = "synthetic-scan".to_string();
    replace_scan_session(PrivacyScanSession {
        #[cfg(windows)]
        browser_application_paths: BTreeMap::new(),
        public_result: PrivacyScanResult {
            schema_version: PRIVACY_SCAN_SCHEMA_VERSION,
            scan_id: scan_id.clone(),
            revision: fingerprint,
            time_range: PrivacyTimeRange::AllTime,
            scanned_at_ms: 1,
            elapsed_ms: 1,
            items: vec![item],
            coverage: Vec::new(),
        },
        candidates: BTreeMap::from([(token.clone(), candidate)]),
    })
    .unwrap();
    clear_pending_plan().unwrap();
    (scan_id, token)
}

#[test]
fn application_permission_denial_remains_distinct_from_unsupported_capability() {
    assert_eq!(
        application_availability_capability(
            PlatformPrivacyApplicationTraceAvailability::PermissionRequired
        ),
        Some(PrivacyCapabilityState::PermissionRequired)
    );
    assert_eq!(
        application_availability_capability(
            PlatformPrivacyApplicationTraceAvailability::Unavailable
        ),
        Some(PrivacyCapabilityState::Unavailable)
    );
    assert_eq!(
        application_availability_capability(PlatformPrivacyApplicationTraceAvailability::Available),
        None
    );
}

#[test]
fn candidate_tokens_never_include_source_labels() {
    let token = candidate_token(
        "nonce",
        "browser",
        Some("Profile 1"),
        PrivacyDataKind::BrowsingHistory,
    );
    assert_eq!(token.len(), 64);
    assert!(!token.contains("Profile"));
}

#[test]
fn detail_pages_are_read_only_and_bound_to_the_current_scan() {
    let _test_guard = test_operation_lock();
    let directory = fixture_directory("details");
    fs::write(directory.join("private-entry.db"), b"private").unwrap();
    let (scan_id, token) = install_directory_scan_session(&directory);

    let page = PrivacyService::details(PrivacyDetailsRequest {
        scan_id: scan_id.clone(),
        token: token.clone(),
        offset: 0,
        limit: 20,
    })
    .unwrap();
    assert_eq!(page.presentation, PrivacyDetailsPresentation::List);
    assert_eq!(page.entries.len(), 1);
    assert_eq!(
        page.entries[0].label,
        directory.join("private-entry.db").to_string_lossy()
    );
    assert_eq!(page.entries[0].item_count, 1);
    assert!(directory.join("private-entry.db").is_file());

    fs::write(directory.join("new-private-entry.db"), b"new private data").unwrap();
    let changed = PrivacyService::details(PrivacyDetailsRequest {
        scan_id: scan_id.clone(),
        token: token.clone(),
        offset: 0,
        limit: 20,
    })
    .unwrap();
    assert_eq!(changed.entries.len(), 2);
    assert!(changed
        .entries
        .iter()
        .any(|entry| entry.label.ends_with("new-private-entry.db")));
    assert!(directory.join("new-private-entry.db").is_file());

    let error = PrivacyService::details(PrivacyDetailsRequest {
        scan_id: "expired-scan".into(),
        token,
        offset: 0,
        limit: 20,
    })
    .unwrap_err();
    assert_eq!(error.code(), CoreErrorCode::InvalidInput);
    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn combined_system_fingerprint_covers_the_native_companion() {
    let files_only = system_trace_fingerprint("files", None);
    let first_native = system_trace_fingerprint("files", Some("native-a"));
    let second_native = system_trace_fingerprint("files", Some("native-b"));

    assert_ne!(files_only, first_native);
    assert_ne!(first_native, second_native);
}

#[test]
fn directory_summary_detects_a_same_size_file_rewrite() {
    let directory = fixture_directory("same-size-rewrite");
    let path = directory.join("history");
    fs::write(&path, b"first").unwrap();
    fs::File::options()
        .write(true)
        .open(&path)
        .unwrap()
        .set_times(
            fs::FileTimes::new().set_modified(UNIX_EPOCH + std::time::Duration::from_secs(10)),
        )
        .unwrap();
    let before = roots_summary_fingerprint(
        std::slice::from_ref(&directory),
        &PlatformCancellation::new(|| false),
    )
    .unwrap()
    .0;

    fs::write(&path, b"other").unwrap();
    fs::File::options()
        .write(true)
        .open(&path)
        .unwrap()
        .set_times(
            fs::FileTimes::new().set_modified(UNIX_EPOCH + std::time::Duration::from_secs(20)),
        )
        .unwrap();
    let after = roots_summary_fingerprint(
        std::slice::from_ref(&directory),
        &PlatformCancellation::new(|| false),
    )
    .unwrap()
    .0;

    assert_ne!(before, after);
    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn shell_history_details_decode_zsh_metadata_and_non_utf8_records() {
    let directory = fixture_directory("shell-details");
    let history = directory.join(".zsh_history");
    fs::write(
        &history,
        b": 1788330000:4;git status\n: 1788330001:0;printf '\xff'\n",
    )
    .unwrap();

    let first_page = text_record_detail_entries(std::slice::from_ref(&history), 0, 1).unwrap();
    let second_page = text_record_detail_entries(&[history], 1, 1).unwrap();

    assert_eq!(first_page.len(), 1);
    assert_eq!(first_page[0].label, "zsh · git status");
    assert_eq!(second_page.len(), 1);
    assert!(second_page[0].label.starts_with("zsh · printf"));
    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn recent_document_details_include_only_shortcuts() {
    let directory = fixture_directory("recent-document-details");
    let shortcut = directory.join("document.lnk");
    fs::write(&shortcut, b"shortcut").unwrap();
    fs::write(directory.join("desktop.ini"), b"metadata").unwrap();

    let entries = system_root_detail_entries(
        PlatformPrivacySystemTraceKind::RecentDocumentHistory,
        std::slice::from_ref(&directory),
        0,
        20,
    )
    .unwrap();

    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].label, shortcut.to_string_lossy());
    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn application_privacy_policy_separates_regenerable_and_user_content_traces() {
    let application_cache = privacy_item(PrivacyItemInput {
        token: "cache".into(),
        source_id: "vscode".into(),
        source_name: "Visual Studio Code".into(),
        profile_id: None,
        profile_name: None,
        kind: PrivacyDataKind::ApplicationCache,
        capability: PrivacyCapabilityState::Ready,
        item_count: 3,
        estimated_bytes: 64,
        requires_browser_close: true,
    });
    let editor_history = privacy_item(PrivacyItemInput {
        token: "history".into(),
        source_id: "vscode".into(),
        source_name: "Visual Studio Code".into(),
        profile_id: None,
        profile_name: None,
        kind: PrivacyDataKind::EditorLocalHistory,
        capability: PrivacyCapabilityState::Ready,
        item_count: 2,
        estimated_bytes: 32,
        requires_browser_close: true,
    });

    assert_eq!(
        application_cache.category,
        PrivacyCategory::ApplicationActivity
    );
    assert_eq!(application_cache.impact, PrivacyImpact::Low);
    assert_eq!(
        application_cache.recommendation,
        PrivacyRecommendation::Recommended
    );
    assert!(application_cache.selected_by_default);
    assert_eq!(
        editor_history.category,
        PrivacyCategory::ApplicationActivity
    );
    assert_eq!(editor_history.impact, PrivacyImpact::DataLoss);
    assert_eq!(editor_history.recommendation, PrivacyRecommendation::Manual);
    assert!(!editor_history.selected_by_default);
}

#[test]
fn high_impact_items_are_never_selected_by_default() {
    let item = privacy_item(PrivacyItemInput {
        token: "token".into(),
        source_id: "browser".into(),
        source_name: "Browser".into(),
        profile_id: None,
        profile_name: None,
        kind: PrivacyDataKind::Cookies,
        capability: PrivacyCapabilityState::Ready,
        item_count: 4,
        estimated_bytes: 10,
        requires_browser_close: false,
    });
    assert!(!item.selected_by_default);
    assert_eq!(item.impact, PrivacyImpact::SignOut);
}

#[test]
fn low_impact_browser_activity_is_recommended_by_default() {
    for (kind, recommendation, selected_by_default) in [
        (
            PrivacyDataKind::BrowsingHistory,
            PrivacyRecommendation::Recommended,
            true,
        ),
        (
            PrivacyDataKind::DownloadHistory,
            PrivacyRecommendation::Recommended,
            true,
        ),
        (
            PrivacyDataKind::Sessions,
            PrivacyRecommendation::Recommended,
            true,
        ),
        (
            PrivacyDataKind::BrowserCache,
            PrivacyRecommendation::Recommended,
            true,
        ),
        (
            PrivacyDataKind::AddressBarShortcuts,
            PrivacyRecommendation::Recommended,
            true,
        ),
        (
            PrivacyDataKind::SearchHistory,
            PrivacyRecommendation::Recommended,
            true,
        ),
        (
            PrivacyDataKind::WebsiteIcons,
            PrivacyRecommendation::Recommended,
            true,
        ),
    ] {
        let item = privacy_item(PrivacyItemInput {
            token: "token".into(),
            source_id: "browser".into(),
            source_name: "Browser".into(),
            profile_id: Some("browser:Default".into()),
            profile_name: Some("Default".into()),
            kind,
            capability: PrivacyCapabilityState::Ready,
            item_count: 4,
            estimated_bytes: 10,
            requires_browser_close: true,
        });
        assert_eq!(item.impact, PrivacyImpact::Low);
        assert_eq!(item.recommendation, recommendation);
        assert_eq!(item.selected_by_default, selected_by_default);
    }
}

#[test]
fn known_low_impact_traces_remain_recommended_while_the_owner_is_running() {
    let item = privacy_item(PrivacyItemInput {
        token: "token".into(),
        source_id: "browser".into(),
        source_name: "Browser".into(),
        profile_id: Some("browser:Default".into()),
        profile_name: Some("Default".into()),
        kind: PrivacyDataKind::BrowsingHistory,
        capability: PrivacyCapabilityState::BrowserRunning,
        item_count: 4,
        estimated_bytes: 10,
        requires_browser_close: true,
    });

    assert_eq!(item.recommendation, PrivacyRecommendation::Recommended);
    assert!(item.selected_by_default);
}

#[test]
fn system_trace_policy_distinguishes_regenerable_and_workflow_data() {
    for (kind, impact, recommendation, selected_by_default) in [
        (
            PrivacyDataKind::JumpLists,
            PrivacyImpact::Workflow,
            PrivacyRecommendation::Manual,
            false,
        ),
        (
            PrivacyDataKind::ShellHistory,
            PrivacyImpact::Workflow,
            PrivacyRecommendation::Manual,
            false,
        ),
        (
            PrivacyDataKind::ApplicationUsageHistory,
            PrivacyImpact::Low,
            PrivacyRecommendation::Recommended,
            true,
        ),
        (
            PrivacyDataKind::NetworkConnectionHistory,
            PrivacyImpact::Low,
            PrivacyRecommendation::Recommended,
            true,
        ),
        (
            PrivacyDataKind::FolderViewHistory,
            PrivacyImpact::Workflow,
            PrivacyRecommendation::Manual,
            false,
        ),
    ] {
        let item = privacy_item(PrivacyItemInput {
            token: "token".into(),
            source_id: "system".into(),
            source_name: "System".into(),
            profile_id: None,
            profile_name: None,
            kind,
            capability: PrivacyCapabilityState::Ready,
            item_count: 4,
            estimated_bytes: 10,
            requires_browser_close: false,
        });
        assert_eq!(item.category, PrivacyCategory::SystemActivity);
        assert_eq!(item.impact, impact);
        assert_eq!(item.recommendation, recommendation);
        assert_eq!(item.selected_by_default, selected_by_default);
    }
}

#[test]
fn recent_application_searches_are_low_risk_and_selected_by_default() {
    let item = privacy_item(PrivacyItemInput {
        token: "searches".into(),
        source_id: "word".into(),
        source_name: "Microsoft Word".into(),
        profile_id: None,
        profile_name: None,
        kind: PrivacyDataKind::RecentSearches,
        capability: PrivacyCapabilityState::Ready,
        item_count: 3,
        estimated_bytes: 0,
        requires_browser_close: true,
    });

    assert_eq!(item.category, PrivacyCategory::ApplicationActivity);
    assert_eq!(item.impact, PrivacyImpact::Low);
    assert_eq!(item.recommendation, PrivacyRecommendation::Recommended);
    assert!(item.selected_by_default);
}

#[test]
fn personal_browser_data_is_user_selectable_but_never_selected_by_default() {
    for kind in [
        PrivacyDataKind::SavedPasswords,
        PrivacyDataKind::AutofillData,
    ] {
        let item = privacy_item(PrivacyItemInput {
            token: "token".into(),
            source_id: "browser".into(),
            source_name: "Browser".into(),
            profile_id: Some("browser:Default".into()),
            profile_name: Some("Default".into()),
            kind,
            capability: PrivacyCapabilityState::Ready,
            item_count: 4,
            estimated_bytes: 10,
            requires_browser_close: true,
        });
        assert_eq!(item.recommendation, PrivacyRecommendation::Manual);
        assert_eq!(item.impact, PrivacyImpact::DataLoss);
        assert!(!item.selected_by_default);
    }
}

#[test]
fn browser_close_requirements_group_running_sources_without_paths() {
    let candidate =
        |token: &str, source_id: &str, source_name: &str, capability| NativePrivacyCandidate {
            token: token.into(),
            item: privacy_item(PrivacyItemInput {
                token: token.into(),
                source_id: source_id.into(),
                source_name: source_name.into(),
                profile_id: Some(format!("{source_id}:profile")),
                profile_name: Some("Profile".into()),
                kind: PrivacyDataKind::Sessions,
                capability,
                item_count: 1,
                estimated_bytes: 1,
                requires_browser_close: true,
            }),
            fingerprint: "fingerprint".into(),
            action: NativePrivacyAction::Directories { roots: Vec::new() },
            browser_process_names: vec!["Browser.exe".into(), "browser.exe".into()],
        };
    let requirements = browser_close_requirements(&[
        candidate(
            "one",
            "browser",
            "Browser",
            PrivacyCapabilityState::BrowserRunning,
        ),
        candidate(
            "two",
            "browser",
            "Browser",
            PrivacyCapabilityState::BrowserRunning,
        ),
        candidate(
            "ready",
            "ready-browser",
            "Ready browser",
            PrivacyCapabilityState::Ready,
        ),
    ]);

    assert_eq!(requirements.len(), 1);
    assert_eq!(requirements[0].source_id, "browser");
    assert_eq!(requirements[0].source_name, "Browser");
    assert_eq!(requirements[0].processes, vec!["Browser.exe"]);
}

#[test]
fn browser_process_requests_remain_bounded_to_the_pending_plan() {
    let _test_guard = test_operation_lock();
    let plan_id = "p".repeat(64);
    let candidate = NativePrivacyCandidate {
        token: "token".into(),
        item: privacy_item(PrivacyItemInput {
            token: "token".into(),
            source_id: "browser".into(),
            source_name: "Browser".into(),
            profile_id: Some("browser:profile".into()),
            profile_name: Some("Profile".into()),
            kind: PrivacyDataKind::Sessions,
            capability: PrivacyCapabilityState::BrowserRunning,
            item_count: 1,
            estimated_bytes: 1,
            requires_browser_close: true,
        }),
        fingerprint: "fingerprint".into(),
        action: NativePrivacyAction::Directories { roots: Vec::new() },
        browser_process_names: vec!["mangodisk-status-fixture-never-running.exe".into()],
    };
    replace_pending_plan(PendingPrivacyPlan {
        #[cfg(windows)]
        browser_application_paths: BTreeMap::new(),
        public_plan: PrivacyExecutionPlan {
            schema_version: PRIVACY_PLAN_SCHEMA_VERSION,
            plan_id: plan_id.clone(),
            scan_id: "scan".into(),
            created_at_ms: crate::shared::clock::unix_time_ms(),
            expires_at_ms: crate::shared::clock::unix_time_ms().saturating_add(60_000),
            items: Vec::new(),
            browser_close_requirements: browser_close_requirements(std::slice::from_ref(
                &candidate,
            )),
            requires_confirmation: true,
            requires_browser_close: true,
        },
        candidates: vec![candidate],
        time_range: PrivacyTimeRange::AllTime,
    })
    .unwrap();

    let error = PrivacyService::close_browsers(PrivacyBrowserCloseRequest {
        plan_id,
        source_ids: vec!["untrusted".into()],
        mode: crate::ApplicationCloseMode::Graceful,
    })
    .unwrap_err();
    assert_eq!(error.code(), CoreErrorCode::InvalidInput);

    let status = PrivacyService::refresh_browser_status(PrivacyBrowserStatusRequest {
        plan_id: "p".repeat(64),
        source_ids: vec!["browser".into()],
    })
    .unwrap();
    assert_eq!(status.running_process_count, 0);
    assert_eq!(status.targets.len(), 1);
    assert!(status.targets[0].running_processes.is_empty());

    let error = PrivacyService::refresh_browser_status(PrivacyBrowserStatusRequest {
        plan_id: "p".repeat(64),
        source_ids: vec!["untrusted".into()],
    })
    .unwrap_err();
    assert_eq!(error.code(), CoreErrorCode::InvalidInput);
    clear_pending_plan().unwrap();
}

#[test]
fn running_browser_drift_can_prepare_a_close_gated_plan() {
    let _test_guard = test_operation_lock();
    let directory = fixture_directory("running-browser-plan");
    let changing_file = directory.join("session");
    fs::write(&changing_file, b"before").unwrap();
    let (scan_id, token) = install_directory_scan_session(&directory);
    let mut session = current_scan_session().unwrap();
    let candidate = session.candidates.get_mut(&token).unwrap();
    candidate.item.capability = PrivacyCapabilityState::BrowserRunning;
    candidate.browser_process_names = vec!["synthetic-browser".into()];
    session.public_result.items[0].capability = PrivacyCapabilityState::BrowserRunning;
    replace_scan_session(session).unwrap();
    fs::write(&changing_file, b"changed-while-running").unwrap();

    let plan = PrivacyService::prepare(PrivacyExecutionRequest {
        scan_id,
        tokens: vec![token],
    })
    .unwrap();
    assert!(plan.requires_browser_close);
    assert_eq!(plan.browser_close_requirements.len(), 1);

    clear_scan_session().unwrap();
    clear_pending_plan().unwrap();
    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn review_only_credential_evidence_cannot_enter_an_execution_plan() {
    let _test_guard = test_operation_lock();
    let directory = fixture_directory("review-only-plan");
    fs::write(directory.join("evidence"), b"aggregate-only").unwrap();
    let (scan_id, token) = install_directory_scan_session(&directory);
    let mut session = current_scan_session().unwrap();
    let candidate = session.candidates.get_mut(&token).unwrap();
    candidate.item.recommendation = PrivacyRecommendation::ReviewOnly;
    session.public_result.items[0].recommendation = PrivacyRecommendation::ReviewOnly;
    replace_scan_session(session).unwrap();

    let error = PrivacyService::prepare(PrivacyExecutionRequest {
        scan_id,
        tokens: vec![token],
    })
    .unwrap_err();
    assert_eq!(error.code(), CoreErrorCode::InvalidInput);

    clear_scan_session().unwrap();
    clear_pending_plan().unwrap();
    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn partial_cancellation_is_recorded_as_completed_with_warnings() {
    let history_item = |status| PrivacyCleanupHistoryItem {
        source_id: "synthetic".into(),
        kind: PrivacyDataKind::RecentItems,
        affected_item_count: 0,
        status,
        failure_reason: None,
    };
    let partial = vec![
        history_item(PrivacyCleanupHistoryItemStatus::Cleared),
        history_item(PrivacyCleanupHistoryItemStatus::Cancelled),
    ];
    assert_eq!(
        privacy_history_outcome(&partial),
        OperationOutcome::CompletedWithWarnings
    );
    assert_eq!(
        privacy_history_outcome(&[history_item(PrivacyCleanupHistoryItemStatus::Cancelled)]),
        OperationOutcome::Cancelled
    );
}

#[test]
fn database_candidate_fingerprint_is_captured_after_wal_read() {
    let directory = fixture_directory("database-wal-fingerprint");
    let path = directory.join("places.sqlite");
    let connection = rusqlite::Connection::open(&path).unwrap();
    connection
        .execute_batch(
            "PRAGMA journal_mode=WAL;
                 CREATE TABLE moz_places(
                     id INTEGER PRIMARY KEY,
                     url TEXT,
                     foreign_count INTEGER NOT NULL DEFAULT 0
                 );
                 CREATE TABLE moz_historyvisits(
                     id INTEGER PRIMARY KEY,
                     place_id INTEGER,
                     visit_type INTEGER,
                     visit_date INTEGER
                 );
                 INSERT INTO moz_places VALUES (1, 'https://fixture.invalid', 0);
                 INSERT INTO moz_historyvisits VALUES (1, 1, 1, 1);",
        )
        .unwrap();
    let mut candidates = BTreeMap::new();
    let mut items = Vec::new();

    add_database_candidate(
        "nonce",
        "firefox",
        "Firefox",
        "firefox:test-profile",
        "Test profile",
        &path,
        PlatformPrivacyBrowserKind::Firefox,
        PrivacyDataKind::BrowsingHistory,
        PrivacyTimeRange::AllTime,
        1,
        false,
        &[],
        &mut candidates,
        &mut items,
    );

    connection
        .execute(
            "INSERT INTO moz_places VALUES (2, 'https://later-query.invalid', 0)",
            [],
        )
        .unwrap();
    assert_ne!(
        candidates.values().next().unwrap().fingerprint,
        file_fingerprint(&path).unwrap()
    );
    refresh_database_fingerprints(&mut candidates).unwrap();
    assert!(candidates
        .values()
        .all(|candidate| candidate.fingerprint == file_fingerprint(&path).unwrap()));
    drop(connection);
    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn running_browser_database_is_counted_but_remains_close_gated() {
    let directory = fixture_directory("running-database-scan");
    let path = directory.join("History");
    let connection = rusqlite::Connection::open(&path).unwrap();
    connection
        .execute_batch(
            "PRAGMA journal_mode=WAL;
                 CREATE TABLE urls(id INTEGER PRIMARY KEY, url TEXT);
                 CREATE TABLE visits(id INTEGER PRIMARY KEY, url INTEGER, visit_time INTEGER);
                 INSERT INTO urls VALUES (1, 'https://fixture.invalid');
                 INSERT INTO visits VALUES (1, 1, 1);",
        )
        .unwrap();
    let mut candidates = BTreeMap::new();
    let mut items = Vec::new();

    add_database_candidate(
        "nonce",
        "chrome",
        "Google Chrome",
        "chrome:work",
        "Work",
        &path,
        PlatformPrivacyBrowserKind::Chromium,
        PrivacyDataKind::BrowsingHistory,
        PrivacyTimeRange::AllTime,
        1,
        true,
        &["Google Chrome".into()],
        &mut candidates,
        &mut items,
    );

    assert_eq!(items[0].item_count, 1);
    assert_eq!(items[0].capability, PrivacyCapabilityState::BrowserRunning);
    assert!(items[0].requires_browser_close);
    assert_eq!(candidates.len(), 1);
    drop(connection);
    fs::remove_dir_all(directory).unwrap();
}

#[cfg(windows)]
#[test]
fn locked_windows_cookie_database_remains_selectable_for_close_and_refresh() {
    use std::os::windows::fs::OpenOptionsExt;

    let directory = fixture_directory("locked-windows-cookie");
    let path = directory.join("Cookies");
    fs::write(&path, b"locked synthetic cookie database").unwrap();
    let locked = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .share_mode(0)
        .open(&path)
        .unwrap();
    let mut candidates = BTreeMap::new();
    let mut items = Vec::new();

    add_database_candidate(
        "nonce",
        "chrome",
        "Google Chrome",
        "chrome:Default",
        "Default",
        &path,
        PlatformPrivacyBrowserKind::Chromium,
        PrivacyDataKind::Cookies,
        PrivacyTimeRange::AllTime,
        1,
        true,
        &["chrome.exe".into()],
        &mut candidates,
        &mut items,
    );

    assert_eq!(items.len(), 1);
    assert_eq!(items[0].capability, PrivacyCapabilityState::BrowserRunning);
    assert_eq!(items[0].item_count, 0);
    assert!(privacy_item_is_actionable(&items[0]));
    assert_eq!(candidates.len(), 1);
    drop(locked);
    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn sequential_items_sharing_one_database_refresh_the_expected_fingerprint() {
    let directory = fixture_directory("shared-database-execution");
    let path = directory.join("History");
    let connection = rusqlite::Connection::open(&path).unwrap();
    connection
        .execute_batch(
            "CREATE TABLE urls(id INTEGER PRIMARY KEY, url TEXT);
                 CREATE TABLE visits(id INTEGER PRIMARY KEY, url INTEGER, visit_time INTEGER);
                 CREATE TABLE downloads(id INTEGER PRIMARY KEY, start_time INTEGER);
                 CREATE TABLE downloads_url_chains(id INTEGER, chain_index INTEGER, url TEXT);
                 INSERT INTO urls VALUES (1, 'https://fixture.invalid');
                 INSERT INTO visits VALUES (1, 1, 1);
                 INSERT INTO downloads VALUES (1, 1);
                 INSERT INTO downloads_url_chains VALUES (1, 0, 'https://download.invalid');",
        )
        .unwrap();
    drop(connection);
    let fingerprint = file_fingerprint(&path).unwrap();
    let candidate = |token: &str, kind| NativePrivacyCandidate {
        token: token.into(),
        item: privacy_item(PrivacyItemInput {
            token: token.into(),
            source_id: "chromium".into(),
            source_name: "Chromium".into(),
            profile_id: Some("chromium:test-profile".into()),
            profile_name: Some("Test profile".into()),
            kind,
            capability: PrivacyCapabilityState::Ready,
            item_count: 1,
            estimated_bytes: 1,
            requires_browser_close: true,
        }),
        fingerprint: fingerprint.clone(),
        action: NativePrivacyAction::Database {
            path: path.clone(),
            browser: PlatformPrivacyBrowserKind::Chromium,
            kind,
            range: PrivacyTimeRange::AllTime,
            scan_now_ms: 0,
        },
        browser_process_names: Vec::new(),
    };
    let history = candidate("history", PrivacyDataKind::BrowsingHistory);
    let downloads = candidate("downloads", PrivacyDataKind::DownloadHistory);
    let mut expectations = database_fingerprint_expectations(&[history.clone(), downloads.clone()]);

    assert_eq!(execute_candidate(&history).unwrap(), 1);
    assert!(!fingerprint_matches_expectation(&downloads, &expectations));
    refresh_database_fingerprint_expectation(&history, &mut expectations);
    assert!(fingerprint_matches_expectation(&downloads, &expectations));
    assert_eq!(execute_candidate(&downloads).unwrap(), 1);

    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn recent_items_cleanup_removes_only_shortcuts() {
    let directory = fixture_directory("recent-items");
    let target = directory.join("important-document.txt");
    let shortcut = directory.join("important-document.lnk");
    let unrelated = directory.join("desktop.ini");
    fs::write(&target, b"protected-target").unwrap();
    fs::write(&shortcut, b"synthetic-shortcut").unwrap();
    fs::write(&unrelated, b"protected-metadata").unwrap();

    assert_eq!(clear_recent_items(&directory).unwrap(), 1);
    assert_eq!(fs::read(&target).unwrap(), b"protected-target");
    assert_eq!(fs::read(&unrelated).unwrap(), b"protected-metadata");
    assert!(!shortcut.exists());
    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn directory_cleanup_supports_a_single_session_file_root() {
    let directory = fixture_directory("session-file-root");
    let session_file = directory.join("LastSession.plist");
    fs::write(&session_file, b"synthetic-session").unwrap();

    assert_eq!(
        clear_directories(std::slice::from_ref(&session_file)).unwrap(),
        1
    );
    assert!(!session_file.exists());
    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn directory_cleanup_supports_multiple_fixed_history_files() {
    let directory = fixture_directory("multiple-history-files");
    let first = directory.join("first-history");
    let second = directory.join("second-history");
    fs::write(&first, b"first").unwrap();
    fs::write(&second, b"second").unwrap();

    assert_eq!(
        clear_directories(&[first.clone(), second.clone()]).unwrap(),
        2
    );
    assert!(!first.exists());
    assert!(!second.exists());
    fs::remove_dir_all(directory).unwrap();
}

#[cfg(unix)]
#[test]
fn directory_cleanup_preserves_confirmed_count_when_a_later_root_fails() {
    use std::os::unix::net::UnixListener;

    let directory = fixture_directory("partial-directory-cleanup");
    let first = directory.join("first-history");
    let unsupported = std::env::temp_dir().join(format!(
        "md-privacy-{}.socket",
        NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed)
    ));
    fs::write(&first, b"first").unwrap();
    let listener = UnixListener::bind(&unsupported).unwrap();

    let failure = clear_directories(&[first.clone(), unsupported.clone()]).unwrap_err();

    assert_eq!(failure.confirmed_affected_item_count, 1);
    assert_eq!(
        failure.error.mutation_state(),
        PlatformMutationState::MayHaveChanged
    );
    assert!(!first.exists());
    assert!(unsupported.exists());
    drop(listener);
    fs::remove_file(unsupported).unwrap();
    fs::remove_dir_all(directory).unwrap();
}

#[cfg(unix)]
#[test]
fn directory_cleanup_refuses_nested_symbolic_links() {
    use std::os::unix::fs::symlink;

    let directory = fixture_directory("directory-link");
    let protected = fixture_directory("protected-target");
    let protected_file = protected.join("keep.txt");
    fs::write(&protected_file, b"protected").unwrap();
    symlink(&protected, directory.join("nested-link")).unwrap();

    assert!(clear_directories(std::slice::from_ref(&directory)).is_err());
    assert_eq!(fs::read(&protected_file).unwrap(), b"protected");
    fs::remove_dir_all(directory).unwrap();
    fs::remove_dir_all(protected).unwrap();
}

#[cfg(unix)]
#[test]
fn directory_cleanup_refuses_a_symbolic_link_root() {
    use std::os::unix::fs::symlink;

    let directory = fixture_directory("root-link-holder");
    let protected = fixture_directory("root-link-target");
    let protected_file = protected.join("keep.txt");
    let root_link = directory.join("storage-link");
    fs::write(&protected_file, b"protected").unwrap();
    symlink(&protected, &root_link).unwrap();

    assert!(clear_directories(std::slice::from_ref(&root_link)).is_err());
    assert_eq!(fs::read(&protected_file).unwrap(), b"protected");
    fs::remove_dir_all(directory).unwrap();
    fs::remove_dir_all(protected).unwrap();
}

#[test]
fn jump_list_drift_blocks_the_complete_plan_without_mutation() {
    let _test_guard = test_operation_lock();
    let browser = fixture_directory("jump-list-browser");
    let jumps = fixture_directory("jump-list-system");
    fs::write(browser.join("cache"), b"browser traces").unwrap();
    fs::write(jumps.join("first.automaticDestinations-ms"), b"first").unwrap();
    let (scan_id, browser_token) = install_directory_scan_session(&browser);
    let mut session = current_scan_session().unwrap();
    let mut jump = session.candidates[&browser_token].clone();
    jump.token = "b".repeat(64);
    jump.item.token = jump.token.clone();
    jump.item.source_id = "jump_lists".into();
    jump.item.kind = PrivacyDataKind::JumpLists;
    jump.action = NativePrivacyAction::System {
        kind: PlatformPrivacySystemTraceKind::JumpLists,
        roots: vec![jumps.clone()],
        has_native_revision: false,
    };
    jump.fingerprint = current_fingerprint(&jump).unwrap();
    let jump_token = jump.token.clone();
    session.public_result.items.push(jump.item.clone());
    session.candidates.insert(jump_token.clone(), jump);
    replace_scan_session(session).unwrap();
    let plan = PrivacyService::prepare(PrivacyExecutionRequest {
        scan_id,
        tokens: vec![browser_token.clone(), jump_token.clone()],
    })
    .unwrap();
    fs::write(jumps.join("new.automaticDestinations-ms"), b"shell update").unwrap();
    let result = PrivacyService::execute(execution_request(plan.plan_id)).unwrap();
    assert_eq!(result.affected_item_count, 0);
    assert_eq!(result.failed_item_count, 1);
    assert_eq!(
        result
            .items
            .iter()
            .find(|item| item.token == browser_token)
            .unwrap()
            .failure_reason
            .as_deref(),
        Some("preflightBlocked")
    );
    assert_eq!(
        result
            .items
            .iter()
            .find(|item| item.token == jump_token)
            .unwrap()
            .failure_reason
            .as_deref(),
        Some("sourceChanged")
    );
    assert_eq!(fs::read(browser.join("cache")).unwrap(), b"browser traces");
    assert_eq!(
        fs::read(jumps.join("new.automaticDestinations-ms")).unwrap(),
        b"shell update"
    );
    clear_scan_session().unwrap();
    fs::remove_dir_all(browser).unwrap();
    fs::remove_dir_all(jumps).unwrap();
}

#[test]
fn unreadable_fingerprint_retains_the_native_cause_and_distinct_reason() {
    let _test_guard = test_operation_lock();
    let directory = fixture_directory("fingerprint-unavailable");
    let file = directory.join("history");
    fs::write(&file, b"history").unwrap();
    let (_, token) = install_directory_scan_session(&directory);
    let mut candidate = current_scan_session().unwrap().candidates[&token].clone();
    candidate.action = NativePrivacyAction::ReviewOnlyFile {
        path: file.clone(),
        browser: PlatformPrivacyBrowserKind::Chromium,
        kind: PrivacyDataKind::BrowsingHistory,
        range: PrivacyTimeRange::AllTime,
        scan_now_ms: 0,
    };
    let expected = current_fingerprint(&candidate).unwrap();
    fs::remove_file(file).unwrap();
    let (reason, error) = fingerprint_failure(&candidate, &expected).unwrap();
    assert_eq!(reason, "sourceUnavailable");
    assert!(error
        .to_string()
        .contains("privacy fingerprint open failed"));
    assert_eq!(error.mutation_state(), PlatformMutationState::NotAttempted);
    clear_scan_session().unwrap();
    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn plan_rejects_forged_tokens_and_stale_sessions_while_execution_blocks_source_drift() {
    let _test_guard = test_operation_lock();
    let directory = fixture_directory("plan-validation");
    fs::write(directory.join("storage"), b"original").unwrap();
    let (scan_id, token) = install_directory_scan_session(&directory);

    let forged = PrivacyService::prepare(PrivacyExecutionRequest {
        scan_id: scan_id.clone(),
        tokens: vec!["f".repeat(64)],
    })
    .unwrap_err();
    assert_eq!(forged.code(), CoreErrorCode::InvalidInput);

    fs::write(directory.join("storage"), b"changed-after-scan").unwrap();
    let changed_plan = PrivacyService::prepare(PrivacyExecutionRequest {
        scan_id: scan_id.clone(),
        tokens: vec![token.clone()],
    })
    .unwrap();
    let changed = PrivacyService::execute(execution_request(changed_plan.plan_id)).unwrap();
    assert_eq!(changed.affected_item_count, 0);
    assert_eq!(changed.failed_item_count, 1);
    assert_eq!(
        changed.items[0].failure_reason.as_deref(),
        Some("sourceChanged")
    );
    assert_eq!(
        fs::read(directory.join("storage")).unwrap(),
        b"changed-after-scan"
    );

    clear_scan_session().unwrap();
    let stale = PrivacyService::prepare(PrivacyExecutionRequest {
        scan_id,
        tokens: vec![token],
    })
    .unwrap_err();
    assert_eq!(stale.code(), CoreErrorCode::InvalidInput);
    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn mismatched_plan_identifier_cannot_execute_a_prepared_action() {
    let _test_guard = test_operation_lock();
    let directory = fixture_directory("plan-identifier");
    let protected = directory.join("storage");
    fs::write(&protected, b"protected").unwrap();
    let (scan_id, token) = install_directory_scan_session(&directory);
    PrivacyService::prepare(PrivacyExecutionRequest {
        scan_id,
        tokens: vec![token],
    })
    .unwrap();

    let error = PrivacyService::execute(execution_request("f".repeat(64))).unwrap_err();
    assert_eq!(error.code(), CoreErrorCode::InvalidInput);
    assert_eq!(fs::read(&protected).unwrap(), b"protected");
    clear_scan_session().unwrap();
    clear_pending_plan().unwrap();
    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn verified_execution_reconciles_the_scan_without_a_second_scan() {
    let _test_guard = test_operation_lock();
    let directory = fixture_directory("execution-scan-reconcile");
    let remaining_directory = fixture_directory("execution-scan-remaining");
    fs::write(directory.join("storage"), b"privacy trace").unwrap();
    fs::write(remaining_directory.join("history"), b"remaining trace").unwrap();
    let (scan_id, token) = install_directory_scan_session(&directory);
    let remaining_token = "b".repeat(64);
    let remaining_roots = vec![remaining_directory.clone()];
    let remaining_item = privacy_item(PrivacyItemInput {
        token: remaining_token.clone(),
        source_id: "synthetic".into(),
        source_name: "Synthetic browser".into(),
        profile_id: Some("synthetic:profile".into()),
        profile_name: Some("Test profile".into()),
        kind: PrivacyDataKind::DownloadHistory,
        capability: PrivacyCapabilityState::Ready,
        item_count: 1,
        estimated_bytes: 1,
        requires_browser_close: false,
    });
    let mut original_session = current_scan_session().unwrap();
    original_session
        .public_result
        .items
        .push(remaining_item.clone());
    original_session.candidates.insert(
        remaining_token.clone(),
        NativePrivacyCandidate {
            token: remaining_token.clone(),
            item: remaining_item,
            fingerprint: roots_summary_fingerprint(
                &remaining_roots,
                &PlatformCancellation::new(|| false),
            )
            .unwrap()
            .0,
            action: NativePrivacyAction::Directories {
                roots: remaining_roots,
            },
            browser_process_names: Vec::new(),
        },
    );
    original_session.public_result.revision = revision_for(&original_session.candidates);
    replace_scan_session(original_session).unwrap();
    let plan = PrivacyService::prepare(PrivacyExecutionRequest {
        scan_id,
        tokens: vec![token.clone()],
    })
    .unwrap();

    let mut progress_updates = Vec::new();
    let result =
        PrivacyService::execute_with_progress(execution_request(plan.plan_id), |progress| {
            progress_updates.push(progress);
        })
        .unwrap();
    let reconciled = result
        .scan
        .expect("verified execution must return an updated scan snapshot");

    assert_eq!(
        progress_updates.first().unwrap().stage,
        PrivacyExecutionStage::Validating
    );
    assert_eq!(progress_updates.first().unwrap().total_item_count, 1);
    assert!(progress_updates
        .iter()
        .any(|progress| progress.stage == PrivacyExecutionStage::Cleaning));
    let final_progress = progress_updates.last().unwrap();
    assert_eq!(final_progress.stage, PrivacyExecutionStage::Finalizing);
    assert_eq!(final_progress.completed_item_count, 1);
    assert_eq!(final_progress.affected_item_count, 1);
    assert_eq!(final_progress.completed_items.len(), 1);
    assert_eq!(final_progress.completed_items[0].token, token);
    assert_eq!(
        final_progress.completed_items[0].status,
        PrivacyExecutionItemStatus::Cleared
    );
    assert_eq!(result.items[0].status, PrivacyExecutionItemStatus::Cleared);
    assert!(result.items[0].verified);
    assert_eq!(reconciled.items[0].token, token);
    assert_eq!(reconciled.items[0].item_count, 0);
    assert_eq!(reconciled.items[0].estimated_bytes, 0);
    assert_eq!(
        reconciled.items[0].capability,
        PrivacyCapabilityState::Empty
    );
    assert!(!reconciled.items[0].selected_by_default);
    let published = current_scan_session().unwrap().public_result;
    assert_eq!(published.scan_id, reconciled.scan_id);
    assert_eq!(published.revision, reconciled.revision);
    assert_eq!(published.items[0].item_count, 0);
    let remaining_plan = PrivacyService::prepare(PrivacyExecutionRequest {
        scan_id: reconciled.scan_id,
        tokens: vec![remaining_token],
    })
    .expect("an untouched item must remain actionable without a second scan");
    assert_eq!(remaining_plan.items.len(), 1);

    clear_scan_session().unwrap();
    clear_pending_plan().unwrap();
    fs::remove_dir_all(directory).unwrap();
    fs::remove_dir_all(remaining_directory).unwrap();
}

#[test]
fn closed_browser_candidates_refresh_without_a_catalog_rescan() {
    let _test_guard = test_operation_lock();
    let directory = fixture_directory("closed-browser-refresh");
    fs::write(directory.join("original"), b"privacy trace").unwrap();
    let (scan_id, token) = install_directory_scan_session(&directory);
    let mut session = current_scan_session().unwrap();
    let candidate = session.candidates.get_mut(&token).unwrap();
    candidate.item.capability = PrivacyCapabilityState::BrowserRunning;
    candidate.item.requires_browser_close = true;
    candidate.browser_process_names = vec!["mangodisk-browser-not-running".into()];
    session.public_result.items[0] = candidate.item.clone();
    replace_scan_session(session).unwrap();
    let plan = PrivacyService::prepare(PrivacyExecutionRequest {
        scan_id,
        tokens: vec![token],
    })
    .unwrap();

    // Simulate the browser flushing one final record while it exits. Execution refreshes only
    // this already-authorized candidate instead of rebuilding the complete privacy catalog.
    fs::write(directory.join("flushed"), b"privacy trace").unwrap();
    let result = PrivacyService::execute(execution_request(plan.plan_id)).unwrap();

    assert_eq!(result.failed_item_count, 0);
    assert_eq!(result.items.len(), 1);
    assert!(result.items[0].verified);
    assert_eq!(fs::read_dir(&directory).unwrap().count(), 0);
    fs::remove_dir_all(&directory).unwrap();
    clear_scan_session().unwrap();
    clear_pending_plan().unwrap();
}

#[test]
fn excluding_all_sources_skips_execution_and_preserves_the_scan() {
    let _test_guard = test_operation_lock();
    let directory = fixture_directory("all-sources-excluded");
    let file = directory.join("history");
    fs::write(&file, b"privacy trace").unwrap();
    let (scan_id, token) = install_directory_scan_session(&directory);
    let mut session = current_scan_session().unwrap();
    let candidate = session.candidates.get_mut(&token).unwrap();
    candidate.item.capability = PrivacyCapabilityState::BrowserRunning;
    candidate.item.requires_browser_close = true;
    candidate.browser_process_names = vec!["mangodisk-browser-not-running".into()];
    session.public_result.items[0] = candidate.item.clone();
    let revision = session.public_result.revision.clone();
    replace_scan_session(session).unwrap();
    let plan = PrivacyService::prepare(PrivacyExecutionRequest {
        scan_id: scan_id.clone(),
        tokens: vec![token.clone()],
    })
    .unwrap();
    let request = PrivacyExecutionRunRequest {
        plan_id: plan.plan_id,
        excluded_source_ids: vec!["synthetic".into()],
    };
    let mut progress = Vec::new();
    let result = PrivacyService::execute_with_progress(request.clone(), |value| {
        progress.push(value);
    })
    .unwrap();

    assert!(result.items.is_empty());
    assert_eq!(result.affected_item_count, 0);
    assert_eq!(result.failed_item_count, 0);
    let scan = result.scan.unwrap();
    assert_eq!(scan.scan_id, scan_id);
    assert_eq!(scan.revision, revision);
    assert_eq!(scan.items[0].item_count, 1);
    assert_eq!(
        progress.len(),
        1,
        "an empty selection must skip validation and cleaning"
    );
    assert_eq!(progress[0].stage, PrivacyExecutionStage::Finalizing);
    assert_eq!(progress[0].total_item_count, 0);
    assert_eq!(fs::read(&file).unwrap(), b"privacy trace");
    assert!(
        PrivacyService::execute(request).is_err(),
        "a skipped plan must still be consumed"
    );
    assert_eq!(
        PrivacyService::prepare(PrivacyExecutionRequest {
            scan_id,
            tokens: vec![token]
        })
        .unwrap()
        .items
        .len(),
        1,
        "excluded sources must remain available for a later confirmation"
    );
    clear_scan_session().unwrap();
    clear_pending_plan().unwrap();
    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn partial_source_exclusion_records_only_the_executed_selection() {
    let _test_guard = test_operation_lock();
    let directory = fixture_directory("partial-exclusion");
    let excluded_root = directory.join("excluded");
    let retained_root = directory.join("retained");
    fs::create_dir_all(&excluded_root).unwrap();
    fs::create_dir_all(&retained_root).unwrap();
    fs::write(excluded_root.join("keep"), b"excluded trace").unwrap();
    fs::write(retained_root.join("clear"), b"authorized trace").unwrap();
    let (scan_id, excluded_token) = install_directory_scan_session(&excluded_root);
    let mut session = current_scan_session().unwrap();
    let excluded = session.candidates.get_mut(&excluded_token).unwrap();
    excluded.item.capability = PrivacyCapabilityState::BrowserRunning;
    excluded.item.requires_browser_close = true;
    excluded.browser_process_names = vec!["mangodisk-browser-not-running".into()];
    session.public_result.items[0] = excluded.item.clone();
    let mut retained = excluded.clone();
    retained.token = "b".repeat(64);
    retained.item.token = retained.token.clone();
    retained.item.source_id = "retained".into();
    retained.item.capability = PrivacyCapabilityState::Ready;
    retained.item.requires_browser_close = false;
    retained.browser_process_names.clear();
    retained.action = NativePrivacyAction::Directories {
        roots: vec![retained_root.clone()],
    };
    retained.fingerprint = current_fingerprint(&retained).unwrap();
    let retained_token = retained.token.clone();
    session.public_result.items.push(retained.item.clone());
    session.candidates.insert(retained_token.clone(), retained);
    replace_scan_session(session).unwrap();
    let plan = PrivacyService::prepare(PrivacyExecutionRequest {
        scan_id,
        tokens: vec![excluded_token.clone(), retained_token.clone()],
    })
    .unwrap();
    let plan_id = plan.plan_id.clone();
    let result = PrivacyService::execute(PrivacyExecutionRunRequest {
        plan_id: plan.plan_id,
        excluded_source_ids: vec!["synthetic".into()],
    })
    .unwrap();

    assert_eq!(result.items.len(), 1);
    assert_eq!(result.items[0].token, retained_token);
    assert_eq!(result.items[0].status, PrivacyExecutionItemStatus::Cleared);
    assert_eq!(result.affected_item_count, 1);
    assert_eq!(fs::read_dir(&retained_root).unwrap().count(), 0);
    assert_eq!(
        fs::read(excluded_root.join("keep")).unwrap(),
        b"excluded trace"
    );
    let scan = result.scan.unwrap();
    assert_eq!(
        scan.items
            .iter()
            .find(|item| item.token == excluded_token)
            .unwrap()
            .item_count,
        1
    );
    let record = HistoryService::list()
        .unwrap()
        .into_iter()
        .find(|record| record.operation_id == format!("privacy-{plan_id}"))
        .expect("nonempty cleanup must retain its history");
    assert_eq!(record.selected_item_count, 1);
    let OperationDetails::PrivacyCleanup(details) = record.details else {
        panic!("privacy cleanup must retain typed history details");
    };
    assert_eq!(details.items.len(), 1);
    assert_eq!(details.items[0].source_id, "retained");
    clear_scan_session().unwrap();
    clear_pending_plan().unwrap();
    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn execution_exclusions_can_only_reduce_browser_close_sources() {
    let _test_guard = test_operation_lock();
    let directory = fixture_directory("browser-exclusion");
    fs::write(directory.join("history"), b"privacy trace").unwrap();
    let (scan_id, token) = install_directory_scan_session(&directory);
    let mut session = current_scan_session().unwrap();
    let candidate = session.candidates.get_mut(&token).unwrap();
    candidate.item.capability = PrivacyCapabilityState::BrowserRunning;
    candidate.item.requires_browser_close = true;
    candidate.browser_process_names = vec!["mangodisk-browser-not-running".into()];
    session.public_result.items[0] = candidate.item.clone();
    replace_scan_session(session).unwrap();
    let plan = PrivacyService::prepare(PrivacyExecutionRequest {
        scan_id,
        tokens: vec![token],
    })
    .unwrap();

    let error = PrivacyService::execute(PrivacyExecutionRunRequest {
        plan_id: plan.plan_id,
        excluded_source_ids: vec!["not-in-plan".into()],
    })
    .unwrap_err();

    assert_eq!(error.code(), CoreErrorCode::InvalidInput);
    assert!(directory.exists());
    clear_scan_session().unwrap();
    clear_pending_plan().unwrap();
    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn reconciliation_does_not_authorize_changed_unselected_data() {
    let _test_guard = test_operation_lock();
    let selected_directory = fixture_directory("reconcile-selected");
    let remaining_directory = fixture_directory("reconcile-drifted");
    fs::write(selected_directory.join("selected"), b"privacy trace").unwrap();
    fs::write(remaining_directory.join("original"), b"privacy trace").unwrap();
    let (scan_id, selected_token) = install_directory_scan_session(&selected_directory);
    let remaining_token = "c".repeat(64);
    let remaining_roots = vec![remaining_directory.clone()];
    let remaining_item = privacy_item(PrivacyItemInput {
        token: remaining_token.clone(),
        source_id: "synthetic".into(),
        source_name: "Synthetic application".into(),
        profile_id: None,
        profile_name: None,
        kind: PrivacyDataKind::RecentItems,
        capability: PrivacyCapabilityState::Ready,
        item_count: 1,
        estimated_bytes: 1,
        requires_browser_close: false,
    });
    let mut original_session = current_scan_session().unwrap();
    original_session
        .public_result
        .items
        .push(remaining_item.clone());
    original_session.candidates.insert(
        remaining_token.clone(),
        NativePrivacyCandidate {
            token: remaining_token.clone(),
            item: remaining_item,
            fingerprint: roots_summary_fingerprint(
                &remaining_roots,
                &PlatformCancellation::new(|| false),
            )
            .unwrap()
            .0,
            action: NativePrivacyAction::Directories {
                roots: remaining_roots,
            },
            browser_process_names: Vec::new(),
        },
    );
    original_session.public_result.revision = revision_for(&original_session.candidates);
    replace_scan_session(original_session).unwrap();
    let plan = PrivacyService::prepare(PrivacyExecutionRequest {
        scan_id,
        tokens: vec![selected_token],
    })
    .unwrap();

    // Simulate an unselected shared source changing while another item is being cleared.
    fs::write(remaining_directory.join("new-record"), b"new privacy trace").unwrap();
    let result = PrivacyService::execute(execution_request(plan.plan_id)).unwrap();
    let reconciled = result
        .scan
        .expect("execution must publish a reconciled scan");
    let remaining_plan = PrivacyService::prepare(PrivacyExecutionRequest {
        scan_id: reconciled.scan_id,
        tokens: vec![remaining_token.clone()],
    })
    .unwrap();
    let result = PrivacyService::execute(execution_request(remaining_plan.plan_id)).unwrap();

    assert_eq!(result.affected_item_count, 0);
    assert_eq!(result.failed_item_count, 1);
    assert_eq!(result.items[0].token, remaining_token);
    assert_eq!(
        result.items[0].failure_reason.as_deref(),
        Some("sourceChanged")
    );
    assert!(remaining_directory.join("original").is_file());
    assert!(remaining_directory.join("new-record").is_file());
    clear_scan_session().unwrap();
    clear_pending_plan().unwrap();
    fs::remove_dir_all(selected_directory).unwrap();
    fs::remove_dir_all(remaining_directory).unwrap();
}

#[test]
#[ignore = "reads the host browser and system privacy source metadata"]
fn actual_privacy_scan_exposes_only_aggregate_evidence() {
    let _test_guard = test_operation_lock();
    let progress = Mutex::new(Vec::new());
    let result = PrivacyService::scan_with_progress(
        PrivacyScanRequest {
            time_range: PrivacyTimeRange::AllTime,
        },
        |value| progress.lock().unwrap().push(value),
    )
    .expect("host privacy scan must complete");
    let progress = progress.into_inner().unwrap();
    assert_eq!(
        progress.first().map(|value| value.stage),
        Some(PrivacyScanStage::Discovering)
    );
    assert_eq!(
        progress.last().map(|value| value.stage),
        Some(PrivacyScanStage::Finalizing)
    );
    assert!(progress
        .iter()
        .any(|value| { value.stage == PrivacyScanStage::Browser && value.source_name.is_some() }));
    assert!(progress.iter().any(|value| {
        value.stage == PrivacyScanStage::Application && value.source_name.is_some()
    }));
    println!(
        "source_count={} item_count={} elapsed_ms={}",
        result.coverage.len(),
        result.items.len(),
        result.elapsed_ms
    );
    assert!(result
        .items
        .iter()
        .all(|item| item.token.len() == 64 && !item.source_name.is_empty()));
    for kind in [
        PrivacyDataKind::BrowserCache,
        PrivacyDataKind::SearchHistory,
        PrivacyDataKind::WebsiteIcons,
        PrivacyDataKind::FrequentlyVisitedSites,
        PrivacyDataKind::AddressBarShortcuts,
        PrivacyDataKind::RecentItems,
        PrivacyDataKind::NetworkConnectionHistory,
    ] {
        let rows = result.items.iter().filter(|item| item.kind == kind).count();
        let count = result
            .items
            .iter()
            .filter(|item| item.kind == kind)
            .map(|item| item.item_count)
            .sum::<u64>();
        println!("expanded_privacy_kind={kind:?} row_count={rows} trace_count={count}");
        assert!(
            rows > 0,
            "expanded source must produce a visible aggregate row"
        );
    }
    #[cfg(target_os = "macos")]
    assert!(result
        .items
        .iter()
        .any(|item| item.kind == PrivacyDataKind::RecentApplications));
    #[cfg(windows)]
    for kind in [
        PrivacyDataKind::ApplicationUsageHistory,
        PrivacyDataKind::FolderViewHistory,
        PrivacyDataKind::PrinterHistory,
    ] {
        assert!(
            result.items.iter().any(|item| item.kind == kind),
            "Windows system privacy source must expose {kind:?}"
        );
    }
    let application_sources = result
        .items
        .iter()
        .filter(|item| item.category == PrivacyCategory::ApplicationActivity)
        .map(|item| item.source_id.as_str())
        .collect::<BTreeSet<_>>();
    println!(
        "application_privacy_source_count={} application_privacy_row_count={}",
        application_sources.len(),
        result
            .items
            .iter()
            .filter(|item| item.category == PrivacyCategory::ApplicationActivity)
            .count()
    );
    assert!(
        application_sources.len() >= 4,
        "host scan must expose multiple independent application sources"
    );
    let mut application_kinds = Vec::new();
    for kind in result
        .items
        .iter()
        .filter(|item| item.category == PrivacyCategory::ApplicationActivity)
        .map(|item| item.kind)
    {
        if !application_kinds.contains(&kind) {
            application_kinds.push(kind);
        }
    }
    assert!(application_kinds.contains(&PrivacyDataKind::ApplicationCache));
    assert!(application_kinds.contains(&PrivacyDataKind::ApplicationLogs));
    assert!(
        application_kinds.len() >= 3,
        "host applications must expose multiple independent privacy trace kinds"
    );
    #[cfg(target_os = "windows")]
    for kind in [
        PrivacyDataKind::JumpLists,
        PrivacyDataKind::RunDialogHistory,
        PrivacyDataKind::FileDialogHistory,
        PrivacyDataKind::SystemSearchHistory,
        PrivacyDataKind::ExplorerPathHistory,
    ] {
        assert!(result.items.iter().any(|item| item.kind == kind));
    }
}

#[cfg(target_os = "macos")]
#[test]
#[ignore = "requires Microsoft Edge with an initialized local profile"]
fn actual_macos_edge_scan_returns_aggregate_evidence() {
    let _test_guard = test_operation_lock();
    let result = PrivacyService::scan(PrivacyScanRequest {
        time_range: PrivacyTimeRange::AllTime,
    })
    .expect("Microsoft Edge privacy scan must complete");
    let edge_items = result
        .items
        .iter()
        .filter(|item| item.source_id == "edge")
        .collect::<Vec<_>>();

    println!(
        "edge_item_count={} edge_trace_count={}",
        edge_items.len(),
        edge_items.iter().map(|item| item.item_count).sum::<u64>()
    );
    assert!(result
        .coverage
        .iter()
        .any(|source| source.source_id == "edge"));
    assert!(!edge_items.is_empty());
    assert!(edge_items.iter().any(|item| item.profile_id.is_some()));
    assert!(edge_items.iter().any(|item| item.item_count > 0));
}

#[cfg(target_os = "macos")]
#[test]
#[ignore = "requires the additional Chromium browsers with initialized local profiles"]
fn actual_macos_additional_chromium_scans_return_aggregate_evidence() {
    let _test_guard = test_operation_lock();
    let result = PrivacyService::scan(PrivacyScanRequest {
        time_range: PrivacyTimeRange::AllTime,
    })
    .expect("additional Chromium privacy scans must complete");

    for source_id in ["brave", "opera", "360_safe_browser", "qq_browser"] {
        let items = result
            .items
            .iter()
            .filter(|item| item.source_id == source_id)
            .collect::<Vec<_>>();
        println!(
            "browser_source={source_id} item_count={} trace_count={}",
            items.len(),
            items.iter().map(|item| item.item_count).sum::<u64>()
        );
        assert!(result
            .coverage
            .iter()
            .any(|source| { source.source_id == source_id && source.icon_path.is_some() }));
        assert!(!items.is_empty());
        assert!(items.iter().any(|item| item.profile_id.is_some()));
    }
}

#[cfg(target_os = "windows")]
#[test]
#[ignore = "requires the supported Windows browsers to be running with initialized profiles"]
fn actual_windows_browser_scans_return_aggregate_evidence() {
    let _test_guard = test_operation_lock();
    let result = PrivacyService::scan(PrivacyScanRequest {
        time_range: PrivacyTimeRange::AllTime,
    })
    .expect("Windows browser privacy scans must complete");

    for source_id in [
        "firefox",
        "brave",
        "opera",
        "360_safe_browser",
        "qq_browser",
    ] {
        let items = result
            .items
            .iter()
            .filter(|item| item.source_id == source_id)
            .collect::<Vec<_>>();
        println!(
            "browser_source={source_id} item_count={} trace_count={}",
            items.len(),
            items.iter().map(|item| item.item_count).sum::<u64>()
        );
        assert!(result
            .coverage
            .iter()
            .any(|source| { source.source_id == source_id && source.icon_path.is_some() }));
        assert!(!items.is_empty());
        assert!(items.iter().any(|item| item.profile_id.is_some()));
        assert!(items.iter().any(|item| {
            item.item_count > 0
                && item.capability == PrivacyCapabilityState::BrowserRunning
                && item.requires_browser_close
        }));
    }
}

#[cfg(target_os = "windows")]
#[test]
#[ignore = "requires a running Google Chrome profile with a Cookie database"]
fn actual_windows_locked_chrome_cookies_remain_selectable_for_close() {
    let _test_guard = test_operation_lock();
    let result = PrivacyService::scan(PrivacyScanRequest {
        time_range: PrivacyTimeRange::AllTime,
    })
    .expect("Windows Chrome privacy scan must complete");
    let cookies = result
        .items
        .iter()
        .filter(|item| item.source_id == "chrome" && item.kind == PrivacyDataKind::Cookies)
        .collect::<Vec<_>>();

    println!(
        "chrome_cookie_profile_count={} deferred_profile_count={}",
        cookies.len(),
        cookies
            .iter()
            .filter(|item| {
                item.capability == PrivacyCapabilityState::BrowserRunning && item.item_count == 0
            })
            .count()
    );
    assert!(!cookies.is_empty());
    assert!(cookies.iter().all(|item| {
        item.capability != PrivacyCapabilityState::Unavailable
            && (item.item_count > 0 || privacy_item_is_actionable(item))
    }));
}

#[cfg(target_os = "windows")]
#[test]
#[ignore = "closes the supported Windows browsers and rescans their initialized profiles"]
fn actual_windows_browser_close_and_rescan_clears_running_state() {
    use crate::applications::process_control::ApplicationCloseMode;

    let _test_guard = test_operation_lock();
    let source_ids = [
        "firefox",
        "brave",
        "opera",
        "360_safe_browser",
        "qq_browser",
    ];
    let scan = PrivacyService::scan(PrivacyScanRequest {
        time_range: PrivacyTimeRange::AllTime,
    })
    .expect("running Windows browser scan must complete");
    let tokens = source_ids
        .iter()
        .map(|source_id| {
            scan.items
                .iter()
                .find(|item| {
                    item.source_id == *source_id
                        && item.item_count > 0
                        && item.capability == PrivacyCapabilityState::BrowserRunning
                        && !matches!(
                            item.recommendation,
                            PrivacyRecommendation::ReviewOnly | PrivacyRecommendation::Unsupported
                        )
                })
                .unwrap_or_else(|| panic!("{source_id} must expose one close-gated item"))
                .token
                .clone()
        })
        .collect::<Vec<_>>();
    let plan = PrivacyService::prepare(PrivacyExecutionRequest {
        scan_id: scan.scan_id,
        tokens,
    })
    .expect("running browser plan must be prepared");
    let source_ids = source_ids.map(str::to_owned).to_vec();
    let graceful = PrivacyService::close_browsers(PrivacyBrowserCloseRequest {
        plan_id: plan.plan_id.clone(),
        source_ids: source_ids.clone(),
        mode: ApplicationCloseMode::Graceful,
    })
    .expect("graceful browser close must complete");
    if graceful.remaining_process_count > 0 {
        let forced = PrivacyService::close_browsers(PrivacyBrowserCloseRequest {
            plan_id: plan.plan_id,
            source_ids: source_ids.clone(),
            mode: ApplicationCloseMode::Force,
        })
        .expect("forced browser close retry must complete");
        assert_eq!(forced.remaining_process_count, 0);
    }

    let rescanned = PrivacyService::scan(PrivacyScanRequest {
        time_range: PrivacyTimeRange::AllTime,
    })
    .expect("closed Windows browser rescan must complete");
    for source_id in source_ids {
        let items = rescanned
            .items
            .iter()
            .filter(|item| item.source_id == source_id)
            .collect::<Vec<_>>();
        assert!(items.iter().any(|item| item.item_count > 0));
        assert!(items
            .iter()
            .all(|item| item.capability != PrivacyCapabilityState::BrowserRunning));
    }
}

#[test]
#[ignore = "requires a supported browser with a non-empty profile to be running"]
fn actual_running_browser_scan_reports_reviewable_counts_and_icon_source() {
    let _test_guard = test_operation_lock();
    let result = PrivacyService::scan(PrivacyScanRequest {
        time_range: PrivacyTimeRange::AllTime,
    })
    .expect("running browser privacy scan must complete");
    let running_source_ids = result
        .items
        .iter()
        .filter(|item| {
            item.capability == PrivacyCapabilityState::BrowserRunning && item.item_count > 0
        })
        .map(|item| item.source_id.as_str())
        .collect::<BTreeSet<_>>();
    assert!(!running_source_ids.is_empty());
    assert!(result.coverage.iter().any(|source| {
        running_source_ids.contains(source.source_id.as_str()) && source.icon_path.is_some()
    }));
    // This host-only check deliberately prints aggregate counts without profile names, paths,
    // URLs, or cookie contents so contributors can compare running-browser coverage safely.
    for kind in [
        PrivacyDataKind::BrowsingHistory,
        PrivacyDataKind::DownloadHistory,
        PrivacyDataKind::Cookies,
        PrivacyDataKind::BrowserCache,
        PrivacyDataKind::SearchHistory,
        PrivacyDataKind::WebsiteIcons,
        PrivacyDataKind::FrequentlyVisitedSites,
        PrivacyDataKind::AddressBarShortcuts,
        PrivacyDataKind::SavedPasswords,
        PrivacyDataKind::AutofillData,
    ] {
        let count = result
            .items
            .iter()
            .filter(|item| item.kind == kind)
            .map(|item| item.item_count)
            .sum::<u64>();
        println!("running_browser_kind={kind:?} aggregate_count={count}");
    }
}

#[test]
#[ignore = "scans installed browser profiles without modifying personal browser data"]
fn actual_personal_browser_data_is_manual_and_never_selected_by_default() {
    let _test_guard = test_operation_lock();
    let result = PrivacyService::scan(PrivacyScanRequest {
        time_range: PrivacyTimeRange::AllTime,
    })
    .expect("host privacy scan must complete");
    let personal_items = result
        .items
        .iter()
        .filter(|item| {
            matches!(
                item.kind,
                PrivacyDataKind::SavedPasswords | PrivacyDataKind::AutofillData
            )
        })
        .collect::<Vec<_>>();
    assert!(
        !personal_items.is_empty(),
        "an installed supported browser must expose personal-data coverage"
    );
    assert!(personal_items.iter().all(|item| {
        item.recommendation == PrivacyRecommendation::Manual && !item.selected_by_default
    }));
    let actionable_count = personal_items
        .iter()
        .filter(|item| {
            item.item_count > 0
                && matches!(
                    item.capability,
                    PrivacyCapabilityState::Ready | PrivacyCapabilityState::BrowserRunning
                )
        })
        .map(|item| item.item_count)
        .sum::<u64>();
    println!("personal_browser_data_actionable_count={actionable_count}");
}

#[test]
#[ignore = "reads bounded detail pages from installed privacy sources without modifying them"]
fn actual_privacy_detail_pages_match_the_active_scan() {
    let _test_guard = test_operation_lock();
    let scan = PrivacyService::scan(PrivacyScanRequest {
        time_range: PrivacyTimeRange::AllTime,
    })
    .expect("host privacy scan must complete");
    let populated = scan
        .items
        .iter()
        .filter(|item| item.item_count > 0)
        .collect::<Vec<_>>();
    assert!(
        !populated.is_empty(),
        "the host must expose at least one populated privacy source"
    );
    #[cfg(windows)]
    {
        let recent_rows = scan
            .items
            .iter()
            .filter(|item| item.kind == PrivacyDataKind::RecentItems)
            .collect::<Vec<_>>();
        assert_eq!(
            recent_rows.len(),
            1,
            "Windows Recent and RecentDocs must be presented as one logical source"
        );
        assert_eq!(recent_rows[0].source_id, "recent_document_history");
    }
    for item in populated {
        let page = PrivacyService::details(PrivacyDetailsRequest {
            scan_id: scan.scan_id.clone(),
            token: item.token.clone(),
            offset: 0,
            limit: 5,
        })
        .unwrap_or_else(|error| {
            panic!(
                "detail page failed source_id={} kind={:?} diagnostic={}",
                item.source_id,
                item.kind,
                error.diagnostic()
            )
        });
        assert_eq!(page.total_item_count, item.item_count);
        assert!(page.entries.len() <= 5);
        assert!(page.entries.iter().all(|entry| entry.item_count > 0));
        #[cfg(windows)]
        if item.source_id == "recent_document_history" {
            assert!(page.entries.iter().all(|entry| {
                Path::new(&entry.label)
                    .extension()
                    .is_some_and(|extension| extension.eq_ignore_ascii_case("lnk"))
            }));
        }
        // Never print labels: this ignored validation can run against real personal data.
        println!(
            "privacy_detail_validated source_id={} kind={:?} presentation={:?} entry_count={}",
            item.source_id,
            item.kind,
            page.presentation,
            page.entries.len()
        );
    }
}

#[test]
#[ignore = "clears the current system clipboard through the native platform adapter"]
fn actual_current_clipboard_execution_is_verified() {
    let _test_guard = test_operation_lock();
    let scan = PrivacyService::scan(PrivacyScanRequest {
        time_range: PrivacyTimeRange::AllTime,
    })
    .expect("host privacy scan must complete");
    let clipboard = scan
        .items
        .iter()
        .find(|item| {
            item.kind == PrivacyDataKind::CurrentClipboard
                && item.capability == PrivacyCapabilityState::Ready
                && item.item_count > 0
        })
        .expect("current clipboard must be actionable after synthetic test data is inserted");
    let plan = PrivacyService::prepare(PrivacyExecutionRequest {
        scan_id: scan.scan_id.clone(),
        tokens: vec![clipboard.token.clone()],
    })
    .expect("current clipboard plan must be prepared");
    let result = PrivacyService::execute(execution_request(plan.plan_id))
        .expect("current clipboard execution must complete");
    assert_eq!(result.failed_item_count, 0);
    assert_eq!(result.items.len(), 1);
    assert_eq!(result.items[0].status, PrivacyExecutionItemStatus::Cleared);
    assert!(result.items[0].verified);
    assert!(result.affected_item_count > 0);
}
