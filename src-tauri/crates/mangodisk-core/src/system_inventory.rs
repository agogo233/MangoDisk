use crate::{applications::catalog::ApplicationInventory, system_capabilities::SystemCapabilities};
#[cfg(test)]
use mangodisk_platform::InstalledApplication;
use mangodisk_platform::{current_platform, Platform, PlatformCancellation, SystemInventory};
use std::{
    sync::{Mutex, OnceLock},
    time::{Duration, Instant},
};
static SYSTEM_INVENTORY: OnceLock<Mutex<Option<CachedSystemInventory>>> = OnceLock::new();

#[derive(Debug, Clone)]
struct CachedSystemInventory {
    revision: String,
    inventory: SystemInventory,
    captured_at: Instant,
}

impl CachedSystemInventory {
    fn reusable(&self, revision: &str, now: Instant) -> bool {
        self.revision == revision
            && self
                .inventory
                .refresh_at
                .is_none_or(|deadline| now < deadline)
            && (self.inventory.capabilities_complete
                // Optional package sources must get another chance after their timeout backoff.
                || now.saturating_duration_since(self.captured_at) < Duration::from_secs(15 * 60))
    }
}

#[derive(Debug, Clone)]
pub(crate) struct SystemInventorySnapshot {
    pub(crate) applications: ApplicationInventory,
    pub(crate) capabilities: SystemCapabilities,
    pub(crate) inventory_complete: bool,
}
impl SystemInventorySnapshot {
    fn from_system(mut system: SystemInventory, inventory_complete: bool) -> Self {
        // Move application rows into their owner; do not clone a full native inventory.
        let applications = ApplicationInventory::from_system(
            SystemInventory {
                installed_applications: std::mem::take(&mut system.installed_applications),
                installed_applications_complete: system.installed_applications_complete,
                ..SystemInventory::default()
            },
            inventory_complete,
        );
        Self {
            applications,
            capabilities: SystemCapabilities::from_system(system, inventory_complete),
            inventory_complete,
        }
    }

    #[cfg(test)]
    pub(crate) fn fixture(
        applications: Vec<InstalledApplication>,
        applications_complete: bool,
    ) -> Self {
        Self::from_system(
            SystemInventory {
                installed_applications: applications,
                installed_applications_complete: applications_complete,
                developer_tools_complete: true,
                filesystem_complete: true,
                capabilities_complete: true,
                ..SystemInventory::default()
            },
            true,
        )
    }
}
#[derive(Debug)]
pub(crate) struct SystemInventoryContext {
    pub(crate) inventory: SystemInventorySnapshot,
}

impl SystemInventoryContext {
    pub(crate) fn capture() -> Self {
        Self::capture_with_revision().0
    }

    /// Custom-only cleanup rules do not depend on installed applications or
    /// platform capabilities. An empty context keeps that narrow scan from
    /// paying for an unrelated system inventory while preserving one rule
    /// applicability interface for the shared scan and execution pipelines.
    pub(crate) fn empty() -> Self {
        Self {
            inventory: SystemInventorySnapshot::from_system(SystemInventory::default(), false),
        }
    }

    /// Captures one inventory and the revision used to validate or populate
    /// its cache. Callers that need a stable before/after comparison reuse
    /// this revision instead of issuing the same expensive operating-system
    /// query twice before any useful work begins.
    pub(crate) fn capture_with_revision() -> (Self, Option<String>) {
        Self::capture_with_revision_and_cancellation(&PlatformCancellation::new(|| false))
    }

    pub(crate) fn capture_with_revision_and_cancellation(
        cancellation: &PlatformCancellation,
    ) -> (Self, Option<String>) {
        let started = Instant::now();
        let (system, inventory_complete, revision) = cached_system_inventory(cancellation);
        let inventory = SystemInventorySnapshot::from_system(system, inventory_complete);
        log::debug!(
            "application_inventory_ready application_count={} tool_count={} filesystem_count={} capability_count={} system_complete={} elapsed_ms={}",
            inventory.applications.application_count,
            inventory.capabilities.tool_count(),
            inventory.capabilities.filesystem_count(),
            inventory.capabilities.capability_count(),
            inventory.inventory_complete,
            started.elapsed().as_millis()
        );
        (Self { inventory }, revision)
    }
}

