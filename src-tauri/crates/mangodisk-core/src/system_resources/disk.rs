use mangodisk_platform::system_resources::disk::{ResourceVolume, VolumeCapacity};
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiskUsage {
    pub volume: ResourceVolume,
    pub total_bytes: u64,
    pub available_bytes: u64,
    pub used_bytes: u64,
    pub used_percent: f64,
}

pub fn select<'a>(
    volumes: &'a [ResourceVolume],
    manual: Option<&str>,
) -> Option<&'a ResourceVolume> {
    volumes
        .iter()
        .find(|volume| manual.map_or(volume.system, |id| id == volume.id))
}

/// A fixed device is sampled through an unambiguous, identity-checked mounted volume.
/// Follow mode keeps the selected volume so spanning devices retain their native Unsupported result.
pub fn temperature_volume<'a>(
    volumes: &'a [ResourceVolume],
    volume_id: Option<&str>,
    device_id: Option<&str>,
) -> Option<&'a ResourceVolume> {
    let usage = select(volumes, volume_id);
    let Some(device_id) = device_id else {
        return usage;
    };
    let matches =
        |volume: &&ResourceVolume| volume.devices.len() == 1 && volume.devices[0].id == device_id;
    usage
        .filter(matches)
        .or_else(|| volumes.iter().find(matches))
        // Keep a known spanning source distinguishable from a disconnected device.
        // Native temperature readers reject ambiguous extents before querying SMART.
        .or_else(|| {
            volumes
                .iter()
                .find(|volume| volume.devices.iter().any(|device| device.id == device_id))
        })
}

pub fn usage(volume: ResourceVolume, capacity: VolumeCapacity) -> Option<DiskUsage> {
    let used = capacity.total_bytes.checked_sub(capacity.available_bytes)?;
    if capacity.total_bytes == 0 {
        return None;
    }
    Some(DiskUsage {
        volume,
        total_bytes: capacity.total_bytes,
        available_bytes: capacity.available_bytes,
        used_bytes: used,
        used_percent: used as f64 / capacity.total_bytes as f64 * 100.0,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn volume(id: &str, system: bool) -> ResourceVolume {
        ResourceVolume {
            devices: Vec::new(),
            mount_points: Vec::new(),
            file_system: None,
            id: id.into(),
            name: "Disk".into(),
            system,
            mount_point: String::new(),
        }
    }

    #[test]
    fn system_volume_is_explicit_and_disconnected_manual_volume_stays_selected() {
        let volumes = [volume("external", false), volume("system", true)];
        assert_eq!(select(&volumes, None).unwrap().id, "system");
        assert!(select(&volumes, Some("missing")).is_none());
        assert_eq!(select(&volumes, Some("external")).unwrap().id, "external");
    }

    #[test]
    fn invalid_capacity_is_not_presented_as_zero_use() {
        assert!(usage(
            volume("one", true),
            VolumeCapacity {
                total_bytes: 0,
                available_bytes: 0
            }
        )
        .is_none());
        assert!(usage(
            volume("one", true),
            VolumeCapacity {
                total_bytes: 100,
                available_bytes: 101
            }
        )
        .is_none());
        assert_eq!(
            usage(
                volume("one", true),
                VolumeCapacity {
                    total_bytes: 100,
                    available_bytes: 25
                }
            )
            .unwrap()
            .used_percent,
            75.0
        );
    }
    #[test]
    fn temperature_follows_the_selected_volume_or_a_fixed_physical_device() {
        use mangodisk_platform::system_resources::disk::ResourceDisk;
        let device = |id: &str, number| ResourceDisk {
            id: id.into(),
            number: Some(number),
            model: Some("SSD".into()),
        };
        let mut c = volume("c", true);
        c.devices = vec![device("a", 0)];
        let mut d = volume("d", false);
        d.devices = c.devices.clone();
        let mut f = volume("f", false);
        f.devices = vec![device("b", 1)];
        let volumes = [c, d, f];
        assert_eq!(
            temperature_volume(&volumes, Some("d"), None).unwrap().id,
            "d"
        );
        assert_eq!(
            temperature_volume(&volumes, Some("d"), Some("b"))
                .unwrap()
                .id,
            "f"
        );
        assert_eq!(
            temperature_volume(&volumes, Some("d"), Some("a"))
                .unwrap()
                .id,
            "d"
        );
        assert_eq!(
            temperature_volume(&volumes, Some("missing"), Some("b"))
                .unwrap()
                .id,
            "f"
        );
        assert!(temperature_volume(&volumes, Some("missing"), None).is_none());
        assert!(temperature_volume(&volumes, None, Some("removed")).is_none());
        let mut spanning = volume("span", true);
        spanning.devices = vec![device("a", 0), device("b", 1)];
        assert_eq!(
            temperature_volume(std::slice::from_ref(&spanning), None, Some("a"))
                .unwrap()
                .id,
            "span"
        );
        assert_eq!(
            temperature_volume(&[spanning], None, None).unwrap().id,
            "span"
        );
    }
}
