#![cfg(windows)]

use std::{
    fs,
    path::PathBuf,
    sync::{
        atomic::{AtomicU64, Ordering},
        Mutex,
    },
    time::Instant,
};

use mangodisk_core::{
    configure_application_paths, AnalysisScanMode, AnalysisService, ApplicationPaths,
    CoreErrorCode, ScanExclusionOptions, TraversalProgress,
};

struct Evidence(Mutex<Vec<String>>);
impl log::Log for Evidence {
    fn enabled(&self, _: &log::Metadata<'_>) -> bool {
        true
    }
    fn log(&self, record: &log::Record<'_>) {
        let line = format!(
            "[{}][{}] {}",
            record.level(),
            record.target(),
            record.args()
        );
        self.0.lock().unwrap().push(line.clone());
        println!("{line}");
    }
    fn flush(&self) {}
}

#[test]
#[ignore = "read-only native scan of MANGODISK_ANALYSIS_VALIDATION_ROOT; run on a Windows test machine"]
fn standard_analysis_on_selected_root() {
    static EVIDENCE: Evidence = Evidence(Mutex::new(Vec::new()));
    log::set_logger(&EVIDENCE).unwrap();
    log::set_max_level(log::LevelFilter::Info);
    let root = std::env::var("MANGODISK_ANALYSIS_VALIDATION_ROOT")
        .expect("set the explicitly selected scan root");
    let evidence = PathBuf::from(
        std::env::var_os("MANGODISK_ANALYSIS_VALIDATION_DIR").expect("set the evidence directory"),
    );
    fs::create_dir_all(&evidence).unwrap();
    let run = evidence.join(format!("scan-{}", std::process::id()));
    configure_application_paths(
        ApplicationPaths::new(run.join("data"), run.join("cache"), run.join("runtime")).unwrap(),
    )
    .unwrap();
    let last_progress_ms = AtomicU64::new(0);
    let started = Instant::now();
    println!(
        "analysis_validation_started pid={} root={root:?}",
        std::process::id()
    );
    let result = AnalysisService::analyze_with_mode_progress(
        Some(root),
        true,
        ScanExclusionOptions::default(),
        AnalysisScanMode::Standard,
        move |progress: TraversalProgress| {
            let previous = last_progress_ms.load(Ordering::Relaxed);
            if progress.elapsed_ms.saturating_sub(previous) >= 1000
                && last_progress_ms
                    .compare_exchange(
                        previous,
                        progress.elapsed_ms,
                        Ordering::Relaxed,
                        Ordering::Relaxed,
                    )
                    .is_ok()
            {
                println!(
                    "analysis_validation_progress {}",
                    serde_json::to_string(&progress).unwrap()
                );
            }
        },
    );
    match result {
        Ok(result) => {
            fs::write(
                run.join("result.json"),
                serde_json::to_vec_pretty(&result).unwrap(),
            )
            .unwrap();
            println!("analysis_validation_completed elapsed_ms={} total_bytes={} skipped_count={} entries={}", started.elapsed().as_millis(), result.total_bytes, result.skipped_count, result.entries.len());
        }
        Err(error) => panic!(
            "analysis_validation_failed elapsed_ms={} error={error}",
            started.elapsed().as_millis()
        ),
    }
    let elapsed_ms = started.elapsed().as_millis() as u64;
    let logs = EVIDENCE.0.lock().unwrap();
    for stage in [
        "cache_validation",
        "native_traversal",
        "result_build",
        "cache_publish",
    ] {
        assert!(
            logs.iter()
                .any(|line| line.contains("analysis_scan_stage_started")
                    && line.contains(&format!("stage={stage}"))),
            "missing stage: {stage}"
        );
    }
    let progress_count = logs
        .iter()
        .filter(|line| line.contains("analysis_scan_progress "))
        .count();
    assert!(progress_count as u64 <= elapsed_ms / 5_000 + 1);
    if elapsed_ms > 6_000 {
        assert!(progress_count > 0);
    }
    assert!(logs
        .iter()
        .any(|line| line.contains("analysis_result_published")));
    drop(logs);

    let fixture = tempfile::tempdir_in(&run).unwrap();
    let payload = b"synthetic-private-analysis-payload";
    for branch in 0..16 {
        let path = fixture.path().join(format!("branch-{branch}"));
        fs::create_dir(&path).unwrap();
        for file in 0..128 {
            fs::write(path.join(format!("file-{file}.bin")), payload).unwrap();
        }
    }
    let fixture_root = fixture.path().to_string_lossy().into_owned();
    let cancelled = AnalysisService::analyze_with_mode_progress(
        Some(fixture_root.clone()),
        true,
        ScanExclusionOptions::default(),
        AnalysisScanMode::Standard,
        |_: TraversalProgress| AnalysisService::cancel(),
    )
    .unwrap_err();
    assert_eq!(cancelled.code(), CoreErrorCode::OperationCancelled);
    let recovered = AnalysisService::analyze_with_mode_progress(
        Some(fixture_root.clone()),
        true,
        ScanExclusionOptions::default(),
        AnalysisScanMode::Standard,
        |_: TraversalProgress| {},
    )
    .unwrap();
    assert_eq!(recovered.total_bytes, 2048 * payload.len() as u64);
    assert_eq!(recovered.entries.len(), 16);
    let denied_path = fixture.path().join("branch-0");
    struct RestoreAccess(PathBuf);
    impl Drop for RestoreAccess {
        fn drop(&mut self) {
            assert!(std::process::Command::new("icacls.exe")
                .arg(&self.0)
                .args(["/remove:d", "*S-1-1-0"])
                .output()
                .unwrap()
                .status
                .success());
        }
    }
    let access = RestoreAccess(denied_path.clone());
    assert!(std::process::Command::new("icacls.exe")
        .arg(&denied_path)
        .args(["/deny", "*S-1-1-0:(RD)"])
        .output()
        .unwrap()
        .status
        .success());
    assert!(
        fs::read_dir(&denied_path).is_err(),
        "the desktop token must observe the fixture ACL"
    );
    let limited = AnalysisService::analyze_with_mode_progress(
        Some(fixture_root),
        true,
        ScanExclusionOptions::default(),
        AnalysisScanMode::Standard,
        |_: TraversalProgress| {},
    )
    .unwrap();
    assert!(limited.skipped_count > 0);
    assert_eq!(limited.total_bytes, 15 * 128 * payload.len() as u64);
    drop(access);
    let failed = AnalysisService::analyze_with_mode_progress(
        Some(
            fixture
                .path()
                .join("missing-root")
                .to_string_lossy()
                .into_owned(),
        ),
        true,
        ScanExclusionOptions::default(),
        AnalysisScanMode::Standard,
        |_: TraversalProgress| {},
    );
    assert!(failed.is_err());
    let logs = EVIDENCE.0.lock().unwrap();
    assert!(logs
        .iter()
        .any(|line| line.contains("analysis_scan_incomplete")
            && line.contains("cancelled=true")
            && line.starts_with("[INFO]")));
    assert!(logs
        .iter()
        .any(|line| line.contains("analysis_request_failed")
            && line.contains("missing-root")
            && line.contains("error=")));
    assert!(!logs
        .iter()
        .any(|line| line.contains(std::str::from_utf8(payload).unwrap())));
    println!("analysis_validation_cases passed=5 cases=selected_root,cancellation,retry,access_denial,missing_root");
}
