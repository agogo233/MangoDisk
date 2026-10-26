//! Preserve each active temperature source's cadence without retaining native handles.
use mangodisk_platform::{
    system_resources::{
        disk::ResourceVolume, disk_health::DiskHealth, disk_temperature::DiskTemperature,
    },
    PlatformResult,
};

type Reading = (u64, PlatformResult<DiskTemperature>);
type Identity = (String, String, Vec<String>);
const INTERVAL_MS: u64 = 30_000;

#[derive(Default)]
pub(super) struct TemperatureSampler {
    cached: Vec<(Identity, Reading)>,
}

impl TemperatureSampler {
    pub fn read(
        &mut self,
        volume: &ResourceVolume,
        now_ms: u64,
        health: Option<&(u64, PlatformResult<DiskHealth>)>,
        state: &mut Option<String>,
    ) -> Reading {
        self.read_with(volume, now_ms, || {
            let reused = health
                .filter(|(sampled, _)| *sampled == now_ms)
                .and_then(|(_, result)| result.as_ref().ok())
                .filter(|health| health.volume_id == volume.id)
                .and_then(|health| {
                    health.temperature_celsius.map(|celsius| DiskTemperature {
                        volume_id: volume.id.clone(),
                        volume_name: volume.name.clone(),
                        celsius,
                        source: health.source,
                    })
                });
            let result = reused.map(Ok).unwrap_or_else(|| {
                mangodisk_platform::system_resources::disk_temperature::read(volume)
            });
            let outcome = match &result {
                Ok(value) => format!("{:?}", value.source),
                Err(error) => format!("{:?}:{}", error.code(), error.diagnostic()),
            };
            let identity = format!("{}:{outcome}", volume.id);
            if state.as_ref() != Some(&identity) {
                match &result {
                    Ok(value) => log::info!(
                        "disk_temperature_state volume={} outcome=ready source={:?} celsius={:.1}",
                        mangodisk_platform::diagnostics::text(&volume.name),
                        value.source,
                        value.celsius
                    ),
                    Err(error) => log::info!(
                        "disk_temperature_state volume={} outcome=unavailable code={:?} error={}",
                        mangodisk_platform::diagnostics::text(&volume.name),
                        error.code(),
                        mangodisk_platform::diagnostics::text(error)
                    ),
                }
                *state = Some(identity);
            }
            (super::sampling_workers::timestamp_ms(), result)
        })
    }

    pub fn read_with(
        &mut self,
        volume: &ResourceVolume,
        now_ms: u64,
        read: impl FnOnce() -> Reading,
    ) -> Reading {
        let identity = (
            volume.id.clone(),
            volume.mount_point.clone(),
            volume
                .devices
                .iter()
                .map(|device| device.id.clone())
                .collect(),
        );
        if let Some((_, cached)) = self.cached.iter().find(|(key, cached)| {
            let same_device = volume.devices.len() == 1 && key.2 == identity.2 && cached.1.is_ok();
            (key == &identity || same_device)
                && now_ms
                    .checked_sub(cached.0)
                    .is_some_and(|elapsed| elapsed < INTERVAL_MS)
        }) {
            let mut reading = cached.clone();
            // Capacity and details still name their own volume, even when two
            // mounted volumes share one physical temperature observation.
            if let Ok(value) = &mut reading.1 {
                value.volume_id.clone_from(&volume.id);
                value.volume_name.clone_from(&volume.name);
            }
            return reading;
        }
        let reading = read();
        self.cached.retain(|(key, _)| key != &identity);
        // Only the display source and detail source are retained, with a strict bound.
        if self.cached.len() == 2 {
            let _ = self.cached.remove(0);
        }
        self.cached.push((identity, reading.clone()));
        reading
    }

