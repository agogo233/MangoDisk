//! Transient status-item menus preserve left-click delivery on macOS 27.
use objc2::MainThreadMarker;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use tauri::tray::{MouseButton, MouseButtonState, TrayIcon};
use tauri::Manager;

use crate::resident::{diagnostics::Failure, panel};

#[derive(Debug, PartialEq, Eq)]
enum EntryAction {
    TogglePanel,
    ShowMenu,
}

#[derive(Default)]
struct MenuTracking {
    active: AtomicBool,
}

struct MenuGuard {
    tracking: Arc<MenuTracking>,
}

impl MenuGuard {
    fn begin(tracking: Arc<MenuTracking>) -> Option<Self> {
        tracking
            .active
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .ok()
            .map(|_| Self { tracking })
    }
}

impl Drop for MenuGuard {
    fn drop(&mut self) {
        self.tracking.active.store(false, Ordering::Release);
    }
}

pub(super) fn install(app: &tauri::AppHandle) {
    app.manage(Arc::new(MenuTracking::default()));
}

fn entry_action(button: MouseButton, state: MouseButtonState) -> Option<EntryAction> {
    // Act on press, before the status button's focus transition completes. A
    // release must never reopen a panel already closed by the same press.
    match (button, state) {
        (MouseButton::Left, MouseButtonState::Down) => Some(EntryAction::TogglePanel),
        (MouseButton::Right, MouseButtonState::Down) => Some(EntryAction::ShowMenu),
        _ => None,
    }
}

pub(super) fn click(tray: &TrayIcon, button: MouseButton, state: MouseButtonState) {
    let _mtm = MainThreadMarker::new().expect("tray callbacks run on the main thread");
    let Some(action) = entry_action(button, state) else {
        return;
    };
    log::info!(
        "resident_macos_entry source={} button={button:?} state={state:?} action={action:?}",
        tray.id().as_ref()
    );
    let app = tray.app_handle().clone();
    let source = tray.id().as_ref().to_owned();
    let menu_guard = if action == EntryAction::ShowMenu {
        let tracking = app.state::<Arc<MenuTracking>>().inner().clone();
        let Some(guard) = MenuGuard::begin(tracking) else {
            log::debug!("resident_macos_menu_skipped source={source} reason=already_tracking");
            return;
        };
        Some(guard)
    } else {
        None
    };
    // Leave Tauri's tray-listener lock before a nested native menu event loop.
    // Never move or clone a TrayIcon on a worker: Tauri marks its Rc-backed
    // wrapper Send, but the reference count and AppKit teardown are not safe
    // there. Panel positioning independently dispatches its handle lifetime.
    tauri::async_runtime::spawn(async move {
        match action {
            EntryAction::TogglePanel => {
                if let Err(error) = panel::toggle_from(&app, &source, None) {
                    Failure::record("panel_entry", &error);
                }
            }
            EntryAction::ShowMenu => {
                let target = app.clone();
                if let Err(error) = app.run_on_main_thread(move || {
                    // Reset on dispatch cancellation, missing entries, and native
                    // failures as well as normal menu dismissal.
                    let _guard = menu_guard;
                    let mtm = MainThreadMarker::new().expect("tray menus run on the main thread");
                    if let Err(error) = show_menu(&target, &source, mtm) {
                        Failure::record("tray_menu_present", &error);
                    }
                }) {
                    Failure::record("tray_menu_dispatch", &error);
                }
            }
        }
    });
}

fn show_menu(app: &tauri::AppHandle, source: &str, _mtm: MainThreadMarker) -> tauri::Result<()> {
    let Some(tray) = app.tray_by_id(source) else {
        return Err(tauri::Error::Io(std::io::Error::other(
            "tray_menu_entry_missing",
        )));
    };
    let menu = super::menu(app, &super::labels::Labels::load(app))?;
    panel::hide(app);
    let result = tray.set_menu(Some(menu)).and_then(|()| {
        tray.with_inner_tray_icon(|inner| {
            let mtm = MainThreadMarker::new().expect("tray menus run on the main thread");
            let item = inner.ns_status_item().ok_or_else(|| {
                tauri::Error::Io(std::io::Error::other("tray_menu_status_item_missing"))
            })?;
            let button = item.button(mtm).ok_or_else(|| {
                tauri::Error::Io(std::io::Error::other("tray_menu_button_missing"))
            })?;
            // Retain native objects without borrowing tray-icon's RefCell
            // across performClick's reentrant event loop (upstream #365).
            // AppKit owns this status button's menu action. The retained
            // item/button remain alive on the main thread until it returns.
            unsafe { button.performClick(None) };
            Ok::<(), tauri::Error>(())
        })?
    });
    // Clear both AppKit's menu and tray-icon's retained copy, including on
    // failure. Locale changes never attach a menu during ordinary refresh.
    let detached = tray.set_menu(None::<tauri::menu::Menu<tauri::Wry>>);
    if let Err(error) = detached {
        Failure::record("tray_menu_detach", &error);
        return Err(error);
    } else {
        log::info!(
            "resident_macos_menu_finished source={} attached=false",
            tray.id().as_ref()
        );
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn menu_tracking_rejects_reentry_until_the_previous_menu_finishes() {
        let tracking = Arc::new(MenuTracking::default());
        let active = MenuGuard::begin(tracking.clone()).unwrap();
        for _ in 0..1000 {
            assert!(MenuGuard::begin(tracking.clone()).is_none());
        }
        drop(active);
        assert!(MenuGuard::begin(tracking).is_some());
    }

    #[test]
    fn failed_or_cancelled_menu_dispatch_releases_the_tracking_guard() {
        let tracking = Arc::new(MenuTracking::default());
        let guard = MenuGuard::begin(tracking.clone()).unwrap();
        let cancelled_dispatch = move || drop(guard);
        std::thread::spawn(move || drop(cancelled_dispatch))
            .join()
            .unwrap();
        assert!(MenuGuard::begin(tracking.clone()).is_some());
        let failed = || -> Result<(), ()> {
            let _guard = MenuGuard::begin(tracking.clone()).unwrap();
            Err(())
        };
        assert_eq!(failed(), Err(()));
        assert!(MenuGuard::begin(tracking).is_some());
    }

    #[test]
    fn each_press_routes_once_and_releases_never_reopen_the_panel() {
        assert_eq!(
            entry_action(MouseButton::Left, MouseButtonState::Down),
            Some(EntryAction::TogglePanel)
        );
        assert_eq!(
            entry_action(MouseButton::Right, MouseButtonState::Down),
            Some(EntryAction::ShowMenu)
        );
        for button in [MouseButton::Left, MouseButton::Right, MouseButton::Middle] {
            assert_eq!(entry_action(button, MouseButtonState::Up), None);
        }
        assert_eq!(
            entry_action(MouseButton::Middle, MouseButtonState::Down),
            None
        );
    }
}
