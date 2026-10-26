//! Passive AppKit surface. Spaces own fullscreen exclusion; menu-bar settings own appearance.
use objc2::MainThreadMarker;
use objc2_app_kit::{NSEvent, NSWindow, NSWindowCollectionBehavior};
use tauri::Manager;

pub(super) fn configure(window: &tauri::WebviewWindow) -> tauri::Result<()> {
    window.with_webview(|webview| {
        let native = unsafe { &*webview.ns_window().cast::<NSWindow>() };
        native.setHidesOnDeactivate(false);
        native.setExcludedFromWindowsMenu(true);
        // Join ordinary desktops without becoming a fullscreen auxiliary or a
        // Mission Control tile. Never activate the application just to show data.
        native.setCollectionBehavior(
            NSWindowCollectionBehavior::CanJoinAllSpaces
                | NSWindowCollectionBehavior::Stationary
                | NSWindowCollectionBehavior::IgnoresCycle
                | NSWindowCollectionBehavior::FullScreenNone,
        );
    })
}

pub(super) fn set_visible(window: &tauri::WebviewWindow, visible: bool) -> tauri::Result<()> {
    window.with_webview(move |webview| {
        let native = unsafe { &*webview.ns_window().cast::<NSWindow>() };
        if visible {
            native.orderFrontRegardless();
        } else {
            native.orderOut(None);
        }
    })
}

pub(super) fn fullscreen(_window: &tauri::WebviewWindow) -> bool {
    // FullScreenNone and the absence of FullScreenAuxiliary leave exclusion to
    // AppKit instead of inspecting other applications or requesting screen access.
    false
}

pub(super) fn dragging() -> bool {
    NSEvent::pressedMouseButtons() & 1 != 0
}

pub(super) fn start_drag(window: tauri::WebviewWindow) -> tauri::Result<()> {
    if window.label() != super::LABEL {
        return Ok(());
    }
    let target = window.clone();
    window.run_on_main_thread(move || {
        let _mtm = MainThreadMarker::new().expect("floating drag runs on the main thread");
        if !dragging() {
            log::info!("resident_floating_drag_skipped reason=button_released");
            return;
        }
        log::info!("resident_floating_drag_started platform=macos");
        if let Err(error) = target.start_dragging() {
            crate::resident::diagnostics::Failure::record("floating_drag", &error);
            return;
        }
        // Placement is suspended while the mouse is held, then captures the
        // release point. Programmatic Moved events never replace the bookmark.
        super::window::finish_drag(target.app_handle());
    })
}
