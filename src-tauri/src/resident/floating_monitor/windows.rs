//! Native Windows visibility and foreground-fullscreen detection.

pub(super) fn set_visible(window: &tauri::WebviewWindow, visible: bool) -> tauri::Result<()> {
    use windows_sys::Win32::UI::WindowsAndMessaging::{ShowWindow, SW_HIDE, SW_SHOWNOACTIVATE};
    // Tao's Show activates a previously hidden window even with focused(false).
    // Own both visibility transitions here: its cached visibility stays hidden,
    // so Tao's Hide would otherwise skip hiding a natively shown monitor.
    unsafe {
        ShowWindow(
            window.hwnd()?.0.cast(),
            if visible { SW_SHOWNOACTIVATE } else { SW_HIDE },
        );
    }
    if window.is_visible()? != visible {
        return Err(std::io::Error::other("floating window visibility was not applied").into());
    }
    Ok(())
}

pub(super) fn fullscreen(window: &tauri::WebviewWindow) -> bool {
    use windows_sys::Win32::{
        Foundation::RECT,
        UI::WindowsAndMessaging::{
            GetClassNameW, GetForegroundWindow, GetWindowLongW, GetWindowRect,
            GetWindowThreadProcessId, GWL_STYLE, WS_CAPTION, WS_MAXIMIZE, WS_THICKFRAME,
        },
    };
    unsafe {
        let foreground = GetForegroundWindow();
        if foreground.is_null() {
            return false;
        }
        let mut pid = 0;
        GetWindowThreadProcessId(foreground, &mut pid);
        if pid == std::process::id() {
            return false;
        }
        let mut class = [0u16; 128];
        let length = GetClassNameW(foreground, class.as_mut_ptr(), class.len() as i32);
        let class = String::from_utf16_lossy(&class[..length.max(0) as usize]);
        if matches!(
            class.as_str(),
            "Progman" | "WorkerW" | "Shell_TrayWnd" | "Shell_SecondaryTrayWnd"
        ) {
            return false;
        }
        let mut rect = RECT::default();
        if GetWindowRect(foreground, &mut rect) == 0 {
            return false;
        }
        let Ok(Some(monitor)) = window.current_monitor() else {
            return false;
        };
        let origin = monitor.position();
        let size = monitor.size();
        let style = GetWindowLongW(foreground, GWL_STYLE) as u32;
        rect.left <= origin.x
            && rect.top <= origin.y
            && rect.right >= origin.x + size.width as i32
            && rect.bottom >= origin.y + size.height as i32
            && (style & (WS_CAPTION | WS_THICKFRAME) == 0
                || (style & WS_MAXIMIZE == 0
                    && rect.left == origin.x
                    && rect.top == origin.y
                    && rect.right == origin.x + size.width as i32
                    && rect.bottom == origin.y + size.height as i32))
    }
}

pub(super) fn dragging() -> bool {
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_LBUTTON};
    unsafe { GetAsyncKeyState(VK_LBUTTON as i32) < 0 }
}