    pub fn clear(&mut self) {
        *self = Self::default();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mangodisk_platform::{PlatformError, PlatformErrorCode};

    fn volume() -> ResourceVolume {
        ResourceVolume {
            devices: Vec::new(),
            mount_points: Vec::new(),
            id: "disk".into(),
            name: "Disk".into(),
            mount_point: "/".into(),
            file_system: None,
            system: true,
        }
    }

    fn missing(now_ms: u64) -> Reading {
        (
            now_ms,
            Err(PlatformError::new(
                PlatformErrorCode::Unsupported,
                "unavailable",
            )),
        )
    }

    #[test]
    fn panel_reopens_reuse_the_observation_timestamp_until_thirty_seconds() {
        let mut sampler = TemperatureSampler::default();
        let disk = volume();
        let first = sampler.read_with(&disk, 1000, || missing(1010));
        for now in [1010, 2000, 30_000, 31_009] {
            let cached = sampler.read_with(&disk, now, || panic!("reopening must not query SMART"));
            assert_eq!(cached.0, first.0);
            assert_eq!(cached.1.unwrap_err().code(), PlatformErrorCode::Unsupported);
        }
        assert_eq!(
            sampler.read_with(&disk, 31_010, || missing(31_020)).0,
            31_020
        );
    }

    #[test]
    fn successful_observations_refresh_once_per_interval_without_extending_freshness() {
        let mut sampler = TemperatureSampler::default();
        let disk = volume();
        let calls = std::cell::Cell::new(0);
        for now in [1000, 1001, 10_000, 30_999, 31_000] {
            let reading = sampler.read_with(&disk, now, || {
                calls.set(calls.get() + 1);
                (
                    now,
                    Ok(DiskTemperature {
                        volume_id: disk.id.clone(),
                        volume_name: disk.name.clone(),
                        celsius: 34.0 + f64::from(calls.get()),
                        source: mangodisk_platform::system_resources::disk_temperature::DiskTemperatureSource::NvmeSmart,
                    }),
                )
            });
            assert_eq!(reading.0, if now < 31_000 { 1000 } else { 31_000 });
            assert_eq!(
                reading.1.unwrap().celsius,
                if now < 31_000 { 35.0 } else { 36.0 }
            );
        }
        assert_eq!(calls.get(), 2);
    }

    #[test]
    fn changed_disks_mounts_clock_rollback_and_disconnect_invalidate_temperature() {
        let mut sampler = TemperatureSampler::default();
        let mut disk = volume();
        let _ = sampler.read_with(&disk, 1000, || missing(1000));
        disk.id = "other".into();
        assert_eq!(sampler.read_with(&disk, 1001, || missing(1001)).0, 1001);
        disk.mount_point = "/other".into();
        assert_eq!(sampler.read_with(&disk, 1002, || missing(1002)).0, 1002);
        assert_eq!(sampler.read_with(&disk, 10, || missing(10)).0, 10);
        sampler.clear();
        assert_eq!(sampler.read_with(&disk, 11, || missing(11)).0, 11);
    }
    #[test]
    fn display_and_detail_sources_share_cadence_without_evicting_each_other() {
        let mut sampler = TemperatureSampler::default();
        let one = volume();
        let mut two = volume();
        two.id = "second-disk".into();
        for now in [1000, 1001, 10_000, 30_999] {
            for disk in [&one, &two] {
                let reading = sampler.read_with(disk, now, || {
                    assert_eq!(
                        now, 1000,
                        "each active source must be queried only once per interval"
                    );
                    missing(1000)
                });
                assert_eq!(reading.0, 1000);
            }
        }
        assert_eq!(sampler.cached.len(), 2);
        assert_eq!(
            sampler.read_with(&one, 31_000, || missing(31_000)).0,
            31_000
        );
        assert_eq!(
            sampler.read_with(&two, 31_000, || missing(31_000)).0,
            31_000
        );
        let mut third = volume();
        third.id = "third".into();
        let _ = sampler.read_with(&third, 31_001, || missing(31_001));
        assert_eq!(sampler.cached.len(), 2);
    }
    #[test]
    fn volumes_on_the_same_physical_disk_reuse_success_without_relabelling_missing_data() {
        use mangodisk_platform::system_resources::{
            disk::ResourceDisk, disk_temperature::DiskTemperatureSource,
        };
        let mut first = volume();
        first.devices = vec![ResourceDisk {
            id: "physical-ssd".into(),
            number: Some(0),
            model: Some("SSD".into()),
        }];
        let mut second = first.clone();
        second.id = "second-volume".into();
        second.name = "Data".into();
        second.mount_point = "D:\\".into();
        let mut sampler = TemperatureSampler::default();
        let _ = sampler.read_with(&first, 1000, || {
            (
                1000,
                Ok(DiskTemperature {
                    volume_id: first.id.clone(),
                    volume_name: first.name.clone(),
                    celsius: 40.0,
                    source: DiskTemperatureSource::WindowsStorage,
                }),
            )
        });
        let reused = sampler.read_with(&second, 1001, || {
            panic!("another volume on the same disk must not query SMART again")
        });
        assert_eq!(reused.0, 1000);
        let value = reused.1.unwrap();
        assert_eq!(value.volume_id, second.id);
        assert_eq!(value.volume_name, "Data");
        assert_eq!(value.celsius, 40.0);
        sampler.clear();
        let _ = sampler.read_with(&first, 1000, || missing(1000));
        assert_eq!(
            sampler.read_with(&second, 1001, || missing(1001)).0,
            1001,
            "volume-specific failures cannot be shared across mounts"
        );
    }
}
