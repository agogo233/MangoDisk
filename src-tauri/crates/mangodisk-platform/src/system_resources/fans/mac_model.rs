//! Exact hardware identities are independent of SMC availability and error text.
use super::FanSample;
use crate::PlatformResult;

pub(super) fn is_known_fanless(model: &str) -> bool {
    // Apple identifies these M1–M5 MacBook Air models at support.apple.com/102869.
    // Intel Air, Pro, desktop and future identities must retain sensor probing.
    matches!(
        model,
        "MacBookAir10,1"
            | "Mac14,2"
            | "Mac14,15"
            | "Mac15,12"
            | "Mac15,13"
            | "Mac16,12"
            | "Mac16,13"
            | "Mac17,3"
            | "Mac17,4"
    )
}

pub(super) fn read_fans(
    model: Option<&str>,
    probe: impl FnOnce() -> PlatformResult<FanSample>,
) -> PlatformResult<FanSample> {
    if model.is_some_and(is_known_fanless) {
        // A successful empty sample already means confirmed zero fans in the protocol.
        // Avoid opening or repeatedly retrying a sensor that this hardware cannot have.
        Ok(FanSample { fans: Vec::new() })
    } else {
        probe()
    }
}

#[cfg(target_os = "macos")]
pub(super) fn current() -> Option<&'static str> {
    static MODEL: std::sync::OnceLock<Option<String>> = std::sync::OnceLock::new();
    MODEL
        .get_or_init(|| match read_model() {
            Ok(model) => Some(model),
            Err(error) => {
                log::warn!(
                    "resident_fan_model_unavailable stage=identify_hardware key=hw.model native_code={:?} error={} outcome=probe_smc",
                    error.raw_os_error(),
                    crate::diagnostics::text(&error)
                );
                None
            }
        })
        .as_deref()
}

#[cfg(target_os = "macos")]
fn read_model() -> std::io::Result<String> {
    let mut bytes = [0u8; 128];
    let mut size = bytes.len();
    if unsafe {
        libc::sysctlbyname(
            c"hw.model".as_ptr(),
            bytes.as_mut_ptr().cast(),
            &mut size,
            std::ptr::null_mut(),
            0,
        )
    } != 0
    {
        return Err(std::io::Error::last_os_error());
    }
    let model = bytes
        .get(..size)
        .and_then(|bytes| std::ffi::CStr::from_bytes_with_nul(bytes).ok())
        .and_then(|value| value.to_str().ok())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::InvalidData, "invalid hw.model string")
        })?;
    Ok(model.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{PlatformError, PlatformErrorCode};

    #[test]
    fn only_confirmed_exact_air_identities_skip_sensor_access() {
        for model in [
            "MacBookAir10,1",
            "Mac14,2",
            "Mac14,15",
            "Mac15,12",
            "Mac15,13",
            "Mac16,12",
            "Mac16,13",
            "Mac17,3",
            "Mac17,4",
        ] {
            let sample =
                read_fans(Some(model), || panic!("fanless model must not open SMC")).unwrap();
            assert!(sample.is_valid());
            assert!(sample.fans.is_empty());
        }
    }

    #[test]
    fn unknown_intel_pro_and_desktop_models_preserve_probe_outcomes() {
        for model in [
            None,
            Some(""),
            Some("MacBookAir9,1"),
            Some("MacBookAir8,1"),
            Some("MacBookPro18,1"),
            Some("Mac15,9"),
            Some("Mac16,10"),
            Some("iMac19,1"),
            Some("Mac18,3"),
            Some("Mac17,30"),
            Some("Mac17,3 "),
        ] {
            for code in [
                PlatformErrorCode::Unsupported,
                PlatformErrorCode::OperationFailed,
            ] {
                let error = read_fans(model, || Err(PlatformError::new(code, "SMC probe failed")))
                    .unwrap_err();
                assert_eq!(error.code(), code);
                assert_eq!(error.diagnostic(), "SMC probe failed");
            }
            let sample = read_fans(model, || super::super::read_sensors(|_| Ok(None)));
            assert_eq!(sample.unwrap_err().code(), PlatformErrorCode::Unsupported);
            let sample = read_fans(model, || {
                super::super::read_sensors(|key| Ok(Some(if key == *b"FNum" { 1.0 } else { 0.0 })))
            })
            .unwrap();
            assert_eq!(sample.fans[0].rpm, Some(0.0));
        }
    }
}
