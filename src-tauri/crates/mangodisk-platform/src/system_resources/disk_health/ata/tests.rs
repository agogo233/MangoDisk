use super::*;

fn volume() -> ResourceVolume {
    ResourceVolume {
        devices: Vec::new(),
        mount_points: Vec::new(),
        id: "disk".into(),
        name: "Disk".into(),
        file_system: None,
        system: true,
        mount_point: "/".into(),
    }
}
fn identity(model: &str, firmware: &str) -> AtaIdentity {
    AtaIdentity {
        model: Some(model.into()),
        firmware: Some(firmware.into()),
        logical_sector_bytes: Some(512),
    }
}
fn sector(records: &[(u8, u8, u64)]) -> [u8; 512] {
    let mut bytes = [0; 512];
    bytes[0] = 1;
    for (record, (id, current, raw)) in bytes[2..362]
        .as_chunks_mut::<12>()
        .0
        .iter_mut()
        .zip(records)
    {
        record[0] = *id;
        record[1..3].copy_from_slice(&0x32u16.to_le_bytes());
        record[3] = *current;
        record[4] = *current;
        record[5..11].copy_from_slice(&raw.to_le_bytes()[..6]);
    }
    checksum(&mut bytes);
    bytes
}
fn checksum(bytes: &mut [u8; 512]) {
    bytes[511] = 0u8.wrapping_sub(
        bytes[..511]
            .iter()
            .fold(0u8, |sum, byte| sum.wrapping_add(*byte)),
    );
}
fn read_as(bytes: &[u8], model: &str) -> DiskHealth {
    read(
        &volume(),
        bytes,
        DiskHealthStatus::Healthy,
        identity(model, ""),
    )
    .unwrap()
}

#[test]
fn samsung_uses_normalized_wear_and_fixed_512_byte_host_units() {
    let bytes = sector(&[
        (9, 92, 35552),
        (12, 98, 1434),
        (177, 77, 339),
        (190, 65, 35),
        (241, 99, 240_826_202_091),
    ]);
    let health = read_as(&bytes, "Samsung SSD 860 EVO 500GB");
    assert_eq!(health.percentage_used, Some(23));
    assert_eq!(health.power_on_hours, Some(35552));
    assert_eq!(health.power_cycles, Some(1434));
    assert_eq!(
        health.total_written_bytes.as_deref(),
        Some("123303015470592")
    );
    assert_eq!(health.total_read_bytes, None);
    assert_eq!(health.temperature_celsius, Some(35.0));
    let ata = health.ata.unwrap();
    assert_eq!(ata.profile, AtaSmartProfile::SamsungConsumer);
    assert_eq!(ata.attributes.len(), 5);
    assert_eq!(ata.attributes[2].current, 77);
    assert_eq!(ata.attributes[2].raw_value(), 339);
}

fn smi_sector() -> [u8; 512] {
    let mut records = vec![(1, 100, 0), (5, 100, 1), (9, 100, 16466), (12, 100, 1143)];
    records.extend([160, 161, 163, 164, 165, 166, 167, 168].map(|id| (id, 100, 0)));
    records.extend([
        (169, 100, 67),
        (175, 100, 0),
        (176, 100, 0),
        (177, 100, 0),
        (178, 100, 1),
        (181, 100, 0),
        (182, 100, 0),
        (192, 100, 270),
        (194, 100, 49),
        (241, 100, 1_943_278),
        (242, 100, 16_254_842),
        (245, 100, 24),
        (246, 100, 2),
    ]);
    sector(&records)
}

