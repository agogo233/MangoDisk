use std::time::Instant;

use crate::{
    filesystem::{
        metadata::diagnostic_path, permanent_delete::delete_analysis_candidate_permanently,
    },
    shared::{
        operation::{CoordinatedOperationKind, OperationGuard},
        CoreResult, TraversalProgress,
    },
    storage::traversal::{AnalysisScanDiagnostics, StorageTraversal},
    storage::{
        analysis::{AnalysisDeleteResult, AnalysisResult},
        index::cache,
    },
    ProgressSink,
};

use super::session::{publish_result_session, resolve_entry_candidate, synchronize_removed_path};

pub struct AnalysisService;

impl AnalysisService {
    pub fn analyze_with_progress(
        path: Option<String>,
        refresh: bool,
        callback: impl ProgressSink,
    ) -> CoreResult<AnalysisResult> {
        let result =
            StorageTraversal::analyze_path_with_progress(path, refresh, move |progress| {
                callback.report(progress);
            })?;
        Ok(publish_result_session(result)?)
    }

    pub(crate) fn analyze_with_diagnostics(
        path: Option<String>,
        refresh: bool,
        callback: impl Fn(TraversalProgress) + Send + Sync + 'static,
    ) -> CoreResult<(AnalysisResult, AnalysisScanDiagnostics)> {
        StorageTraversal::analyze_path_with_diagnostics(path, refresh, callback)
    }

    pub fn cancel() {
        StorageTraversal::cancel_analysis();
    }

    /// Resolves an external-open request against the authoritative scan snapshot.
    ///
    /// The platform adapter owns launching the system handler. Core only proves
    /// that the requested path was published to the current UI by a real scan.
    pub fn resolve_open_target(scan_id: u64, selected_path: String) -> CoreResult<String> {
        Ok(resolve_entry_candidate(scan_id, &selected_path)?.path)
    }

    pub fn delete_entry_permanently(
        scan_id: u64,
        selected_path: String,
    ) -> CoreResult<AnalysisDeleteResult> {
        let candidate = resolve_entry_candidate(scan_id, &selected_path)?;
        let operation = OperationGuard::start(CoordinatedOperationKind::PermanentDelete)?;
        let started = Instant::now();
        let is_directory = candidate.is_directory;
        let outcome = match delete_analysis_candidate_permanently(candidate) {
            Ok(outcome) => outcome,
            Err(error) => {
                if error.is_partial() {
                    // A partially changed directory no longer matches any derived index snapshot.
                    // Clearing the rebuildable cache prevents stale sizes from surviving the
                    // irreversible boundary.
                    if let Err(cache_error) = cache::clear_all() {
                        log::error!(
                            "analysis_partial_delete_cache_clear_failed operation_id={} scan_id={} error_digest={}",
                            operation.id(),
                            scan_id,
                            blake3::hash(cache_error.to_string().as_bytes()).to_hex()
                        );
                    }
                }
                log::warn!(
                    "analysis_permanent_delete_failed operation_id={} scan_id={} partial={} released_logical_bytes={} error_digest={}",
                    operation.id(),
                    scan_id,
                    error.is_partial(),
                    error.released_bytes(),
                    blake3::hash(error.to_string().as_bytes()).to_hex()
                );
                let mut core_error = crate::shared::CoreError::operation_failed(error.to_string());
                if let Some(reason) = error.reason() {
                    core_error = core_error.with_reason(reason);
                }
                return Err(core_error);
            }
        };
        cache::remove_entry(
            &outcome.target,
            outcome.removed_usage,
            outcome.result.removed_file_count,
            is_directory,
        );
        synchronize_removed_path(scan_id, &outcome.target, outcome.result.released_bytes)?;
        log::info!(
            "analysis_permanent_delete_finished operation_id={} scan_id={} path={} entry_kind={} removed_logical_bytes={} released_allocated_bytes={} file_count={} elapsed_ms={}",
            operation.id(),
            scan_id,
            diagnostic_path(&outcome.target),
            if is_directory { "directory" } else { "file" },
            outcome.removed_usage.logical_bytes,
            outcome.result.released_bytes,
            outcome.result.removed_file_count,
            started.elapsed().as_millis()
        );
        operation.complete();
        Ok(outcome.result)
    }
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        path::PathBuf,
        sync::{Arc, Mutex},
    };

    use super::*;

    struct AnalysisFixture {
        root: PathBuf,
    }

    impl AnalysisFixture {
        fn new() -> Self {
            let root = std::env::temp_dir().join(format!(
                "mangodisk-analysis-service-{}-{}",
                std::process::id(),
                crate::filesystem::metadata::now_ms()
            ));
            fs::create_dir_all(&root).expect("the analysis service fixture should be created");
            Self { root }
        }

        fn file(&self) -> PathBuf {
            self.root.join("candidate.bin")
        }
    }

    impl Drop for AnalysisFixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    #[test]
    fn analysis_service_deletes_the_current_direct_child_and_synchronizes_its_session() {
        let _operation_lock = crate::shared::operation::test_operation_lock();
        cache::clear_all().expect("the analysis cache should be clear before the service test");
        let fixture = AnalysisFixture::new();
        let path = fixture.file();
        fs::write(&path, vec![1_u8; 16 * 1024]).expect("the analysis candidate should be written");
        let progress_events = Arc::new(Mutex::new(Vec::new()));
        let captured_events = Arc::clone(&progress_events);

        let initial = AnalysisService::analyze_with_progress(
            Some(fixture.root.to_string_lossy().into_owned()),
            true,
            move |progress| {
                captured_events
                    .lock()
                    .expect("the analysis progress fixture should remain available")
                    .push(progress)
            },
        )
        .expect("the analysis service should scan the isolated fixture");
        let selected_path = initial
            .entries
            .iter()
            .find(|entry| entry.name == "candidate.bin")
            .expect("the analysis result should contain the fixture file")
            .path
            .clone();
        assert_eq!(
            AnalysisService::resolve_open_target(initial.scan_id, selected_path.clone())
                .expect("the published analysis entry should resolve"),
            selected_path
        );
        assert!(
            !progress_events
                .lock()
                .expect("the analysis progress fixture should remain readable")
                .is_empty(),
            "the service adapter must forward traversal progress"
        );

        assert!(
            AnalysisService::delete_entry_permanently(
                initial.scan_id,
                fixture
                    .root
                    .join("fabricated.bin")
                    .to_string_lossy()
                    .into_owned(),
            )
            .is_err(),
            "the service must reject a path that was not published by the scan"
        );
        // Analysis deletion intentionally authorizes the current regular direct child even when
        // it changed after measurement. The permanent-delete boundary pins its physical identity
        // during execution; stale scan sizes are accounting facts rather than preflight gates.
        fs::write(&path, vec![2_u8; 32 * 1024])
            .expect("the analysis candidate should change after the scan");
        let deleted =
            AnalysisService::delete_entry_permanently(initial.scan_id, selected_path.clone())
                .expect("the current direct child should be deleted safely");

        assert_eq!(deleted.removed_path, selected_path);
        assert_eq!(deleted.removed_file_count, 1);
        assert!(!path.exists());
        assert!(
            AnalysisService::resolve_open_target(initial.scan_id, deleted.removed_path).is_err(),
            "a deleted entry must disappear from the authoritative result session"
        );
        cache::clear_all().expect("the analysis cache should be clear after the service test");
    }
}
