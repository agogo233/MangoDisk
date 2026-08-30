use std::{path::Path, time::Instant};

use crate::{
    filesystem::{
        metadata::{diagnostic_path, now_ms},
        permanent_delete::delete_file_candidate_permanently,
        PermanentDeleteBatchResult, PermanentDeleteFailure,
    },
    history::{file_cleanup_record, FileCleanupHistoryCategory, HistoryService},
    shared::{
        operation::{CoordinatedOperationKind, OperationGuard},
        CoreResult, TraversalProgress,
    },
    storage::index::cache,
    storage::large_files::LargeFilesResult,
    storage::traversal::{LargeFileScanDiagnostics, StorageTraversal},
    ProgressSink,
};

use super::session::{
    publish_result_session, resolve_delete_candidates, resolve_open_target,
    synchronize_removed_paths,
};

pub struct LargeFileService;

impl LargeFileService {
    pub fn find_with_progress(
        path: Option<String>,
        minimum_bytes: u64,
        refresh: bool,
        callback: impl ProgressSink,
    ) -> CoreResult<LargeFilesResult> {
        let result = StorageTraversal::find_large_files_with_progress(
            path,
            minimum_bytes,
            refresh,
            move |progress| callback.report(progress),
        )?;
        Ok(publish_result_session(result)?)
    }

    pub(crate) fn find_with_diagnostics(
        path: Option<String>,
        minimum_bytes: u64,
        refresh: bool,
        callback: impl Fn(TraversalProgress) + Send + Sync + 'static,
    ) -> CoreResult<(LargeFilesResult, LargeFileScanDiagnostics)> {
        StorageTraversal::find_large_files_with_diagnostics(path, minimum_bytes, refresh, callback)
    }

    pub fn cancel() {
        StorageTraversal::cancel_large_files();
    }

    pub fn resolve_open_target(scan_id: u64, selected_path: String) -> CoreResult<String> {
        Ok(resolve_open_target(scan_id, &selected_path)?)
    }