fn cached_system_inventory(
    cancellation: &PlatformCancellation,
) -> (SystemInventory, bool, Option<String>) {
    // The progress event is emitted before inventory capture so adapters can
    // cancel immediately. Honor that request before even the cheap revision
    // probe; on large application roots the probe itself is observable work.
    if cancellation.is_cancelled() {
        return (SystemInventory::default(), false, None);
    }
    let cache = SYSTEM_INVENTORY.get_or_init(|| Mutex::new(None));
    let revision = current_platform().system_inventory_revision_with_cancellation(cancellation);
    if let (Ok(revision), Ok(guard)) = (&revision, cache.lock()) {
        if let Some(cached) = guard
            .as_ref()
            .filter(|cached| cached.reusable(revision, Instant::now()))
        {
            return (cached.inventory.clone(), true, Some(revision.clone()));
        }
    }

    match current_platform().system_inventory_with_cancellation(cancellation) {
        Ok(inventory) => {
            let revision = revision.ok();
            if let Some(revision) = &revision {
                if let Ok(mut guard) = cache.lock() {
                    *guard = Some(CachedSystemInventory {
                        revision: revision.clone(),
                        inventory: inventory.clone(),
                        captured_at: Instant::now(),
                    });
                }
            }
            (inventory, true, revision)
        }
        Err(error) => {
            if cancellation.is_cancelled() {
                return (SystemInventory::default(), false, None);
            }
            log::warn!(
                "application_inventory_capture_failed error={}",
                mangodisk_platform::diagnostics::text(&error)
            );
            // A stale inventory is more useful than an empty one after a
            // revision-probe failure, but only positive matches remain safe.
            if let Ok(guard) = cache.lock() {
                if let Some(cached) = guard.as_ref() {
                    return (cached.inventory.clone(), false, None);
                }
            }
            (SystemInventory::default(), false, None)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn incomplete_capability_cache_retries_without_an_application_revision_change() {
        let now = Instant::now();
        let mut cached = CachedSystemInventory {
            revision: "first".into(),
            inventory: SystemInventory::default(),
            captured_at: now,
        };
        assert!(cached.reusable("first", now + Duration::from_secs(899)));
        assert!(!cached.reusable("first", now + Duration::from_secs(900)));
        assert!(!cached.reusable("second", now));
        cached.inventory.capabilities_complete = true;
        assert!(cached.reusable("first", now + Duration::from_secs(1800)));
    }

    #[test]
    fn resumed_inventory_preserves_the_optional_source_retry_deadline() {
        let captured_at = Instant::now();
        let cached = CachedSystemInventory {
            revision: "unchanged".into(),
            inventory: SystemInventory {
                refresh_at: Some(captured_at + Duration::from_secs(10)),
                ..SystemInventory::default()
            },
            captured_at,
        };
        assert!(cached.reusable("unchanged", captured_at + Duration::from_secs(9)));
        assert!(!cached.reusable("unchanged", captured_at + Duration::from_secs(10)));
        assert!(!cached.reusable("unchanged", captured_at + Duration::from_secs(60)));
    }

    #[test]
    fn completeness_stays_independent_for_each_native_fact_set() {
        for flags in 0..32 {
            let complete = flags & 1 != 0;
            let applications = flags & 2 != 0;
            let tools = flags & 4 != 0;
            let filesystems = flags & 8 != 0;
            let capabilities = flags & 16 != 0;
            let snapshot = SystemInventorySnapshot::from_system(
                SystemInventory {
                    installed_applications_complete: applications,
                    developer_tools_complete: tools,
                    filesystem_complete: filesystems,
                    capabilities_complete: capabilities,
                    filesystem_kinds: vec![" APFS ".into()],
                    capabilities: vec![" Native.Cleanup ".into()],
                    os_version: "15.7".into(),
                    ..SystemInventory::default()
                },
                complete,
            );
            assert_eq!(snapshot.inventory_complete, complete);
            assert_eq!(
                snapshot.applications.applications_complete(),
                complete && applications
            );
            assert_eq!(
                snapshot.capabilities.executable_inventory_complete(),
                complete && tools
            );
            assert_eq!(snapshot.capabilities.developer_tools_complete(), tools);
            assert_eq!(snapshot.capabilities.filesystem_complete(), filesystems);
            assert_eq!(snapshot.capabilities.capabilities_complete(), capabilities);
            assert!(snapshot.capabilities.has_filesystem_kind(&["apfs".into()]));
            assert!(snapshot
                .capabilities
                .has_capability(&["native.cleanup".into()]));
            assert_eq!(snapshot.capabilities.os_version(), "15.7");
        }
    }
}
