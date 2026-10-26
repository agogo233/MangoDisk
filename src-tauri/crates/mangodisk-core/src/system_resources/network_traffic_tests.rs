use super::*;

#[test]
fn restart_switch_and_reset_preserve_independent_exact_totals() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("traffic.json");
    let traffic = NetworkTraffic::open(path.clone()).unwrap();
    assert!(traffic.total("lan").is_none());
    traffic.record("lan", 100, 200, 1000, 2000).unwrap();
    traffic.record("wifi", 20, 30, 2000, 3000).unwrap();
    traffic.record("lan", 300, 400, 3000, 4000).unwrap();
    assert_eq!(traffic.total("lan").unwrap().received_bytes, "400");
    assert!(traffic.flush(true).unwrap());
    drop(traffic);
    let traffic = NetworkTraffic::open(path).unwrap();
    assert_eq!(traffic.total("wifi").unwrap().transmitted_bytes, "30");
    assert_eq!(traffic.total("lan").unwrap().started_at_ms, 1000);
    traffic.reset("lan", 1000, 5000).unwrap();
    assert_eq!(
        traffic
            .record("lan", 999, 999, 4500, 5500)
            .unwrap()
            .received_bytes,
        "0"
    );
    assert_eq!(
        traffic
            .record("lan", 3, 4, 5500, 6500)
            .unwrap()
            .received_bytes,
        "3"
    );
    assert_eq!(traffic.total("wifi").unwrap().received_bytes, "20");
    assert!(traffic.reset("lan", 1000, 7000).is_err());
    traffic.flush(true).unwrap();
    let before = traffic.total("lan").unwrap().received_bytes;
    traffic.record("lan", 9, 9, 7000, 8000);
    assert_eq!(traffic.total("lan").unwrap().received_bytes, before);
}

#[test]
fn failed_reset_preserves_memory_and_previous_history() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("traffic.json");
    let traffic = NetworkTraffic::open(path.clone()).unwrap();
    traffic.record("lan", 10, 20, 1000, 2000);
    traffic.flush(false).unwrap();
    let previous = fs::read(&path).unwrap();
    fs::remove_file(&path).unwrap();
    fs::create_dir(&path).unwrap();
    assert!(traffic.reset("lan", 1000, 3000).is_err());
    assert_eq!(traffic.total("lan").unwrap().received_bytes, "10");
    fs::remove_dir(&path).unwrap();
    fs::write(&path, previous).unwrap();
    assert_eq!(traffic.total("lan").unwrap().started_at_ms, 1000);
}

#[test]
fn malformed_future_oversized_history_and_second_writer_are_rejected_without_overwrite() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("traffic.json");
    for bytes in [
        b"broken".to_vec(),
        br#"{"schemaVersion":2,"interfaces":{}}"#.to_vec(),
        vec![b' '; MAX_FILE_BYTES as usize + 1],
    ] {
        fs::write(&path, &bytes).unwrap();
        assert!(NetworkTraffic::open(path.clone()).is_err());
        assert_eq!(fs::read(&path).unwrap(), bytes);
    }
    fs::remove_file(&path).unwrap();
    let _first = NetworkTraffic::open(path.clone()).unwrap();
    assert!(NetworkTraffic::open(path).is_err());
}

#[test]
fn zero_overflow_invalid_intervals_and_bounded_interfaces_are_safe() {
    let directory = tempfile::tempdir().unwrap();
    let traffic = NetworkTraffic::open(directory.path().join("traffic.json")).unwrap();
    assert!(traffic.record("", 1, 1, 1000, 2000).is_none());
    assert!(traffic.record("lan", 1, 1, 2000, 1000).is_none());
    assert!(traffic.record("lan", 1, 1, 0, 1000).is_none());
    assert!(!traffic.flush(false).unwrap());
    traffic.record("lan", 0, 0, 1000, 2000);
    assert_eq!(traffic.total("lan").unwrap().received_bytes, "0");
    traffic.record("lan", u64::MAX, u64::MAX, 2000, 3000);
    traffic.record("lan", 1, 1, 3000, 4000);
    traffic.flush(false).unwrap();
    assert_eq!(
        traffic.total("lan").unwrap().received_bytes,
        u64::MAX.to_string()
    );
    for id in 0..MAX_INTERFACES {
        traffic.record(&id.to_string(), 1, 1, 1000, 2000);
    }
    assert!(traffic.record("excess", 1, 1, 1000, 2000).is_none());
    assert!(traffic.record("lan", 0, 0, 5000, 6000).is_some());
}

#[test]
fn concurrent_periodic_save_cannot_restore_pre_reset_totals() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("traffic.json");
    let traffic = std::sync::Arc::new(NetworkTraffic::open(path.clone()).unwrap());
    traffic.record("lan", 100, 200, 1000, 2000);
    let writer = traffic.clone();
    let thread = std::thread::spawn(move || {
        for _ in 0..20 {
            writer.flush(false).unwrap();
        }
    });
    traffic.reset("lan", 1000, 3000).unwrap();
    thread.join().unwrap();
    drop(traffic);
    let traffic = NetworkTraffic::open(path).unwrap();
    assert_eq!(traffic.total("lan").unwrap().received_bytes, "0");
    assert_eq!(traffic.total("lan").unwrap().started_at_ms, 3000);
}

#[test]
fn idle_and_reset_overlapping_intervals_do_not_dirty_saved_history() {
    let directory = tempfile::tempdir().unwrap();
    let traffic = NetworkTraffic::open(directory.path().join("traffic.json")).unwrap();
    traffic.record("lan", 0, 0, 1000, 2000);
    assert!(traffic.flush(false).unwrap());
    traffic.record("lan", 0, 0, 2000, 3000);
    assert!(!traffic.flush(false).unwrap());
    traffic.reset("lan", 1000, 4000).unwrap();
    traffic.record("lan", 999, 999, 3500, 4500);
    assert!(!traffic.flush(false).unwrap());
    traffic.record("lan", 10, 20, 4500, 5500);
    assert!(traffic.flush(false).unwrap());
    assert_eq!(traffic.total("lan").unwrap().received_bytes, "10");
}
