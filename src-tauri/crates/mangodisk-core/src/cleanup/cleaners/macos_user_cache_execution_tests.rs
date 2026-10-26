use super::*;
use crate::{
    cleanup::{
        source_selection::SourceSelectionPolicy, CleanupSourceSelection, CleanupSourceSelectionMode,
    },
    shared::operation::{test_operation_lock, CoordinatedOperationKind},
    system_inventory::SystemInventoryContext,
};

struct CacheFixture {
    _directory: tempfile::TempDir,
    home: PathBuf,
    cache_root: PathBuf,
    candidates: Vec<CacheCandidate>,
    inventory: SystemInventorySnapshot,
}

impl CacheFixture {
    fn new() -> Self {
        let directory = tempfile::tempdir().unwrap();
        let home = directory.path().canonicalize().unwrap();
        let cache_root = home.join("Library/Caches");
        for name in ["first", "second"] {
            let path = cache_root.join(name);
            fs::create_dir_all(&path).unwrap();
            fs::write(path.join("cache.bin"), name.as_bytes()).unwrap();
        }
        let mut candidates = discover_in_root(
            &cache_root,
            &home,
            &[],
            &|| false,
            &|_| {},
            &|_, _, _| {},
            &|_| false,
        )
        .unwrap()
        .candidates;
        candidates.sort_by(|left, right| left.path.cmp(&right.path));
        assert_eq!(candidates.len(), 2);
        replace_last_preview(Some(candidates.clone())).unwrap();
        Self {
            _directory: directory,
            home,
            cache_root,
            candidates,
            inventory: SystemInventoryContext::empty().inventory,
        }
    }

    fn execute(
        &self,
        candidates: &[CacheCandidate],
        processes: &ProcessSnapshot,
        dry_run: bool,
    ) -> CleanupActionResult {
        let operation = OperationGuard::start(CoordinatedOperationKind::Cleanup).unwrap();
        let action = execute_candidates(
            candidates,
            &CacheValidationContext {
                inventory: &self.inventory,
                processes,
                declared_roots: &[],
                home: &self.home,
                cache_root: &self.cache_root,
            },
            dry_run,
            &operation,
        );
        operation.complete();
        action
    }

    fn pending(&self) -> Vec<CacheCandidate> {
        LAST_PREVIEW.get().unwrap().lock().unwrap().clone().unwrap()
    }
}

impl Drop for CacheFixture {
    fn drop(&mut self) {
        replace_last_preview(None).unwrap();
    }
}

#[test]
fn cleaning_one_reviewed_source_keeps_other_sources_actionable() {
    let _operation_lock = test_operation_lock();
    let fixture = CacheFixture::new();
    for (index, candidate) in fixture.candidates.iter().enumerate() {
        let action = fixture.execute(
            std::slice::from_ref(candidate),
            &ProcessSnapshot::default(),
            false,
        );
        assert_eq!(action.status, CleanupActionStatus::Completed);
        assert_eq!(action.released_bytes, candidate.bytes);
        assert_eq!(action.failed_item_count, 0);
        assert!(!candidate.path.exists());
        assert_eq!(fixture.pending(), fixture.candidates[index + 1..]);
    }
    assert!(fixture.pending().is_empty());

    // Recreated data has never been reviewed. A stale source checkbox must not authorize it.
    let recreated = &fixture.candidates[0];
    fs::create_dir(&recreated.path).unwrap();
    fs::write(recreated.path.join("new.bin"), b"unreviewed").unwrap();
    let selections = SourceSelectionPolicy::from_request(
        &HashSet::from([CLEANER_ID.into()]),
        &[CleanupSourceSelection {
            rule_id: CLEANER_ID.into(),
            mode: CleanupSourceSelectionMode::Include,
            paths: vec![display_path(&recreated.path)],
        }],
    )
    .unwrap();
    let operation = OperationGuard::start(CoordinatedOperationKind::Cleanup).unwrap();
    let action = execute(
        &fixture.inventory,
        &[],
        selections.scope(CLEANER_ID),
        false,
        &operation,
    );
    operation.complete();
    assert_eq!(
        action.reason_code,
        Some(CleanupActionReason::PreflightFailed)
    );
    assert_eq!(
        fs::read(recreated.path.join("new.bin")).unwrap(),
        b"unreviewed"
    );
}

