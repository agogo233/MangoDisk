use std::path::PathBuf;

use sysinfo::{MemoryRefreshKind, ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};

use crate::{PlatformError, PlatformErrorCode, PlatformResult};

#[derive(Debug, Clone)]
pub struct ProcessMemory {
    pub pid: u32,
    pub name: String,
    pub executable: Option<PathBuf>,
    pub resident_bytes: u64,
}

#[derive(Debug, Clone)]
pub struct NativeMemorySnapshot {
    pub total_bytes: u64,
    pub used_bytes: u64,
    pub free_bytes: u64,
    pub swap_used_bytes: u64,
    /// None means no process enumeration was requested, not an empty process list.
    pub processes: Option<Vec<ProcessMemory>>,
}

/// Allows deterministic Core tests without replacing native APIs in production.
pub trait MemorySource: Send {
    fn sample(&mut self, include_processes: bool) -> PlatformResult<NativeMemorySnapshot>;
}

pub struct MemorySampler {
    system: System,
}

impl Default for MemorySampler {
    fn default() -> Self {
        // Do not load CPU topology, disks, users, or process command lines at startup.
        Self {
            system: System::new(),
        }
    }
}

impl MemorySource for MemorySampler {
    fn sample(&mut self, include_processes: bool) -> PlatformResult<NativeMemorySnapshot> {
        self.system
            .refresh_memory_specifics(MemoryRefreshKind::everything());
        if self.system.total_memory() == 0 {
            return Err(PlatformError::new(
                PlatformErrorCode::OperationFailed,
                "system memory counters are unavailable",
            ));
        }
        let processes = include_processes.then(|| {
            // Memory and image paths are the only required fields. Remove exited processes
            // on every refresh, and never request environment variables or command lines.
            self.system.refresh_processes_specifics(
                ProcessesToUpdate::All,
                true,
                ProcessRefreshKind::nothing()
                    .with_memory()
                    .with_exe(UpdateKind::OnlyIfNotSet),
            );
            self.system
                .processes()
                .iter()
                .map(|(pid, process)| ProcessMemory {
                    pid: pid.as_u32(),
                    name: process.name().to_string_lossy().into_owned(),
                    executable: process.exe().map(PathBuf::from),
                    resident_bytes: process.memory(),
                })
                .collect()
        });
        Ok(NativeMemorySnapshot {
            total_bytes: self.system.total_memory(),
            used_bytes: self.system.used_memory(),
            // Expose the native free-page counter explicitly. sysinfo's macOS
            // "available" estimate can saturate at zero with a large compressor;
            // free memory is intentionally not presented as memory pressure.
            free_bytes: self.system.free_memory(),
            swap_used_bytes: self.system.used_swap(),
            processes,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_sampling_can_skip_processes_and_observe_this_process() {
        let mut sampler = MemorySampler::default();
        let overview = sampler
            .sample(false)
            .expect("memory counters should be readable");
        assert!(overview.total_bytes > 0);
        assert!(overview.processes.is_none());
        let detailed = sampler
            .sample(true)
            .expect("process sampling should complete");
        assert!(detailed
            .processes
            .unwrap()
            .iter()
            .any(|process| { process.pid == std::process::id() && process.resident_bytes > 0 }));
    }
}
