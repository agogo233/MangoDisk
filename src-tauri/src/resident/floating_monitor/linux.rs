//! Passive GTK surface; the desktop window manager owns stacking and native dragging.
use gtk::prelude::*;
use tauri::Manager;

pub(super) fn configure(window: &tauri::WebviewWindow) -> tauri::Result<()> {
    let target = window.clone();
    // GTK objects must stay on the UI thread, including during asynchronous prewarming.
    window.with_webview(move |webview| {
        // WebKitGTK otherwise requests a 200px minimum in each direction,
        // which overrides compact native sizes when the widget tree is mapped.
        let view = webview.inner();
        view.set_size_request(1, 1);
        match target.gtk_window() {
            Ok(native) => {
                native.set_focus_on_map(false);
                native.set_skip_pager_hint(true);
                let native = native.downgrade();
                let app = target.app_handle().clone();
                view.connect_button_press_event(move |_, event| {
                    if event.button() != 1 {
                        return gtk::glib::Propagation::Proceed;
                    }
                    // A passive surface can enter the move loop immediately. Keep
                    // the original GTK event time rather than waiting for WebView IPC.
                    if let Some(native) = native.upgrade() {
                        let (x, y) = event.root();
                        native.begin_move_drag(1, x as i32, y as i32, event.time());
                        super::window::finish_drag(&app);
                        let app = app.clone();
                        // Persist placement even when no selected metric is publishing.
                        gtk::glib::timeout_add_local(std::time::Duration::from_millis(400), move || {
                            if dragging() {
                                return gtk::glib::ControlFlow::Continue;
                            }
                            app.state::<std::sync::Arc<crate::resident::runtime::ResidentState>>()
                                .wake();
                            gtk::glib::ControlFlow::Break
                        });
                        log::info!("resident_floating_drag_started platform=linux x={x} y={y}");
                    }
                    gtk::glib::Propagation::Stop
                });
            }
            Err(error) => {
                crate::resident::diagnostics::Failure::record("floating_native_window", &error);
            }
        }
    })
}

pub(super) fn set_visible(window: &tauri::WebviewWindow, visible: bool) -> tauri::Result<()> {
    // Queue visibility after the placement requests. GTK must show the complete
    // widget tree, and mapping it early can replace the requested compact size.
    if visible {
        window.show()
    } else {
        window.hide()
    }
}

pub(super) fn set_bounds(
    window: &tauri::WebviewWindow,
    point: (i32, i32),
    size: (u32, u32),
) -> tauri::Result<()> {
    let native = window.gtk_window()?;
    let scale = f64::from(native.scale_factor());
    let point = tauri::PhysicalPosition::new(point.0, point.1).to_logical::<i32>(scale);
    let size = tauri::PhysicalSize::new(size.0, size.1).to_logical::<i32>(scale);
    native.resize(size.width, size.height);
    if !native.is_mapped() {
        // GTK resets positioning on unmap. Supply the bookmark through GTK so
        // the next map does not choose the window manager's default position.
        native.move_(point.x, point.y);
    } else if let Some(surface) = native.window() {
        // Separate GTK move/resize requests let GNOME clamp against the old size.
        // One GDK geometry request preserves the bookmark when a layout shrinks.
        surface.move_resize(point.x, point.y, size.width, size.height);
    }
    Ok(())
}

pub(super) fn fullscreen(_window: &tauri::WebviewWindow) -> bool {
    // Keep the passive surface under the compositor's stacking policy. Linux does
    // not expose the Windows-only foreground-fullscreen preference.
    false
}

fn pointer() -> Option<(gtk::gdk::Device, gtk::gdk::Window)> {
    let display = gtk::gdk::Display::default()?;
    let device = display.default_seat()?.pointer()?;
    let root = display.default_screen().root_window()?;
    Some((device, root))
}

pub(super) fn dragging() -> bool {
    pointer().is_some_and(|(device, root)| {
        root.device_position(&device)
            .3
            .contains(gtk::gdk::ModifierType::BUTTON1_MASK)
    })
}
