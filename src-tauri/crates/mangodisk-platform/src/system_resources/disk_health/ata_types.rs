use serde::Serialize;

/// A profile identifies an interpretation, never a guessed controller manufacturer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum AtaSmartProfile {
    Unknown,
    SamsungConsumer,
    SiliconMotionLegacy,
    SeagateBarracuda,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AtaSmart {
    pub model: Option<String>,
    pub firmware: Option<String>,
    pub profile: AtaSmartProfile,
    pub revision: u16,
    pub attributes: Vec<AtaSmartAttribute>,
}

/// Raw bytes retain vendor encoding; normalized values are not percentages by default.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AtaSmartAttribute {
    pub id: u8,
    pub flags: u16,
    pub current: u8,
    pub worst: u8,
    pub raw: [u8; 6],
    pub reserved: u8,
    pub decoded: Option<AtaSmartValue>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AtaSmartValue {
    pub metric: AtaSmartMetric,
    pub value: String,
    pub unit: AtaSmartUnit,
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum AtaSmartUnit {
    Count,
    Hours,
    Bytes,
    Percent,
    Celsius,
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum AtaSmartMetric {
    ReallocatedSectors,
    PowerOnHours,
    PowerCycles,
    RemainingLife,
    HostWrittenBytes,
    HostReadBytes,
    UncorrectableErrors,
    SpareBlocks,
    InitialBadBlocks,
    TotalEraseCount,
    MaximumEraseCount,
    MinimumEraseCount,
    AverageEraseCount,
    RatedEraseCount,
    RuntimeInvalidBlocks,
    ProgramFailures,
    EraseFailures,
    Temperature,
    ReallocationEvents,
    PendingSectors,
    OfflineUncorrectableSectors,
    InterfaceCrcErrors,
    TlcWrittenBytes,
    SlcWrittenBytes,
    StartStopCycles,
    LoadCycles,
    HeadFlyingHours,
}

#[cfg(any(target_os = "macos", windows, test))]
impl AtaSmartAttribute {
    pub(super) fn raw_value(&self) -> u64 {
        let mut value = [0; 8];
        value[..6].copy_from_slice(&self.raw);
        u64::from_le_bytes(value)
    }
}
