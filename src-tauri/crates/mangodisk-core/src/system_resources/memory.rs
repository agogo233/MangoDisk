use std::collections::BTreeMap;

use mangodisk_platform::system_resources::memory::{NativeMemorySnapshot, ProcessMemory};

use super::models::{ApplicationMemory, MemoryOverview, ProcessMemorySummary};
use crate::{applications::running_identity, CoreError, CoreResult};

pub(super) fn overview(raw: &NativeMemorySnapshot) -> CoreResult<MemoryOverview> {
    if raw.total_bytes == 0 {
        return Err(CoreError::operation_failed(
            "memory capacity is unavailable",
        ));
    }
    // Counters are sampled separately by the OS. Bound transient inconsistencies without
    // treating free memory as available memory or inventing a pressure classification.
    let used_bytes = raw.used_bytes.min(raw.total_bytes);
    Ok(MemoryOverview {
        total_bytes: raw.total_bytes,
        used_bytes,
        free_bytes: raw.free_bytes.min(raw.total_bytes),
        swap_used_bytes: raw.swap_used_bytes,
        used_percent: ((used_bytes as u128 * 100 + raw.total_bytes as u128 / 2)
            / raw.total_bytes as u128) as u8,
    })
}

pub(super) fn summarize(processes: Vec<ProcessMemory>, current_pid: u32) -> ProcessMemorySummary {
    // Resolve our grouped identity before consuming the snapshot: a helper row
    // must not accidentally expose a quit action for MangoDisk's own bundle.
    let own_path = processes
        .iter()
        .find(|process| process.pid == current_pid)
        .and_then(|process| process.executable.as_ref())
        .map(|path| running_identity::application_path(path));
    let mut groups = BTreeMap::<String, ApplicationMemory>::new();
    let mut readable_process_count = 0;
    let mut omitted_process_count = 0;
    for process in processes {
        if process.resident_bytes == 0 || process.name.trim().is_empty() {
            omitted_process_count += 1;
            continue;
        }
        readable_process_count += 1;
        let application_path = process
            .executable
            .as_deref()
            .map(running_identity::application_path);
        let is_bundle = application_path
            .as_deref()
            .is_some_and(running_identity::is_bundle);
        let id = running_identity::id(application_path.as_deref(), process.pid);
        let name = application_path
            .as_deref()
            .filter(|_| is_bundle)
            .and_then(|path| path.file_stem())
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or(process.name);
        let row = groups
            .entry(id.clone())
            .or_insert_with(|| ApplicationMemory {
                id,
                name,
                resident_bytes: 0,
                process_count: 0,
                icon_path: application_path
                    .as_ref()
                    .map(|path| path.to_string_lossy().into_owned()),
                is_bundle,
                can_quit: application_path
                    .as_deref()
                    .is_some_and(|path| running_identity::can_quit(path, own_path.as_deref())),
            });
        row.resident_bytes = row.resident_bytes.saturating_add(process.resident_bytes);
        row.process_count += 1;
    }
    let mut applications = groups.into_values().collect::<Vec<_>>();
    applications.sort_by(|left, right| {
        right
            .resident_bytes
            .cmp(&left.resident_bytes)
            .then_with(|| left.name.cmp(&right.name))
            .then_with(|| left.id.cmp(&right.id))
    });
    // Keep a useful scrollable ranking while bounding native icon requests and IPC payloads.
    applications.truncate(30);
    ProcessMemorySummary {
        applications,
        readable_process_count,
        omitted_process_count,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn process(pid: u32, path: Option<&str>, bytes: u64) -> ProcessMemory {
        ProcessMemory {
            pid,
            name: "Helper".into(),
            executable: path.map(PathBuf::from),
            resident_bytes: bytes,
        }
    }

    #[test]
    fn bundles_aggregate_helpers_but_same_names_and_unknown_images_stay_separate() {
        let summary = summarize(vec![
            process(1, Some("/Applications/Browser.app/Contents/MacOS/Browser"), 10),
            process(2, Some("/Applications/Browser.app/Contents/Frameworks/Helper.app/Contents/MacOS/Helper"), 20),
            process(3, Some("/elsewhere/Browser.app/Contents/MacOS/Browser"), 5),
            process(4, None, 4), process(5, None, 3), process(6, None, 0),
        ], u32::MAX);
        assert_eq!(summary.applications.len(), 4);
        assert_eq!(summary.applications[0].name, "Browser");
        assert_eq!(summary.applications[0].resident_bytes, 30);
        assert_eq!(summary.applications[0].process_count, 2);
        assert_eq!(summary.readable_process_count, 5);
        assert_eq!(summary.omitted_process_count, 1);
    }

    #[test]
    fn own_bundle_helpers_and_unknown_images_cannot_offer_quit() {
        let summary = summarize(
            vec![
                process(1, Some("/MangoDisk.app/Contents/MacOS/MangoDisk"), 10),
                process(
                    2,
                    Some("/MangoDisk.app/Contents/Frameworks/Helper.app/Contents/MacOS/Helper"),
                    5,
                ),
                process(3, Some("/Browser.app/Contents/MacOS/Browser"), 20),
                process(4, None, 4),
                process(5, Some("/usr/bin/node"), 3),
            ],
            1,
        );
        assert!(
            summary
                .applications
                .iter()
                .find(|row| row.name == "Browser")
                .unwrap()
                .can_quit
        );
        assert!(summary
            .applications
            .iter()
            .filter(|row| row.name != "Browser")
            .all(|row| !row.can_quit));
    }

    #[test]
    fn ranking_is_bounded_deterministic_and_resists_overflow() {
        let inputs = (1..=40)
            .map(|id| process(id, None, id as u64))
            .collect::<Vec<_>>();
        let forward = summarize(inputs.clone(), u32::MAX);
        let reverse = summarize(inputs.into_iter().rev().collect(), u32::MAX);
        assert_eq!(forward.applications.len(), 30);
        assert_eq!(forward.applications[0].resident_bytes, 40);
        assert_eq!(
            serde_json::to_value(forward).unwrap(),
            serde_json::to_value(reverse).unwrap()
        );
        let overflow = summarize(
            vec![
                process(1, Some("/app.exe"), u64::MAX),
                process(2, Some("/app.exe"), 1),
            ],
            u32::MAX,
        );
        assert_eq!(overflow.applications[0].resident_bytes, u64::MAX);
    }

    #[test]
    fn missing_capacity_is_not_zero_usage_and_racing_counters_are_bounded() {
        let mut raw = NativeMemorySnapshot {
            total_bytes: 0,
            used_bytes: 20,
            free_bytes: 30,
            swap_used_bytes: 5,
            processes: None,
        };
        assert!(overview(&raw).is_err());
        raw.total_bytes = 10;
        let result = overview(&raw).unwrap();
        assert_eq!(result.used_percent, 100);
        assert_eq!(result.free_bytes, 10);
        raw.total_bytes = u64::MAX;
        raw.used_bytes = u64::MAX;
        assert_eq!(overview(&raw).unwrap().used_percent, 100);
    }
}
