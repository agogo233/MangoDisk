pub mod format;
pub mod labels;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
mod macos_interaction;
#[cfg(any(target_os = "macos", test))]
mod macos_presentation;
#[cfg(target_os = "macos")]
pub(super) mod macos_thread;
pub mod usage_color;
#[cfg(windows)]
pub mod windows_bitmap;

use crate::resident::{
    main_window, panel, preferences::ResidentPreferences, runtime::ResidentReading,
};
use format::{DisplayEntry, DisplayId};
use std::{collections::HashMap, sync::Mutex};
use tauri::{
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    Manager,
};

#[derive(Default)]
pub struct DisplayState {
    entries: HashMap<DisplayId, DisplayEntry>,
    locale: String,
    visible: Vec<DisplayId>,
    summary: String,
    pub renders: u64,
    #[cfg(target_os = "macos")]
    macos: macos::Cache,
    #[cfg(windows)]
    appearance: Option<windows_bitmap::Appearance>,
    #[cfg(target_os = "linux")]
    indicator_title: Option<String>,
    failed: bool,
    color_rules: Option<(bool, Vec<super::color_rules::MetricColorRule>)>,
    color_disk_volume: Option<String>,
    color_disk_temperature_volume: Option<String>,
    color_gpu_adapter: Option<String>,
    color_gpu_temperature_adapter: Option<String>,
    color_matches: HashMap<DisplayId, Option<usize>>,
}

impl DisplayState {
    // Hysteresis belongs to the sampled volume, not the display slot. A newly
    // selected disk can arrive without an intervening unavailable frame.
    fn apply_colors(
        &mut self,
        entries: &mut [DisplayEntry],
        preferences: &ResidentPreferences,
        disk_volume: Option<&str>,
        disk_temperature_volume: Option<&str>,
        gpu_adapter: Option<&str>,
        gpu_temperature_adapter: Option<&str>,
    ) {
        let color_rules = (
            preferences.usage_colors && preferences.enabled,
            preferences.color_rules.clone(),
        );
        for entry in entries.iter_mut() {
            let id = match entry.id {
                DisplayId::Cpu => super::preference_schema::DisplayMetricId::Cpu,
                DisplayId::CpuTemperature => {
                    super::preference_schema::DisplayMetricId::CpuTemperature
                }
                DisplayId::Gpu => super::preference_schema::DisplayMetricId::Gpu,
                DisplayId::GpuTemperature => {
                    super::preference_schema::DisplayMetricId::GpuTemperature
                }
                DisplayId::BatteryTemperature => {
                    super::preference_schema::DisplayMetricId::BatteryTemperature
                }
                DisplayId::ChargingPower => {
                    super::preference_schema::DisplayMetricId::ChargingPower
                }
                DisplayId::SystemPower => super::preference_schema::DisplayMetricId::SystemPower,
                DisplayId::BatteryPower => super::preference_schema::DisplayMetricId::BatteryPower,
                DisplayId::Memory => super::preference_schema::DisplayMetricId::Memory,
                DisplayId::Disk => super::preference_schema::DisplayMetricId::Disk,
                DisplayId::DiskTemperature => {
                    super::preference_schema::DisplayMetricId::DiskTemperature
                }
                _ => {
                    entry.tone = Default::default();
                    continue;
                }
            };
            let same_source = match entry.id {
                DisplayId::Disk => self.color_disk_volume.as_deref() == disk_volume,
                DisplayId::DiskTemperature => {
                    self.color_disk_temperature_volume.as_deref() == disk_temperature_volume
                }
                DisplayId::Gpu => self.color_gpu_adapter.as_deref() == gpu_adapter,
                DisplayId::GpuTemperature => {
                    self.color_gpu_temperature_adapter.as_deref() == gpu_temperature_adapter
                }
                _ => true,
            };
            let previous = if self.color_rules.as_ref() == Some(&color_rules) && same_source {
                self.color_matches.get(&entry.id).copied().flatten()
            } else {
                None
            };
            let rule = color_rules.1.iter().find(|rule| rule.id == id);
            let matched = rule
                .filter(|_| color_rules.0)
                .and_then(|rule| usage_color::next_rule(previous, entry.color_value, rule));
            entry.tone = matched
                .and_then(|index| rule.map(|rule| rule.rules[index].color().into()))
                .unwrap_or_default();
            self.color_matches.insert(entry.id, matched);
        }
        // Unavailable or hidden entries must not retain an older match.
        self.color_matches
            .retain(|id, _| entries.iter().any(|entry| entry.id == *id));
        self.color_rules = Some(color_rules);
        self.color_disk_volume = disk_volume.map(str::to_owned);
        self.color_disk_temperature_volume = disk_temperature_volume.map(str::to_owned);
        self.color_gpu_adapter = gpu_adapter.map(str::to_owned);
        self.color_gpu_temperature_adapter = gpu_temperature_adapter.map(str::to_owned);
    }
}

