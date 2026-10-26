use std::{
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, AtomicU64, Ordering},
    time::Instant,
};

use crate::{diagnostic_path, TraversalProgress};

pub(super) const REPORT_INTERVAL_MS: u64 = 5_000;

pub(super) struct AnalysisScanLog<'a> {
    operation_id: u64,
    root: PathBuf,
    started: Instant,
    stage_started: Instant,
    stage: &'static str,
    completed: bool,
    cancelled: &'a AtomicBool,
}

impl<'a> AnalysisScanLog<'a> {
    pub(super) fn new(operation_id: u64, root: &Path, cancelled: &'a AtomicBool) -> Self {
        let started = Instant::now();
        Self {
            operation_id,
            root: root.to_path_buf(),
            started,
            stage_started: started,
            stage: "initializing",
            completed: false,
            cancelled,
        }
    }

    pub(super) fn stage(&mut self, stage: &'static str) {
        log::info!(
            "analysis_scan_stage_started operation_id={} process_id={} root={} stage={} previous_stage={} previous_stage_ms={} elapsed_ms={}",
            self.operation_id, std::process::id(), diagnostic_path(&self.root), stage,
            self.stage, self.stage_started.elapsed().as_millis(), self.started.elapsed().as_millis()
        );
        self.stage = stage;
        self.stage_started = Instant::now();
    }

    pub(super) fn complete(&mut self) {
        self.completed = true;
    }
}

impl Drop for AnalysisScanLog<'_> {
    fn drop(&mut self) {
        if !self.completed {
            let cancelled = self.cancelled.load(Ordering::Relaxed);
            log::log!(if cancelled { log::Level::Info } else { log::Level::Warn },
                "analysis_scan_incomplete operation_id={} process_id={} root={} stage={} cancelled={} unwinding={} stage_elapsed_ms={} elapsed_ms={}",
                self.operation_id, std::process::id(), diagnostic_path(&self.root), self.stage,
                cancelled, std::thread::panicking(), self.stage_started.elapsed().as_millis(), self.started.elapsed().as_millis()
            );
        }
    }
}

pub(super) fn report_due(last: &AtomicU64, elapsed_ms: u64) -> bool {
    let previous = last.load(Ordering::Relaxed);
    elapsed_ms.saturating_sub(previous) >= REPORT_INTERVAL_MS
        && last
            .compare_exchange(previous, elapsed_ms, Ordering::Relaxed, Ordering::Relaxed)
            .is_ok()
}

pub(super) fn log_progress(progress: &TraversalProgress, last: &AtomicU64) {
    if report_due(last, progress.elapsed_ms) {
        log::info!(
            "analysis_scan_progress operation_id={} process_id={} path={} files={} bytes={} elapsed_ms={}",
            progress.operation_id, std::process::id(), mangodisk_platform::diagnostics::text(&progress.current_path),
            progress.items_scanned, progress.bytes_scanned, progress.elapsed_ms
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn concurrent_progress_is_rate_limited_and_tolerates_older_observations() {
        let last = AtomicU64::new(0);
        assert!(!report_due(&last, REPORT_INTERVAL_MS - 1));
        let winners = std::thread::scope(|scope| {
            let handles = (0..16)
                .map(|_| scope.spawn(|| report_due(&last, REPORT_INTERVAL_MS)))
                .collect::<Vec<_>>();
            handles
                .into_iter()
                .map(|handle| handle.join().unwrap())
                .filter(|won| *won)
                .count()
        });
        assert_eq!(winners, 1);
        assert!(!report_due(&last, 1));
        assert!(!report_due(&last, REPORT_INTERVAL_MS * 2 - 1));
        assert!(report_due(&last, REPORT_INTERVAL_MS * 2));
    }
}
