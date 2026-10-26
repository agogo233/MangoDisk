//! Read-only whole-system power from the identified Apple SMC PSTR sensor.
use crate::{PlatformError, PlatformErrorCode, PlatformResult};
use serde::Serialize;

pub const SYSTEM_POWER_FRESHNESS_MS: u64 = 5_000;
pub const SYSTEM_POWER_MAX_WATTS: f64 = 3_000.0;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SystemPower {
    pub watts: f64,
}

#[derive(Default)]
pub struct SystemPowerReader {
    #[cfg(target_os = "macos")]
    smc: Option<super::macos_smc::ValueReader>,
    last_outcome: Option<String>,
    samples: u64,
}

impl SystemPowerReader {
    pub fn read(&mut self) -> PlatformResult<SystemPower> {
        #[cfg(target_os = "macos")]
        let result = self.read_macos();
        #[cfg(not(target_os = "macos"))]
        let result: PlatformResult<SystemPower> = Err(PlatformError::new(
            PlatformErrorCode::Unsupported,
            "system_power stage=capability reason=no_verified_whole_system_meter component_or_battery_power_not_substituted",
        ));
        let sensor = if cfg!(target_os = "macos") {
            "PSTR"
        } else {
            "none"
        };
        self.samples = self.samples.wrapping_add(1);
        let outcome = match &result {
            Ok(_) => "ready".into(),
            Err(error) => format!(
                "code={:?} error={}",
                error.code(),
                crate::diagnostics::text(&error.to_string())
            ),
        };
        if self.last_outcome.as_ref() != Some(&outcome) || self.samples.is_multiple_of(30) {
            match &result {
                Ok(value) => log::info!(
                    "resident_system_power_observation sensor={sensor} outcome={outcome} watts={}",
                    value.watts
                ),
                Err(_) => {
                    log::warn!(
                        "resident_system_power_observation sensor={sensor} outcome={outcome}"
                    )
                }
            }
        }
        self.last_outcome = Some(outcome);
        result
    }

    #[cfg(target_os = "macos")]
    fn read_macos(&mut self) -> PlatformResult<SystemPower> {
        if self.smc.is_none() {
            self.smc = Some(super::macos_smc::ValueReader::new()?);
        }
        let result = self
            .smc
            .as_ref()
            .expect("SMC reader initialized")
            .read(*b"PSTR");
        if result.is_err() {
            // Reopen on the next scheduled sample after wake or a native failure.
            self.smc = None;
        }
        let watts = result?.ok_or_else(|| {
            PlatformError::new(PlatformErrorCode::Unsupported, "SMC PSTR sensor absent")
        })?;
        if !watts.is_finite() || !(0.0..=SYSTEM_POWER_MAX_WATTS).contains(&watts) {
            return Err(PlatformError::new(
                PlatformErrorCode::OperationFailed,
                format!("SMC PSTR reading invalid watts={watts}"),
            ));
        }
        Ok(SystemPower { watts })
    }
}

#[cfg(all(test, not(target_os = "macos")))]
mod tests {
    use super::*;

    #[test]
    fn non_macos_system_power_is_unavailable_instead_of_zero() {
        let Err(error) = SystemPowerReader::default().read() else {
            panic!("whole-system power must be unavailable outside macOS");
        };
        assert_eq!(error.code(), PlatformErrorCode::Unsupported);
    }
}