pub fn install(app: &tauri::AppHandle) -> tauri::Result<()> {
    app.manage(Mutex::new(DisplayState::default()));
    #[cfg(target_os = "macos")]
    {
        macos_interaction::install(app);
        macos_thread::call(app, |app, _mtm| {
            ensure(app, DisplayId::App, &labels::Labels::load(app))
        })
    }
    #[cfg(not(target_os = "macos"))]
    ensure(app, DisplayId::App, &labels::Labels::load(app))
}

pub fn log_summary(app: &tauri::AppHandle, loop_max_ms: u128) {
    #[cfg(target_os = "macos")]
    {
        let target = app.clone();
        // Diagnostics must not make the sampling loop wait for native menu
        // tracking. Only the queued callback acquires native tray handles.
        if let Err(error) = app.run_on_main_thread(move || summary(&target, loop_max_ms)) {
            super::diagnostics::Failure::record("display_summary_dispatch", &error);
        }
    }
    #[cfg(not(target_os = "macos"))]
    summary(app, loop_max_ms);
}

fn summary(app: &tauri::AppHandle, loop_max_ms: u128) {
    #[cfg(target_os = "macos")]
    let _mtm = objc2::MainThreadMarker::new().expect("tray diagnostics run on the main thread");
    let renders = app
        .state::<Mutex<DisplayState>>()
        .try_lock()
        .ok()
        .map(|state| state.renders);
    let handles = DisplayId::ALL
        .iter()
        .filter(|id| app.tray_by_id(id.tray_id()).is_some())
        .count();
    log::info!("resident_runtime_summary tray_renders={renders:?} tray_handles={handles} loop_max_ms={loop_max_ms}");
}

fn menu(app: &tauri::AppHandle, labels: &labels::Labels) -> tauri::Result<Menu<tauri::Wry>> {
    // AppIndicator does not emit tray clicks on Linux. Keep the detail panel
    // reachable through the native menu that the desktop opens instead.
    #[cfg(target_os = "linux")]
    let status = MenuItem::with_id(
        app,
        "resident-status",
        labels.text("details"),
        true,
        None::<&str>,
    )?;
    let open = MenuItem::with_id(
        app,
        "resident-open",
        labels.text("openMain"),
        true,
        None::<&str>,
    )?;
    let settings = MenuItem::with_id(
        app,
        "resident-settings",
        labels.text("settings"),
        true,
        None::<&str>,
    )?;
    let quit = MenuItem::with_id(
        app,
        "resident-quit",
        labels.text("quit"),
        true,
        None::<&str>,
    )?;
    let menu = Menu::new(app)?;
    #[cfg(target_os = "linux")]
    menu.append(&status)?;
    menu.append_items(&[&open, &settings, &quit])?;
    Ok(menu)
}

pub fn brand_icon() -> tauri::image::Image<'static> {
    #[cfg(target_os = "macos")]
    {
        tauri::include_image!("icons/tray-template.png")
    }
    #[cfg(not(target_os = "macos"))]
    {
        tauri::include_image!("icons/tray-color.png")
    }
}

