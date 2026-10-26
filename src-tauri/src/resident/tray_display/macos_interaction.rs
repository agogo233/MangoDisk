//! Route panel presses while tray-icon owns native context-menu tracking.
use objc2::MainThreadMarker;
use tauri::tray::{MouseButton, MouseButtonState, TrayIcon};

use crate::resident::{diagnostics::Failure, panel};

#[derive(Debug, PartialEq, Eq)]
enum EntryAction {
    TogglePanel,
    HidePanel,
}

fn entry_action(button: MouseButton, state: MouseButtonState) -> Option<EntryAction> {
    // Act on press, before the status button's focus transition completes. A
    // release must never reopen a panel already closed by the same press.
    match (button, state) {
        (MouseButton::Left, MouseButtonState::Down) => Some(EntryAction::TogglePanel),
        (MouseButton::Right, MouseButtonState::Down) => Some(EntryAction::HidePanel),
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
    // Never move or clone a TrayIcon on a worker: its Rc-backed wrapper
    // requires a main-thread lifetime. Native menu tracking belongs to
    // tray-icon, which attaches the menu only while presenting it (#365).
    tauri::async_runtime::spawn(async move {
        match action {
            EntryAction::TogglePanel => {
                if let Err(error) = panel::toggle_from(&app, &source, None) {
                    Failure::record("panel_entry", &error);
                }
            }
            EntryAction::HidePanel => panel::hide(&app),
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_press_routes_once_and_releases_never_reopen_the_panel() {
        assert_eq!(
            entry_action(MouseButton::Left, MouseButtonState::Down),
            Some(EntryAction::TogglePanel)
        );
        assert_eq!(
            entry_action(MouseButton::Right, MouseButtonState::Down),
            Some(EntryAction::HidePanel)
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
