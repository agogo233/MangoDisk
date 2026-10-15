//! Compiled only into the helper binary; the desktop adapter carries no vendor bytes.
pub(crate) static PAWNIO_INSTALLER_BYTES: &[u8] =
    include_bytes!("../vendors/pawnio/2.2.0/PawnIO_setup.exe");
pub(crate) static INTEL_MSR_MODULE_BYTES: &[u8] =
    include_bytes!("../vendors/pawnio/modules/0.2.11/IntelMSR.bin");

pub(crate) static AMD_ZEN_MODULE_BYTES: &[u8] =
    include_bytes!("../vendors/pawnio/modules/0.2.11/AMDFamily17.bin");

#[cfg(test)]
mod tests {
    use super::*;
    use mangodisk_privileged::artifacts::{AMD_ZEN_MODULE, INTEL_MODULE, PAWNIO_INSTALLER};
    use sha2::{Digest, Sha256};

    #[test]
    fn embedded_vendor_payloads_match_pinned_official_releases() {
        for (bytes, artifact, expected_size) in [
            (PAWNIO_INSTALLER_BYTES, &PAWNIO_INSTALLER, 3_410_960),
            (INTEL_MSR_MODULE_BYTES, &INTEL_MODULE, 5_324),
            (AMD_ZEN_MODULE_BYTES, &AMD_ZEN_MODULE, 10_652),
        ] {
            assert_eq!(bytes.len(), expected_size, "{}", artifact.file_name);
            assert!(bytes.len() as u64 <= artifact.maximum_bytes);
            assert_eq!(format!("{:x}", Sha256::digest(bytes)), artifact.sha256);
        }
    }
}
