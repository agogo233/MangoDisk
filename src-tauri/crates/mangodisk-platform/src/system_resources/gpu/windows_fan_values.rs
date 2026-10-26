//! Bounded decoding for the NVIDIA client fan-status V1 ABI used by LibreHardwareMonitor.
//! ABI reference: LibreHardwareMonitor v0.9.6, Interop/NvApi.cs, NvFanCoolersStatus.
use crate::{PlatformError, PlatformErrorCode, PlatformResult};

const MAX_COOLERS: usize = 32;

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct Cooler {
    id: u32,
    rpm: u32,
    levels: [u32; 3],
    reserved: [u32; 8],
}

#[repr(C)]
pub(super) struct CoolerStatus {
    version: u32,
    count: u32,
    reserved: [u64; 4],
    items: [Cooler; MAX_COOLERS],
}

const COOLER_STATUS_VERSION: u32 = std::mem::size_of::<CoolerStatus>() as u32 | (1 << 16);

impl Default for CoolerStatus {
    fn default() -> Self {
        Self {
            version: COOLER_STATUS_VERSION,
            count: 0,
            reserved: [0; 4],
            items: [Cooler::default(); MAX_COOLERS],
        }
    }
}

impl CoolerStatus {
    pub(super) fn speeds(&self) -> PlatformResult<Vec<f64>> {
        let invalid = |reason| {
            PlatformError::new(
                PlatformErrorCode::InvalidData,
                format!(
                    "gpu_fan source=nvapi stage=client_fan_status reason={reason} count={}",
                    self.count
                ),
            )
        };
        if self.version != COOLER_STATUS_VERSION {
            return Err(invalid("unexpected_version"));
        }
        let count = self.count as usize;
        if count > MAX_COOLERS {
            return Err(invalid("invalid_count"));
        }
        if count == 0 {
            return Err(PlatformError::new(
                PlatformErrorCode::Unsupported,
                "gpu_fan source=nvapi stage=client_fan_status reason=no_tachometers",
            ));
        }
        for (index, cooler) in self.items[..count].iter().enumerate() {
            if f64::from(cooler.rpm) > super::super::fans::MAX_FAN_RPM {
                return Err(invalid("invalid_rpm"));
            }
            if self.items[..index]
                .iter()
                .any(|previous| previous.id == cooler.id)
            {
                return Err(invalid("duplicate_cooler_id"));
            }
        }
        // A successful, nonempty vendor query establishes support for stopped fans too.
        Ok(self.items[..count]
            .iter()
            .map(|cooler| f64::from(cooler.rpm))
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn client_fan_v1_layout_matches_the_vendor_abi() {
        assert_eq!(std::mem::size_of::<Cooler>(), 52);
        assert_eq!(std::mem::size_of::<CoolerStatus>(), 1704);
        assert_eq!(std::mem::offset_of!(CoolerStatus, items), 40);
        assert_eq!(CoolerStatus::default().version, 0x106a8);
    }

    #[test]
    fn all_real_tachometers_are_retained_including_stopped_fans() {
        let mut status = CoolerStatus {
            count: 2,
            ..Default::default()
        };
        status.items[0].id = 1;
        status.items[0].rpm = 1600;
        status.items[1].id = 2;
        assert_eq!(status.speeds().unwrap(), vec![1600.0, 0.0]);
        status.count = 0;
        assert_eq!(
            status.speeds().unwrap_err().code(),
            PlatformErrorCode::Unsupported
        );
    }

    #[test]
    fn malformed_native_results_never_become_zero_or_partial_samples() {
        let mut status = CoolerStatus {
            count: 33,
            ..Default::default()
        };
        assert_eq!(
            status.speeds().unwrap_err().code(),
            PlatformErrorCode::InvalidData
        );
        status.count = 2;
        assert!(status
            .speeds()
            .unwrap_err()
            .diagnostic()
            .contains("duplicate_cooler_id"));
        status.items[1].id = 1;
        status.items[1].rpm = 30_001;
        assert!(status
            .speeds()
            .unwrap_err()
            .diagnostic()
            .contains("invalid_rpm"));
        status.items[1].rpm = 1000;
        status.version += 1;
        assert!(status
            .speeds()
            .unwrap_err()
            .diagnostic()
            .contains("unexpected_version"));
    }
}