#[test]
fn active_cache_failure_preserves_the_snapshot_for_a_safe_retry() {
    let _operation_lock = test_operation_lock();
    let fixture = CacheFixture::new();
    let candidate = &fixture.candidates[0];
    let active = ProcessSnapshot::from_process_names(vec![candidate.ownership_key.clone()]);
    let action = fixture.execute(std::slice::from_ref(candidate), &active, false);
    assert_eq!(
        action.reason_code,
        Some(CleanupActionReason::PreflightFailed)
    );
    assert_eq!(action.released_bytes, 0);
    assert_eq!(fixture.pending(), fixture.candidates);
    assert!(candidate.path.join("cache.bin").exists());

    let action = fixture.execute(&fixture.pending(), &ProcessSnapshot::default(), false);
    assert_eq!(action.status, CleanupActionStatus::Completed);
    assert_eq!(action.failed_item_count, 0);
    assert!(fixture.pending().is_empty());
}

#[test]
fn changed_cache_stays_blocked_across_retries_while_other_sources_can_be_cleaned() {
    let _operation_lock = test_operation_lock();
    let fixture = CacheFixture::new();
    let changed = &fixture.candidates[0];
    fs::write(
        changed.path.join("cache.bin"),
        b"changed after confirmation",
    )
    .unwrap();
    let action = fixture.execute(&fixture.candidates, &ProcessSnapshot::default(), false);
    assert_eq!(action.status, CleanupActionStatus::Partial);
    assert_eq!(action.failed_item_count, 1);
    assert_eq!(fixture.pending(), vec![changed.clone()]);
    assert!(!fixture.candidates[1].path.exists());

    let retry = fixture.execute(&fixture.pending(), &ProcessSnapshot::default(), false);
    assert_eq!(retry.status, CleanupActionStatus::Failed);
    assert_eq!(retry.bytes_expected, changed.bytes);
    assert_eq!(
        retry.reason_code,
        Some(CleanupActionReason::PreflightFailed)
    );
    assert_eq!(fixture.pending(), vec![changed.clone()]);
    assert_eq!(
        fs::read(changed.path.join("cache.bin")).unwrap(),
        b"changed after confirmation"
    );
}

#[test]
fn dry_run_and_cancellation_preserve_reviewed_candidates() {
    let _operation_lock = test_operation_lock();
    let fixture = CacheFixture::new();
    let action = fixture.execute(&fixture.candidates, &ProcessSnapshot::default(), true);
    assert_eq!(action.status, CleanupActionStatus::Previewed);
    assert_eq!(action.released_bytes, 0);
    assert_eq!(fixture.pending(), fixture.candidates);

    let operation = OperationGuard::start(CoordinatedOperationKind::Cleanup).unwrap();
    OperationGuard::cancel(CoordinatedOperationKind::Cleanup);
    let action = execute_candidates(
        &fixture.candidates,
        &CacheValidationContext {
            inventory: &fixture.inventory,
            processes: &ProcessSnapshot::default(),
            declared_roots: &[],
            home: &fixture.home,
            cache_root: &fixture.cache_root,
        },
        false,
        &operation,
    );
    assert_eq!(action.reason_code, Some(CleanupActionReason::Cancelled));
    assert_eq!(fixture.pending(), fixture.candidates);
    assert!(fixture
        .candidates
        .iter()
        .all(|candidate| candidate.path.exists()));
}

#[test]
fn a_cache_replaced_with_a_symbolic_link_is_never_deleted() {
    let _operation_lock = test_operation_lock();
    let fixture = CacheFixture::new();
    let candidate = &fixture.candidates[0];
    let original = fixture.home.join("original");
    fs::rename(&candidate.path, &original).unwrap();
    std::os::unix::fs::symlink(&original, &candidate.path).unwrap();
    let action = fixture.execute(
        std::slice::from_ref(candidate),
        &ProcessSnapshot::default(),
        false,
    );
    assert_eq!(
        action.reason_code,
        Some(CleanupActionReason::PreflightFailed)
    );
    assert_eq!(action.released_bytes, 0);
    assert_eq!(fixture.pending(), fixture.candidates);
    assert!(original.join("cache.bin").exists());
    assert!(fs::symlink_metadata(&candidate.path)
        .unwrap()
        .file_type()
        .is_symlink());
}

