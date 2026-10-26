#![cfg(windows)]

use std::{
    fs::{self, File, OpenOptions},
    io::Write,
    os::windows::fs::OpenOptionsExt,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{Mutex, OnceLock},
    time::{Duration, Instant},
};

use mangodisk_core::{
    configure_application_paths, ApplicationPaths, CoreErrorCode, HistoryService, OperationDetails,
    OperationOutcome, PrivacyCapabilityState, PrivacyDataKind, PrivacyExecutionItemStatus,
    PrivacyExecutionPlan, PrivacyExecutionRequest, PrivacyExecutionRunRequest,
    PrivacyExecutionStage, PrivacyItem, PrivacyScanRequest, PrivacyScanResult, PrivacyService,
    PrivacyTimeRange,
};
use rusqlite::Connection;

const PAYLOAD: &[u8] = b"private-fixture-content-must-not-enter-diagnostics";
static EVIDENCE: OnceLock<Evidence> = OnceLock::new();

struct Evidence {
    file: Mutex<File>,
    lines: Mutex<Vec<String>>,
}

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
        writeln!(self.file.lock().unwrap(), "{line}").unwrap();
        self.lines.lock().unwrap().push(line);
    }
    fn flush(&self) {
        self.file.lock().unwrap().flush().unwrap();
    }
}

struct ProfileFixture {
    root: PathBuf,
    profile_id: String,
}

impl ProfileFixture {
    fn new(browser: &str) -> Self {
        let (relative, source) = match browser {
            "chromium" => ("Chromium/User Data", "chromium"),
            "chrome" => ("Google/Chrome/User Data", "chrome"),
            "edge" => ("Microsoft/Edge/User Data", "edge"),
            _ => panic!("unsupported fixture browser"),
        };
        let name = format!("Profile MangoDisk-验证-{}", uuid::Uuid::new_v4());
        let root = PathBuf::from(std::env::var_os("LOCALAPPDATA").unwrap())
            .join(relative)
            .join(&name);
        fs::create_dir_all(root.parent().unwrap()).unwrap();
        fs::create_dir(&root).unwrap();
        for directory in ["Cache", "Code Cache", "GPUCache", "Local Storage"] {
            fs::create_dir(root.join(directory)).unwrap();
            fs::write(root.join(directory).join("payload.bin"), PAYLOAD).unwrap();
        }
        fs::write(
            root.join("Preferences"),
            br#"{"profile":{"name":"MangoDisk disposable validation"}}"#,
        )
        .unwrap();
        for name in ["Bookmarks", "Login Data", "Cookies", "downloaded-file.bin"] {
            fs::write(root.join(name), PAYLOAD).unwrap();
        }
        let connection = Connection::open(root.join("History")).unwrap();
        connection
            .execute_batch(
                "CREATE TABLE urls(id INTEGER PRIMARY KEY, url TEXT);
             CREATE TABLE visits(id INTEGER PRIMARY KEY, url INTEGER, visit_time INTEGER);
             CREATE TABLE downloads(id INTEGER PRIMARY KEY, start_time INTEGER, target_path TEXT);
             CREATE TABLE downloads_url_chains(id INTEGER, chain_index INTEGER, url TEXT);
             INSERT INTO urls VALUES (1, 'https://fixture.invalid');
             INSERT INTO visits VALUES (1, 1, 1), (2, 1, 2);
             INSERT INTO downloads VALUES (1, 1, 'fixture-download'), (2, 2, 'fixture-download');
             INSERT INTO downloads_url_chains VALUES (1, 0, 'https://fixture.invalid');",
            )
            .unwrap();
        Self {
            root,
            profile_id: format!("{source}:{name}"),
        }
    }

    fn preserved(&self) {
        for name in ["Bookmarks", "Login Data", "Cookies", "downloaded-file.bin"] {
            assert_eq!(fs::read(self.root.join(name)).unwrap(), PAYLOAD);
        }
    }