    pub fn delete_files_permanently(
        scan_id: u64,
        selected_paths: Vec<String>,
    ) -> CoreResult<PermanentDeleteBatchResult> {
        let selection = resolve_delete_candidates(scan_id, selected_paths)?;
        let expected_bytes = selection.expected_allocated_bytes;
        let candidates = selection.candidates;
        let operation = OperationGuard::start(CoordinatedOperationKind::PermanentDelete)?;
        let started = Instant::now();
        let started_at_ms = now_ms();
        let requested_count = candidates.len();
        let selected_paths = candidates
            .iter()
            .map(|candidate| candidate.path.clone())
            .collect::<Vec<_>>();
        let path_sample = candidates
            .iter()
            .take(3)
            .map(|candidate| diagnostic_path(Path::new(&candidate.path)))
            .collect::<Vec<_>>();
        let mut result = PermanentDeleteBatchResult {
            removed_paths: Vec::new(),
            failed: Vec::new(),
            released_bytes: 0,
        };
        for candidate in candidates {
            match delete_file_candidate_permanently(&candidate) {
                Ok((target, usage)) => {
                    result.released_bytes =
                        result.released_bytes.saturating_add(usage.allocated_bytes);
                    result.removed_paths.push(candidate.path);
                    cache::remove_entry(&target, usage, 1, false);
                }
                Err(error) => result.failed.push(PermanentDeleteFailure {
                    path: candidate.path,
                    message: error.to_string(),
                }),
            }
        }
        synchronize_removed_paths(scan_id, &result.removed_paths)?;
        let history_record = file_cleanup_record(
            format!("large-file-cleanup-{}-{}", operation.id(), now_ms()),
            FileCleanupHistoryCategory::LargeFiles,
            started_at_ms,
            now_ms(),
            selected_paths,
            expected_bytes,
            &result,
        );
        if let Err(error) = HistoryService::append(history_record) {
            log::warn!(
                "large_file_history_save_failed operation_id={} error_digest={}",
                operation.id(),
                blake3::hash(error.diagnostic().as_bytes()).to_hex()
            );
        }
        log::info!(
            "permanent_delete_batch_finished operation_id={} scan_id={} requested_count={} path_sample={:?} selected_allocated_bytes={} removed_count={} failed_count={} released_allocated_bytes={} elapsed_ms={}",
            operation.id(),
            scan_id,
            requested_count,
            path_sample,
            expected_bytes,
            result.removed_paths.len(),
            result.failed.len(),
            result.released_bytes,
            started.elapsed().as_millis()
        );
        operation.complete();
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use std::{fs, io::Write, path::PathBuf};

    use super::*;
    use crate::storage::index::cache::LARGE_FILE_INDEX_FLOOR_BYTES;

    struct LargeFileFixture {
        root: PathBuf,
    }

    impl LargeFileFixture {
        fn new() -> Self {
            let root = std::env::temp_dir().join(format!(
                "mangodisk-large-file-service-{}-{}",
                std::process::id(),
                now_ms()
            ));
            fs::create_dir_all(&root).expect("the large-file service fixture should be created");
            Self { root }
        }

        fn file(&self) -> PathBuf {
            self.root.join("candidate.bin")
        }
    }

    impl Drop for LargeFileFixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    #[test]
    fn changed_large_file_is_preserved_until_a_fresh_snapshot_authorizes_delete() {
        let _operation_lock = crate::shared::operation::test_operation_lock();
        cache::clear_all().expect("the large-file cache should be clear before the service test");
        HistoryService::clear().expect("the test history should be clear before the service test");
        let fixture = LargeFileFixture::new();
        let path = fixture.file();
        let initial_bytes = LARGE_FILE_INDEX_FLOOR_BYTES.saturating_add(1024 * 1024);
        fs::write(&path, vec![3_u8; initial_bytes as usize])
            .expect("the dense large-file candidate should be written");

        let initial = LargeFileService::find_with_progress(
            Some(fixture.root.to_string_lossy().into_owned()),
            1,
            true,
            |_| {},
        )
        .expect("the large-file service should scan the isolated fixture");
        assert_eq!(initial.entries.len(), 1);
        let selected_path = initial.entries[0].path.clone();
        assert_eq!(
            LargeFileService::resolve_open_target(initial.scan_id, selected_path.clone())
                .expect("the published large file should resolve"),
            selected_path
        );

        fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .expect("the large-file candidate should reopen")
            .write_all(&[9])
            .expect("the large-file candidate should change after the scan");
        let stale_delete = LargeFileService::delete_files_permanently(
            initial.scan_id,
            vec![selected_path.clone()],
        )
        .expect("a live preflight failure should remain a typed batch result");
        assert!(stale_delete.removed_paths.is_empty());
        assert_eq!(stale_delete.failed.len(), 1);
        assert!(
            path.exists(),
            "failed preflight must preserve the changed file"
        );

        let refreshed = LargeFileService::find_with_progress(
            Some(fixture.root.to_string_lossy().into_owned()),
            1,
            true,
            |_| {},
        )
        .expect("the changed large-file fixture should rescan successfully");
        let deleted = LargeFileService::delete_files_permanently(
            refreshed.scan_id,
            vec![selected_path.clone()],
        )
        .expect("a candidate matching the fresh snapshot should be deleted");

        assert_eq!(deleted.removed_paths, vec![selected_path.clone()]);
        assert!(deleted.failed.is_empty());
        assert!(!path.exists());
        assert!(
            LargeFileService::resolve_open_target(refreshed.scan_id, selected_path).is_err(),
            "a deleted file must disappear from the authoritative result session"
        );
        let history = HistoryService::list().expect("large-file cleanup history should load");
        assert_eq!(history.len(), 2);
        assert_eq!(history[0].affected_item_count, 1);
        assert_eq!(history[1].failed_item_count, 1);

        HistoryService::clear().expect("the test history should be clear after the service test");
        cache::clear_all().expect("the large-file cache should be clear after the service test");
    }
}
