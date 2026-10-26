//! Pointer hit testing and the taskbar context menu stay on the native thread.
use super::Window;
use crate::resident::{main_window, tray_display::labels::Labels};
use std::ptr;
use windows_sys::Win32::{
    Foundation::*, Graphics::Gdi::InvalidateRect, UI::WindowsAndMessaging::*,
};

impl Window {
    pub(super) unsafe fn update_hover(&mut self, hwnd: HWND) {
        let mut point = POINT::default();
        GetCursorPos(&mut point);
        let hover =
            self.surface.cells.iter().position(|cell| {
                cell.contains(point.x - self.bounds.left, point.y - self.bounds.top)
            });
        if self.hover != hover {
            self.hover = hover;
            InvalidateRect(hwnd, ptr::null(), 0);
        }
    }
    pub(super) unsafe fn menu(&self, hwnd: HWND) {
        let locale = self
            .service
            .model
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .locale
            .clone();
        let labels = Labels::for_locale(&locale);
        let menu = CreatePopupMenu();
        if menu.is_null() {
            return;
        }
        for (id, key) in [(1, "openMain"), (2, "settings"), (3, "quit")] {
            let text: Vec<u16> = labels.text(key).encode_utf16().chain(Some(0)).collect();
            AppendMenuW(menu, MF_STRING, id, text.as_ptr());
        }
        let mut point = POINT::default();
        GetCursorPos(&mut point);
        SetForegroundWindow(hwnd);
        let command = TrackPopupMenu(
            menu,
            TPM_RETURNCMD | TPM_RIGHTBUTTON,
            point.x,
            point.y,
            0,
            hwnd,
            ptr::null(),
        );
        DestroyMenu(menu);
        PostMessageW(hwnd, WM_NULL, 0, 0);
        match command {
            1 => main_window::request(
                &self.service.app,
                main_window::Destination::Main,
                "taskbar_menu",
            ),
            2 => main_window::request(
                &self.service.app,
                main_window::Destination::Settings,
                "taskbar_menu",
            ),
            3 => self.service.app.exit(0),
            _ => {}
        }
    }
}
