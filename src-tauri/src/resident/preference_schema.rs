//! Versioned user preferences. Unknown or malformed persisted data is never overwritten.
use mangodisk_core::system_resources::metrics::MetricId;
use serde::{Deserialize, Serialize};

pub(super) const PREFERENCE_SCHEMA_VERSION: u32 = 22;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TaskbarPosition {
    #[default]
    Auto,
    Left,
    Right,
}

/// Display selection is independent of the five resource sampling workers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DisplayMetricId {
    Cpu,
    CpuTemperature,
    Gpu,
    GpuTemperature,
    Memory,
    Disk,
    DiskTemperature,
    Network,
    BatteryTemperature,
    ChargingPower,
    BatteryPower,
    SystemPower,
}

impl DisplayMetricId {
    pub fn metric(self) -> Option<MetricId> {
        match self {
            Self::Cpu => Some(MetricId::Cpu),
            Self::DiskTemperature
            | Self::CpuTemperature
            | Self::GpuTemperature
            | Self::BatteryTemperature
            | Self::ChargingPower
            | Self::BatteryPower
            | Self::SystemPower => None,
            Self::Gpu => Some(MetricId::Gpu),
            Self::Memory => Some(MetricId::Memory),
            Self::Disk => Some(MetricId::Disk),
            Self::Network => Some(MetricId::Network),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DisplayMetric {
    pub id: DisplayMetricId,
    pub enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ResidentPreferences {
    pub schema_version: u32,
    pub revision: u64,
    pub windows_cpu_temperature_enabled: bool,
    pub enabled: bool,
    pub show_icon: bool,
    pub taskbar_position: TaskbarPosition,
    pub taskbar_background: bool,
    pub taskbar_compact: bool,
    pub menu_bar_compact: bool,
    pub usage_colors: bool,
    pub color_rules: Vec<super::color_rules::MetricColorRule>,
    pub metrics: Vec<DisplayMetric>,
    pub network_interface: Option<String>,
    pub disk_volume: Option<String>,
    pub gpu_adapter: Option<String>,
}

const DISPLAY_ORDER: [DisplayMetricId; 12] = [
    DisplayMetricId::Cpu,
    DisplayMetricId::CpuTemperature,
    DisplayMetricId::Gpu,
    DisplayMetricId::GpuTemperature,
    DisplayMetricId::Memory,
    DisplayMetricId::Disk,
    DisplayMetricId::DiskTemperature,
    DisplayMetricId::BatteryTemperature,
    DisplayMetricId::ChargingPower,
    DisplayMetricId::BatteryPower,
    DisplayMetricId::SystemPower,
    DisplayMetricId::Network,
];

const PREVIOUS_DEFAULT_ORDER: [DisplayMetricId; 11] = [
    DisplayMetricId::Cpu,
    DisplayMetricId::CpuTemperature,
    DisplayMetricId::Gpu,
    DisplayMetricId::GpuTemperature,
    DisplayMetricId::Memory,
    DisplayMetricId::Disk,
    DisplayMetricId::Network,
    DisplayMetricId::BatteryTemperature,
    DisplayMetricId::ChargingPower,
    DisplayMetricId::BatteryPower,
    DisplayMetricId::SystemPower,
];

impl Default for ResidentPreferences {
    fn default() -> Self {
        Self {
            schema_version: PREFERENCE_SCHEMA_VERSION,
            revision: 0,
            windows_cpu_temperature_enabled: false,
            enabled: true,
            show_icon: true,
            taskbar_position: TaskbarPosition::default(),
            taskbar_background: true,
            taskbar_compact: false,
            menu_bar_compact: false,
            usage_colors: true,
            color_rules: super::color_rules::defaults(),
            metrics: DISPLAY_ORDER
                .into_iter()
                .map(|id| DisplayMetric {
                    id,
                    enabled: matches!(id, DisplayMetricId::Cpu | DisplayMetricId::Memory),
                })
                .collect(),
            network_interface: None,
            disk_volume: None,
            gpu_adapter: None,
        }
    }
}

impl ResidentPreferences {
    pub fn shows(&self, id: MetricId) -> bool {
        self.metrics
            .iter()
            .any(|metric| metric.id.metric() == Some(id) && metric.enabled)
    }

    pub fn effective_icon(&self) -> bool {
        self.show_icon
            || !self.metrics.iter().any(|metric| {
                metric.enabled
                    && match metric.id {
                        DisplayMetricId::CpuTemperature => self.temperature_display_supported(),
                        DisplayMetricId::DiskTemperature | DisplayMetricId::GpuTemperature => {
                            cfg!(any(target_os = "macos", windows))
                        }
                        DisplayMetricId::BatteryTemperature
                        | DisplayMetricId::ChargingPower
                        | DisplayMetricId::BatteryPower
                        | DisplayMetricId::SystemPower => {
                            cfg!(target_os = "macos")
                        }
                        _ => true,
                    }
            })
    }

    fn temperature_display_supported(&self) -> bool {
        cfg!(target_os = "macos") || (cfg!(windows) && self.windows_cpu_temperature_enabled)
    }
    pub fn shows_cpu_temperature(&self) -> bool {
        self.temperature_display_supported()
            && self
                .metrics
                .iter()
                .any(|metric| metric.id == DisplayMetricId::CpuTemperature && metric.enabled)
    }

    pub fn shows_disk_temperature(&self) -> bool {
        cfg!(any(target_os = "macos", windows))
            && self
                .metrics
                .iter()
                .any(|metric| metric.id == DisplayMetricId::DiskTemperature && metric.enabled)
    }

    pub fn shows_gpu_temperature(&self) -> bool {
        cfg!(any(target_os = "macos", windows))
            && self
                .metrics
                .iter()
                .any(|metric| metric.id == DisplayMetricId::GpuTemperature && metric.enabled)
    }

    pub fn shows_battery_item(&self, id: DisplayMetricId) -> bool {
        cfg!(target_os = "macos")
            && self
                .metrics
                .iter()
                .any(|metric| metric.id == id && metric.enabled)
    }
    pub fn shows_system_power(&self) -> bool {
        cfg!(target_os = "macos")
            && self
                .metrics
                .iter()
                .any(|metric| metric.id == DisplayMetricId::SystemPower && metric.enabled)
    }

    pub fn shows_battery(&self) -> bool {
        self.shows_battery_item(DisplayMetricId::BatteryTemperature)
            || self.shows_battery_item(DisplayMetricId::ChargingPower)
            || self.shows_battery_item(DisplayMetricId::BatteryPower)
    }

    pub fn normalize(mut self) -> Result<Self, &'static str> {
        if self.schema_version != PREFERENCE_SCHEMA_VERSION {
            return Err("preferences_version");
        }
        if !super::color_rules::valid(&self.color_rules)
            || self.metrics.len() > 64
            || [
                &self.network_interface,
                &self.disk_volume,
                &self.gpu_adapter,
            ]
            .iter()
            .any(|id| {
                id.as_ref()
                    .is_some_and(|id| id.is_empty() || id.len() > 1024 || id.contains('\0'))
            })
        {
            return Err("preferences_invalid");
        }
        let mut seen = Vec::new();
        self.metrics.retain(|metric| {
            if seen.contains(&metric.id) {
                return false;
            }
            seen.push(metric.id);
            true
        });
        // Preserve saved relative order. New items precede a trailing network
        // column so it remains last, including when older preferences migrate.
        for id in DISPLAY_ORDER
            .into_iter()
            .filter(|id| {
                !matches!(
                    id,
                    DisplayMetricId::DiskTemperature
                        | DisplayMetricId::CpuTemperature
                        | DisplayMetricId::GpuTemperature
                        | DisplayMetricId::BatteryTemperature
                        | DisplayMetricId::ChargingPower
                        | DisplayMetricId::BatteryPower
                        | DisplayMetricId::SystemPower
                )
            })
            .chain([
                DisplayMetricId::CpuTemperature,
                DisplayMetricId::GpuTemperature,
                DisplayMetricId::BatteryTemperature,
                DisplayMetricId::ChargingPower,
                DisplayMetricId::BatteryPower,
                DisplayMetricId::SystemPower,
            ])
        {
            if !seen.contains(&id) {
                let index = if self
                    .metrics
                    .last()
                    .is_some_and(|metric| metric.id == DisplayMetricId::Network)
                {
                    self.metrics.len() - 1
                } else {
                    self.metrics.len()
                };
                self.metrics
                    .insert(index, DisplayMetric { id, enabled: false });
            }
        }
        if !seen.contains(&DisplayMetricId::DiskTemperature) {
            let index = self
                .metrics
                .iter()
                .position(|metric| metric.id == DisplayMetricId::Disk)
                .map_or(self.metrics.len(), |index| index + 1);
            self.metrics.insert(
                index,
                DisplayMetric {
                    id: DisplayMetricId::DiskTemperature,
                    enabled: false,
                },
            );
        }
        Ok(self)
    }
}

pub fn decode(mut value: serde_json::Value) -> Result<ResidentPreferences, &'static str> {
    let version = value.get("schemaVersion").and_then(|v| v.as_u64());
    if matches!(version, Some(2..=21)) {
        if value
            .get("metrics")
            .and_then(|value| value.as_array())
            .is_some_and(|metrics| {
                metrics.iter().any(|metric| {
                    metric.get("id").and_then(|id| id.as_str()) == Some("diskTemperature")
                })
            })
        {
            return Err("preferences_invalid");
        }
        if let Some(rules) = value
            .get_mut("colorRules")
            .and_then(|value| value.as_array_mut())
        {
            if rules
                .iter()
                .any(|rule| rule.get("id").and_then(|id| id.as_str()) == Some("diskTemperature"))
            {
                return Err("preferences_invalid");
            }
            if version.is_some_and(|version| version >= 16) {
                rules.push(
                    serde_json::to_value(super::color_rules::MetricColorRule::defaults(
                        DisplayMetricId::DiskTemperature,
                    ))
                    .map_err(|_| "preferences_invalid")?,
                );
            }
        }
    }
    if matches!(version, Some(2..=12))
        && value
            .get("metrics")
            .and_then(|v| v.as_array())
            .is_some_and(|metrics| {
                metrics.iter().any(|metric| {
                    metric.get("id").and_then(|id| id.as_str()) == Some("gpuTemperature")
                })
            })
    {
        return Err("preferences_invalid");
    }
    if matches!(version, Some(2..=11)) {
        if value.get("windowsCpuTemperatureEnabled").is_some() {
            return Err("preferences_invalid");
        }
        value["windowsCpuTemperatureEnabled"] = false.into();
    }
    if matches!(version, Some(2..=10))
        && value
            .get("metrics")
            .and_then(|v| v.as_array())
            .is_some_and(|metrics| {
                metrics.iter().any(|metric| {
                    metric.get("id").and_then(|id| id.as_str()) == Some("cpuTemperature")
                })
            })
    {
        return Err("preferences_invalid");
    }
    if matches!(version, Some(2..=9)) {
        if value.get("gpuAdapter").is_some() {
            return Err("preferences_invalid");
        }
        value["gpuAdapter"] = serde_json::Value::Null;
    }
    if matches!(version, Some(2..=7)) {
        for field in [
            "menuBarCompact",
            "usageColors",
            "usageWarningPercent",
            "usageCriticalPercent",
        ] {
            if value.get(field).is_some() {
                return Err("preferences_invalid");
            }
        }
        // Upgrade only the former default sequence; keep deliberate custom order.
        if let Some(metrics) = value.get_mut("metrics").and_then(|v| v.as_array_mut()) {
            let ids: Vec<_> = metrics
                .iter()
                .filter_map(|m| m.get("id").and_then(|id| id.as_str()))
                .collect();
            if ids == ["cpu", "memory", "network", "disk"] {
                metrics.swap(2, 3);
            }
        }
        value["menuBarCompact"] = false.into();
        value["usageColors"] = true.into();
        value["usageWarningPercent"] = 70.into();
        value["usageCriticalPercent"] = 90.into();
    }
    // Versions 2–6 had no density preference. Preserve their appearance and
    // reject fields that did not belong to the declared legacy protocol.
    if matches!(
        value.get("schemaVersion").and_then(|v| v.as_u64()),
        Some(2..=6)
    ) {
        if value.get("taskbarCompact").is_some() {
            return Err("preferences_invalid");
        }
        value["taskbarCompact"] = false.into();
    }
    let mut preferences = match value
        .get("schemaVersion")
        .and_then(serde_json::Value::as_u64)
    {
        Some(1) => {
            #[derive(Deserialize)]
            #[serde(rename_all = "camelCase", deny_unknown_fields)]
            struct Previous {
                schema_version: u32,
                enabled: bool,
                show_memory: bool,
            }
            let previous: Previous =
                serde_json::from_value(value).map_err(|_| "preferences_invalid")?;
            if previous.schema_version != 1 {
                return Err("preferences_version");
            }
            let mut migrated = ResidentPreferences {
                enabled: previous.enabled,
                color_rules: super::color_rules::legacy(70, 90),
                // Preserve legacy metric and position choices. Windows now uses
                // taskbar presentation for every supported preference version.
                taskbar_position: TaskbarPosition::Right,
                ..Default::default()
            };
            for metric in &mut migrated.metrics {
                metric.enabled = metric.id == DisplayMetricId::Memory && previous.show_memory;
            }
            Ok(migrated)
        }
        Some(2) => {
            // Version 2 predates taskbar settings. Retain metric choices and
            // migrate the display destination to the sole supported taskbar mode.
            if value.get("windowsDisplayMode").is_some()
                || value.get("taskbarPosition").is_some()
                || value.get("taskbarBackground").is_some()
            {
                return Err("preferences_invalid");
            }
            value["schemaVersion"] = 14.into();
            value["taskbarBackground"] = true.into();
            value["windowsDisplayMode"] = "tray".into();
            value["taskbarPosition"] = "right".into();
            migrate_display_destination(value)
        }
        Some(3) => {
            if value.get("taskbarPosition").is_some() || value.get("taskbarBackground").is_some() {
                return Err("preferences_invalid");
            }
            value["schemaVersion"] = 14.into();
            value["taskbarBackground"] = true.into();
            value["taskbarPosition"] = "right".into();
            migrate_display_destination(value)
        }
        Some(4) => {
            if !matches!(
                value.get("taskbarPosition").and_then(|v| v.as_str()),
                Some("left" | "right")
            ) {
                return Err("preferences_invalid");
            }
            if value.get("taskbarBackground").is_some() {
                return Err("preferences_invalid");
            }
            // Existing installations keep the opaque presentation until the
            // user explicitly selects transparency.
            value["schemaVersion"] = 14.into();
            value["taskbarBackground"] = true.into();
            migrate_display_destination(value)
        }
        Some(5) => {
            // Version 5 predates automatic placement. Do not infer whether a saved
            // left/right value was a default or an explicit user decision.
            if !matches!(
                value.get("taskbarPosition").and_then(|v| v.as_str()),
                Some("left" | "right")
            ) {
                return Err("preferences_invalid");
            }
            value["schemaVersion"] = 14.into();
            migrate_display_destination(value)
        }
        Some(6) => {
            value["schemaVersion"] = 14.into();
            migrate_display_destination(value)
        }
        Some(7..=13) => {
            value["schemaVersion"] = 14.into();
            migrate_display_destination(value)
        }
        Some(14) => {
            if value.get("windowsDisplayMode").is_some() {
                return Err("preferences_invalid");
            }
            migrate_display_destination(value)
        }
        Some(15) => {
            let rules = super::color_rules::migrate_bands(
                value
                    .get("colorRules")
                    .cloned()
                    .ok_or("preferences_invalid")?,
            )?;
            value["colorRules"] = serde_json::to_value(rules).map_err(|_| "preferences_invalid")?;
            value["schemaVersion"] = 16.into();
            migrate_battery(value)
        }
        Some(16 | 17) => migrate_battery(value),
        Some(18) => migrate_system_power(value),
        Some(19) => {
            let mut preferences = serde_json::from_value::<ResidentPreferences>(value)
                .map_err(|_| "preferences_invalid")?;
            upgrade_battery_temperature_default(&mut preferences);
            preferences.schema_version = PREFERENCE_SCHEMA_VERSION;
            preferences.normalize()
        }
        Some(20 | 21) => {
            let mut preferences = serde_json::from_value::<ResidentPreferences>(value)
                .map_err(|_| "preferences_invalid")?;
            preferences.schema_version = PREFERENCE_SCHEMA_VERSION;
            preferences.normalize()
        }
        Some(22) => serde_json::from_value::<ResidentPreferences>(value)
            .map_err(|_| "preferences_invalid")?
            .normalize(),
        _ => Err("preferences_version"),
    }?;
    // Upgrade the exact former default once. Version-21 custom orders, including
    // ones that resemble the old default, must remain stable across reloads.
    if version.is_some_and(|version| version < 21)
        && preferences
            .metrics
            .iter()
            .map(|metric| metric.id)
            .filter(|id| *id != DisplayMetricId::DiskTemperature)
            .eq(PREVIOUS_DEFAULT_ORDER)
    {
        let index = preferences
            .metrics
            .iter()
            .position(|metric| metric.id == DisplayMetricId::Network)
            .unwrap();
        let network = preferences.metrics.remove(index);
        preferences.metrics.push(network);
    }
    Ok(preferences)
}

fn upgrade_battery_temperature_default(preferences: &mut ResidentPreferences) {
    if let Some(rule) = preferences.color_rules.iter_mut().find(|rule| {
        rule.id == DisplayMetricId::BatteryTemperature && !rule.enabled && rule.rules.is_empty()
    }) {
        // Match only the old inactive, empty default. Saved custom rules and
        // version-20 users who deliberately clear rules must remain unchanged.
        *rule = super::color_rules::MetricColorRule::defaults(DisplayMetricId::BatteryTemperature);
    }
}

fn migrate_system_power(value: serde_json::Value) -> Result<ResidentPreferences, &'static str> {
    let mut preferences =
        serde_json::from_value::<ResidentPreferences>(value).map_err(|_| "preferences_invalid")?;
    if preferences
        .metrics
        .iter()
        .any(|metric| metric.id == DisplayMetricId::SystemPower)
    {
        return Err("preferences_invalid");
    }
    // Only append the new rule. Existing malformed version-18 rules must still
    // fail validation instead of being silently repaired and persisted.
    preferences
        .color_rules
        .push(super::color_rules::MetricColorRule::defaults(
            DisplayMetricId::SystemPower,
        ));
    preferences.schema_version = PREFERENCE_SCHEMA_VERSION;
    upgrade_battery_temperature_default(&mut preferences);
    preferences.normalize()
}

fn migrate_battery(mut value: serde_json::Value) -> Result<ResidentPreferences, &'static str> {
    let rules = value
        .get_mut("colorRules")
        .and_then(|value| value.as_array_mut())
        .ok_or("preferences_invalid")?;
    for id in [
        DisplayMetricId::BatteryTemperature,
        DisplayMetricId::ChargingPower,
        DisplayMetricId::BatteryPower,
        DisplayMetricId::SystemPower,
    ] {
        let name = serde_json::to_value(id).map_err(|_| "preferences_invalid")?;
        if !rules.iter().any(|rule| rule.get("id") == Some(&name)) {
            rules.push(
                serde_json::to_value(super::color_rules::MetricColorRule::defaults(id))
                    .map_err(|_| "preferences_invalid")?,
            );
        }
    }
    value["schemaVersion"] = PREFERENCE_SCHEMA_VERSION.into();
    let mut preferences =
        serde_json::from_value::<ResidentPreferences>(value).map_err(|_| "preferences_invalid")?;
    upgrade_battery_temperature_default(&mut preferences);
    preferences.normalize()
}

