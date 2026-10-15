//! Immutable official artifacts bundled in the Windows helper.
pub struct Artifact {
    pub file_name: &'static str,
    pub sha256: &'static str,
    pub maximum_bytes: u64,
}
pub const PAWNIO_INSTALLER: Artifact = Artifact {
    file_name: "PawnIO_setup.exe",
    sha256: "1f519a22e47187f70a1379a48ca604981c4fcf694f4e65b734aaa74a9fba3032",
    maximum_bytes: 8 * 1024 * 1024,
};
pub const INTEL_MODULE: Artifact = Artifact {
    file_name: "IntelMSR.bin",
    sha256: "d6ed85d65ab17a22f813ef98207d6d537155ee2ded5976a21cb48413c9b92e5f",
    maximum_bytes: 64 * 1024,
};

pub const AMD_ZEN_MODULE: Artifact = Artifact {
    file_name: "AMDFamily17.bin",
    sha256: "dae74615761b78bdf064dfb3e136252ddcc6fc727d88f14738d0e5800d427a91",
    maximum_bytes: 64 * 1024,
};
