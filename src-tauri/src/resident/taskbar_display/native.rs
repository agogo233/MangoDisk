//! The window and its GDI objects stay on one native thread. Other threads only
//! replace a bounded model and post a wake message; callbacks never create WebViews.
use super::{
    directwrite, draw,
    layout::{self, Bounds},
    position::{self, Edge, Environment},
    presentation::{self, Column},
    surface::Surface,
    transparent, DisplayStatus, Service,
};
use crate::resident::{
    main_window, panel,
    tray_display::{labels::Labels, windows_bitmap},
};
use std::{
    cell::RefCell,
    ptr,
    sync::{atomic::Ordering, Arc},
    time::Duration,
};
use windows_sys::{
    core::w,
    Win32::{
        Foundation::*,
        Graphics::Gdi::*,
        System::Threading::{
            OpenProcess, QueryFullProcessImageNameW, PROCESS_QUERY_LIMITED_INFORMATION,
        },
        UI::{HiDpi::*, WindowsAndMessaging::*},
    },
};
pub const UPDATE: u32 = WM_APP + 71;
struct Window {
    service: Arc<Service>,
    columns: Vec<Column>,
    bounds: Bounds,
    surface: Surface,
    dpi: u32,
    color: [u8; 3],
    hover: Option<usize>,
    visible: bool,
    background: bool,
    paint_failed: bool,
    text_renderer: Option<directwrite::Renderer>,
    position_failed: bool,
    placement_policy: Option<(
        crate::resident::preference_schema::TaskbarPosition,
        Environment,
        Edge,
    )>,
    foreground: usize,
    shell_flyout: bool,
    appearance_checked: std::time::Instant,
}

pub fn start(service: Arc<Service>) {
    std::thread::spawn(move || unsafe {
        // This affects only our native thread, never Explorer or Tauri's process DPI.
        SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        let class = WNDCLASSW {
            lpfnWndProc: Some(procedure),
            lpszClassName: w!("MangoDiskTaskbarStatus"),
            hCursor: LoadCursorW(ptr::null_mut(), IDC_HAND),
            ..Default::default()
        };
        if RegisterClassW(&class) == 0 {
            log::warn!(
                "resident_taskbar_native_failed stage=register_class code={}",
                GetLastError()
            );
            service.publish(DisplayStatus::ShellUnavailable);
            return;
        }
        loop {
            let shell = FindWindowW(w!("Shell_TrayWnd"), ptr::null());
            if shell.is_null() {
                service.publish(DisplayStatus::ShellUnavailable);
                std::thread::sleep(Duration::from_millis(200));
                continue;
            }
            let mut data = Box::new(RefCell::new(Window {
                service: service.clone(),
                columns: Vec::new(),
                bounds: Bounds::default(),
                surface: Surface::default(),
                dpi: 96,
                color: [0; 3],
                hover: None,
                visible: false,
                background: true,
                paint_failed: false,
                text_renderer: None,
                position_failed: false,
                placement_policy: None,
                foreground: 0,
                shell_flyout: false,
                appearance_checked: std::time::Instant::now() - Duration::from_secs(2),
            }));
            // Bind top-level ownership at creation. Changing ownership afterward can
            // be ignored by the Windows 11 shell; this is not child-window parenting.
            let hwnd = CreateWindowExW(
                WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE | WS_EX_TOPMOST,
                w!("MangoDiskTaskbarStatus"),
                w!("MangoDisk Status"),
                WS_POPUP,
                0,
                0,
                1,
                1,
                shell,
                ptr::null_mut(),
                ptr::null_mut(),
                (&mut *data as *mut RefCell<Window>).cast(),
            );
            if hwnd.is_null() {
                log::warn!(
                    "resident_taskbar_native_failed stage=create_window code={}",
                    GetLastError()
                );
                service.publish(DisplayStatus::ShellUnavailable);
                return;
            }
            service.window.store(hwnd as usize, Ordering::Relaxed);
            PostMessageW(hwnd, UPDATE, 0, 0);
            let mut message = MSG::default();
            let mut result = GetMessageW(&mut message, ptr::null_mut(), 0, 0);
            while result > 0 {
                TranslateMessage(&message);
                DispatchMessageW(&message);
                result = GetMessageW(&mut message, ptr::null_mut(), 0, 0);
            }
            // Do not release the callback data while a native window can still
            // refer to it, including the exceptional GetMessage failure path.
            if IsWindow(hwnd) != 0 {
                DestroyWindow(hwnd);
            }
            service.window.store(0, Ordering::Relaxed);
            service
                .bounds
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .clear();
            service.publish(DisplayStatus::ShellUnavailable);
            if result < 0 {
                log::warn!("resident_taskbar_native_failed stage=message_loop");
                return;
            }
            // Explorer owns only this native surface. Recreate it after a shell
            // restart while retaining the sampler, model and existing WebViews.
            std::thread::sleep(Duration::from_millis(200));
        }
    });
}

