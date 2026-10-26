use super::*;

#[test]
fn failed_creation_does_not_stop_the_replacement_message_loop() {
    std::thread::spawn(|| unsafe {
        unsafe extern "system" fn fail_creation(
            window: HWND,
            message: u32,
            wparam: WPARAM,
            lparam: LPARAM,
        ) -> LRESULT {
            if message == WM_CREATE {
                return -1;
            }
            if message == WM_NCDESTROY {
                PostQuitMessage(0);
            }
            DefWindowProcW(window, message, wparam, lparam)
        }
        let class = WNDCLASSW {
            lpfnWndProc: Some(fail_creation),
            lpszClassName: w!("MangoDiskFailedCreationTest"),
            ..Default::default()
        };
        assert_ne!(RegisterClassW(&class), 0);
        let window = CreateWindowExW(
            0,
            class.lpszClassName,
            ptr::null(),
            WS_POPUP,
            0,
            0,
            1,
            1,
            ptr::null_mut(),
            ptr::null_mut(),
            ptr::null_mut(),
            ptr::null(),
        );
        assert!(window.is_null());
        let mut message = MSG::default();
        assert_ne!(
            PeekMessageW(&mut message, ptr::null_mut(), WM_QUIT, WM_QUIT, PM_NOREMOVE),
            0,
            "failed creation must reproduce the pending destruction message"
        );
        clear_stale_quit();
        PostQuitMessage(17);
        assert_eq!(GetMessageW(&mut message, ptr::null_mut(), 0, 0), 0);
        assert_eq!(
            message.wParam, 17,
            "replacement must receive only its own exit"
        );
        UnregisterClassW(class.lpszClassName, ptr::null_mut());
    })
    .join()
    .unwrap();
}
