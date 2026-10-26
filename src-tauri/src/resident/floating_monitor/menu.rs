//! Native menus stay outside the compact WebView bounds and remain keyboard accessible.
use super::{menu_preferences, window, LABEL};
use crate::resident::{preference_schema::FloatingLayout, preferences, runtime::ResidentState};
use std::sync::Arc;
use tauri::{
    menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem, Submenu},
    Manager,
};

pub fn install(window: &tauri::WebviewWindow) {
    let app = window.app_handle().clone();
    window.on_menu_event(move |_, event| {
        let action = event.id().as_ref().to_owned();
        if !action.starts_with("floating-") {
            return;
        }
        let app = app.clone();
        // Leave the native menu callback before creating windows or applying preferences.
        tauri::async_runtime::spawn(async move {
            if let Err(error) = act(&app, &action) {
                crate::resident::diagnostics::Failure::record("floating_menu", &error);
                crate::resident::main_window::request(
                    &app,
                    crate::resident::main_window::Destination::DisplaySettings,
                    "floating_menu_failed",
                );
            }
        });
    });
}

pub fn open(window: &tauri::WebviewWindow) -> tauri::Result<()> {
    let app = window.app_handle();
    let labels = crate::services::native_labels::NativeLabels::load(app);
    let preferences = app
        .state::<Arc<ResidentState>>()
        .preferences
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone();
    if !window::requested(&preferences) || (cfg!(windows) && preferences.floating.click_through) {
        return Ok(());
    }
    let item = |id, key| MenuItem::with_id(app, id, labels.text(key), true, None::<&str>);
    let check = |id, key, selected| {
        CheckMenuItem::with_id(app, id, labels.text(key), true, selected, None::<&str>)
    };
    let horizontal = check(
        "floating-horizontal",
        "/floatingMonitor/horizontal",
        preferences.floating.layout == FloatingLayout::Horizontal,
    )?;
    let vertical = check(
        "floating-vertical",
        "/floatingMonitor/vertical",
        preferences.floating.layout == FloatingLayout::Vertical,
    )?;
    #[cfg(windows)]
    let top = check(
        "floating-top",
        "/floatingMonitor/alwaysOnTop",
        preferences.floating.always_on_top,
    )?;
    #[cfg(windows)]
    let locked = check(
        "floating-lock",
        "/floatingMonitor/locked",
        preferences.floating.locked,
    )?;
    #[cfg(windows)]
    let snap = check(
        "floating-snap",
        "/floatingMonitor/snapToEdges",
        preferences.floating.snap_to_edges,
    )?;
    #[cfg(windows)]
    let fullscreen = check(
        "floating-fullscreen",
        "/floatingMonitor/hideOnFullscreen",
        preferences.floating.hide_on_fullscreen,
    )?;
    let bold = check(
        "floating-bold",
        "/floatingMonitor/bold",
        preferences.floating.bold,
    )?;
    let font_size = Submenu::new(app, labels.text("/floatingMonitor/fontSize"), true)?;
    for (id, value) in menu_preferences::FONT_SIZES {
        font_size.append(&CheckMenuItem::with_id(
            app,
            format!("floating-font-{id}"),
            labels.text(&format!("/floatingMonitor/fontSizes/{id}")),
            true,
            preferences.floating.font_size == value,
            None::<&str>,
        )?)?;
    }
    let background = Submenu::new(app, labels.text("/floatingMonitor/background"), true)?;
    for (id, value) in menu_preferences::BACKGROUNDS {
        background.append(&CheckMenuItem::with_id(
            app,
            format!("floating-background-{id}"),
            labels.text(&format!("/floatingMonitor/backgrounds/{id}")),
            true,
            preferences.floating.background == value,
            None::<&str>,
        )?)?;
    }
    let dividers = check(
        "floating-dividers",
        "/floatingMonitor/dividers",
        preferences.floating.dividers,
    )?;
    let settings = item("floating-settings", "/systemStatus/configureAction")?;
    let reset = item("floating-reset", "/floatingMonitor/resetPosition")?;
    let opacity = Submenu::new(app, labels.text("/floatingMonitor/backgroundOpacity"), true)?;
    for value in menu_preferences::OPACITIES {
        opacity.append(&CheckMenuItem::with_id(
            app,
            format!("floating-opacity-{value}"),
            format!("{value}%"),
            true,
            preferences.floating.background_opacity == value,
            None::<&str>,
        )?)?;
    }
    let separator = PredefinedMenuItem::separator(app)?;
    let second = PredefinedMenuItem::separator(app)?;
    #[cfg(windows)]
    let interaction_separator = PredefinedMenuItem::separator(app)?;
    let appearance = Submenu::with_items(
        app,
        labels.text("/systemStatus/settingsTabs/appearance"),
        true,
        &[&font_size, &bold, &background, &opacity, &dividers],
    )?;
    let menu = Menu::with_items(
        app,
        &[
            &horizontal,
            &vertical,
            &separator,
            #[cfg(windows)]
            &top,
            #[cfg(windows)]
            &locked,
            #[cfg(windows)]
            &snap,
            #[cfg(windows)]
            &fullscreen,
            #[cfg(windows)]
            &interaction_separator,
            &appearance,
            &second,
            &settings,
            &reset,
        ],
    )?;
    log::info!("resident_floating_menu_open locale={}", labels.locale);
    window.popup_menu(&menu)
}

fn act(app: &tauri::AppHandle, action: &str) -> tauri::Result<()> {
    match action {
        "floating-reset" => {
            window::reset(app);
            return Ok(());
        }
        "floating-settings" => {
            return crate::resident::main_window::open(
                app,
                crate::resident::main_window::Destination::DisplaySettings,
                LABEL,
            );
        }
        _ => {}
    }
    // A menu can remain open while another window saves. Re-read and retry only
    // a revision conflict; never overwrite the other window's unrelated fields.
    for attempt in 0..2 {
        let mut requested = app
            .state::<Arc<ResidentState>>()
            .preferences
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        if !menu_preferences::apply(&mut requested.floating, action) {
            return Ok(());
        }
        if !window::requested(&requested) {
            return Ok(());
        }
        match preferences::apply(app, requested) {
            Ok(_) => {
                log::info!("resident_floating_menu action={action} outcome=committed");
                return Ok(());
            }
            Err(error) if attempt == 0 && error.is_preference_conflict() => continue,
            Err(error) => return Err(tauri::Error::Anyhow(error.into())),
        }
    }
    Ok(())
}
