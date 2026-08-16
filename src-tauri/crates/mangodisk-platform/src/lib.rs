mod command;
mod contracts;
mod current;
mod file_icon;
mod inventory;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(windows)]
mod windows;

#[cfg(windows)]
pub use command::configure_background_process;
pub use command::{
    run_controlled_command, ControlledCommandError, ControlledCommandLimits,
    ControlledCommandOutput, ControlledEnvironmentPolicy, ControlledExecutable,
};
pub use contracts::*;
pub use current::{application_directories, current_platform, CurrentPlatform};
pub use file_icon::{
    NativeFileIconAsset, NativeFileIconAssignment, NativeFileIconItemKind,
    NativeFileIconLoadResult, NativeFileIconMode, NativeFileIconRequest, NativeFileIconService,
};
#[cfg(target_os = "macos")]
pub use macos::{
    macos_privileged_application_removal_supported, remove_application_bundle_with_privileges,
};
#[cfg(windows)]
pub use windows::{
    execute_windows_disk_cleanup, fresh_windows_disk_cleanup_estimates,
    windows_disk_cleanup_estimates,
};