fn migrate_display_destination(
    mut value: serde_json::Value,
) -> Result<ResidentPreferences, &'static str> {
    if let Some(mode) = value
        .as_object_mut()
        .and_then(|object| object.remove("windowsDisplayMode"))
    {
        if !matches!(mode.as_str(), Some("taskbar" | "tray")) {
            return Err("preferences_invalid");
        }
    }
    if value.get("colorRules").is_some() {
        return Err("preferences_invalid");
    }
    let warning = value
        .get("usageWarningPercent")
        .and_then(|v| v.as_u64())
        .ok_or("preferences_invalid")?;
    let critical = value
        .get("usageCriticalPercent")
        .and_then(|v| v.as_u64())
        .ok_or("preferences_invalid")?;
    if warning < 1 || warning >= critical || critical > 100 {
        return Err("preferences_invalid");
    }
    value
        .as_object_mut()
        .ok_or("preferences_invalid")?
        .remove("usageWarningPercent");
    value
        .as_object_mut()
        .ok_or("preferences_invalid")?
        .remove("usageCriticalPercent");
    value["colorRules"] =
        serde_json::to_value(super::color_rules::legacy(warning as u16, critical as u16))
            .map_err(|_| "preferences_invalid")?;
    value["schemaVersion"] = 16.into();
    migrate_battery(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn disk_temperature_migrates_after_disk_disabled_without_changing_saved_settings() {
        let mut old = serde_json::to_value(ResidentPreferences::default()).unwrap();
        old["schemaVersion"] = 21.into();
        old["networkInterface"] = "saved-vpn".into();
        old["diskVolume"] = "saved-volume".into();
        for field in ["metrics", "colorRules"] {
            old[field]
                .as_array_mut()
                .unwrap()
                .retain(|row| row["id"] != "diskTemperature");
        }
        old["metrics"].as_array_mut().unwrap().reverse();
        let before = old.clone();
        let migrated = super::decode(old).unwrap();
        let disk = migrated
            .metrics
            .iter()
            .position(|row| row.id == DisplayMetricId::Disk)
            .unwrap();
        assert_eq!(
            migrated.metrics[disk + 1],
            DisplayMetric {
                id: DisplayMetricId::DiskTemperature,
                enabled: false
            }
        );
        let current = serde_json::to_value(&migrated).unwrap();
        assert_eq!(previous_rows(&current["metrics"]), before["metrics"]);
        assert_eq!(previous_rows(&current["colorRules"]), before["colorRules"]);
        assert_eq!(migrated.network_interface.as_deref(), Some("saved-vpn"));
        assert_eq!(migrated.disk_volume.as_deref(), Some("saved-volume"));
        assert_eq!(
            migrated
                .color_rules
                .iter()
                .find(|rule| rule.id == DisplayMetricId::DiskTemperature),
            Some(&super::super::color_rules::MetricColorRule::defaults(
                DisplayMetricId::DiskTemperature
            ))
        );
        assert_eq!(super::decode(current).unwrap(), migrated);
        let mut invalid = before;
        invalid["metrics"]
            .as_array_mut()
            .unwrap()
            .push(serde_json::json!({"id":"diskTemperature", "enabled":true}));
        assert_eq!(super::decode(invalid).unwrap_err(), "preferences_invalid");
    }

    #[test]
    fn saved_disk_temperature_colors_remain_explicit_including_disabled_empty_rules() {
        use super::super::color_rules::{ColorRule, DisplayColor};
        for enabled in [false, true] {
            for custom in [false, true] {
                let mut preferences = ResidentPreferences::default();
                let rule = preferences
                    .color_rules
                    .iter_mut()
                    .find(|rule| rule.id == DisplayMetricId::DiskTemperature)
                    .unwrap();
                rule.enabled = enabled;
                rule.rules = if custom {
                    vec![ColorRule::AtLeast {
                        value: 80,
                        color: DisplayColor::Blue,
                    }]
                } else {
                    vec![]
                };
                assert_eq!(
                    super::decode(serde_json::to_value(&preferences).unwrap()).unwrap(),
                    preferences
                );
            }
        }
    }

    fn previous_rows(value: &serde_json::Value) -> serde_json::Value {
        serde_json::Value::Array(
            value
                .as_array()
                .unwrap()
                .iter()
                .filter(|row| row["id"] != "diskTemperature")
                .cloned()
                .collect(),
        )
    }
    fn decode_fixture(mut value: serde_json::Value) -> Result<ResidentPreferences, &'static str> {
        if value
            .get("schemaVersion")
            .and_then(|value| value.as_u64())
            .is_some_and(|version| version < 22)
        {
            for field in ["metrics", "colorRules"] {
                if let Some(rows) = value.get_mut(field).and_then(|value| value.as_array_mut()) {
                    rows.retain(|row| {
                        row.get("id").and_then(|id| id.as_str()) != Some("diskTemperature")
                    });
                }
            }
        }
        super::decode(value)
    }
    #[test]
    fn version_twenty_upgrades_only_the_complete_previous_default_order() {
        for custom in [false, true] {
            let mut previous = ResidentPreferences {
                schema_version: 20,
                revision: 123,
                metrics: PREVIOUS_DEFAULT_ORDER
                    .into_iter()
                    .enumerate()
                    .map(|(index, id)| DisplayMetric {
                        id,
                        enabled: index % 2 == 0,
                    })
                    .collect(),
                ..Default::default()
            };
            if custom {
                previous.metrics.swap(0, 1);
            }
            let mut expected = previous.clone();
            expected.schema_version = 22;
            if !custom {
                let network = expected.metrics.remove(6);
                expected.metrics.push(network);
            }
            let migrated = decode_fixture(serde_json::to_value(previous).unwrap()).unwrap();
            expected
                .color_rules
                .retain(|rule| rule.id != DisplayMetricId::DiskTemperature);
            expected
                .color_rules
                .push(super::super::color_rules::MetricColorRule::defaults(
                    DisplayMetricId::DiskTemperature,
                ));
            assert_eq!(migrated, expected.normalize().unwrap());
            assert_eq!(
                decode_fixture(serde_json::to_value(&migrated).unwrap()).unwrap(),
                migrated
            );
        }
    }
    #[test]
    fn current_custom_order_can_match_the_former_default_without_being_reordered() {
        let mut preferences = ResidentPreferences::default();
        preferences.metrics.sort_by_key(|metric| {
            PREVIOUS_DEFAULT_ORDER
                .iter()
                .position(|id| *id == metric.id)
                .unwrap_or(PREVIOUS_DEFAULT_ORDER.len())
        });
        assert_eq!(
            decode_fixture(serde_json::to_value(&preferences).unwrap()).unwrap(),
            preferences
        );
    }
    #[test]
    fn missing_display_items_keep_a_trailing_network_last() {
        let expected = ResidentPreferences::default();
        assert_eq!(
            expected.metrics.last().unwrap().id,
            DisplayMetricId::Network
        );
        let mut partial = expected.clone();
        partial.metrics.retain(|metric| {
            !matches!(
                metric.id,
                DisplayMetricId::BatteryTemperature
                    | DisplayMetricId::ChargingPower
                    | DisplayMetricId::BatteryPower
                    | DisplayMetricId::SystemPower
            )
        });
        assert_eq!(partial.normalize().unwrap(), expected);
    }
    #[test]
    fn version_nineteen_upgrades_only_the_old_empty_battery_temperature_default() {
        use super::super::color_rules::{ColorRule, DisplayColor, MetricColorRule};
        for custom in [false, true] {
            let mut preferences = ResidentPreferences {
                schema_version: 19,
                ..Default::default()
            };
            let rule = preferences
                .color_rules
                .iter_mut()
                .find(|rule| rule.id == DisplayMetricId::BatteryTemperature)
                .unwrap();
            rule.enabled = false;
            rule.rules = if custom {
                vec![ColorRule::AtLeast {
                    value: 50,
                    color: DisplayColor::Blue,
                }]
            } else {
                vec![]
            };
            let before = preferences.clone();
            let migrated = decode_fixture(serde_json::to_value(preferences).unwrap()).unwrap();
            let current = migrated
                .color_rules
                .iter()
                .find(|rule| rule.id == DisplayMetricId::BatteryTemperature)
                .unwrap();
            if custom {
                assert_eq!(
                    current,
                    before
                        .color_rules
                        .iter()
                        .find(|rule| rule.id == DisplayMetricId::BatteryTemperature)
                        .unwrap()
                );
            } else {
                assert_eq!(
                    *current,
                    MetricColorRule::defaults(DisplayMetricId::BatteryTemperature)
                );
                assert!(current.enabled);
            }
            assert_eq!(migrated.metrics, before.metrics);
            let mut cleared = migrated;
            let current = cleared
                .color_rules
                .iter_mut()
                .find(|rule| rule.id == DisplayMetricId::BatteryTemperature)
                .unwrap();
            current.enabled = false;
            current.rules.clear();
            assert_eq!(
                decode_fixture(serde_json::to_value(&cleared).unwrap()).unwrap(),
                cleared
            );
        }
    }

    #[test]
    fn version_eighteen_does_not_repair_malformed_existing_color_rules() {
        let mut old = serde_json::to_value(ResidentPreferences::default()).unwrap();
        old["schemaVersion"] = 18.into();
        old["metrics"]
            .as_array_mut()
            .unwrap()
            .retain(|row| row["id"] != "systemPower");
        old["colorRules"]
            .as_array_mut()
            .unwrap()
            .retain(|row| row["id"] != "systemPower" && row["id"] != "batteryPower");
        assert!(decode_fixture(old).is_err());
    }
    #[test]
    fn version_eighteen_preserves_settings_and_appends_system_power_disabled() {
        let mut old = serde_json::to_value(ResidentPreferences::default()).unwrap();
        old["schemaVersion"] = 18.into();
        old["revision"] = 123.into();
        for key in ["metrics", "colorRules"] {
            old[key]
                .as_array_mut()
                .unwrap()
                .retain(|row| row["id"] != "systemPower");
            old[key].as_array_mut().unwrap().reverse();
        }
        let before = old.clone();
        let migrated = serde_json::to_value(decode_fixture(old).unwrap()).unwrap();
        assert_eq!(migrated["revision"], before["revision"]);
        for key in ["metrics", "colorRules"] {
            let previous = previous_rows(&before[key]);
            let previous = previous.as_array().unwrap();
            let current = previous_rows(&migrated[key]);
            let current = current.as_array().unwrap();
            assert_eq!(&current[..previous.len()], previous);
            assert_eq!(current.last().unwrap()["id"], "systemPower");
            assert_eq!(current.last().unwrap()["enabled"], false);
        }
    }

    #[test]
    fn actual_version_fifteen_six_color_metrics_migrate_with_empty_power_rules() {
        let mut old = serde_json::to_value(ResidentPreferences::default()).unwrap();
        old["schemaVersion"] = 15.into();
        old["colorRules"] = serde_json::json!(["cpu", "cpuTemperature", "gpu", "gpuTemperature", "memory", "disk"])
            .as_array().unwrap().iter().map(|id| serde_json::json!({
                "id": id, "enabled": true,
                "bands": [{"from":0,"color":"default"},{"from":70,"color":"orange"},{"from":90,"color":"red"}]
            })).collect();
        let migrated = decode_fixture(old).unwrap();
        let power = migrated
            .color_rules
            .iter()
            .find(|rule| rule.id == DisplayMetricId::SystemPower)
            .unwrap();
        assert!(!power.enabled);
        assert!(power.rules.is_empty());
    }

    #[test]
    fn version_seventeen_preserves_battery_selections_and_appends_battery_power_disabled() {
        let mut preferences = ResidentPreferences::default();
        for row in &mut preferences.metrics {
            if matches!(
                row.id,
                DisplayMetricId::BatteryTemperature | DisplayMetricId::ChargingPower
            ) {
                row.enabled = true;
            }
        }
        let mut old = serde_json::to_value(preferences).unwrap();
        old["schemaVersion"] = 17.into();
        for key in ["metrics", "colorRules"] {
            old[key].as_array_mut().unwrap().retain(|row| {
                row["id"] != "batteryPower"
                    && row["id"] != "systemPower"
                    && row["id"] != "diskTemperature"
            });
        }
        let old_metrics = old["metrics"].clone();
        let old_colors = old["colorRules"].clone();
        let migrated = decode_fixture(old).unwrap();
        assert!(
            migrated.shows_battery_item(DisplayMetricId::ChargingPower)
                || !cfg!(target_os = "macos")
        );
        let current = serde_json::to_value(migrated).unwrap();
        assert_eq!(
            current["metrics"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|row| {
                    row["id"] != "batteryPower"
                        && row["id"] != "systemPower"
                        && row["id"] != "diskTemperature"
                })
                .collect::<Vec<_>>(),
            old_metrics
                .as_array()
                .unwrap()
                .iter()
                .filter(|row| row["id"] != "diskTemperature")
                .collect::<Vec<_>>()
        );
        assert_eq!(
            &current["colorRules"].as_array().unwrap()[..8],
            previous_rows(&old_colors).as_array().unwrap()
        );
        for id in ["batteryPower", "systemPower"] {
            let row = current["metrics"]
                .as_array()
                .unwrap()
                .iter()
                .find(|row| row["id"] == id)
                .unwrap();
            assert_eq!(row["enabled"], false);
        }
    }
    #[test]
    fn version_sixteen_preserves_custom_colors_and_appends_disabled_battery_items() {
        let mut old = serde_json::to_value(ResidentPreferences::default()).unwrap();
        old["schemaVersion"] = 16.into();
        for key in ["metrics", "colorRules"] {
            old[key].as_array_mut().unwrap().retain(|row| {
                !matches!(
                    row["id"].as_str(),
                    Some("batteryTemperature" | "chargingPower" | "batteryPower" | "systemPower")
                )
            });
        }
        old["revision"] = 123.into();
        old["metrics"].as_array_mut().unwrap().reverse();
        let old_metrics = old["metrics"].clone();
        let old_colors = old["colorRules"].clone();
        let migrated = decode_fixture(old).unwrap();
        assert_eq!(migrated.revision, 123);
        assert!(migrated
            .metrics
            .iter()
            .filter(|row| matches!(
                row.id,
                DisplayMetricId::BatteryTemperature
                    | DisplayMetricId::ChargingPower
                    | DisplayMetricId::BatteryPower
            ))
            .all(|row| !row.enabled));
        let current = serde_json::to_value(&migrated).unwrap();
        assert_eq!(
            &previous_rows(&current["metrics"]).as_array().unwrap()[..7],
            previous_rows(&old_metrics).as_array().unwrap()
        );
        assert_eq!(
            &current["colorRules"].as_array().unwrap()[..6],
            previous_rows(&old_colors).as_array().unwrap()
        );
        assert_eq!(decode_fixture(current).unwrap(), migrated);
    }

    #[test]
    fn version_fifteen_migrates_colors_without_changing_selections_or_boundaries() {
        let mut old = serde_json::to_value(ResidentPreferences::default()).unwrap();
        old["schemaVersion"] = 15.into();
        old["colorRules"] = serde_json::Value::Array(super::super::color_rules::COLOR_METRICS.iter().map(|id| serde_json::json!({
            "id": id, "enabled": false, "bands": [{"from": 0, "color": "blue"}, {"from": 55, "color": "yellow"}, {"from": 95, "color": "red"}]
        })).collect());
        let migrated = decode_fixture(old.clone()).unwrap();
        assert_eq!(migrated.schema_version, 22);
        assert_eq!(
            previous_rows(&serde_json::to_value(&migrated.metrics).unwrap()),
            previous_rows(&old["metrics"])
        );
        for rule in migrated
            .color_rules
            .iter()
            .filter(|rule| rule.id != DisplayMetricId::DiskTemperature)
        {
            assert!(!rule.enabled);
            assert_eq!(rule.rules[0].bounds(), (0, Some(55)));
            assert_eq!(rule.rules[1].bounds(), (55, Some(95)));
            assert_eq!(rule.rules[2].bounds(), (95, None));
            assert_eq!(
                rule.rules[0].color(),
                super::super::color_rules::DisplayColor::Blue
            );
        }
        old["colorRules"][0]["bands"][1]["from"] = 0.into();
        assert!(decode_fixture(old).is_err());
    }

    #[test]
    fn current_rules_round_trip_empty_single_and_sparse_conditions() {
        let mut preferences = ResidentPreferences::default();
        preferences.color_rules[0].rules.clear();
        preferences.color_rules[1].rules.truncate(1);
        preferences.color_rules[2].rules.remove(1);
        assert_eq!(
            decode_fixture(serde_json::to_value(&preferences).unwrap()).unwrap(),
            preferences
        );
        let mut malformed = serde_json::to_value(&preferences).unwrap();
        malformed["colorRules"][1]["rules"][0]["from"] = 1.into();
        assert!(decode_fixture(malformed).is_err());
    }

    #[test]
    fn current_rules_reject_unknown_colors_missing_metrics_and_legacy_fields() {
        let current = serde_json::to_value(ResidentPreferences::default()).unwrap();
        assert_eq!(decode_fixture(current.clone()).unwrap().schema_version, 22);
        let mut value = current.clone();
        value["colorRules"][0]["rules"][0]["color"] = "purple".into();
        assert!(decode_fixture(value).is_err());
        let mut value = current.clone();
        value["colorRules"].as_array_mut().unwrap().pop();
        assert!(decode_fixture(value).is_err());
        let mut value = current.clone();
        value["usageWarningPercent"] = 70.into();
        assert!(decode_fixture(value).is_err());
        let mut value = current;
        value["schemaVersion"] = 14.into();
        value["usageWarningPercent"] = 70.into();
        value["usageCriticalPercent"] = 90.into();
        assert!(decode_fixture(value).is_err());
    }

    #[test]
    fn version_thirteen_removes_tray_destination_and_preserves_user_settings() {
        for mode in ["tray", "taskbar"] {
            let mut old = version_fourteen_preferences();
            old["schemaVersion"] = 13.into();
            old["windowsDisplayMode"] = mode.into();
            old["gpuAdapter"] = "selected-adapter".into();
            old["enabled"] = false.into();
            old["revision"] = 47.into();
            let migrated = decode_fixture(old).unwrap();
            assert_eq!(migrated.schema_version, 22);
            assert_eq!(migrated.revision, 47);
            assert!(!migrated.enabled);
            assert_eq!(migrated.gpu_adapter.as_deref(), Some("selected-adapter"));
            let serialized = serde_json::to_value(&migrated).unwrap();
            assert!(serialized.get("windowsDisplayMode").is_none());
            assert_eq!(decode_fixture(serialized).unwrap(), migrated);
        }
        let mut current = serde_json::to_value(ResidentPreferences::default()).unwrap();
        current["windowsDisplayMode"] = "tray".into();
        assert!(decode_fixture(current).is_err());
    }

    #[test]
    fn version_twelve_preserves_gpu_selection_and_appends_unselected_temperature() {
        let mut value = version_fourteen_preferences();
        value["schemaVersion"] = 12.into();
        value["revision"] = 84.into();
        value["gpuAdapter"] = "fixed-gpu".into();
        let metrics = value["metrics"].as_array_mut().unwrap();
        metrics.retain(|metric| metric["id"] != "gpuTemperature");
        metrics.reverse();
        let mut expected = metrics.clone();
        expected.push(serde_json::json!({"id":"gpuTemperature", "enabled":false}));
        let migrated = decode_fixture(value.clone()).unwrap();
        assert_eq!(migrated.revision, 84);
        assert_eq!(migrated.gpu_adapter.as_deref(), Some("fixed-gpu"));
        assert!(!migrated.shows_gpu_temperature());
        assert_eq!(
            previous_rows(&serde_json::to_value(&migrated).unwrap()["metrics"]),
            previous_rows(&serde_json::json!(expected))
        );
        value["metrics"]
            .as_array_mut()
            .unwrap()
            .push(serde_json::json!({"id":"gpuTemperature", "enabled":true}));
        assert!(decode_fixture(value).is_err());
    }

    #[test]
    fn version_eleven_never_opts_into_privileged_collection_during_migration() {
        let mut old = version_fourteen_preferences();
        old["metrics"]
            .as_array_mut()
            .unwrap()
            .retain(|m| m["id"] != "gpuTemperature");
        old["schemaVersion"] = 11.into();
        old["revision"] = 43.into();
        old["metrics"].as_array_mut().unwrap().reverse();
        old["metrics"][0]["enabled"] = true.into();
        old.as_object_mut()
            .unwrap()
            .remove("windowsCpuTemperatureEnabled");
        let migrated = decode_fixture(old.clone()).unwrap();
        let mut expected = old["metrics"].clone();
        expected
            .as_array_mut()
            .unwrap()
            .push(serde_json::json!({"id":"gpuTemperature","enabled":false}));
        assert!(!migrated.windows_cpu_temperature_enabled);
        assert_eq!(migrated.revision, 43);
        assert_eq!(
            previous_rows(&serde_json::to_value(migrated).unwrap()["metrics"]),
            previous_rows(&expected)
        );
        old["windowsCpuTemperatureEnabled"] = true.into();
        assert!(decode_fixture(old).is_err());
    }

    #[test]
    fn version_ten_preserves_choices_and_appends_disabled_temperature() {
        let mut old = legacy_current_preferences();
        old["schemaVersion"] = 10.into();
        old.as_object_mut()
            .unwrap()
            .remove("windowsCpuTemperatureEnabled");
        old["revision"] = 42.into();
        old["metrics"].as_array_mut().unwrap().reverse();
        let migrated = decode_fixture(old.clone()).unwrap();
        assert_eq!(migrated.schema_version, 22);
        assert_eq!(migrated.revision, 42);
        let mut expected = old["metrics"].clone();
        expected
            .as_array_mut()
            .unwrap()
            .push(serde_json::json!({"id":"cpuTemperature","enabled":false}));
        expected
            .as_array_mut()
            .unwrap()
            .push(serde_json::json!({"id":"gpuTemperature","enabled":false}));
        assert_eq!(
            previous_rows(&serde_json::to_value(&migrated).unwrap()["metrics"]),
            previous_rows(&expected)
        );
        assert!(!migrated.shows_cpu_temperature());
        old["metrics"]
            .as_array_mut()
            .unwrap()
            .push(serde_json::json!({"id":"cpuTemperature","enabled":true}));
        assert!(decode_fixture(old).is_err());
        let mut selected = migrated;
        let last = selected.metrics.pop().unwrap();
        selected.metrics.insert(
            1,
            DisplayMetric {
                enabled: true,
                ..last
            },
        );
        assert_eq!(
            decode_fixture(serde_json::to_value(&selected).unwrap()).unwrap(),
            selected
        );
        let mut invalid = serde_json::to_value(selected).unwrap();
        invalid["metrics"][1]["enabled"] = "true".into();
        assert!(decode_fixture(invalid).is_err());
    }

    #[test]
    fn temperature_only_suppresses_logo_fallback_only_on_macos() {
        let mut prefs = ResidentPreferences {
            show_icon: false,
            ..Default::default()
        };
        for metric in &mut prefs.metrics {
            metric.enabled = metric.id == DisplayMetricId::CpuTemperature;
        }
        assert_eq!(prefs.effective_icon(), !cfg!(target_os = "macos"));
        prefs
            .metrics
            .iter_mut()
            .for_each(|metric| metric.enabled = false);
        assert!(prefs.effective_icon());
    }

    fn legacy_current_preferences() -> serde_json::Value {
        let mut value = version_fourteen_preferences();
        value["metrics"]
            .as_array_mut()
            .unwrap()
            .retain(|metric| metric["id"] != "cpuTemperature" && metric["id"] != "gpuTemperature");
        value
    }

    fn legacy_preferences() -> serde_json::Value {
        let mut value = legacy_current_preferences();
        value.as_object_mut().unwrap().remove("gpuAdapter");
        value["schemaVersion"] = 7.into();
        value
            .as_object_mut()
            .unwrap()
            .remove("windowsCpuTemperatureEnabled");
        value["metrics"]
            .as_array_mut()
            .unwrap()
            .retain(|metric| metric["id"] != "gpu");
        for field in [
            "menuBarCompact",
            "usageColors",
            "usageWarningPercent",
            "usageCriticalPercent",
        ] {
            value.as_object_mut().unwrap().remove(field);
        }
        value
    }

    #[test]
    fn version_nine_defaults_to_automatic_gpu_and_preserves_current_choices() {
        let mut old = legacy_current_preferences();
        old["schemaVersion"] = 9.into();
        old.as_object_mut()
            .unwrap()
            .remove("windowsCpuTemperatureEnabled");
        old["networkInterface"] = "wifi".into();
        old.as_object_mut().unwrap().remove("gpuAdapter");
        let migrated = decode_fixture(old.clone()).unwrap();
        assert_eq!(migrated.schema_version, 22);
        assert_eq!(migrated.network_interface.as_deref(), Some("wifi"));
        assert!(migrated.gpu_adapter.is_none());
        old["gpuAdapter"] = "unexpected".into();
        assert!(decode_fixture(old).is_err());
        let fixed = ResidentPreferences {
            gpu_adapter: Some("pci:10de:2684:1:0:0:0".into()),
            ..migrated
        };
        assert_eq!(
            decode_fixture(serde_json::to_value(&fixed).unwrap()).unwrap(),
            fixed
        );
        assert!(ResidentPreferences {
            gpu_adapter: Some("".into()),
            ..fixed
        }
        .normalize()
        .is_err());
    }
    #[test]
    fn version_eight_appends_disabled_gpu_without_changing_saved_choices() {
        let mut old = legacy_current_preferences();
        old["schemaVersion"] = 8.into();
        old.as_object_mut()
            .unwrap()
            .remove("windowsCpuTemperatureEnabled");
        old.as_object_mut().unwrap().remove("gpuAdapter");
        old["revision"] = 42.into();
        old["metrics"] = serde_json::json!([
            {"id":"memory", "enabled":true}, {"id":"cpu", "enabled":false},
            {"id":"network", "enabled":true}, {"id":"disk", "enabled":false}
        ]);
        let migrated = decode_fixture(old).unwrap();
        assert_eq!(migrated.schema_version, 22);
        assert_eq!(migrated.revision, 42);
        assert_eq!(
            migrated
                .metrics
                .iter()
                .map(|metric| metric.id)
                .filter(|id| *id != DisplayMetricId::DiskTemperature)
                .collect::<Vec<_>>(),
            vec![
                DisplayMetricId::Memory,
                DisplayMetricId::Cpu,
                DisplayMetricId::Network,
                DisplayMetricId::Disk,
                DisplayMetricId::Gpu,
                DisplayMetricId::CpuTemperature,
                DisplayMetricId::GpuTemperature,
                DisplayMetricId::BatteryTemperature,
                DisplayMetricId::ChargingPower,
                DisplayMetricId::BatteryPower,
                DisplayMetricId::SystemPower,
            ]
        );
        assert!(!migrated.shows(MetricId::Gpu));
        assert!(migrated.shows(MetricId::Network));
        let mut selected = migrated;
        selected
            .metrics
            .iter_mut()
            .find(|metric| metric.id == DisplayMetricId::Gpu)
            .unwrap()
            .enabled = true;
        assert_eq!(
            decode_fixture(serde_json::to_value(&selected).unwrap()).unwrap(),
            selected
        );
        let mut future = serde_json::to_value(selected).unwrap();
        future["schemaVersion"] = (PREFERENCE_SCHEMA_VERSION + 1).into();
        assert!(decode_fixture(future).is_err());
    }

    #[test]
    fn old_color_rules_preserve_thresholds_and_do_not_enable_temperature() {
        let mut saved = version_fourteen_preferences();
        saved["usageColors"] = false.into();
        saved["usageWarningPercent"] = 55.into();
        saved["usageCriticalPercent"] = 80.into();
        let restored = decode_fixture(saved).unwrap();
        assert!(!restored.usage_colors);
        assert_eq!(restored.color_rules[0].rules[1].bounds().0, 55);
        assert_eq!(restored.color_rules[0].rules[2].bounds().0, 80);
        assert_eq!(
            restored.color_rules[0].rules[0].color(),
            super::super::color_rules::DisplayColor::Default
        );
        assert!(!restored.color_rules[1].enabled);
        let serialized = serde_json::to_value(&restored).unwrap();
        assert!(serialized.get("usageWarningPercent").is_none());
        assert_eq!(decode_fixture(serialized).unwrap(), restored);
    }

    fn version_fourteen_preferences() -> serde_json::Value {
        let mut value = serde_json::to_value(ResidentPreferences::default()).unwrap();
        value.as_object_mut().unwrap().remove("colorRules");
        value["schemaVersion"] = 14.into();
        value["usageWarningPercent"] = 70.into();
        value["usageCriticalPercent"] = 90.into();
        value
    }

    #[test]
    fn version_seven_preserves_order_and_adds_display_options() {
        let mut old = legacy_preferences();
        old["metrics"].as_array_mut().unwrap().reverse();
        let mut order = old["metrics"].clone();
        order
            .as_array_mut()
            .unwrap()
            .push(serde_json::json!({"id":"gpu", "enabled":false}));
        order
            .as_array_mut()
            .unwrap()
            .push(serde_json::json!({"id":"cpuTemperature", "enabled":false}));
        order
            .as_array_mut()
            .unwrap()
            .push(serde_json::json!({"id":"gpuTemperature","enabled":false}));
        let migrated = decode_fixture(old).unwrap();
        assert_eq!(migrated.schema_version, 22);
        assert!(!migrated.menu_bar_compact);
        assert!(migrated.usage_colors);
        assert_eq!(
            previous_rows(&serde_json::to_value(migrated).unwrap()["metrics"]),
            previous_rows(&order)
        );
        assert_eq!(
            ResidentPreferences::default()
                .metrics
                .iter()
                .map(|m| m.id)
                .collect::<Vec<_>>(),
            DISPLAY_ORDER
        );
    }

    #[test]
    fn legacy_default_order_moves_network_last_without_changing_enabled_flags() {
        let mut old = legacy_preferences();
        old["metrics"] = serde_json::json!([
            {"id":"cpu", "enabled":false}, {"id":"memory", "enabled":true},
            {"id":"network", "enabled":true}, {"id":"disk", "enabled":false}
        ]);
        let migrated = decode_fixture(old).unwrap();
        assert_eq!(
            migrated
                .metrics
                .iter()
                .map(|m| m.id)
                .filter(|id| *id != DisplayMetricId::DiskTemperature)
                .collect::<Vec<_>>(),
            [
                DisplayMetricId::Cpu,
                DisplayMetricId::Memory,
                DisplayMetricId::Disk,
                DisplayMetricId::Gpu,
                DisplayMetricId::CpuTemperature,
                DisplayMetricId::GpuTemperature,
                DisplayMetricId::BatteryTemperature,
                DisplayMetricId::ChargingPower,
                DisplayMetricId::BatteryPower,
                DisplayMetricId::SystemPower,
                DisplayMetricId::Network,
            ]
        );
        assert!(migrated.shows(MetricId::Network));
        assert!(!migrated.shows(MetricId::Disk));
    }

    #[test]
    fn legacy_thresholds_reject_invalid_ranges() {
        for (warning, critical) in [(0, 90), (90, 90), (95, 90), (70, 101)] {
            let mut value = version_fourteen_preferences();
            value["usageWarningPercent"] = warning.into();
            value["usageCriticalPercent"] = critical.into();
            assert!(decode_fixture(value).is_err());
        }
        let mut malformed = legacy_preferences();
        malformed["usageColors"] = true.into();
        assert!(decode_fixture(malformed).is_err());
    }

    #[test]
    fn version_five_preserves_manual_positions_and_current_auto_round_trips() {
        for position in ["left", "right"] {
            let mut old = legacy_preferences();
            old["schemaVersion"] = 5.into();
            old.as_object_mut()
                .unwrap()
                .remove("windowsCpuTemperatureEnabled");
            old.as_object_mut().unwrap().remove("gpuAdapter");
            old.as_object_mut().unwrap().remove("taskbarCompact");
            old["taskbarPosition"] = position.into();
            let migrated = decode_fixture(old).unwrap();
            assert_eq!(migrated.schema_version, 22);
            assert_eq!(
                serde_json::to_value(migrated).unwrap()["taskbarPosition"],
                position
            );
        }
        let current = ResidentPreferences::default();
        assert_eq!(
            decode_fixture(serde_json::to_value(&current).unwrap()).unwrap(),
            current
        );
    }

    #[test]
    fn version_six_preserves_automatic_position_and_defaults_to_standard_density() {
        let mut old = legacy_preferences();
        old["schemaVersion"] = 6.into();
        old.as_object_mut()
            .unwrap()
            .remove("windowsCpuTemperatureEnabled");
        old.as_object_mut().unwrap().remove("gpuAdapter");
        old.as_object_mut().unwrap().remove("taskbarCompact");
        let migrated = decode_fixture(old).unwrap();
        assert_eq!(migrated.taskbar_position, TaskbarPosition::Auto);
        assert!(!migrated.taskbar_compact);
        let mut compact = migrated;
        compact.taskbar_compact = true;
        assert_eq!(
            decode_fixture(serde_json::to_value(&compact).unwrap()).unwrap(),
            compact
        );
    }

    #[test]
    fn fresh_preferences_show_logo_cpu_memory_with_automatic_taskbar_position() {
        let preferences = ResidentPreferences::default();
        assert!(preferences.enabled && preferences.show_icon && preferences.taskbar_background);
        assert_eq!(preferences.taskbar_position, TaskbarPosition::Auto);
        assert_eq!(
            preferences
                .metrics
                .iter()
                .filter(|metric| metric.enabled)
                .map(|metric| metric.id)
                .filter(|id| *id != DisplayMetricId::DiskTemperature)
                .collect::<Vec<_>>(),
            vec![DisplayMetricId::Cpu, DisplayMetricId::Memory]
        );
        assert!(preferences.network_interface.is_none() && preferences.disk_volume.is_none());
    }

    #[test]
    fn saved_preferences_are_not_replaced_by_new_install_defaults() {
        let mut saved = ResidentPreferences {
            taskbar_position: TaskbarPosition::Right,
            enabled: false,
            show_icon: false,
            taskbar_background: false,
            ..Default::default()
        };
        for metric in &mut saved.metrics {
            metric.enabled = matches!(metric.id, DisplayMetricId::Network | DisplayMetricId::Disk);
        }
        assert_eq!(
            decode_fixture(serde_json::to_value(&saved).unwrap()).unwrap(),
            saved
        );
    }

    #[test]
    fn migration_preserves_old_background_and_memory_choices() {
        for enabled in [false, true] {
            for memory in [false, true] {
                let migrated = decode_fixture(
                    serde_json::json!({"schemaVersion":1,"enabled":enabled,"showMemory":memory}),
                )
                .unwrap();
                assert_eq!(migrated.taskbar_position, TaskbarPosition::Right);
                assert_eq!(migrated.enabled, enabled);
                assert_eq!(migrated.shows(MetricId::Memory), memory);
                assert!(migrated.show_icon);
                assert!(!migrated.shows(MetricId::Cpu));
                assert_eq!(migrated.schema_version, 22);
            }
        }
    }

    #[test]
    fn version_two_migrates_to_taskbar_without_changing_metric_choices() {
        let mut old = legacy_preferences();
        old["schemaVersion"] = 2.into();
        old.as_object_mut()
            .unwrap()
            .remove("windowsCpuTemperatureEnabled");
        old.as_object_mut().unwrap().remove("gpuAdapter");
        old.as_object_mut().unwrap().remove("taskbarCompact");
        old.as_object_mut().unwrap().remove("taskbarBackground");
        old["revision"] = 17.into();
        old["showIcon"] = false.into();
        old.as_object_mut().unwrap().remove("windowsDisplayMode");
        old.as_object_mut().unwrap().remove("taskbarPosition");
        let migrated = decode_fixture(old).unwrap();
        assert_eq!(migrated.revision, 17);
        assert!(!migrated.show_icon);
        assert_eq!(
            decode_fixture(serde_json::to_value(&migrated).unwrap()).unwrap(),
            migrated
        );
    }

    #[test]
    fn version_three_preserves_choices_and_defaults_to_right() {
        let mut old = legacy_preferences();
        old["schemaVersion"] = 3.into();
        old.as_object_mut()
            .unwrap()
            .remove("windowsCpuTemperatureEnabled");
        old.as_object_mut().unwrap().remove("gpuAdapter");
        old.as_object_mut().unwrap().remove("taskbarCompact");
        old.as_object_mut().unwrap().remove("taskbarBackground");
        old["windowsDisplayMode"] = "taskbar".into();
        old["revision"] = 42.into();
        old.as_object_mut().unwrap().remove("taskbarPosition");
        let migrated = decode_fixture(old).unwrap();
        assert_eq!(migrated.taskbar_position, TaskbarPosition::Right);
        assert_eq!(migrated.revision, 42);
        let mut current = serde_json::to_value(migrated).unwrap();
        current["taskbarPosition"] = "left".into();
        assert_eq!(
            decode_fixture(current.clone()).unwrap().taskbar_position,
            TaskbarPosition::Left
        );
        current["taskbarPosition"] = "middle".into();
        assert!(decode_fixture(current).is_err());
    }

    #[test]
    fn version_four_keeps_background_and_current_preserves_transparency() {
        let mut old = legacy_preferences();
        old["schemaVersion"] = 4.into();
        old.as_object_mut()
            .unwrap()
            .remove("windowsCpuTemperatureEnabled");
        old.as_object_mut().unwrap().remove("gpuAdapter");
        old.as_object_mut().unwrap().remove("taskbarCompact");
        old["taskbarPosition"] = "left".into();
        old["windowsDisplayMode"] = "taskbar".into();
        old.as_object_mut().unwrap().remove("taskbarBackground");
        let migrated = decode_fixture(old.clone()).unwrap();
        assert!(migrated.taskbar_background);
        assert_eq!(migrated.taskbar_position, TaskbarPosition::Left);
        let mut current = serde_json::to_value(migrated).unwrap();
        current["taskbarBackground"] = false.into();
        assert!(!decode_fixture(current.clone()).unwrap().taskbar_background);
        current["taskbarBackground"] = "false".into();
        assert!(decode_fixture(current).is_err());
        old["taskbarBackground"] = false.into();
        assert!(decode_fixture(old).is_err());
    }

    #[test]
    fn duplicate_ids_preserve_first_order_and_missing_items_are_disabled() {
        let preferences = ResidentPreferences {
            metrics: vec![
                DisplayMetric {
                    id: DisplayMetricId::Network,
                    enabled: true,
                },
                DisplayMetric {
                    id: DisplayMetricId::Network,
                    enabled: false,
                },
            ],
            ..Default::default()
        }
        .normalize()
        .unwrap();
        assert_eq!(preferences.metrics.len(), 12);
        assert_eq!(
            preferences.metrics.last().unwrap().id,
            DisplayMetricId::Network
        );
        assert!(preferences.shows(MetricId::Network));
        assert!(!preferences.shows(MetricId::Memory));
    }

    #[test]
    fn unknown_versions_and_invalid_types_are_rejected_without_migration() {
        for value in [
            serde_json::json!({"schemaVersion":99}),
            serde_json::json!({"schemaVersion":1,"enabled":"true","showMemory":false}),
            serde_json::json!({"schemaVersion":1,"enabled":true}),
        ] {
            assert!(decode_fixture(value).is_err());
        }
        let mut value = version_fourteen_preferences();
        value.as_object_mut().unwrap().remove("gpuAdapter");
        value["metrics"][0]["id"] = "npu".into();
        assert!(decode_fixture(value).is_err());
    }

    #[test]
    fn every_display_combination_retains_an_entry() {
        for bits in 0..(1 << DISPLAY_ORDER.len()) {
            for show_icon in [false, true] {
                let mut preferences = ResidentPreferences {
                    show_icon,
                    ..Default::default()
                };
                for (index, metric) in preferences.metrics.iter_mut().enumerate() {
                    metric.enabled = bits & (1 << index) != 0;
                }
                assert_eq!(
                    preferences.effective_icon(),
                    show_icon
                        || !preferences.metrics.iter().any(|m| {
                            m.enabled
                                && (cfg!(target_os = "macos")
                                    || !matches!(m.id, DisplayMetricId::CpuTemperature))
                                && (cfg!(any(target_os = "macos", windows))
                                    || !matches!(
                                        m.id,
                                        DisplayMetricId::GpuTemperature
                                            | DisplayMetricId::DiskTemperature
                                    ))
                                && (cfg!(target_os = "macos")
                                    || !matches!(
                                        m.id,
                                        DisplayMetricId::BatteryTemperature
                                            | DisplayMetricId::ChargingPower
                                            | DisplayMetricId::BatteryPower
                                            | DisplayMetricId::SystemPower
                                    ))
                        })
                );
                assert!(
                    preferences.effective_icon()
                        || preferences.metrics.iter().any(|metric| metric.enabled)
                );
            }
        }
    }
}
