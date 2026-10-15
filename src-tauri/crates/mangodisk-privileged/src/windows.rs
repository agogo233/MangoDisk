mod bootstrap;
pub mod client;
mod cpu;
pub mod deployment;
mod diagnostics;
mod pawnio_removal;
mod pci_bus;
mod security;
mod server;

use std::{ffi::OsString, io};

pub fn run(
    installer: &[u8],
    intel_module: &[u8],
    amd_module: &[u8],
) -> Result<(), Box<dyn std::error::Error>> {
    let result = execute(installer, intel_module, amd_module);
    if let Err(error) = &result {
        diagnostics::error(format!("privileged_operation_failed error={error}"));
    }
    result
}

fn execute(
    installer: &[u8],
    intel_module: &[u8],
    amd_module: &[u8],
) -> Result<(), Box<dyn std::error::Error>> {
    let arguments: Vec<OsString> = std::env::args_os().skip(1).collect();
    match arguments.as_slice() {
        [] => server::run(),
        [action] if action == "--install-cpu-temperature" => {
            let artifact = cpu::module_artifact().ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::Unsupported,
                    "verified CPU hardware unavailable",
                )
            })?;
            let module = if artifact.file_name == crate::artifacts::INTEL_MODULE.file_name {
                intel_module
            } else {
                amd_module
            };
            deployment::install(installer, module, artifact)
        }
        [action] if action == "--uninstall" => deployment::uninstall(false),
        [action] if action == "--uninstall-with-pawnio" => deployment::uninstall(true),
        [action] if action == "--health" => {
            println!(
                "{}",
                serde_json::to_string(&client::request(
                    mangodisk_privileged_protocol::Operation::Health
                )?)?
            );
            Ok(())
        }
        [action] if action == "--temperature" => {
            println!(
                "{}",
                serde_json::to_string(&client::request(
                    mangodisk_privileged_protocol::Operation::CpuTemperature
                )?)?
            );
            Ok(())
        }
        _ => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "unknown privileged service command",
        )
        .into()),
    }
}
