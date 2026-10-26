use super::ata_types::{AtaSmartMetric, AtaSmartUnit, AtaSmartValue};
use super::{AtaSmart, AtaSmartAttribute, AtaSmartProfile, DiskHealth, DiskHealthStatus};
use crate::{
    system_resources::{
        disk::ResourceVolume,
        disk_temperature::{ata_temperature, DiskTemperatureSource},
    },
    PlatformError, PlatformErrorCode, PlatformResult,
};

#[derive(Default)]
pub(crate) struct AtaIdentity {
    pub model: Option<String>,
    pub firmware: Option<String>,
    pub logical_sector_bytes: Option<u32>,
}

fn invalid(reason: &str) -> PlatformError {
    PlatformError::new(
        PlatformErrorCode::InvalidData,
        format!("disk_health stage=ata_parse {reason}"),
    )
}

fn attributes(bytes: &[u8]) -> PlatformResult<Vec<AtaSmartAttribute>> {
    if bytes.len() != 512
        || bytes.iter().fold(0u8, |sum, byte| sum.wrapping_add(*byte)) != 0
        || bytes.iter().all(|byte| *byte == 0)
        || bytes.iter().all(|byte| *byte == 255)
    {
        return Err(invalid("invalid_length_or_checksum"));
    }
    let mut seen = [false; 256];
    let mut result = Vec::with_capacity(30);
    for record in bytes[2..362].as_chunks::<12>().0 {
        if record[0] == 0 {
            continue;
        }
        if std::mem::replace(&mut seen[usize::from(record[0])], true) {
            return Err(invalid("duplicate_attribute"));
        }
        result.push(AtaSmartAttribute {
            id: record[0],
            flags: u16::from_le_bytes([record[1], record[2]]),
            current: record[3],
            worst: record[4],
            raw: record[5..11].try_into().unwrap(),
            reserved: record[11],
            decoded: None,
        });
    }
    if result.is_empty() {
        return Err(invalid("empty_attributes"));
    }
    Ok(result)
}

fn profile(identity: &AtaIdentity, attributes: &[AtaSmartAttribute]) -> AtaSmartProfile {
    let model = identity
        .model
        .as_deref()
        .unwrap_or_default()
        .trim()
        .to_ascii_uppercase();
    let samsung = model.strip_prefix("SAMSUNG SSD ").is_some_and(|model| {
        [
            "830 SERIES",
            "840 SERIES",
            "840 PRO SERIES",
            "840 EVO ",
            "850 EVO ",
            "850 PRO ",
            "860 EVO ",
            "860 PRO ",
            "860 QVO ",
            "870 EVO ",
            "870 QVO ",
        ]
        .iter()
        .any(|prefix| model.starts_with(prefix))
    });
    if samsung {
        return AtaSmartProfile::SamsungConsumer;
    }
    // Early SMI tables use raw A9 as remaining life and F1/F2 in 32 MiB.
    // Require the complete layout and tested model/firmware, not a brand substring.
    const SMI_PREFIX: [u8; 20] = [
        1, 5, 9, 12, 160, 161, 163, 164, 165, 166, 167, 168, 169, 175, 176, 177, 178, 181, 182, 192,
    ];
    let legacy_smi = (model == "ACER AS500A 128G"
        && identity
            .firmware
            .as_deref()
            .is_some_and(|firmware| firmware.trim() == "Q1115A0"))
        || ["ADATA SU800", "ADATA SU900", "ADATA SP550"].contains(&model.as_str())
        || model.starts_with("KINGDIAN S280 ");
    if legacy_smi
        && attributes
            .iter()
            .map(|value| value.id)
            .take(20)
            .eq(SMI_PREFIX)
    {
        return AtaSmartProfile::SiliconMotionLegacy;
    }
    let seagate = model
        .strip_prefix("ST")
        .and_then(|value| value.split_once('-'))
        .is_some_and(|(series, suffix)| {
            !suffix.is_empty()
                && [
                    "500DM009",
                    "1000DM010",
                    "2000DM006",
                    "2000DM007",
                    "3000DM008",
                    "3000DM009",
                    "4000DM005",
                    "2000DM005",
                    "2000DM008",
                    "3000DM007",
                    "4000DM004",
                    "8000DM004",
                    "6000DM003",
                ]
                .contains(&series)
        });
    if seagate {
        AtaSmartProfile::SeagateBarracuda
    } else {
        AtaSmartProfile::Unknown
    }
}

