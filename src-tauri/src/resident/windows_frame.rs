//! Recalculate the hidden caption before sizing shadowed borderless windows.
pub(super) fn refresh(window: &tauri::WebviewWindow) -> tauri::Result<()> {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        SetWindowPos, SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER,
    };

    // Before its first show, a shadowed borderless window can still report the
    // hidden caption as a client-area offset. Tao adds that stale offset when
    // sizing, extending the popup over the taskbar. Recalculate the native frame
    // before measuring offsets, including after a target-monitor DPI transition.
    let hwnd = window.hwnd()?.0;
    let updated = unsafe {
        SetWindowPos(
            hwnd.cast(),
            std::ptr::null_mut(),
            0,
            0,
            0,
            0,
            SWP_FRAMECHANGED | SWP_NOACTIVATE | SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER,
        )
    };
    if updated == 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    Ok(())
}
