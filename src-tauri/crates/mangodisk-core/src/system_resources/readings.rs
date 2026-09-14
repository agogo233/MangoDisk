//! Independent cached results and interval calculations; no clocks, threads, or desktop APIs.
use mangodisk_platform::system_resources::{
    cpu::CpuCounters,
    disk::{ResourceVolume, VolumeCapacity},
    network::{InterfaceSample, NetworkInterface},
};
use serde::Serialize;

use super::{
    cpu::{CpuBaselineReason, CpuDelta},
    disk::{self, DiskUsage},
    disk_io::{DiskIoDelta, DiskIoRate},
    metrics::{CpuUsage, MetricId, MetricReading, MetricStatus, Trend, TrendPoint},
    models::SystemResourceSnapshot,
    network::{self, NetworkDelta, NetworkRate, NetworkSelectionReason},
};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourceReadings {
    pub schema_version: u32,
    pub observed_at_ms: u64,
    pub cpu: MetricReading<CpuUsage>,
    pub memory: MetricReading<SystemResourceSnapshot>,
    pub network: MetricReading<NetworkRate>,
    pub disk: MetricReading<DiskUsage>,
    pub disk_io: MetricReading<DiskIoRate>,
    pub interfaces: Vec<NetworkInterface>,
    pub volumes: Vec<ResourceVolume>,
    pub cpu_history: Vec<TrendPoint>,
    pub network_history: Vec<TrendPoint>,
    pub memory_history: Vec<TrendPoint>,
    pub disk_io_history: Vec<TrendPoint>,
}

impl Default for ResourceReadings {
    fn default() -> Self {
        Self {
            schema_version: 3,
            observed_at_ms: 0,
            cpu: MetricReading::default(),
            memory: MetricReading::default(),
            network: MetricReading::default(),
            disk: MetricReading::default(),
            disk_io: MetricReading::default(),
            interfaces: Vec::new(),
            volumes: Vec::new(),
            cpu_history: Vec::new(),
            network_history: Vec::new(),
            memory_history: Vec::new(),
            disk_io_history: Vec::new(),
        }
    }
}

#[derive(Default)]
pub struct ResourceCache {
    readings: ResourceReadings,
    cpu_delta: CpuDelta,
    network_delta: NetworkDelta,
    disk_io_delta: DiskIoDelta,
    cpu_history: Trend,
    network_history: Trend,
    memory_history: Trend,
    disk_io_history: Trend,
    selected_interface: Option<String>,
}

impl ResourceCache {
    pub fn cpu(
        &mut self,
        counters: CpuCounters,
        monotonic_ms: u64,
        timestamp_ms: u64,
    ) -> Option<CpuBaselineReason> {
        match self.cpu_delta.sample(counters, monotonic_ms) {
            Ok(value) => {
                self.cpu_history.push(TrendPoint {
                    sampled_at_ms: timestamp_ms,
                    primary: value.used_percent,
                    secondary: None,
                });
                self.readings.cpu = MetricReading::ready(value, timestamp_ms);
                None
            }
            Err(reason) => {
                // A rejected interval is not a new reading. Keep the last real
                // value and history with their original timestamps until expiry;
                // rebuilding one baseline must not erase an entire minute.
                self.readings
                    .cpu
                    .expire(timestamp_ms, MetricId::Cpu.freshness_ms());
                Some(reason)
            }
        }
    }

    pub fn memory(&mut self, snapshot: SystemResourceSnapshot) {
        let sampled_at_ms = snapshot.sampled_at_ms;
        self.memory_history.push(TrendPoint {
            sampled_at_ms,
            primary: f64::from(snapshot.memory.used_percent),
            secondary: None,
        });
        self.readings.memory = MetricReading::ready(snapshot, sampled_at_ms);
    }

    pub fn disk_io(
        &mut self,
        devices: Vec<mangodisk_platform::system_resources::disk_io::DeviceCounters>,
        monotonic_ms: u64,
        timestamp_ms: u64,
    ) {
        if let Some(value) = self.disk_io_delta.sample(devices, monotonic_ms) {
            self.disk_io_history.push(TrendPoint {
                sampled_at_ms: timestamp_ms,
                primary: value.read_bytes_per_second,
                secondary: Some(value.written_bytes_per_second),
            });
            self.readings.disk_io = MetricReading::ready(value, timestamp_ms);
        } else {
            self.readings.disk_io.expire(timestamp_ms, 5000);
        }
    }
    pub fn stop_disk_io(&mut self) {
        // Closing a panel pauses collection, not the history of real activity.
        self.disk_io_delta.reset();
    }
    pub fn fail_disk_io(&mut self, status: MetricStatus) {
        self.readings.disk_io.status = status;
        self.disk_io_delta.reset();
    }