#[test]
fn completion_does_not_discard_a_newer_same_path_preview() {
    let _operation_lock = test_operation_lock();
    let fixture = CacheFixture::new();
    let old = fixture.candidates[0].clone();
    let mut newer = old.clone();
    newer.bytes += 1;
    newer.fingerprint = "newer reviewed content".into();
    replace_last_preview(Some(vec![newer.clone(), fixture.candidates[1].clone()])).unwrap();

    retain_pending_candidates(&HashSet::from([old])).unwrap();

    assert_eq!(
        fixture.pending(),
        vec![newer, fixture.candidates[1].clone()]
    );
}

#[test]
fn unreadable_cache_descendant_retains_its_path_and_native_cause() {
    use std::os::unix::fs::PermissionsExt;

    struct RestorePermissions(PathBuf, fs::Permissions);
    impl Drop for RestorePermissions {
        fn drop(&mut self) {
            fs::set_permissions(&self.0, self.1.clone()).unwrap();
        }
    }

    let _operation_lock = test_operation_lock();
    let fixture = CacheFixture::new();
    let mut candidate = fixture.candidates[0].clone();
    let denied = candidate.path.join("unreadable-child");
    fs::create_dir(&denied).unwrap();
    fs::write(denied.join("keep.bin"), b"protected fixture").unwrap();
    let snapshot = snapshot_metadata_tree(
        &candidate.path,
        &candidate.boundary_root,
        ScanPurpose::Cleanup,
    );
    candidate.bytes = snapshot.bytes;
    candidate.file_count = snapshot.file_count;
    candidate.fingerprint = display_fingerprint(snapshot.fingerprint.unwrap());
    candidate.modified_at_ms = modified_ms(&fs::symlink_metadata(&candidate.path).unwrap());
    replace_last_preview(Some(vec![candidate.clone()])).unwrap();
    let _restore = RestorePermissions(denied.clone(), fs::metadata(&denied).unwrap().permissions());
    fs::set_permissions(&denied, fs::Permissions::from_mode(0o000)).unwrap();

    let error = revalidate_candidate(
        &candidate,
        &CacheValidationContext {
            inventory: &fixture.inventory,
            processes: &ProcessSnapshot::default(),
            declared_roots: &[],
            home: &fixture.home,
            cache_root: &fixture.cache_root,
        },
    )
    .unwrap_err();
    assert!(error.contains("unreadable-child"), "{error}");
    assert!(error.contains("os error 13"), "{error}");
    let action = fixture.execute(
        std::slice::from_ref(&candidate),
        &ProcessSnapshot::default(),
        false,
    );
    assert_eq!(action.status, CleanupActionStatus::Failed);
    assert_eq!(action.released_bytes, 0);
    assert!(fixture.candidates[0].path.join("cache.bin").exists());
    assert_eq!(fixture.pending(), vec![candidate]);
    drop(_restore);
    let retry = fixture.execute(&fixture.pending(), &ProcessSnapshot::default(), false);
    assert_eq!(retry.status, CleanupActionStatus::Completed);
}

#[test]
fn same_metadata_replacement_is_not_authorized_by_a_retained_cache_snapshot() {
    assert_cache_replacement_is_rejected(false);
}

#[test]
fn same_metadata_root_replacement_is_not_authorized_by_a_retained_cache_snapshot() {
    assert_cache_replacement_is_rejected(true);
}