unsafe extern "system" fn procedure(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if message == WM_NCCREATE {
        let create = &*(lparam as *const CREATESTRUCTW);
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, create.lpCreateParams as isize);
    }
    let raw = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut RefCell<Window>;
    if raw.is_null() {
        return DefWindowProcW(hwnd, message, wparam, lparam);
    }
    if message == WM_NCDESTROY {
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
        PostQuitMessage(0);
        return 0;
    }
    // Default processing can synchronously reenter this callback. Borrow state
    // only for handled messages. Owned popups handle right release themselves,
    // otherwise DefWindowProc forwards the context menu to the shell owner.
    if !matches!(
        message,
        UPDATE | WM_TIMER | WM_PAINT | WM_MOUSEMOVE | WM_LBUTTONUP | WM_RBUTTONUP | WM_CONTEXTMENU
    ) {
        return match message {
            WM_ERASEBKGND => 1,
            WM_MOUSEACTIVATE => MA_NOACTIVATE as _,
            WM_CLOSE => {
                ShowWindow(hwnd, SW_HIDE);
                0
            }
            _ => DefWindowProcW(hwnd, message, wparam, lparam),
        };
    }
    // ShowWindow/SetWindowPos and popup menus may synchronously reenter this
    // callback. Never manufacture overlapping mutable references to window state.
    let Ok(mut window) = (*raw).try_borrow_mut() else {
        return DefWindowProcW(hwnd, message, wparam, lparam);
    };
    match message {
        UPDATE | WM_TIMER => {
            if message == UPDATE {
                if window.service.requested.load(Ordering::Relaxed) {
                    SetTimer(hwnd, 1, 100, None);
                } else {
                    KillTimer(hwnd, 1);
                }
            }
            window.update(hwnd);
            0
        }
        WM_PAINT => {
            if window.background {
                draw::paint(
                    hwnd,
                    &window.columns,
                    &window.surface,
                    window.dpi,
                    window.color,
                    window.hover,
                );
            } else {
                // UpdateLayeredWindow owns the pixels; validate WM_PAINT without
                // trying to BitBlt to a layered window's redirected surface.
                ValidateRect(hwnd, ptr::null());
                window.paint_transparent(hwnd);
            }
            0
        }
        WM_MOUSEMOVE => {
            window.update_hover(hwnd);
            0
        }
        WM_LBUTTONUP => {
            window.update_hover(hwnd);
            if let Some(column) = window.hover.and_then(|i| window.columns.get(i)) {
                let app = window.service.app.clone();
                let id = column.id;
                tauri::async_runtime::spawn(async move {
                    if let Err(error) =
                        panel::toggle_from(&app, &format!("taskbar-{}", id.tray_id()), id.metric())
                    {
                        crate::resident::diagnostics::Failure::record(
                            "taskbar_panel_entry",
                            &error,
                        );
                    }
                });
            }
            0
        }
        WM_RBUTTONUP | WM_CONTEXTMENU => {
            window.menu(hwnd);
            0
        }
        _ => DefWindowProcW(hwnd, message, wparam, lparam),
    }
}

impl Window {
    unsafe fn paint_transparent(&mut self, hwnd: HWND) -> bool {
        let initializing = self.text_renderer.is_none();
        match transparent::paint(
            &mut self.text_renderer,
            hwnd,
            &self.columns,
            &self.surface,
            self.dpi,
            self.color,
            self.hover,
        ) {
            Ok(()) => {
                if initializing {
                    log::info!(
                        "resident_taskbar_text_renderer backend=directwrite antialias=grayscale"
                    );
                }
                if self.paint_failed {
                    log::info!("resident_taskbar_paint_recovered");
                }
                self.paint_failed = false;
                true
            }
            Err(error) => {
                // Discard device resources after a failed frame so the next poll
                // can recover without restarting the application.
                self.text_renderer = None;
                if !self.paint_failed {
                    log::warn!(
                        "resident_taskbar_native_failed stage={} code={}",
                        error.stage,
                        error.code
                    );
                }
                self.paint_failed = true;
                self.hide(hwnd);
                self.service.publish(DisplayStatus::ShellUnavailable);
                false
            }
        }
    }

