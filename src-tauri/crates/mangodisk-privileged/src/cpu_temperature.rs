//! Hardware identification and deterministic decoding of fixed temperature registers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CpuTemperatureSensor {
    IntelPackageDts,
    AmdZen { tdie_offset: u8 },
}

#[cfg(any(all(windows, target_arch = "x86_64"), test))]
pub(crate) fn identify(
    vendor: [u32; 3],
    signature: u32,
    thermal: u32,
    brand: &str,
) -> Option<CpuTemperatureSensor> {
    let base_family = (signature >> 8) & 0xf;
    let family = base_family
        + if base_family == 0xf {
            (signature >> 20) & 0xff
        } else {
            0
        };
    match vendor {
        [0x756e6547, 0x49656e69, 0x6c65746e] if family == 6 && thermal & 0x41 == 0x41 => {
            Some(CpuTemperatureSensor::IntelPackageDts)
        }
        [0x68747541, 0x69746e65, 0x444d4163] if matches!(family, 0x17 | 0x19 | 0x1a) => {
            // Early Zen models expose Tctl above Tdie. Keep the documented model
            // offsets separate from the register's 49-degree range adjustment.
            let tdie_offset = if family != 0x17 {
                0
            } else if [
                "AMD Ryzen 5 1600X",
                "AMD Ryzen 7 1700X",
                "AMD Ryzen 7 1800X",
            ]
            .iter()
            .any(|id| brand.starts_with(id))
            {
                20
            } else if brand.starts_with("AMD Ryzen 7 2700X") {
                10
            } else if ["AMD Ryzen Threadripper 19", "AMD Ryzen Threadripper 29"]
                .iter()
                .any(|id| brand.starts_with(id))
            {
                27
            } else {
                0
            };
            Some(CpuTemperatureSensor::AmdZen { tdie_offset })
        }
        _ => None,
    }
}

/// Zen THM_TCON_CUR_TMP uses eighth-degree units and explicit range selectors.
pub(crate) fn amd_zen_celsius(raw: u64, tdie_offset: u8) -> Option<f64> {
    let register = u32::try_from(raw).ok()?;
    if register == 0 || register == u32::MAX {
        return None;
    }
    let adjust_range = register & (1 << 19) != 0 || register & (3 << 16) == 3 << 16;
    let celsius = f64::from(register >> 21) * 0.125
        - if adjust_range { 49.0 } else { 0.0 }
        - f64::from(tdie_offset);
    (celsius > 0.0 && celsius <= 150.0).then_some(celsius)
}

#[cfg(test)]
mod tests {
    use super::*;
    const AMD: [u32; 3] = [0x68747541, 0x69746e65, 0x444d4163];

    #[test]
    fn ryzen_7950x_and_documented_zen_families_use_the_amd_sensor() {
        assert_eq!(
            identify(AMD, 0x00a60f12, 0, "AMD Ryzen 9 7950X 16-Core Processor"),
            Some(CpuTemperatureSensor::AmdZen { tdie_offset: 0 })
        );
        for family in [0x17, 0x19, 0x1a] {
            assert!(identify(AMD, ((family - 0xf) << 20) | 0xf00, 0, "").is_some());
        }
        for family in [0x15, 0x16, 0x18, 0x1b] {
            assert!(identify(AMD, ((family - 0xf) << 20) | 0xf00, 0, "").is_none());
        }
        assert!(identify([0; 3], 0x00a60f12, 0, "AMD Ryzen 9 7950X").is_none());
        assert!(identify([0x756e6547, 0x49656e69, 0x6c65746e], 0x906a4, 0, "Intel").is_none());
    }

    #[test]
    fn zen_temperature_handles_both_range_selectors_and_rejects_sentinels() {
        assert_eq!(amd_zen_celsius(520 << 21, 0), Some(65.0));
        assert_eq!(amd_zen_celsius((912 << 21) | (1 << 19), 0), Some(65.0));
        assert_eq!(amd_zen_celsius((912 << 21) | (3 << 16), 0), Some(65.0));
        assert_eq!(
            amd_zen_celsius((912 << 21) | (1 << 19) | (3 << 16), 0),
            Some(65.0)
        );
        assert_eq!(amd_zen_celsius(761 << 21, 0), Some(95.125));
        for raw in [
            0,
            u32::MAX as u64,
            u64::MAX,
            1201 << 21,
            (100 << 21) | (1 << 19),
        ] {
            assert_eq!(amd_zen_celsius(raw, 0), None);
        }
    }

    #[test]
    fn early_zen_tctl_offsets_are_removed_only_for_identified_models() {
        for (name, offset) in [
            ("AMD Ryzen 5 1600X Six-Core Processor", 20),
            ("AMD Ryzen 7 2700X Eight-Core Processor", 10),
            ("AMD Ryzen Threadripper 1950X", 27),
            ("AMD Ryzen Threadripper 2990WX", 27),
            ("AMD Ryzen 5 1600", 0),
        ] {
            assert_eq!(
                identify(AMD, 0x800f11, 0, name),
                Some(CpuTemperatureSensor::AmdZen {
                    tdie_offset: offset
                })
            );
            assert_eq!(
                amd_zen_celsius(640 << 21, offset),
                Some(80.0 - f64::from(offset))
            );
        }
    }
}