fn assert_cache_replacement_is_rejected(replace_root: bool) {
    let _operation_lock = test_operation_lock();
    let fixture = CacheFixture::new();
    let candidate = &fixture.candidates[0];
    let file = candidate.path.join("cache.bin");
    let analysis_before = snapshot_metadata_tree(
        &candidate.path,
        &candidate.boundary_root,
        ScanPurpose::Analysis,
    );
    let original_file_time = fs::metadata(&file).unwrap().modified().unwrap();
    let original_root_time = fs::metadata(&candidate.path).unwrap().modified().unwrap();
    if replace_root {
        fs::rename(&candidate.path, fixture.home.join("original-cache")).unwrap();
        fs::create_dir(&candidate.path).unwrap();
    } else {
        fs::rename(&file, fixture.home.join("original-cache.bin")).unwrap();
    }
    fs::write(&file, b"fresh").unwrap();
    fs::File::open(&file)
        .unwrap()
        .set_times(fs::FileTimes::new().set_modified(original_file_time))
        .unwrap();
    fs::File::open(&candidate.path)
        .unwrap()
        .set_times(fs::FileTimes::new().set_modified(original_root_time))
        .unwrap();

    let analysis_after = snapshot_metadata_tree(
        &candidate.path,
        &candidate.boundary_root,
        ScanPurpose::Analysis,
    );
    assert_eq!(analysis_before.fingerprint, analysis_after.fingerprint);

    let action = fixture.execute(
        std::slice::from_ref(candidate),
        &ProcessSnapshot::default(),
        false,
    );

    assert_eq!(action.status, CleanupActionStatus::Failed);
    assert_eq!(fs::read(&file).unwrap(), b"fresh");
    assert_eq!(fixture.pending(), fixture.candidates);
}

#[test]
#[ignore = "installs a process-wide diagnostic logger; run this test in isolation"]
fn failure_diagnostics_preserve_native_causes_and_bound_item_details() {
    struct Evidence(std::sync::Mutex<Vec<String>>);
    impl log::Log for Evidence {
        fn enabled(&self, _: &log::Metadata<'_>) -> bool {
            true
        }
        fn log(&self, record: &log::Record<'_>) {
            if record.target().ends_with("macos_user_cache_inventory") {
                self.0.lock().unwrap().push(record.args().to_string());
            }
        }
        fn flush(&self) {}
    }
    static EVIDENCE: Evidence = Evidence(std::sync::Mutex::new(Vec::new()));
    log::set_logger(&EVIDENCE).expect("run this diagnostic test in an isolated process");
    log::set_max_level(log::LevelFilter::Info);
    let _operation_lock = test_operation_lock();
    let fixture = CacheFixture::new();
    let missing = (0..25)
        .map(|index| {
            let mut candidate = fixture.candidates[0].clone();
            candidate.path = fixture.cache_root.join(format!("missing\ncache-{index}"));
            candidate
        })
        .collect::<Vec<_>>();
    let action = fixture.execute(&missing, &ProcessSnapshot::default(), false);
    assert_eq!(action.failed_item_count, 25);
    let logs = EVIDENCE.0.lock().unwrap();
    let failures = logs
        .iter()
        .filter(|line| line.starts_with("additional_user_cache_item_failed "))
        .collect::<Vec<_>>();
    assert_eq!(failures.len(), MAX_EXECUTION_FAILURE_DETAILS as usize);
    assert!(
        failures.iter().all(|line| {
            line.contains("operation_id=")
                && line.contains("stage=prepare_delete")
                && line.contains("(os error 2)")
                && line.contains("outcome=skipped")
                && line.contains("missing\\ncache-")
                && !line.contains('\n')
        }),
        "first failure: {}",
        failures[0]
    );
    assert!(logs.iter().any(|line| {
        line.starts_with("additional_user_cache_execution_finished ")
            && line.contains("failed_source_count=25")
            && line.contains("failure_details_omitted=5")
    }));
    drop(logs);
    drop(fixture);
    drop(_operation_lock);

    unreadable_cache_descendant_retains_its_path_and_native_cause();
    let logs = EVIDENCE.0.lock().unwrap();
    let unreadable_failure = logs
        .iter()
        .find(|line| {
            line.starts_with("additional_user_cache_item_failed ")
                && line.contains("stage=revalidate_candidate")
                && line.contains("unreadable-child")
                && line.contains("stage=read_directory")
                && line.contains("os error 13")
        })
        .expect("permission failures must retain the nested path and native cause in the log");
    println!("{unreadable_failure}");
    assert!(!logs.iter().any(|line| line.contains("protected fixture")));
}
