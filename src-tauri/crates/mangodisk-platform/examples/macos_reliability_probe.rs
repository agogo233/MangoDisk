use mangodisk_platform::{
    current_platform,
    system_resources::process_cpu::{ProcessCpuSampler, ProcessCpuSource},
    FastAnalysisQuery, NameExclusions, Platform, ScanPurpose,
};
use std::{
    path::PathBuf,
    process::ExitCode,
    time::{Duration, Instant},
};

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    match args.next().as_deref() {
        Some("fixture") => {
            let root = PathBuf::from(args.next().expect("fixture requires a root"));
            std::fs::create_dir(&root).expect("fixture root must not already exist");
            for group in 0..256 {
                let directory = root.join(format!("group-{group:03}"));
                std::fs::create_dir(&directory).expect("create fixture directory");
                for file in 0_u8..32 {
                    std::fs::write(directory.join(format!("file-{file:02}.bin")), [file; 1024])
                        .expect("write fixture file");
                }
            }
            println!("{}", serde_json::json!({"files": 8192, "directories": 257}));
        }
        Some("scan") => {
            let root = PathBuf::from(args.next().expect("scan requires a root"));
            let budget_secs = args
                .next()
                .map(|s| s.parse::<u64>().expect("invalid scan budget"))
                .unwrap_or(180);
            let start = Instant::now();
            let mut records = 0_u64;
            let result = current_platform().fast_analysis_records(
                FastAnalysisQuery {
                    retained_file_limit: 64,
                    name_exclusions: &NameExclusions::default(),
                    excluded_roots: &[],
                    root: &root,
                    purpose: ScanPurpose::Analysis,
                    large_file_minimum_bytes: 100 * 1024 * 1024,
                    should_prune_directory: |_| false,
                },
                &|| start.elapsed() > Duration::from_secs(budget_secs),
                &mut |_, _, _| {},
                &mut |_| {
                    records += 1;
                    Ok(())
                },
            );
            println!(
                "{}",
                serde_json::json!({
                    "elapsedMs": start.elapsed().as_millis(), "records": records,
                    "summary": result.as_ref().ok().map(|s| format!("{s:?}")),
                    "error": result.as_ref().err().map(|e| format!("{e:?}")),
                })
            );
            return if result.is_err() {
                ExitCode::FAILURE
            } else {
                ExitCode::SUCCESS
            };
        }
        Some("cpu") => {
            let mut source = ProcessCpuSampler::default();
            for _ in 0..20 {
                let start = Instant::now();
                let snapshot = source.sample().expect("read CPU snapshot");
                println!(
                    "{}",
                    serde_json::json!({
                        "elapsedMs": start.elapsed().as_millis(),
                        "processes": snapshot.processes.len(),
                        "readable": snapshot.processes.iter().filter(|p| p.cpu_time_ms.is_some()).count(),
                    })
                );
                std::thread::sleep(Duration::from_millis(500));
            }
        }
        _ => panic!("expected fixture ROOT, scan ROOT [BUDGET_SECS], or cpu"),
    }
    ExitCode::SUCCESS
}
