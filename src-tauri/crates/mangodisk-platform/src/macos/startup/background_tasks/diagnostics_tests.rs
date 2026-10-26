use super::*;

#[test]
#[ignore = "installs a process-wide diagnostic logger; run this test in isolation"]
fn archive_diagnostics_identify_native_errors_and_schema_mismatches() {
    struct Evidence(std::sync::Mutex<Vec<String>>);
    impl log::Log for Evidence {
        fn enabled(&self, _: &log::Metadata<'_>) -> bool {
            true
        }
        fn log(&self, record: &log::Record<'_>) {
            self.0.lock().unwrap().push(record.args().to_string());
        }
        fn flush(&self) {}
    }
    static LOG: Evidence = Evidence(std::sync::Mutex::new(Vec::new()));
    log::set_logger(&LOG).expect("isolated process owns the logger");
    log::set_max_level(log::LevelFilter::Info);
    let fixture = tempfile::tempdir().unwrap();
    let path = fixture.path().join("BackgroundItems-v14\n.btm");
    let error = std::io::Error::from_raw_os_error(13);
    assert_eq!(
        archive_io_error(&path, "read_metadata", error),
        ParseError::AccessDenied
    );
    fs::write(&path, b"bplist00private-payload-must-not-be-logged").unwrap();
    assert_eq!(
        read_archive(&path, &fs::metadata(&path).unwrap()),
        Err(ParseError::InvalidData)
    );
    report_archive_parse_failure(&path, ParseError::Unsupported, 13, Some(14));
    let logs = LOG.0.lock().unwrap();
    assert!(logs.iter().any(|line| line.contains("stage=read_metadata")
        && line.contains("native_code=Some(13)")
        && line.contains("PermissionDenied")));
    assert!(logs
        .iter()
        .any(|line| line.contains("stage=decode_plist") && line.contains("outcome=unavailable")));
    assert!(logs.iter().any(
        |line| line.contains("expected_version=13 observed_version=Some(14)")
            && line.contains("reason=Unsupported")
    ));
    assert!(logs
        .iter()
        .all(|line| line.contains("BackgroundItems-v14\\n.btm")
            && !line.contains('\n')
            && !line.contains("private-payload")));
}
