//! Read-only fan tachometers. A stopped fan is valid; an absent sensor is not zero.
use crate::{PlatformError, PlatformErrorCode, PlatformResult};
use serde::Serialize;

#[cfg(any(target_os = "macos", test))]
mod mac_model;

pub const FAN_FRESHNESS_MS: u64 = 5_000;
pub const MAX_FANS: u8 = 10;
pub const MAX_FAN_RPM: f64 = 30_000.0;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FanSpeed {
    pub id: u8,
    pub rpm: Option<f64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FanSample {
    pub fans: Vec<FanSpeed>,
}

impl FanSample {
    pub fn is_valid(&self) -> bool {
        let mut seen = [false; MAX_FANS as usize];
        self.fans.len() <= MAX_FANS as usize
            && self.fans.iter().all(|fan| {
                if fan.id >= MAX_FANS || seen[fan.id as usize] {
                    return false;
                }
                seen[fan.id as usize] = true;
                fan.rpm
                    .is_none_or(|rpm| rpm.is_finite() && (0.0..=MAX_FAN_RPM).contains(&rpm))
            })
    }
}

#[derive(Default)]
pub struct FanReader {
    #[cfg(target_os = "macos")]
    smc: Option<super::macos_smc::ValueReader>,
    last_outcome: Option<String>,
}

impl FanReader {
    pub fn read(&mut self) -> PlatformResult<FanSample> {
        #[cfg(target_os = "macos")]
        let result = self.read_macos();
        #[cfg(not(target_os = "macos"))]
        let result: PlatformResult<FanSample> = Err(PlatformError::new(
            PlatformErrorCode::Unsupported,
            "fan tachometers unavailable on this platform",
        ));
        #[cfg(target_os = "macos")]
        let model = mac_model::current().unwrap_or("unknown");
        #[cfg(not(target_os = "macos"))]
        let model = "unknown";
        let outcome = match &result {
            Ok(sample) => format!(
                "ready count={} reason={} unreadable_keys={}",
                sample.fans.len(),
                if sample.fans.is_empty() {
                    #[cfg(target_os = "macos")]
                    if mac_model::is_known_fanless(model) {
                        "confirmed_fanless_model"
                    } else {
                        "smc_fan_count_zero"
                    }
                    #[cfg(not(target_os = "macos"))]
                    "fan_count_zero"
                } else {
                    "fan_sensors_present"
                },
                sample
                    .fans
                    .iter()
                    .filter(|fan| fan.rpm.is_none())
                    .map(|fan| format!("F{}Ac", fan.id))
                    .collect::<Vec<_>>()
                    .join(",")
            ),
            Err(error) => format!(
                "code={:?} error={}",
                error.code(),
                crate::diagnostics::text(error)
            ),
        };
        if self.last_outcome.as_ref() != Some(&outcome) {
            match &result {
                Ok(_) => log::info!(
                    "resident_fan_observation model={} outcome={outcome}",
                    crate::diagnostics::text(model)
                ),
                Err(error) if error.code() == PlatformErrorCode::Unsupported => {
                    log::info!(
                        "resident_fan_observation model={} outcome={outcome}",
                        crate::diagnostics::text(model)
                    )
                }
                Err(_) => log::warn!(
                    "resident_fan_observation model={} outcome={outcome}",
                    crate::diagnostics::text(model)
                ),
            }
        }
        self.last_outcome = Some(outcome);
        result
    }

    #[cfg(target_os = "macos")]
    fn read_macos(&mut self) -> PlatformResult<FanSample> {
        let result = mac_model::read_fans(mac_model::current(), || {
            if self.smc.is_none() {
                self.smc = Some(super::macos_smc::ValueReader::new()?);
            }
            read_sensors(|key| self.smc.as_ref().expect("SMC reader initialized").read(key))
        });
        if result.is_err() {
            // Drop stale key metadata and reopen after wake or a native failure.
            self.smc = None;
        }
        result
    }
}

#[cfg(any(target_os = "macos", test))]
fn read_sensors(
    mut read: impl FnMut([u8; 4]) -> PlatformResult<Option<f64>>,
) -> PlatformResult<FanSample> {
    let count = read(*b"FNum")?.ok_or_else(|| {
        PlatformError::new(
            PlatformErrorCode::Unsupported,
            "SMC fan count absent key=FNum",
        )
    })?;
    if !count.is_finite() || !(0.0..=f64::from(MAX_FANS)).contains(&count) || count.fract() != 0.0 {
        return Err(PlatformError::new(
            PlatformErrorCode::InvalidData,
            format!("SMC fan count invalid key=FNum count={count}"),
        ));
    }
    let mut fans = Vec::with_capacity(count as usize);
    for id in 0..count as u8 {
        let key = [b'F', b'0' + id, b'A', b'c'];
        let rpm = read(key)?;
        if rpm.is_some_and(|rpm| !rpm.is_finite() || !(0.0..=MAX_FAN_RPM).contains(&rpm)) {
            return Err(PlatformError::new(
                PlatformErrorCode::InvalidData,
                format!("SMC fan speed invalid fan={id} key=F{id}Ac rpm={rpm:?}"),
            ));
        }
        fans.push(FanSpeed { id, rpm });
    }
    Ok(FanSample { fans })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_fans_stopped_fans_and_missing_tachometers_remain_distinct() {
        let empty = read_sensors(|key| {
            assert_eq!(key, *b"FNum");
            Ok(Some(0.0))
        })
        .unwrap();
        assert!(empty.fans.is_empty());
        let sample = read_sensors(|key| {
            Ok(match &key {
                b"FNum" => Some(2.0),
                b"F0Ac" => Some(0.0),
                b"F1Ac" => None,
                _ => panic!("unexpected SMC key"),
            })
        })
        .unwrap();
        assert_eq!(sample.fans[0].rpm, Some(0.0));
        assert_eq!(sample.fans[1].rpm, None);
        assert!(sample.is_valid());
        assert_eq!(
            read_sensors(|_| Ok(None)).unwrap_err().code(),
            PlatformErrorCode::Unsupported
        );
    }

    #[test]
    fn malformed_counts_and_speeds_are_rejected_before_display() {
        for count in [-1.0, 0.5, 11.0, f64::NAN, f64::INFINITY] {
            assert_eq!(
                read_sensors(|_| Ok(Some(count))).unwrap_err().code(),
                PlatformErrorCode::InvalidData
            );
        }
        for rpm in [-1.0, 30_001.0, f64::NAN, f64::INFINITY] {
            let result = read_sensors(|key| Ok(Some(if key == *b"FNum" { 1.0 } else { rpm })));
            assert_eq!(result.unwrap_err().code(), PlatformErrorCode::InvalidData);
        }
        let result = read_sensors(|_| {
            Err(PlatformError::new(
                PlatformErrorCode::OperationFailed,
                "SMC disconnected",
            ))
        });
        assert_eq!(
            result.unwrap_err().code(),
            PlatformErrorCode::OperationFailed
        );
    }

    #[test]
    fn fan_identity_and_bounds_are_checked_at_the_contract_boundary() {
        for fans in [
            vec![FanSpeed {
                id: MAX_FANS,
                rpm: Some(1.0),
            }],
            vec![
                FanSpeed {
                    id: 0,
                    rpm: Some(1.0)
                };
                2
            ],
            vec![FanSpeed {
                id: 0,
                rpm: Some(f64::NAN),
            }],
        ] {
            assert!(!FanSample { fans }.is_valid());
        }
        assert!(FanSample {
            fans: vec![FanSpeed {
                id: 0,
                rpm: Some(MAX_FAN_RPM)
            }]
        }
        .is_valid());
    }
}
