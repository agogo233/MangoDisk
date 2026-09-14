//! One-shot native reclamation. Sampling never invokes these operations automatically.
#[cfg(target_os = "macos")]
mod macos;
#[cfg(windows)]
mod windows;

use crate::PlatformResult;

pub fn release_memory() -> PlatformResult<()> {
    #[cfg(target_os = "macos")]
    {
        macos::release()
    }
    #[cfg(windows)]
    {
        windows::release()
    }
    #[cfg(not(any(target_os = "macos", windows)))]
    {
        Err(crate::PlatformError::new(
            crate::PlatformErrorCode::Unsupported,
            "memory reclamation is unavailable",
        ))
    }
}
