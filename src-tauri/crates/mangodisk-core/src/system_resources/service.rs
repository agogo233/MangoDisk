use mangodisk_platform::system_resources::memory::{MemorySampler, MemorySource};

use super::{memory, models::SystemResourceSnapshot};
use crate::CoreResult;

pub struct SystemResourceService<S = MemorySampler> {
    source: S,
}

impl Default for SystemResourceService {
    fn default() -> Self {
        Self::new(MemorySampler::default())
    }
}

impl<S: MemorySource> SystemResourceService<S> {
    pub fn new(source: S) -> Self {
        Self { source }
    }

    /// The adapter owns cadence and the clock. Each call produces a fresh snapshot, so
    /// process exits cannot leave stale rows and overview-only samples remain inexpensive.
    pub fn sample(
        &mut self,
        include_processes: bool,
        sampled_at_ms: u64,
    ) -> CoreResult<SystemResourceSnapshot> {
        let raw = self.source.sample(include_processes)?;
        let memory = memory::overview(&raw)?;
        Ok(SystemResourceSnapshot {
            schema_version: 1,
            sampled_at_ms,
            memory,
            processes: raw
                .processes
                .map(|processes| memory::summarize(processes, std::process::id())),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mangodisk_platform::{
        system_resources::memory::{NativeMemorySnapshot, ProcessMemory},
        PlatformResult,
    };

    struct Source {
        reads: u32,
    }
    impl MemorySource for Source {
        fn sample(&mut self, details: bool) -> PlatformResult<NativeMemorySnapshot> {
            self.reads += 1;
            Ok(NativeMemorySnapshot {
                total_bytes: 100,
                used_bytes: 45,
                free_bytes: 55,
                swap_used_bytes: 2,
                processes: details.then(|| {
                    if self.reads == 1 {
                        vec![ProcessMemory {
                            pid: 1,
                            name: "Editor".into(),
                            executable: None,
                            resident_bytes: 10,
                        }]
                    } else {
                        vec![]
                    }
                }),
            })
        }
    }

    #[test]
    fn protocol_distinguishes_unrequested_processes_from_exited_processes() {
        let mut service = SystemResourceService::new(Source { reads: 0 });
        let first = service.sample(true, 10).unwrap();
        assert_eq!(first.processes.unwrap().applications.len(), 1);
        assert!(service
            .sample(true, 20)
            .unwrap()
            .processes
            .unwrap()
            .applications
            .is_empty());
        let overview = service.sample(false, 30).unwrap();
        let json = serde_json::to_value(overview).unwrap();
        assert_eq!(json["schemaVersion"], 1);
        assert_eq!(json["sampledAtMs"], 30);
        assert_eq!(json["memory"]["usedPercent"], 45);
        assert!(json["processes"].is_null());
    }
}