    pub fn network(
        &mut self,
        interfaces: Vec<InterfaceSample>,
        manual: Option<&str>,
        monotonic_ms: u64,
        timestamp_ms: u64,
    ) -> NetworkSelectionReason {
        self.readings.interfaces = interfaces
            .iter()
            .map(|sample| sample.interface.clone())
            .collect();
        let (selected, reason) =
            network::select(&interfaces, manual, self.selected_interface.as_deref());
        let Some(selected) = selected.filter(|sample| sample.interface.connected) else {
            self.fail(MetricId::Network, MetricStatus::Disconnected);
            return reason;
        };
        if self.selected_interface.as_deref() != Some(&selected.interface.id) {
            self.network_history.clear();
            self.readings.network = MetricReading::default();
        }
        self.selected_interface = Some(selected.interface.id.clone());
        let Some(counters) = selected.counters else {
            self.fail(MetricId::Network, MetricStatus::Failed);
            return reason;
        };
        if let Some(value) =
            self.network_delta
                .sample(&selected.interface, counters, monotonic_ms, reason)
        {
            self.network_history.push(TrendPoint {
                sampled_at_ms: timestamp_ms,
                primary: value.received_bytes_per_second,
                secondary: Some(value.transmitted_bytes_per_second),
            });
            self.readings.network = MetricReading::ready(value, timestamp_ms);
        } else {
            self.readings
                .network
                .expire(timestamp_ms, MetricId::Network.freshness_ms());
        }
        reason
    }

    pub fn volumes(&mut self, volumes: Vec<ResourceVolume>) {
        self.readings.volumes = volumes;
    }

    pub fn disk(&mut self, volume: ResourceVolume, capacity: VolumeCapacity, timestamp_ms: u64) {
        if let Some(value) = disk::usage(volume, capacity) {
            self.readings.disk = MetricReading::ready(value, timestamp_ms);
        } else {
            self.fail(MetricId::Disk, MetricStatus::Failed);
        }
    }

    pub fn fail(&mut self, metric: MetricId, status: MetricStatus) {
        match metric {
            MetricId::Cpu => {
                self.readings.cpu.status = status;
                self.cpu_delta.reset();
            }
            MetricId::Memory => self.readings.memory.status = status,
            MetricId::Network => {
                self.readings.network.status = status;
                self.network_delta.reset();
            }
            MetricId::Disk => self.readings.disk.status = status,
        }
    }

    /// Pausing collection preserves real history and original freshness. A new
    /// baseline prevents rates from averaging across the disabled interval.
    pub fn suspend(&mut self, metric: MetricId) {
        match metric {
            MetricId::Cpu => self.cpu_delta.reset(),
            MetricId::Network => self.network_delta.reset(),
            MetricId::Memory | MetricId::Disk => {}
        }
    }

    /// A changed source must not attribute an old device's history to a new one.
    pub fn reset(&mut self, metric: MetricId) {
        match metric {
            MetricId::Cpu => {
                self.cpu_delta.reset();
                self.cpu_history.clear();
                self.readings.cpu = MetricReading::default();
            }
            MetricId::Memory => {
                self.readings.memory = MetricReading::default();
                self.memory_history.clear();
            }
            MetricId::Network => {
                self.network_delta.reset();
                self.network_history.clear();
                self.readings.network = MetricReading::default();
            }
            MetricId::Disk => self.readings.disk = MetricReading::default(),
        }
    }

