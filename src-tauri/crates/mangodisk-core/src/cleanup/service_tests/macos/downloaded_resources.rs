#[test]
fn downloaded_resources_use_explicit_opt_in_file_boundaries() {
    let rules = registry().unwrap();
    for (id, root_count) in [
        ("system.aerial-video-downloads", 1),
        ("system.ios-firmware-downloads", 2),
    ] {
        let rule = rules.iter().find(|rule| rule.id == id).unwrap();
        assert_eq!(rule.roots.len(), root_count);
        assert!(!rule.default_selected);
        assert!(!rule.recommended_selected);
        assert!(!rule.deletes_whole_root());
        assert!(!rule.remove_empty_directories);
        assert_eq!(rule.risk, crate::cleanup::rules::RuleRiskLevel::Recoverable);
        assert!(rule.roots.iter().all(|root| {
            let path = root.to_string_lossy();
            !path.ends_with("Customer")
                && !path.ends_with("com.apple.wallpaper")
                && !path.contains("MobileSync")
                && !path.contains("Downloads")
                && !path.starts_with("/Applications")
                && !path.contains("idleassetsd")
        }));
    }
}

#[test]
fn downloaded_resources_preview_and_delete_preserve_non_candidates() {
    let _operation_lock = crate::shared::operation::test_operation_lock();
    let catalog = registry().unwrap();
    for (id, extension) in [
        ("system.aerial-video-downloads", "mov"),
        ("system.ios-firmware-downloads", "ipsw"),
    ] {
        let sandbox = tempfile::tempdir().unwrap();
        let root = sandbox.path().canonicalize().unwrap().join("downloads");
        fs::create_dir_all(root.join("nested")).unwrap();
        fs::create_dir_all(root.join(format!("directory.{extension}"))).unwrap();
        let selected = root.join(format!("completed.{extension}"));
        let recent = root.join(format!("recent.{extension}"));
        let nested = root.join("nested").join(format!("keep.{extension}"));
        let partial = root.join(format!("active.{extension}.download"));
        let unrelated = root.join("entries.json");
        let directory_file = root.join(format!("directory.{extension}/keep.{extension}"));
        let outside = sandbox.path().join(format!("outside.{extension}"));
        for file in [
            &selected,
            &recent,
            &nested,
            &partial,
            &unrelated,
            &outside,
            &directory_file,
        ] {
            fs::write(file, b"downloaded resource fixture").unwrap();
        }
        for file in [&selected, &nested, &partial, &unrelated, &outside] {
            fs::File::options()
                .write(true)
                .open(file)
                .unwrap()
                .set_times(
                    fs::FileTimes::new()
                        .set_modified(SystemTime::now() - Duration::from_secs(8 * 86400)),
                )
                .unwrap();
        }
        let link = root.join(format!("linked.{extension}"));
        symlink(&outside, &link).unwrap();
        let mut rule = catalog.iter().find(|rule| rule.id == id).unwrap().clone();
        rule.roots = vec![root.clone()];
        let plan = compile_scan_plan(vec![rule], &[true], &[]).unwrap();
        let measurement =
            measure_owned_rule(&plan, 0, None, &CleanupExclusions::default()).unwrap();
        let expected_count = if extension == "mov" { 2 } else { 1 };
        assert_eq!(measurement.file_count, expected_count);
        assert_eq!(measurement.bytes, 27 * expected_count);
        let snapshot = ProcessSnapshot::default();
        let operation = OperationGuard::start(CoordinatedOperationKind::Cleanup).unwrap();
        let context = RuleExecutionContext {
            ownership_plan: &plan,
            process_snapshot: &snapshot,
            source_scope: None,
            empty_directory_authorizations: None,
            operation: &operation,
            dry_run: true,
        };
        let preview = execute_rule(
            &plan.rules[0],
            0,
            Some(measurement),
            &context,
            &mut |_, _| {},
        );
        assert_eq!(preview.released_bytes, 0);
        assert!(selected.exists());
        assert!(recent.exists());
        let context = RuleExecutionContext {
            dry_run: false,
            ..context
        };
        let result = execute_rule(&plan.rules[0], 0, None, &context, &mut |_, _| {});
        operation.complete();
        assert_eq!(result.affected_item_count, expected_count, "{result:?}");
        assert_eq!(result.released_bytes, 27 * expected_count);
        assert!(!selected.exists());
        assert_eq!(recent.exists(), extension == "ipsw");
        for file in [&nested, &partial, &unrelated, &outside, &directory_file] {
            assert_eq!(fs::read(file).unwrap(), b"downloaded resource fixture");
        }
        assert!(link.symlink_metadata().unwrap().file_type().is_symlink());
        assert!(root.join(format!("directory.{extension}")).is_dir());
        assert!(root.is_dir());
    }
}

#[test]
fn downloaded_resources_missing_and_linked_roots_do_not_delete_targets() {
    let _operation_lock = crate::shared::operation::test_operation_lock();
    let sandbox = tempfile::tempdir().unwrap();
    let outside = sandbox.path().canonicalize().unwrap().join("outside");
    fs::create_dir(&outside).unwrap();
    let file = outside.join("keep.mov");
    fs::write(&file, b"must remain").unwrap();
    let linked_root = sandbox.path().join("linked-root");
    symlink(&outside, &linked_root).unwrap();
    let mut rule = registry()
        .unwrap()
        .into_iter()
        .find(|rule| rule.id == "system.aerial-video-downloads")
        .unwrap();
    rule.roots = vec![sandbox.path().join("missing"), linked_root];
    let plan = compile_scan_plan(vec![rule], &[true], &[]).unwrap();
    let snapshot = ProcessSnapshot::default();
    let operation = OperationGuard::start(CoordinatedOperationKind::Cleanup).unwrap();
    let result = execute_rule(
        &plan.rules[0],
        0,
        None,
        &RuleExecutionContext {
            ownership_plan: &plan,
            process_snapshot: &snapshot,
            source_scope: None,
            empty_directory_authorizations: None,
            operation: &operation,
            dry_run: false,
        },
        &mut |_, _| {},
    );
    operation.complete();
    assert_eq!(result.affected_item_count, 0);
    assert!(result.failed_item_count > 0);
    assert_eq!(fs::read(file).unwrap(), b"must remain");
}

#[test]
#[ignore = "read-only preview of real downloaded resources; run explicitly on macOS"]
fn downloaded_resources_real_read_only_preview() {
    let _operation_lock = crate::shared::operation::test_operation_lock();
    let started = Instant::now();
    let result = CleanupService::execute(CleanupRequest {
        rule_ids: vec![
            "system.aerial-video-downloads".into(),
            "system.ios-firmware-downloads".into(),
        ],
        dry_run: true,
        project_roots: Vec::new(),
        source_selections: Vec::new(),
    })
    .unwrap();
    assert!(result.dry_run);
    assert_eq!(result.released_bytes, 0);
    assert_eq!(result.actions.len(), 2);
    println!(
        "downloaded_resources_real_preview elapsed_us={} result={}",
        started.elapsed().as_micros(),
        serde_json::to_string(&result).unwrap()
    );
}