#[test]
fn legacy_smi_requires_model_firmware_and_attribute_layout_before_using_raw_life() {
    let bytes = smi_sector();
    let health = read(
        &volume(),
        &bytes,
        DiskHealthStatus::Healthy,
        identity("Acer AS500A 128G", "Q1115A0"),
    )
    .unwrap();
    assert_eq!(health.percentage_used, Some(33));
    assert_eq!(health.power_on_hours, Some(16466));
    assert_eq!(health.power_cycles, Some(1143));
    assert_eq!(
        health.total_written_bytes,
        Some((1_943_278u64 * 33_554_432).to_string())
    );
    assert_eq!(
        health.total_read_bytes,
        Some((16_254_842u64 * 33_554_432).to_string())
    );
    assert_eq!(
        health.ata.unwrap().profile,
        AtaSmartProfile::SiliconMotionLegacy
    );
    for (model, firmware) in [
        ("Acer AS500A 128G", "OTHER"),
        ("Acer SSD SA100 128GB", "Q1115A0"),
        ("Other SSD", "Q1115A0"),
    ] {
        let unknown = read(
            &volume(),
            &bytes,
            DiskHealthStatus::Healthy,
            identity(model, firmware),
        )
        .unwrap();
        assert_eq!(unknown.percentage_used, None);
        assert_eq!(unknown.total_written_bytes, None);
        assert_eq!(unknown.ata.unwrap().profile, AtaSmartProfile::Unknown);
    }
    let mut altered = bytes;
    altered[2] = 2;
    checksum(&mut altered);
    assert_eq!(
        read(
            &volume(),
            &altered,
            DiskHealthStatus::Healthy,
            identity("Acer AS500A 128G", "Q1115A0")
        )
        .unwrap()
        .percentage_used,
        None
    );
}

#[test]
fn seagate_uses_logical_sectors_and_separates_packed_hours() {
    let mut bytes = sector(&[
        (9, 73, 24433),
        (12, 100, 406),
        (194, 34, 34),
        (241, 100, 71_556_888_267),
        (242, 100, 929_874_415_457),
    ]);
    bytes[10] = 0x12;
    bytes[11] = 0x34;
    checksum(&mut bytes);
    let health = read_as(&bytes, "ST3000DM008-2DM166");
    assert_eq!(health.power_on_hours, Some(24433));
    assert_eq!(health.percentage_used, None);
    assert_eq!(
        health.total_written_bytes,
        Some((71_556_888_267u64 * 512).to_string())
    );
    let mut id = identity("ST3000DM008-2DM166", "");
    id.logical_sector_bytes = None;
    let health = read(&volume(), &bytes, DiskHealthStatus::Healthy, id).unwrap();
    assert_eq!(health.total_written_bytes, None);
    assert_eq!(health.power_on_hours, Some(24433));
    for invalid in [0, 511, 513, u32::MAX] {
        let mut id = identity("ST3000DM008-2DM166", "");
        id.logical_sector_bytes = Some(invalid);
        assert!(read(&volume(), &bytes, DiskHealthStatus::Healthy, id)
            .unwrap()
            .total_read_bytes
            .is_none());
    }
    let smr = sector(&[(9, 100, (0xabcd_u64 << 32) | 0x1234_5678)]);
    assert_eq!(
        read_as(&smr, "ST4000DM004-2CV104").power_on_hours,
        Some(0x1234_5678)
    );
}

#[test]
fn preserves_all_unknown_vendor_bytes_without_guessing_units_or_percentages() {
    let mut bytes = sector(&[
        (9, 100, 99),
        (12, 100, 12),
        (169, 100, 67),
        (241, 100, (1u64 << 48) - 1),
        (253, 255, 0xabcdef),
    ]);
    bytes[2 + 4 * 12 + 11] = 0x81;
    checksum(&mut bytes);
    let health = read(
        &volume(),
        &bytes,
        DiskHealthStatus::Warning,
        AtaIdentity::default(),
    )
    .unwrap();
    assert_eq!(health.status, DiskHealthStatus::Warning);
    assert_eq!(health.percentage_used, None);
    assert_eq!(health.power_on_hours, None);
    assert_eq!(health.power_cycles, None);
    assert_eq!(health.total_written_bytes, None);
    let ata = health.ata.unwrap();
    assert_eq!(ata.revision, 1);
    assert_eq!(ata.attributes[4].reserved, 0x81);
    assert_eq!(ata.attributes[3].raw_value(), (1u64 << 48) - 1);
    assert_eq!(ata.attributes[4].current, 255);
}

#[test]
fn rejects_corruption_duplicate_ids_empty_and_truncated_tables() {
    let mut bytes = sector(&[(9, 100, 123)]);
    assert!(attributes(&bytes[..511]).is_err());
    bytes[100] ^= 1;
    assert!(attributes(&bytes).is_err());
    for bytes in [
        [0; 512],
        [255; 512],
        sector(&[]),
        sector(&[(9, 100, 1), (9, 99, 2)]),
    ] {
        assert!(attributes(&bytes).is_err());
    }
}