    pub fn snapshot(&mut self, now_ms: u64) -> ResourceReadings {
        self.readings.observed_at_ms = now_ms;
        self.readings
            .cpu
            .expire(now_ms, MetricId::Cpu.freshness_ms());
        self.readings
            .memory
            .expire(now_ms, MetricId::Memory.freshness_ms());
        self.readings
            .network
            .expire(now_ms, MetricId::Network.freshness_ms());
        self.readings
            .disk
            .expire(now_ms, MetricId::Disk.freshness_ms());
        self.readings.disk_io.expire(now_ms, 5000);
        self.readings.memory_history = self.memory_history.snapshot(now_ms);
        self.readings.disk_io_history = self.disk_io_history.snapshot(now_ms);
        self.readings.cpu_history = self.cpu_history.snapshot(now_ms);
        self.readings.network_history = self.network_history.snapshot(now_ms);
        self.readings.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn network_suspend_preserves_history_but_switching_interfaces_clears_it() {
        use mangodisk_platform::system_resources::network::{InterfaceKind, NetworkCounters};
        let sample = |id: &str, received| {
            vec![InterfaceSample {
                interface: NetworkInterface {
                    id: id.into(),
                    name: "test".into(),
                    kind: InterfaceKind::Ethernet,
                    connected: true,
                    physical: true,
                    default_route_metric: Some(1),
                },
                counters: Some(NetworkCounters {
                    received,
                    transmitted: 0,
                }),
            }]
        };
        let mut cache = ResourceCache::default();
        cache.network(sample("one", 0), None, 0, 0);
        cache.network(sample("one", 1000), None, 1000, 1000);
        cache.suspend(MetricId::Network);
        cache.network(sample("one", 2000), None, 2000, 2000);
        assert_eq!(cache.snapshot(2000).network_history.len(), 1);
        assert_eq!(cache.snapshot(2000).network.sampled_at_ms, Some(1000));
        cache.network(sample("two", 5000), None, 3000, 3000);
        assert!(cache.snapshot(3000).network_history.is_empty());
        assert_eq!(cache.snapshot(3000).network.status, MetricStatus::Loading);
        cache.network(sample("two", 6000), None, 4000, 4000);
        assert_eq!(cache.snapshot(4000).network_history.len(), 1);
    }

    #[test]
    fn disk_activity_preserves_real_history_while_rebuilding_a_baseline() {
        use mangodisk_platform::system_resources::disk_io::DeviceCounters;
        let mut cache = ResourceCache::default();
        let sample = |value| {
            vec![DeviceCounters {
                id: "device".into(),
                read_bytes: value,
                written_bytes: value,
            }]
        };
        cache.disk_io(sample(0), 0, 0);
        assert_eq!(cache.snapshot(0).disk_io.status, MetricStatus::Loading);
        cache.disk_io(sample(2000), 2000, 2000);
        let snapshot = cache.snapshot(2000);
        assert_eq!(
            snapshot.disk_io.value.unwrap().read_bytes_per_second,
            1000.0
        );
        assert_eq!(snapshot.disk.status, MetricStatus::Loading);
        assert_eq!(snapshot.disk_io_history.len(), 1);
        cache.fail_disk_io(MetricStatus::Unsupported);
        assert_eq!(
            cache.snapshot(3000).disk_io.status,
            MetricStatus::Unsupported
        );
        cache.disk_io(sample(4000), 4000, 4000);
        assert_eq!(cache.snapshot(4000).disk_io_history.len(), 1);
        cache.disk_io(sample(6000), 6000, 6000);
        assert_eq!(cache.snapshot(11001).disk_io.status, MetricStatus::Stale);
        cache.stop_disk_io();
        cache.disk_io(sample(8000), 8000, 8000);
        assert_eq!(cache.snapshot(12000).disk_io.status, MetricStatus::Stale);
    }

    #[test]
    fn memory_history_contains_occupancy_and_clears_on_demand_reset() {
        let mut cache = ResourceCache::default();
        for time in [0, 3000, 6000] {
            cache.memory(SystemResourceSnapshot {
                schema_version: 1,
                sampled_at_ms: time,
                memory: super::super::models::MemoryOverview {
                    total_bytes: 100,
                    used_bytes: 80,
                    free_bytes: 20,
                    swap_used_bytes: 5,
                    used_percent: 80,
                },
                processes: None,
            });
        }
        assert_eq!(cache.snapshot(6000).memory_history.len(), 3);
        assert_eq!(cache.snapshot(6000).memory_history[0].primary, 80.0);
        cache.reset(MetricId::Memory);
        assert!(cache.snapshot(6000).memory_history.is_empty());
    }

    #[test]
    fn cpu_recovery_and_panel_suspend_preserve_real_readings_until_expiry() {
        let mut cache = ResourceCache::default();
        let first = CpuCounters { busy: 10, idle: 90 };
        assert_eq!(cache.cpu(first, 0, 0), Some(CpuBaselineReason::FirstSample));
        let next = CpuCounters {
            busy: 20,
            idle: 180,
        };
        assert_eq!(cache.cpu(next, 2000, 2000), None);
        assert_eq!(
            cache.cpu(next, 4000, 4000),
            Some(CpuBaselineReason::NoProgress)
        );
        let reading = cache.snapshot(4000);
        assert_eq!(reading.cpu.status, MetricStatus::Ready);
        assert_eq!(reading.cpu.sampled_at_ms, Some(2000));
        assert_eq!(reading.cpu_history.len(), 1);
        cache.suspend(MetricId::Cpu);
        assert_eq!(
            cache.cpu(next, 4200, 4200),
            Some(CpuBaselineReason::FirstSample)
        );
        assert_eq!(cache.snapshot(4200).cpu.status, MetricStatus::Ready);
        assert_eq!(cache.snapshot(7001).cpu.status, MetricStatus::Stale);
        assert_eq!(cache.snapshot(7001).cpu_history.len(), 1);
        assert_eq!(
            cache.cpu(
                CpuCounters {
                    busy: 30,
                    idle: 270
                },
                8000,
                8000
            ),
            None
        );
        assert_eq!(cache.snapshot(8000).cpu.status, MetricStatus::Ready);
    }

    #[test]
    fn one_metric_failure_preserves_other_values_and_expires_them_independently() {
        let mut cache = ResourceCache::default();
        let _ = cache.cpu(CpuCounters { busy: 0, idle: 0 }, 0, 0);
        let _ = cache.cpu(CpuCounters { busy: 10, idle: 90 }, 2000, 2000);
        cache.fail(MetricId::Network, MetricStatus::Failed);
        let values = cache.snapshot(2000);
        assert_eq!(values.cpu.status, MetricStatus::Ready);
        assert_eq!(values.network.status, MetricStatus::Failed);
        assert_eq!(values.cpu.value.unwrap().used_percent, 10.0);
        assert_eq!(cache.snapshot(7001).cpu.status, MetricStatus::Stale);
        cache.reset(MetricId::Cpu);
        let resumed = cache.snapshot(8000);
        assert_eq!(resumed.cpu.status, MetricStatus::Loading);
        assert!(resumed.cpu_history.is_empty());
        assert_eq!(resumed.network.status, MetricStatus::Failed);
    }
}