    fn cache_preserved(&self) {
        for name in ["Cache", "Code Cache", "GPUCache", "Local Storage"] {
            assert_eq!(
                fs::read(self.root.join(name).join("payload.bin")).unwrap(),
                PAYLOAD
            );
        }
        self.preserved();
    }
}

impl Drop for ProfileFixture {
    fn drop(&mut self) {
        // This unique generated profile is the only browser location owned by the test.
        let _ = fs::remove_dir_all(&self.root);
    }
}

struct FixtureProcess(Child);
impl FixtureProcess {
    fn start(directory: &Path) -> Self {
        fs::create_dir_all(directory).unwrap();
        let executable = directory.join("chrome.exe");
        if !executable.exists() {
            fs::copy(std::env::current_exe().unwrap(), &executable).unwrap();
        }
        let ready = directory.join(format!("ready-{}", uuid::Uuid::new_v4()));
        let child = Command::new(executable)
            .args(["--ignored", "--exact", "privacy_fixture_process_waits"])
            .env("MANGODISK_PRIVACY_FIXTURE_READY", &ready)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let started = Instant::now();
        while !ready.exists() {
            assert!(started.elapsed() < Duration::from_secs(15));
            std::thread::sleep(Duration::from_millis(50));
        }
        Self(child)
    }
}
impl Drop for FixtureProcess {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[test]
#[ignore = "helper process started only by native_privacy_execution_matrix"]
fn privacy_fixture_process_waits() {
    let ready = std::env::var_os("MANGODISK_PRIVACY_FIXTURE_READY").unwrap();
    fs::write(ready, b"ready").unwrap();
    std::thread::sleep(Duration::from_secs(180));
}

fn scan_items(
    fixtures: &[&ProfileFixture],
    kinds: &[PrivacyDataKind],
) -> (PrivacyScanResult, Vec<PrivacyItem>) {
    let scan = PrivacyService::scan(PrivacyScanRequest {
        time_range: PrivacyTimeRange::AllTime,
    })
    .unwrap();
    let items = scan
        .items
        .iter()
        .filter(|item| {
            fixtures
                .iter()
                .any(|fixture| item.profile_id.as_deref() == Some(fixture.profile_id.as_str()))
                && kinds.contains(&item.kind)
                && item.item_count > 0
        })
        .cloned()
        .collect::<Vec<_>>();
    assert_eq!(
        items.len(),
        fixtures.len() * kinds.len(),
        "generated sources must be discovered completely"
    );
    (scan, items)
}

fn prepare(scan: &PrivacyScanResult, items: &[PrivacyItem]) -> PrivacyExecutionPlan {
    PrivacyService::prepare(PrivacyExecutionRequest {
        scan_id: scan.scan_id.clone(),
        tokens: items.iter().map(|item| item.token.clone()).collect(),
    })
    .unwrap()
}

fn request(plan: &PrivacyExecutionPlan, excluded: &[&str]) -> PrivacyExecutionRunRequest {
    PrivacyExecutionRunRequest {
        plan_id: plan.plan_id.clone(),
        excluded_source_ids: excluded.iter().map(|source| (*source).into()).collect(),
    }
}

fn history(plan: &PrivacyExecutionPlan) -> Option<mangodisk_core::OperationRecord> {
    HistoryService::list()
        .unwrap()
        .into_iter()
        .find(|record| record.operation_id == format!("privacy-{}", plan.plan_id))
}

fn case(results: &mut Vec<serde_json::Value>, name: &str, action: impl FnOnce()) {
    let started = Instant::now();
    let passed = std::panic::catch_unwind(std::panic::AssertUnwindSafe(action)).is_ok();
    println!(
        "CASE {name} {} elapsed_ms={}",
        if passed { "passed" } else { "FAILED" },
        started.elapsed().as_millis()
    );
    results.push(
        serde_json::json!({"case":name,"passed":passed,"elapsedMs":started.elapsed().as_millis()}),
    );
}

#[test]
#[ignore = "creates and cleans only generated native browser profiles; run explicitly on a Windows test machine"]
fn native_privacy_execution_matrix() {
    let directory = PathBuf::from(
        std::env::var_os("MANGODISK_PRIVACY_VALIDATION_DIR").expect("set an evidence directory"),
    );
    fs::create_dir_all(&directory).unwrap();
    let run = directory.join(format!("matrix-{}", std::process::id()));
    fs::create_dir(&run).unwrap();
    configure_application_paths(
        ApplicationPaths::new(run.join("data"), run.join("cache"), run.join("runtime")).unwrap(),
    )
    .unwrap();
    let evidence = EVIDENCE.get_or_init(|| Evidence {
        file: Mutex::new(File::create(run.join("events.log")).unwrap()),
        lines: Mutex::new(Vec::new()),
    });
    log::set_logger(evidence).unwrap();
    log::set_max_level(log::LevelFilter::Info);
    let mut results = Vec::new();
    let process_root = run.join("process");

    case(
        &mut results,
        "all_sources_excluded_preserve_scan_and_history",
        || {
            let a = ProfileFixture::new("chromium");
            let b = ProfileFixture::new("chrome");
            let _process = FixtureProcess::start(&process_root);
            let (scan, items) = scan_items(
                &[&a, &b],
                &[PrivacyDataKind::BrowserCache, PrivacyDataKind::SiteStorage],
            );
            assert!(items
                .iter()
                .all(|item| item.capability == PrivacyCapabilityState::BrowserRunning));
            let plan = prepare(&scan, &items);
            assert_eq!(plan.browser_close_requirements.len(), 2);
            let mut progress = Vec::new();
            let result = PrivacyService::execute_with_progress(
                request(&plan, &["chromium", "chrome"]),
                |value| progress.push(value),
            )
            .unwrap();
            assert!(result.items.is_empty());
            assert_eq!(result.affected_item_count, 0);
            assert_eq!(progress.len(), 1);
            assert_eq!(progress[0].stage, PrivacyExecutionStage::Finalizing);
            let retained = result.scan.unwrap();
            assert_eq!(retained.scan_id, scan.scan_id);
            assert_eq!(retained.revision, scan.revision);
            assert!(history(&plan).is_none());
            assert!(PrivacyService::execute(request(&plan, &[])).is_err());
            assert_eq!(prepare(&retained, &items).items.len(), 4);
            a.cache_preserved();
            b.cache_preserved();
        },
    );

    case(
        &mut results,
        "partial_exclusion_cleans_only_authorized_source",
        || {
            let excluded = ProfileFixture::new("chromium");
            let retained = ProfileFixture::new("edge");
            let _process = FixtureProcess::start(&process_root);
            let (scan, items) =
                scan_items(&[&excluded, &retained], &[PrivacyDataKind::BrowserCache]);
            let plan = prepare(&scan, &items);
            let result = PrivacyService::execute(request(&plan, &["chromium"])).unwrap();
            assert_eq!(result.items.len(), 1);
            assert_eq!(result.failed_item_count, 0, "{result:?}");
            assert_eq!(result.items[0].status, PrivacyExecutionItemStatus::Cleared);
            excluded.cache_preserved();
            retained.preserved();
            assert!(!retained.root.join("Cache/payload.bin").exists());
            let record = history(&plan).unwrap();
            assert_eq!(record.selected_item_count, 1);
            let OperationDetails::PrivacyCleanup(details) = record.details else {
                panic!("typed history required")
            };
            assert_eq!(details.items[0].source_id, "edge");
        },
    );

    case(
        &mut results,
        "closed_browser_refresh_and_database_cleanup",
        || {
            let fixture = ProfileFixture::new("chromium");
            let process = FixtureProcess::start(&process_root);
            let (scan, items) = scan_items(
                &[&fixture],
                &[
                    PrivacyDataKind::BrowsingHistory,
                    PrivacyDataKind::DownloadHistory,
                ],
            );
            let plan = prepare(&scan, &items);
            let connection = Connection::open(fixture.root.join("History")).unwrap();
            connection
                .execute("INSERT INTO visits VALUES (3, 1, 3)", [])
                .unwrap();
            drop(connection);
            drop(process);
            let result = PrivacyService::execute(request(&plan, &[])).unwrap();
            assert_eq!(result.failed_item_count, 0, "{result:?}");
            assert_eq!(result.affected_item_count, 5);
            assert!(result.items.iter().all(|item| item.verified));
            let connection = Connection::open(fixture.root.join("History")).unwrap();
            for table in ["visits", "downloads", "downloads_url_chains"] {
                assert_eq!(
                    connection
                        .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| row
                            .get::<_, i64>(0))
                        .unwrap(),
                    0
                );
            }
            fixture.cache_preserved();
            assert!(history(&plan).is_some());
        },
    );

