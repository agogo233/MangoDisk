use mangodisk_platform::FileSpaceUsage;

/// The byte metric is a snapshot fact, independent of result or chart protocols.
#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum ByteMetric {
    #[default]
    Allocated,
    Logical,
}
impl ByteMetric {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Allocated => "allocated",
            Self::Logical => "logical",
        }
    }
    pub(crate) const fn displayed_bytes(self, usage: FileSpaceUsage) -> u64 {
        match self {
            Self::Allocated => usage.allocated_bytes,
            Self::Logical => usage.logical_bytes,
        }
    }
}