fn ensure(app: &tauri::AppHandle, id: DisplayId, labels: &labels::Labels) -> tauri::Result<()> {
    #[cfg(target_os = "macos")]
    let _mtm = objc2::MainThreadMarker::new().expect("tray handles stay on the main thread");
    if app.tray_by_id(id.tray_id()).is_some() {
        return Ok(());
    }
    #[cfg(windows)]
    let icon = if let Some(metric) = id.metric() {
        let marker = match metric {
            mangodisk_core::system_resources::metrics::MetricId::Cpu => "C",
            mangodisk_core::system_resources::metrics::MetricId::Gpu => "G",
            mangodisk_core::system_resources::metrics::MetricId::Memory => "M",
            mangodisk_core::system_resources::metrics::MetricId::Disk => "D",
            _ => {
                if id == DisplayId::Upload {
                    "↑"
                } else {
                    "↓"
                }
            }
        };
        let appearance = windows_bitmap::appearance();
        let rgba = windows_bitmap::render(marker, "—", appearance)
            .map_err(|stage| tauri::Error::Io(std::io::Error::other(stage)))?;
        tauri::image::Image::new_owned(rgba, appearance.size, appearance.size)
    } else {
        brand_icon()
    };
    #[cfg(not(windows))]
    let icon = brand_icon();
    let builder = TrayIconBuilder::with_id(id.tray_id())
        .icon(icon)
        .icon_as_template(cfg!(target_os = "macos"))
        .tooltip("MangoDisk")
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id().as_ref() {
            #[cfg(target_os = "linux")]
            "resident-status" => {
                let app = app.clone();
                // Menu activation is also a native callback; leave it before
                // creating or focusing the panel's WebView.
                tauri::async_runtime::spawn(async move {
                    if let Err(error) = panel::open(&app) {
                        super::diagnostics::Failure::record("panel_menu_entry", &error);
                    }
                });
            }
            "resident-open" => {
                main_window::request(app, main_window::Destination::Main, "tray_menu")
            }
            "resident-settings" => {
                main_window::request(app, main_window::Destination::Settings, "tray_menu")
            }
            "resident-quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(move |tray, event| {
            #[cfg(target_os = "macos")]
            if let TrayIconEvent::Click {
                button,
                button_state,
                ..
            } = event
            {
                macos_interaction::click(tray, button, button_state);
                return;
            }
            #[cfg(windows)]
            if matches!(event, TrayIconEvent::Leave { .. }) {
                panel::tray_pointer_left(tray.app_handle());
            }
            if matches!(
                event,
                TrayIconEvent::Click {
                    button: MouseButton::Left,
                    button_state: MouseButtonState::Up,
                    ..
                }
            ) {
                let app = tray.app_handle().clone();
                // Never create WebView2 in the native event callback.
                tauri::async_runtime::spawn(async move {
                    if let Err(error) = panel::toggle_from(&app, id.tray_id(), id.metric()) {
                        super::diagnostics::Failure::record("panel_entry", &error);
                    }
                });
            }
        });
    // A resident NSMenu swallows left clicks on macOS 27 before tray-icon can
    // route them. The macOS adapter attaches a menu only during presentation.
    #[cfg(not(target_os = "macos"))]
    let builder = builder.menu(&menu(app, labels)?);
    #[cfg(target_os = "macos")]
    let _ = labels;
    let tray = builder.build(app)?;
    tray.set_visible(false)?;
    Ok(())
}

/// Apply a complete desired set before persistence. On rejection, the caller
/// reapplies the last committed preferences so settings and native entries agree.
pub fn apply_preferences(
    app: &tauri::AppHandle,
    preferences: &ResidentPreferences,
    reading: &ResidentReading,
) -> tauri::Result<()> {
    let request = DisplayRequest::new(app, preferences, reading);
    #[cfg(target_os = "macos")]
    return macos_thread::call(app, move |app, _mtm| apply_request(app, request));
    #[cfg(not(target_os = "macos"))]
    apply_request(app, request)
}

fn apply_request(app: &tauri::AppHandle, request: DisplayRequest) -> tauri::Result<()> {
    let state = app.state::<Mutex<DisplayState>>();
    let mut state = state.lock().unwrap_or_else(|error| error.into_inner());
    render(app, request, &mut state)
}