pub(in crate::system_resources) fn read(
    volume: &ResourceVolume,
    bytes: &[u8],
    status: DiskHealthStatus,
    identity: AtaIdentity,
) -> PlatformResult<DiskHealth> {
    let mut attributes = attributes(bytes)?;
    let profile = profile(&identity, &attributes);
    let attr = |id| attributes.iter().find(|value| value.id == id);
    let raw = |id| attr(id).map(AtaSmartAttribute::raw_value);
    let remaining = match profile {
        AtaSmartProfile::SamsungConsumer => attr(177).map(|value| u64::from(value.current)),
        AtaSmartProfile::SiliconMotionLegacy => raw(169),
        _ => None,
    };
    let mut result = DiskHealth::basic(volume, status, DiskTemperatureSource::AtaSmart);
    result.temperature_celsius = ata_temperature(bytes).ok();
    result.percentage_used = remaining
        .filter(|value| *value <= 100)
        .map(|value| 100 - value as u8);
    if profile != AtaSmartProfile::Unknown {
        // SMI uses 24-bit hours; Seagate SMR models pack sub-hour data in high bytes.
        result.power_on_hours = attr(9).map(|value| {
            if profile == AtaSmartProfile::SamsungConsumer {
                return value.raw_value();
            }
            if profile == AtaSmartProfile::SiliconMotionLegacy {
                return value.raw_value() & 0x00ff_ffff;
            }
            let model = identity
                .model
                .as_deref()
                .unwrap_or_default()
                .to_ascii_uppercase();
            let smr = [
                "ST2000DM005-",
                "ST2000DM008-",
                "ST3000DM007-",
                "ST4000DM004-",
                "ST8000DM004-",
                "ST6000DM003-",
            ]
            .iter()
            .any(|prefix| model.starts_with(prefix));
            let width = if smr { 4 } else { 3 };
            let mut raw = [0; 8];
            raw[..width].copy_from_slice(&value.raw[..width]);
            u64::from_le_bytes(raw)
        });
        result.power_cycles = raw(12);
    }
    let unit = match profile {
        AtaSmartProfile::SamsungConsumer => Some(512u64),
        AtaSmartProfile::SiliconMotionLegacy => Some(32 * 1024 * 1024),
        AtaSmartProfile::SeagateBarracuda => identity
            .logical_sector_bytes
            .filter(|value| value.is_power_of_two() && (512..=65536).contains(value))
            .map(u64::from),
        AtaSmartProfile::Unknown => None,
    };
    if let Some(unit) = unit {
        result.total_written_bytes = raw(241)
            .and_then(|value| value.checked_mul(unit))
            .map(|value| value.to_string());
        result.total_read_bytes = raw(242)
            .and_then(|value| value.checked_mul(unit))
            .map(|value| value.to_string());
    }
    for attribute in &mut attributes {
        attribute.decoded = decode(attribute, profile, &result);
    }
    result.ata = Some(Box::new(AtaSmart {
        model: identity.model,
        firmware: identity.firmware,
        profile,
        revision: u16::from_le_bytes(bytes[..2].try_into().unwrap()),
        attributes,
    }));
    Ok(result)
}

fn decode(
    attribute: &AtaSmartAttribute,
    profile: AtaSmartProfile,
    health: &DiskHealth,
) -> Option<AtaSmartValue> {
    use AtaSmartMetric as Metric;
    use AtaSmartUnit as Unit;
    if profile == AtaSmartProfile::Unknown {
        return None;
    }
    let raw = attribute.raw_value();
    let count = |metric, value| {
        Some(AtaSmartValue {
            metric,
            value: u64::to_string(&value),
            unit: Unit::Count,
        })
    };
    let value = |metric, value: Option<String>, unit| {
        value.map(|value| AtaSmartValue {
            metric,
            value,
            unit,
        })
    };
    match attribute.id {
        5 => count(Metric::ReallocatedSectors, raw & 0xffff),
        9 => value(
            Metric::PowerOnHours,
            health.power_on_hours.map(|value| value.to_string()),
            Unit::Hours,
        ),
        12 => count(Metric::PowerCycles, raw),
        190 | 194 => {
            let temperature = f64::from(attribute.raw[0]);
            super::super::disk_temperature::valid_celsius(temperature).then(|| AtaSmartValue {
                metric: Metric::Temperature,
                value: attribute.raw[0].to_string(),
                unit: Unit::Celsius,
            })
        }
        196 => count(Metric::ReallocationEvents, raw & 0xffff),
        197 => count(Metric::PendingSectors, raw),
        198 => count(Metric::OfflineUncorrectableSectors, raw),
        199 => count(Metric::InterfaceCrcErrors, raw),
        241 => value(
            Metric::HostWrittenBytes,
            health.total_written_bytes.clone(),
            Unit::Bytes,
        ),
        242 => value(
            Metric::HostReadBytes,
            health.total_read_bytes.clone(),
            Unit::Bytes,
        ),
        177 if profile == AtaSmartProfile::SamsungConsumer => value(
            Metric::RemainingLife,
            health.percentage_used.map(|used| (100 - used).to_string()),
            Unit::Percent,
        ),
        187 if profile == AtaSmartProfile::SamsungConsumer
            || profile == AtaSmartProfile::SeagateBarracuda =>
        {
            count(Metric::UncorrectableErrors, raw)
        }
        4 if profile == AtaSmartProfile::SeagateBarracuda => count(Metric::StartStopCycles, raw),
        193 if profile == AtaSmartProfile::SeagateBarracuda => count(Metric::LoadCycles, raw),
        240 if profile == AtaSmartProfile::SeagateBarracuda => value(
            Metric::HeadFlyingHours,
            Some((raw & 0xffff_ffff).to_string()),
            Unit::Hours,
        ),
        id if profile == AtaSmartProfile::SiliconMotionLegacy => match id {
            160 => count(Metric::UncorrectableErrors, raw),
            161 => count(Metric::SpareBlocks, raw),
            163 => count(Metric::InitialBadBlocks, raw),
            164 => count(Metric::TotalEraseCount, raw),
            165 => count(Metric::MaximumEraseCount, raw),
            166 => count(Metric::MinimumEraseCount, raw),
            167 => count(Metric::AverageEraseCount, raw),
            168 => count(Metric::RatedEraseCount, raw),
            169 => value(
                Metric::RemainingLife,
                health.percentage_used.map(|used| (100 - used).to_string()),
                Unit::Percent,
            ),
            178 => count(Metric::RuntimeInvalidBlocks, raw),
            181 => count(Metric::ProgramFailures, raw),
            182 => count(Metric::EraseFailures, raw),
            245 | 246 => value(
                if id == 245 {
                    Metric::TlcWrittenBytes
                } else {
                    Metric::SlcWrittenBytes
                },
                raw.checked_mul(32 * 1024 * 1024)
                    .map(|value| value.to_string()),
                Unit::Bytes,
            ),
            _ => None,
        },
        _ => None,
    }
}

#[cfg(test)]
mod tests;