#[test]
fn keeps_valid_zero_and_exhausted_values_but_rejects_out_of_range_life() {
    for (remaining, used) in [(0, Some(100)), (100, Some(0)), (101, None), (255, None)] {
        let bytes = sector(&[
            (9, 100, 0),
            (12, 100, 0),
            (177, remaining, 999),
            (241, 100, 0),
        ]);
        let health = read_as(&bytes, "Samsung SSD 870 EVO 1TB");
        assert_eq!(health.percentage_used, used);
        assert_eq!(health.power_on_hours, Some(0));
        assert_eq!(health.total_written_bytes.as_deref(), Some("0"));
    }
    let maximum = sector(&[(241, 100, (1u64 << 48) - 1)]);
    let health = read_as(&maximum, "Samsung SSD 860 EVO 500GB");
    assert_eq!(
        health.total_written_bytes,
        Some((((1u64 << 48) - 1) * 512).to_string())
    );
}

#[test]
fn decoded_attributes_keep_typed_units_and_do_not_relabel_other_vendors() {
    let bytes = smi_sector();
    let health = read(
        &volume(),
        &bytes,
        DiskHealthStatus::Healthy,
        identity("Acer AS500A 128G", "Q1115A0"),
    )
    .unwrap();
    let value = serde_json::to_value(&health).unwrap();
    let attributes = value["ata"]["attributes"].as_array().unwrap();
    let attribute = |id| attributes.iter().find(|value| value["id"] == id).unwrap();
    assert_eq!(attribute(9)["decoded"]["unit"], "hours");
    assert_eq!(attribute(169)["decoded"]["metric"], "remainingLife");
    assert_eq!(attribute(169)["decoded"]["value"], "67");
    assert_eq!(attribute(169)["decoded"]["unit"], "percent");
    assert_eq!(attribute(161)["decoded"]["metric"], "spareBlocks");
    assert_eq!(attribute(161)["decoded"]["unit"], "count");
    assert_eq!(attribute(241)["decoded"]["unit"], "bytes");
    assert_eq!(attribute(245)["decoded"]["metric"], "tlcWrittenBytes");
    assert_eq!(attribute(246)["decoded"]["metric"], "slcWrittenBytes");
    assert!(attribute(177)["decoded"].is_null());
    assert!(health.available_spare.is_none());
    assert!(health.media_errors.is_none());
    assert!(serde_json::to_vec(&health).unwrap().len() < 16_384);
    let seagate = read_as(&bytes, "ST3000DM008-2DM166");
    assert!(seagate
        .ata
        .unwrap()
        .attributes
        .iter()
        .find(|value| value.id == 169)
        .unwrap()
        .decoded
        .is_none());
}

#[test]
fn unavailable_unit_and_overflow_preserve_raw_values_without_wrapping_byte_counts() {
    let mut bytes = smi_sector();
    let offset = 2 + 21 * 12 + 5;
    bytes[offset..offset + 6].fill(255);
    checksum(&mut bytes);
    let health = read(
        &volume(),
        &bytes,
        DiskHealthStatus::Healthy,
        identity("Acer AS500A 128G", "Q1115A0"),
    )
    .unwrap();
    assert!(health.total_written_bytes.is_none());
    let attribute = health
        .ata
        .unwrap()
        .attributes
        .into_iter()
        .find(|value| value.id == 241)
        .unwrap();
    assert_eq!(attribute.raw_value(), (1u64 << 48) - 1);
    assert!(attribute.decoded.is_none());
    let bytes = sector(&[(241, 100, 10), (242, 100, 20)]);
    let mut id = identity("ST3000DM008-2DM166", "");
    id.logical_sector_bytes = Some(4096);
    let health = read(&volume(), &bytes, DiskHealthStatus::Healthy, id).unwrap();
    assert_eq!(health.total_written_bytes.as_deref(), Some("40960"));
    assert_eq!(health.total_read_bytes.as_deref(), Some("81920"));
}

#[test]
fn legacy_smi_power_on_hours_excludes_vendor_bytes_in_the_upper_half() {
    let mut bytes = smi_sector();
    let offset = 2 + 2 * 12 + 5;
    bytes[offset + 3..offset + 6].copy_from_slice(&[1, 2, 3]);
    checksum(&mut bytes);
    let health = read(
        &volume(),
        &bytes,
        DiskHealthStatus::Healthy,
        identity("Acer AS500A 128G", "Q1115A0"),
    )
    .unwrap();
    assert_eq!(health.power_on_hours, Some(16466));
    assert_eq!(health.ata.unwrap().attributes[2].raw[3..], [1, 2, 3]);
}
