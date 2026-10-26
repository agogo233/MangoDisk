//! Start the native move loop with correctly packed signed screen coordinates.
use crate::resident::runtime::ResidentState;
use std::sync::Arc;
use tauri::Manager;

pub fn start(window: tauri::WebviewWindow) -> tauri::Result<()> {
    if window.label() != super::LABEL {
        return Ok(());
    }
    let target = window.clone();
    window.run_on_main_thread(move || {
        let app = target.app_handle();
        let floating = app
            .state::<Arc<ResidentState>>()
            .preferences
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .floating
            .clone();
        if floating.locked || floating.click_through {
            return;
        }
        if let Err(error) = begin(&target) {
            crate::resident::diagnostics::Failure::record("floating_drag", &error);
        }
    })
}

fn begin(window: &tauri::WebviewWindow) -> tauri::Result<()> {
    use windows_sys::Win32::{
        Foundation::POINT,
        UI::{
            Input::KeyboardAndMouse::{GetAsyncKeyState, ReleaseCapture, VK_LBUTTON},
            WindowsAndMessaging::{
                GetCursorPos, PostMessageW, SendMessageW, HTCAPTION, WM_LBUTTONUP, WM_NCLBUTTONDOWN,
            },
        },
    };
    unsafe {
        if GetAsyncKeyState(VK_LBUTTON as i32) >= 0 {
            log::info!("resident_floating_drag_skipped reason=button_released");
            return Ok(());
        }
        let mut point = POINT::default();
        if GetCursorPos(&mut point) == 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        ReleaseCapture();
        // WM_NCLBUTTONDOWN consumes a packed POINTS value, not a pointer to it.
        // Keeping this boundary here avoids the pinned Tao drag implementation's
        // pointer-valued LPARAM and preserves negative monitor coordinates.
        let packed = pack_point(point.x, point.y);
        let hwnd = window.hwnd()?.0.cast();
        log::info!("resident_floating_drag_started x={} y={}", point.x, point.y);
        SendMessageW(hwnd, WM_NCLBUTTONDOWN, HTCAPTION as usize, packed);
        super::window::finish_drag(window.app_handle());
        // The native move loop consumes the physical release. Reset the client
        // button state just as Tao does after its own drag loop finishes.
        if PostMessageW(hwnd, WM_LBUTTONUP, 0, 0) == 0 {
            return Err(std::io::Error::last_os_error().into());
        }
    }
    Ok(())
}

fn pack_point(x: i32, y: i32) -> isize {
    (u32::from(x as u16) | (u32::from(y as u16) << 16)) as isize
}

#[cfg(test)]
mod tests {
    #[test]
    fn drag_message_retains_signed_screen_coordinates() {
        for (x, y) in [(-1920, -20), (3000, 1700), (0, 0)] {
            let packed = super::pack_point(x, y);
            assert_eq!(packed as i16 as i32, x);
            assert_eq!((packed >> 16) as i16 as i32, y);
        }
    }
}
