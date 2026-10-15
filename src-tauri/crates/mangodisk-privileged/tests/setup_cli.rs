#![cfg(windows)]

use std::process::Command;

#[test]
fn setup_commands_reject_external_paths_and_extra_actions() {
    for arguments in [
        ["--install-cpu-temperature", r"C:\untrusted-vendor-stage"],
        ["--uninstall", r"C:\untrusted-vendor-stage"],
        ["--uninstall-with-pawnio", r"C:\untrusted-vendor-stage"],
        ["--install-cpu-temperature", "--uninstall"],
    ] {
        let result = Command::new(env!("CARGO_BIN_EXE_MangoDiskPrivileged"))
            .args(arguments)
            .output()
            .expect("the compiled helper must run");
        assert_eq!(result.status.code(), Some(1));
        assert!(result.stdout.is_empty());
        assert!(
            String::from_utf8_lossy(&result.stderr).contains("unknown privileged service command")
        );
    }
}