    unsafe fn hide(&mut self, hwnd: HWND) {
        // Explorer can restore an owned popup independently of our cache.
        // Reconcile actual visibility so fullscreen/auto-hide stays respected.
        if self.visible || IsWindowVisible(hwnd) != 0 {
            ShowWindow(hwnd, SW_HIDE);
            self.visible = false;
        }
        self.service
            .bounds
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clear();
    }
    unsafe fn update(&mut self, hwnd: HWND) {
        if !self.service.requested.load(Ordering::Relaxed) {
            self.hide(hwnd);
            self.service.publish(DisplayStatus::Tray);
            return;
        }
        let geometry = self
            .service
            .geometry
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        let Some(geometry) = geometry.filter(|g| g.sampled.elapsed() < Duration::from_secs(3))
        else {
            self.hide(hwnd);
            self.service.publish(DisplayStatus::ShellUnavailable);
            return;
        };
        if geometry.hidden {
            self.hide(hwnd);
            self.service.publish(DisplayStatus::Taskbar);
            return;
        }
        let model = self
            .service
            .model
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        let columns = presentation::columns(&model.entries, geometry.dpi, model.compact);
        let Some(surface) = Surface::arrange(&columns, geometry.bar, geometry.dpi) else {
            self.hide(hwnd);
            self.service.publish(DisplayStatus::UnsupportedLayout);
            return;
        };
        let edge = position::resolve(model.position, geometry.environment);
        let policy = (model.position, geometry.environment, edge);
        if self.placement_policy != Some(policy) {
            log::info!(
                "resident_taskbar_position requested={:?} environment={:?} resolved={:?}",
                model.position,
                geometry.environment,
                edge
            );
            self.placement_policy = Some(policy);
        }
        let gap = (4 * geometry.dpi / 96) as i32;
        let Some(bounds) = layout::place(
            geometry.bar,
            &geometry.occupied,
            surface.width,
            surface.height,
            gap,
            edge,
            model.position == crate::resident::preference_schema::TaskbarPosition::Auto,
        ) else {
            self.hide(hwnd);
            self.service.publish(DisplayStatus::NoSpace);
            return;
        };
        let color = if self.appearance_checked.elapsed() >= Duration::from_secs(1) {
            self.appearance_checked = std::time::Instant::now();
            windows_bitmap::appearance().color
        } else {
            self.color
        };
        let layout_changed = self.bounds != bounds || self.surface != surface;
        let changed = layout_changed
            || self.columns != columns
            || self.color != color
            || self.dpi != geometry.dpi
            || self.background != model.background
            || self.paint_failed;
        if self.bounds != bounds || self.surface.vertical != surface.vertical {
            log::info!(
                "resident_taskbar_layout vertical={} preference={:?} compact={} width={} height={} left={} top={} dpi={}",
                surface.vertical,
                model.position,
                model.compact,
                surface.width,
                surface.height,
                bounds.left,
                bounds.top,
                geometry.dpi
            );
        }
        if self.background != model.background {
            let style = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
            SetWindowLongPtrW(
                hwnd,
                GWL_EXSTYLE,
                if model.background {
                    style & !(WS_EX_LAYERED as isize)
                } else {
                    style | WS_EX_LAYERED as isize
                },
            );
            self.background = model.background;
            log::debug!("resident_taskbar_background enabled={}", self.background);
        }
        self.bounds = bounds;
        self.surface = surface;
        self.columns = columns;
        self.color = color;
        self.dpi = geometry.dpi;
        let shell = geometry.shell as HWND;
        if IsWindow(shell) == 0 {
            self.hide(hwnd);
            self.service.publish(DisplayStatus::ShellUnavailable);
            return;
        }
        // Auto-hide and fullscreen temporarily hide this surface, without turning
        // the tray set on/off on every animation. A genuine layout failure does fallback.
        let mut current = RECT::default();
        GetWindowRect(shell, &mut current);
        let shell_moved = current.left != geometry.bar.left || current.top != geometry.bar.top;
        let center = POINT {
            x: (bounds.left + bounds.right) / 2,
            y: (bounds.top + bounds.bottom) / 2,
        };
        let monitor = MonitorFromPoint(center, MONITOR_DEFAULTTONULL);
        let mut info = MONITORINFO {
            cbSize: std::mem::size_of::<MONITORINFO>() as _,
            ..Default::default()
        };
        let inside = !monitor.is_null()
            && GetMonitorInfoW(monitor, &mut info) != 0
            && bounds.left >= info.rcMonitor.left
            && bounds.right <= info.rcMonitor.right
            && bounds.top >= info.rcMonitor.top
            && bounds.bottom <= info.rcMonitor.bottom;
        let foreground = GetForegroundWindow();
        if self.foreground != foreground as usize {
            self.foreground = foreground as usize;
            self.shell_flyout = is_shell_flyout(foreground);
        }
        // System flyouts have their own activation/stacking rules. Yield the
        // strip while they are open rather than opening a panel behind them or
        // forcing input queues together. Restore the strip on normal foreground.
        if self.shell_flyout {
            self.hide(hwnd);
            self.service.publish(DisplayStatus::Taskbar);
            return;
        }
        if GetWindowLongPtrW(hwnd, GWLP_HWNDPARENT) != shell as isize {
            DestroyWindow(hwnd);
            return;
        }
        let mut front = RECT::default();
        // A maximized window extends to the screen edge when auto-hide is enabled;
        // it must not be mistaken for fullscreen while the user reveals the bar.
        let covered = IsZoomed(foreground) == 0
            && foreground != GetShellWindow()
            && foreground != hwnd
            && foreground != shell
            && !foreground.is_null()
            && GetWindowRect(foreground, &mut front) != 0
            && front.left <= bounds.left
            && front.right >= bounds.right
            && front.top <= bounds.top
            && front.bottom >= bounds.bottom;
        if shell_moved || !inside || IsWindowVisible(shell) == 0 || covered {
            self.hide(hwnd);
            self.service.publish(DisplayStatus::Taskbar);
            return;
        }
        // Ownership maintains shell-relative order without repeatedly lifting
        // native shell menus. Repair only a lost topmost state.
        let needs_topmost = GetWindowLongPtrW(hwnd, GWL_EXSTYLE) & WS_EX_TOPMOST as isize == 0;
        if changed || !self.visible || IsWindowVisible(hwnd) == 0 || needs_topmost {
            let positioned = SetWindowPos(
                hwnd,
                if needs_topmost {
                    HWND_TOPMOST
                } else {
                    ptr::null_mut()
                },
                bounds.left,
                bounds.top,
                bounds.width(),
                bounds.height(),
                SWP_NOACTIVATE | SWP_SHOWWINDOW | if needs_topmost { 0 } else { SWP_NOZORDER },
            );
            let position_error = if positioned == 0 { GetLastError() } else { 0 };
            // Window regions do not resize with SetWindowPos. Update the shape before
            // checking occlusion: after rotating the taskbar, the new center can lie
            // outside the old region and incorrectly hit Explorer underneath it.
            let radius = (8 * geometry.dpi / 96) as i32;
            let region = CreateRoundRectRgn(
                0,
                0,
                bounds.width() + 1,
                bounds.height() + 1,
                radius,
                radius,
            );
            if SetWindowRgn(hwnd, region, 1) == 0 {
                DeleteObject(region);
            }
            // Publish nonzero-alpha pixels before hit testing. A newly layered
            // window has no surface and would otherwise look like shell occlusion.
            if !self.background && !self.paint_transparent(hwnd) {
                return;
            }
            let topmost = GetWindowLongPtrW(hwnd, GWL_EXSTYLE) & WS_EX_TOPMOST as isize != 0;
            let surface = WindowFromPoint(center);
            let shell_still_covers = !surface.is_null() && GetAncestor(surface, GA_ROOT) == shell;
            if positioned == 0 || !topmost || shell_still_covers {
                if !self.position_failed {
                    log::warn!(
                        "resident_taskbar_native_failed stage=position code={position_error} topmost={topmost} shell_covers={shell_still_covers}"
                    );
                }
                self.position_failed = true;
                self.hide(hwnd);
                self.service.publish(DisplayStatus::ShellUnavailable);
                return;
            }
            self.position_failed = false;
            InvalidateRect(hwnd, ptr::null(), 0);
            self.visible = true;
        }
        *self
            .service
            .bounds
            .lock()
            .unwrap_or_else(|e| e.into_inner()) = self
            .columns
            .iter()
            .zip(&self.surface.cells)
            .map(|(column, cell)| {
                (
                    column.id,
                    Bounds {
                        left: bounds.left + cell.left,
                        right: bounds.left + cell.right,
                        top: bounds.top + cell.top,
                        bottom: bounds.top + cell.bottom,
                    },
                )
            })
            .collect();
        self.update_hover(hwnd);
        self.service.publish(DisplayStatus::Taskbar);
    }
    unsafe fn update_hover(&mut self, hwnd: HWND) {
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
    unsafe fn menu(&self, hwnd: HWND) {
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

/// Classify only the foreground process, once per foreground-window change.
/// Names are stable Windows executables; full paths never leave this function.
unsafe fn is_shell_flyout(hwnd: HWND) -> bool {
    let mut pid = 0;
    GetWindowThreadProcessId(hwnd, &mut pid);
    let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
    if process.is_null() {
        return false;
    }
    let mut buffer = [0u16; 512];
    let mut length = buffer.len() as u32;
    let success = QueryFullProcessImageNameW(process, 0, buffer.as_mut_ptr(), &mut length);
    CloseHandle(process);
    if success == 0 {
        return false;
    }
    let path = String::from_utf16_lossy(&buffer[..length as usize]);
    let name = path.rsplit('\\').next().unwrap_or("");
    [
        "StartMenuExperienceHost.exe",
        "SearchHost.exe",
        "SearchApp.exe",
        "ShellExperienceHost.exe",
    ]
    .iter()
    .any(|expected| name.eq_ignore_ascii_case(expected))
}