    case(&mut results, "drift_blocks_the_complete_plan", || {
        let fixture = ProfileFixture::new("chromium");
        let (scan, items) = scan_items(
            &[&fixture],
            &[PrivacyDataKind::BrowserCache, PrivacyDataKind::SiteStorage],
        );
        let plan = prepare(&scan, &items);
        fs::write(
            fixture.root.join("Cache/payload.bin"),
            b"changed-after-scan",
        )
        .unwrap();
        let result = PrivacyService::execute(request(&plan, &[])).unwrap();
        assert_eq!(result.affected_item_count, 0);
        assert_eq!(result.failed_item_count, 1);
        assert!(result
            .items
            .iter()
            .any(|item| item.failure_reason.as_deref() == Some("sourceChanged")));
        assert!(result
            .items
            .iter()
            .any(|item| item.failure_reason.as_deref() == Some("preflightBlocked")));
        assert_eq!(
            fs::read(fixture.root.join("Local Storage/payload.bin")).unwrap(),
            PAYLOAD
        );
        fixture.preserved();
        let (fresh_scan, fresh_items) = scan_items(
            &[&fixture],
            &[PrivacyDataKind::BrowserCache, PrivacyDataKind::SiteStorage],
        );
        let fresh_plan = prepare(&fresh_scan, &fresh_items);
        let retry = PrivacyService::execute(request(&fresh_plan, &[])).unwrap();
        assert_eq!(
            retry.failed_item_count, 0,
            "new confirmation after a rescan must recover: {retry:?}"
        );
        assert!(retry.items.iter().all(|item| item.verified));
    });