pub fn refresh(
    app: &tauri::AppHandle,
    preferences: &ResidentPreferences,
    reading: &ResidentReading,
) {
    let request = DisplayRequest::new(app, preferences, reading);
    #[cfg(target_os = "macos")]
    let result = macos_thread::call(app, move |app, _mtm| refresh_request(app, request));
    #[cfg(not(target_os = "macos"))]
    let result = refresh_request(app, request);
    if let Err(error) = result {
        super::diagnostics::Failure::record("display_dispatch", &error);
    }
}

fn refresh_request(app: &tauri::AppHandle, request: DisplayRequest) -> tauri::Result<()> {
    let state = app.state::<Mutex<DisplayState>>();
    let mut state = state.lock().unwrap_or_else(|error| error.into_inner());
    match render(app, request, &mut state) {
        Ok(()) => {
            if state.failed {
                log::info!("resident_display_recovered");
            }
            state.failed = false;
        }
        Err(error) => {
            if !state.failed {
                super::diagnostics::Failure::record("display_refresh", &error);
            }
            state.failed = true;
        }
    }
    Ok(())
}

// Prepare only the data native rendering needs. Detailed process lists and
// resource histories stay on the presentation worker rather than being cloned
// again for main-thread dispatch.
struct DisplayRequest {
    preferences: ResidentPreferences,
    labels: labels::Labels,
    entries: Vec<DisplayEntry>,
    disk_volume: Option<String>,
    disk_temperature_volume: Option<String>,
    gpu_adapter: Option<String>,
    gpu_temperature_adapter: Option<String>,
}

impl DisplayRequest {
    fn new(
        app: &tauri::AppHandle,
        preferences: &ResidentPreferences,
        reading: &ResidentReading,
    ) -> Self {
        let labels = labels::Labels::load(app);
        let entries = format::entries(
            preferences,
            &reading.resources,
            &labels,
            if cfg!(windows) { 1024.0 } else { 1000.0 },
        );
        Self {
            preferences: preferences.clone(),
            labels,
            entries,
            gpu_adapter: reading
                .resources
                .gpu
                .value
                .as_ref()
                .map(|gpu| gpu.adapter_id.clone()),
            gpu_temperature_adapter: reading
                .resources
                .gpu_temperature
                .value
                .as_ref()
                .map(|gpu| gpu.adapter_id.clone()),
            disk_temperature_volume: reading
                .resources
                .disk_temperature
                .value
                .as_ref()
                .map(|disk| disk.volume_id.clone()),
            disk_volume: reading
                .resources
                .disk
                .value
                .as_ref()
                .map(|disk| disk.volume.id.clone()),
        }
    }
}

fn render(
    app: &tauri::AppHandle,
    request: DisplayRequest,
    state: &mut DisplayState,
) -> tauri::Result<()> {
    #[cfg(target_os = "macos")]
    let _mtm = objc2::MainThreadMarker::new().expect("tray rendering runs on the main thread");
    let DisplayRequest {
        preferences,
        labels,
        mut entries,
        disk_volume,
        disk_temperature_volume,
        gpu_adapter,
        gpu_temperature_adapter,
    } = request;
    let preferences = &preferences;
    #[cfg(not(target_os = "macos"))]
    let changed_locale = state.locale != labels.locale;
    state.apply_colors(
        &mut entries,
        preferences,
        disk_volume.as_deref(),
        disk_temperature_volume.as_deref(),
        gpu_adapter.as_deref(),
        gpu_temperature_adapter.as_deref(),
    );
    #[cfg(not(windows))]
    let all_desired = format::desired(preferences);
    #[cfg(windows)]
    let desired = format::windows_desired(
        preferences,
        super::taskbar_display::update(app, preferences, &entries, &labels.locale),
    );
    #[cfg(not(windows))]
    let desired = if !all_desired.is_empty() {
        vec![DisplayId::App]
    } else {
        Vec::new()
    };
    for id in &desired {
        ensure(app, *id, &labels)?;
    }
    for id in DisplayId::ALL {
        if let Some(tray) = app.tray_by_id(id.tray_id()) {
            // Handles are retained and reused, with at most six for the process.
            // Hiding removes the entry from the system notification area.
            if state.visible.contains(&id) != desired.contains(&id) {
                tray.set_visible(desired.contains(&id))?;
                #[cfg(target_os = "macos")]
                {
                    state.macos = macos::Cache::default();
                }
                state.visible.retain(|visible| *visible != id);
                if desired.contains(&id) {
                    state.visible.push(id);
                    state.entries.remove(&id);
                }
            }
            #[cfg(not(target_os = "macos"))]
            if changed_locale {
                tray.set_menu(Some(menu(app, &labels)?))?;
            }
        }
    }
    state.visible = desired;
    if !preferences.enabled {
        state.locale = labels.locale;
        return Ok(());
    }
    let summary = entries
        .iter()
        .map(|entry| entry.tooltip.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    // Windows removes hidden icons from the notification area, so NIM_MODIFY
    // fails for the retained brand handle. Keep its tooltip cache unchanged
    // while hidden and apply the latest summary when that entry is shown again.
    if state.visible.contains(&DisplayId::App) && state.summary != summary {
        if let Some(tray) = app.tray_by_id(DisplayId::App.tray_id()) {
            tray.set_tooltip(Some(format!("MangoDisk\n{summary}")))?;
        }
        state.summary = summary.clone();
    }
    #[cfg(target_os = "linux")]
    if state.visible.contains(&DisplayId::App) {
        let title = format::indicator_title(&entries, preferences.taskbar_compact);
        if state.indicator_title != title {
            if let Some(tray) = app.tray_by_id(DisplayId::App.tray_id()) {
                tray.set_title(title.as_deref())?;
                state.renders += 1;
            }
            state.indicator_title = title;
        }
    }
    #[cfg(target_os = "macos")]
    if let Some(tray) = app.tray_by_id(DisplayId::App.tray_id()) {
        if macos::apply(
            &tray,
            &entries,
            preferences.effective_icon() || entries.is_empty(),
            preferences.menu_bar_compact,
            &summary,
            &mut state.macos,
        )? {
            state.renders += 1;
        }
    }
    #[cfg(windows)]
    {
        let appearance = windows_bitmap::appearance();
        for entry in &entries {
            if !state.visible.contains(&entry.id) {
                continue;
            }
            if let Some(tray) = app.tray_by_id(entry.id.tray_id()) {
                let previous = state.entries.get(&entry.id);
                if state.appearance != Some(appearance)
                    || !previous.is_some_and(|old| {
                        old.marker == entry.marker
                            && old.digits == entry.digits
                            && old.tone == entry.tone
                    })
                {
                    let rgba = windows_bitmap::render_colored(
                        &entry.marker,
                        &entry.digits,
                        appearance,
                        entry.tone,
                    )
                    .map_err(|stage| tauri::Error::Io(std::io::Error::other(stage)))?;
                    tray.set_icon(Some(tauri::image::Image::new_owned(
                        rgba,
                        appearance.size,
                        appearance.size,
                    )))?;
                    state.renders += 1;
                }
                if previous.is_none_or(|old| old.tooltip != entry.tooltip) {
                    tray.set_tooltip(Some(&entry.tooltip))?;
                }
            }
        }
        state.appearance = Some(appearance);
    }
    state.entries = entries.into_iter().map(|entry| (entry.id, entry)).collect();
    state.locale = labels.locale;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use usage_color::UsageTone;

    #[test]
    fn battery_power_color_rules_apply_to_their_own_slot() {
        use crate::resident::color_rules::{ColorRule, DisplayColor};
        use crate::resident::preference_schema::DisplayMetricId;
        let mut preferences = ResidentPreferences::default();
        let rule = preferences
            .color_rules
            .iter_mut()
            .find(|rule| rule.id == DisplayMetricId::BatteryPower)
            .unwrap();
        rule.enabled = true;
        rule.rules = vec![ColorRule::Below {
            value: 100,
            color: DisplayColor::Blue,
        }];
        let mut entries = vec![DisplayEntry {
            id: DisplayId::BatteryPower,
            tone: Default::default(),
            power_state: None,
            color_value: Some(50),
            marker: "W".into(),
            digits: "50.0".into(),
            text: String::new(),
            tooltip: String::new(),
        }];
        DisplayState::default().apply_colors(&mut entries, &preferences, None, None, None, None);
        assert_eq!(entries[0].tone, UsageTone::Blue);
    }

    #[test]
    fn temperature_and_percentage_rules_are_independent_and_unavailable_is_neutral() {
        let mut state = DisplayState::default();
        let preferences = ResidentPreferences::default();
        let mut entries: Vec<_> = [
            DisplayId::Cpu,
            DisplayId::CpuTemperature,
            DisplayId::GpuTemperature,
        ]
        .into_iter()
        .map(|id| DisplayEntry {
            id,
            tone: Default::default(),
            power_state: None,
            color_value: Some(65),
            marker: String::new(),
            digits: String::new(),
            text: String::new(),
            tooltip: String::new(),
        })
        .collect();
        state.apply_colors(&mut entries, &preferences, None, None, Some("a"), Some("a"));
        assert_eq!(entries[0].tone, UsageTone::Green);
        assert_eq!(entries[1].tone, UsageTone::Warning);
        assert_eq!(entries[2].tone, UsageTone::Warning);
        entries[2].color_value = Some(80);
        state.apply_colors(&mut entries, &preferences, None, None, Some("a"), Some("a"));
        assert_eq!(entries[2].tone, UsageTone::Critical);
        entries[2].color_value = Some(79);
        state.apply_colors(&mut entries, &preferences, None, None, Some("a"), Some("a"));
        assert_eq!(entries[2].tone, UsageTone::Critical);
        state.apply_colors(&mut entries, &preferences, None, None, Some("b"), Some("b"));
        assert_eq!(entries[2].tone, UsageTone::Warning);
        entries[1].color_value = None;
        state.apply_colors(&mut entries, &preferences, None, None, Some("b"), Some("b"));
        assert_eq!(entries[1].tone, UsageTone::Normal);
    }

    #[test]
    fn switching_disks_resets_hysteresis_without_resetting_cpu() {
        let mut state = DisplayState::default();
        let preferences = ResidentPreferences::default();
        let mut entries: Vec<_> = [DisplayId::Cpu, DisplayId::Disk]
            .into_iter()
            .map(|id| DisplayEntry {
                id,
                tone: UsageTone::Normal,
                power_state: None,
                color_value: Some(95),
                marker: String::new(),
                digits: String::new(),
                text: String::new(),
                tooltip: String::new(),
            })
            .collect();
        state.apply_colors(&mut entries, &preferences, Some("disk-a"), None, None, None);
        assert!(entries
            .iter()
            .all(|entry| entry.tone == UsageTone::Critical));
        state.entries = entries
            .iter()
            .cloned()
            .map(|entry| (entry.id, entry))
            .collect();
        for entry in &mut entries {
            entry.color_value = Some(89);
        }
        state.apply_colors(&mut entries, &preferences, Some("disk-a"), None, None, None);
        assert!(entries
            .iter()
            .all(|entry| entry.tone == UsageTone::Critical));
        state.apply_colors(&mut entries, &preferences, Some("disk-b"), None, None, None);
        assert_eq!(entries[0].tone, UsageTone::Critical);
        assert_eq!(entries[1].tone, UsageTone::Warning);
        // Simulate a failed native frame by leaving the paint cache unchanged.
        state.apply_colors(&mut entries, &preferences, Some("disk-b"), None, None, None);
        assert_eq!(entries[1].tone, UsageTone::Warning);
        let mut higher_threshold = preferences;
        for rule in &mut higher_threshold.color_rules {
            rule.rules = super::super::color_rules::legacy(70, 91)
                .into_iter()
                .find(|updated| updated.id == rule.id)
                .unwrap()
                .rules;
        }
        state.apply_colors(
            &mut entries,
            &higher_threshold,
            Some("disk-b"),
            None,
            None,
            None,
        );
        state.apply_colors(
            &mut entries,
            &higher_threshold,
            Some("disk-b"),
            None,
            None,
            None,
        );
        assert!(entries.iter().all(|entry| entry.tone == UsageTone::Warning));
    }
}