    case(
        &mut results,
        "exclusive_file_lock_preserves_native_failure",
        || {
            let fixture = ProfileFixture::new("chromium");
            let (scan, items) = scan_items(&[&fixture], &[PrivacyDataKind::BrowserCache]);
            let plan = prepare(&scan, &items);
            let lock = OpenOptions::new()
                .read(true)
                .share_mode(0)
                .open(fixture.root.join("Cache/payload.bin"))
                .unwrap();
            let result = PrivacyService::execute(request(&plan, &[])).unwrap();
            assert!(result.failed_item_count > 0, "{result:?}");
            assert!(result.items.iter().all(|item| !item.verified));
            drop(lock);
            fixture.cache_preserved();
            assert!(evidence
                .lines
                .lock()
                .unwrap()
                .iter()
                .any(|line| line.contains("privacy_item_failed")
                    && line.contains("payload.bin")
                    && (line.contains("os error 32") || line.contains("os error 5"))));
        },
    );

    case(
        &mut results,
        "junction_added_after_scan_preserves_external_data",
        || {
            let fixture = ProfileFixture::new("chromium");
            let (scan, items) = scan_items(&[&fixture], &[PrivacyDataKind::BrowserCache]);
            let plan = prepare(&scan, &items);
            let outside = tempfile::tempdir().unwrap();
            fs::write(outside.path().join("keep.bin"), PAYLOAD).unwrap();
            let junction = fixture.root.join("Cache/linked");
            let output = Command::new("cmd.exe")
                .args(["/d", "/c", "mklink", "/J"])
                .arg(&junction)
                .arg(outside.path())
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "junction creation must succeed on the test machine"
            );
            let result = PrivacyService::execute(request(&plan, &[])).unwrap();
            assert!(result.failed_item_count > 0, "{result:?}");
            assert_eq!(result.affected_item_count, 0);
            assert_eq!(fs::read(outside.path().join("keep.bin")).unwrap(), PAYLOAD);
            assert!(evidence
                .lines
                .lock()
                .unwrap()
                .iter()
                .any(|line| line.contains("privacy_item_failed")
                    && line.contains("sourceUnavailable")
                    && line.contains("linked")));
            fs::remove_dir(junction).unwrap();
            fixture.cache_preserved();
        },
    );

    case(
        &mut results,
        "forged_and_duplicate_exclusions_are_rejected",
        || {
            let fixture = ProfileFixture::new("chromium");
            let _process = FixtureProcess::start(&process_root);
            let (scan, items) = scan_items(&[&fixture], &[PrivacyDataKind::BrowserCache]);
            for excluded in [vec!["unknown"], vec!["chromium", "chromium"]] {
                let plan = prepare(&scan, &items);
                let error = PrivacyService::execute(request(&plan, &excluded)).unwrap_err();
                assert_eq!(error.code(), CoreErrorCode::InvalidInput);
                fixture.cache_preserved();
            }
            assert!(PrivacyService::prepare(PrivacyExecutionRequest {
                scan_id: scan.scan_id,
                tokens: vec!["f".repeat(64)]
            })
            .is_err());
        },
    );

    case(
        &mut results,
        "running_browser_blocks_the_complete_plan",
        || {
            let fixture = ProfileFixture::new("chromium");
            let (scan, items) = scan_items(&[&fixture], &[PrivacyDataKind::BrowserCache]);
            let plan = prepare(&scan, &items);
            let _process = FixtureProcess::start(&process_root);
            let result = PrivacyService::execute(request(&plan, &[])).unwrap();
            assert_eq!(result.affected_item_count, 0);
            assert_eq!(
                result.items[0].failure_reason.as_deref(),
                Some("browserRunning")
            );
            fixture.cache_preserved();
        },
    );

    case(
        &mut results,
        "cancellation_after_one_item_keeps_truthful_partial_history",
        || {
            let fixture = ProfileFixture::new("chromium");
            let (scan, items) = scan_items(
                &[&fixture],
                &[PrivacyDataKind::BrowserCache, PrivacyDataKind::SiteStorage],
            );
            let plan = prepare(&scan, &items);
            let result = PrivacyService::execute_with_progress(request(&plan, &[]), |progress| {
                if progress.stage == PrivacyExecutionStage::Cleaning
                    && progress.completed_item_count == 1
                {
                    PrivacyService::cancel_execution();
                }
            })
            .unwrap();
            assert_eq!(
                result
                    .items
                    .iter()
                    .filter(|item| item.status == PrivacyExecutionItemStatus::Cleared)
                    .count(),
                1
            );
            assert_eq!(
                result
                    .items
                    .iter()
                    .filter(|item| item.status == PrivacyExecutionItemStatus::Cancelled)
                    .count(),
                1
            );
            assert!(result.affected_item_count > 0);
            let record = history(&plan).unwrap();
            assert_eq!(record.outcome, OperationOutcome::CompletedWithWarnings);
            assert_eq!(record.affected_item_count, result.affected_item_count);
            fixture.preserved();
        },
    );

    case(
        &mut results,
        "missing_source_blocks_other_reviewed_roots",
        || {
            let fixture = ProfileFixture::new("chromium");
            let (scan, items) = scan_items(&[&fixture], &[PrivacyDataKind::BrowserCache]);
            let plan = prepare(&scan, &items);
            fs::remove_dir_all(fixture.root.join("Cache")).unwrap();
            let result = PrivacyService::execute(request(&plan, &[])).unwrap();
            assert_eq!(result.affected_item_count, 0);
            assert_eq!(result.failed_item_count, 1);
            assert_eq!(
                fs::read(fixture.root.join("Code Cache/payload.bin")).unwrap(),
                PAYLOAD
            );
            assert_eq!(
                fs::read(fixture.root.join("GPUCache/payload.bin")).unwrap(),
                PAYLOAD
            );
            fixture.preserved();
        },
    );

    case(
        &mut results,
        "empty_sources_cannot_be_prepared_for_cleanup",
        || {
            let fixture = ProfileFixture::new("chromium");
            for name in ["Cache", "Code Cache", "GPUCache"] {
                fs::remove_file(fixture.root.join(name).join("payload.bin")).unwrap();
            }
            let scan = PrivacyService::scan(PrivacyScanRequest {
                time_range: PrivacyTimeRange::AllTime,
            })
            .unwrap();
            let item = scan
                .items
                .iter()
                .find(|item| {
                    item.profile_id.as_deref() == Some(fixture.profile_id.as_str())
                        && item.kind == PrivacyDataKind::BrowserCache
                })
                .unwrap();
            assert_eq!(item.item_count, 0);
            assert_eq!(item.capability, PrivacyCapabilityState::Empty);
            assert_eq!(
                PrivacyService::prepare(PrivacyExecutionRequest {
                    scan_id: scan.scan_id.clone(),
                    tokens: vec![item.token.clone()]
                })
                .unwrap_err()
                .code(),
                CoreErrorCode::InvalidInput
            );
            fixture.preserved();
        },
    );

    case(
        &mut results,
        "history_write_failure_does_not_hide_verified_cleanup",
        || {
            struct Blocker(PathBuf);
            impl Drop for Blocker {
                fn drop(&mut self) {
                    let _ = fs::remove_dir(&self.0);
                }
            }
            let fixture = ProfileFixture::new("chromium");
            let (scan, items) = scan_items(&[&fixture], &[PrivacyDataKind::BrowserCache]);
            let plan = prepare(&scan, &items);
            let blocker = Blocker(run.join("data/history.json.tmp"));
            fs::create_dir(&blocker.0).unwrap();
            let result = PrivacyService::execute(request(&plan, &[])).unwrap();
            assert_eq!(result.failed_item_count, 0, "{result:?}");
            assert!(result.items[0].verified);
            assert!(!fixture.root.join("Cache/payload.bin").exists());
            assert!(history(&plan).is_none());
            let lines = evidence.lines.lock().unwrap().clone();
            assert!(lines
                .iter()
                .any(|line| line.contains("privacy_history_save_failed")
                    && line.contains("history.json.tmp")
                    && line.contains("operation_id=")
                    && line.contains("candidate_count=1")
                    && line.contains("result_count=1")
                    && line.contains("os error")));
            assert!(lines
                .iter()
                .any(|line| line.contains("privacy_execution_completed")
                    && line.contains("history_saved=false")));
            fixture.preserved();
        },
    );

    case(
        &mut results,
        "macos_only_cache_rule_is_rejected_on_windows",
        || {
            let error = mangodisk_core::CleanupService::execute(mangodisk_core::CleanupRequest {
                rule_ids: vec!["special.additional-user-caches".into()],
                dry_run: false,
                project_roots: Vec::new(),
                source_selections: Vec::new(),
            })
            .unwrap_err();
            assert_eq!(error.code(), CoreErrorCode::InvalidInput);
            assert!(error.to_string().contains("unknown rule"));
        },
    );

    let lines = evidence.lines.lock().unwrap();
    assert!(!lines
        .iter()
        .any(|line| line.contains(std::str::from_utf8(PAYLOAD).unwrap())));
    assert_eq!(
        lines
            .iter()
            .filter(|line| line.contains("privacy_history_save_failed"))
            .count(),
        1,
        "only the deliberate history write failure may warn"
    );
    assert!(lines
        .iter()
        .any(|line| line.contains("privacy_execution_skipped")
            && line.contains("selected_item_count=4")
            && line.contains("excluded_source_count=2")
            && line.contains("history_saved=false")));
    drop(lines);
    fs::write(
        run.join("cases.json"),
        serde_json::to_vec_pretty(&results).unwrap(),
    )
    .unwrap();
    println!("EVIDENCE {}", run.display());
    assert!(
        results.iter().all(|case| case["passed"] == true),
        "every native boundary case must pass"
    );
}
