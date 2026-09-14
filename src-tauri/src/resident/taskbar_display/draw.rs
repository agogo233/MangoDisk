//! Small GDI backbuffer, released after every frame. Layout width depends on the
//! selected metric, never on changing numeric values, preventing taskbar jitter.
use super::{
    presentation::Column,
    surface::Surface,
    text_layout::{self, Alignment, Run},
};
use windows_sys::{
    core::w,
    Win32::{
        Foundation::{HWND, RECT},
        Graphics::Gdi::*,
    },
};

pub unsafe fn paint(
    hwnd: HWND,
    columns: &[Column],
    surface: &Surface,
    dpi: u32,
    color: [u8; 3],
    hover: Option<usize>,
) {
    let mut ps = PAINTSTRUCT::default();
    let target = BeginPaint(hwnd, &mut ps);
    let width = surface.width;
    let height = surface.height;
    if target.is_null() || width <= 0 || height <= 0 {
        EndPaint(hwnd, &ps);
        return;
    }
    let dc = CreateCompatibleDC(target);
    let bitmap = CreateCompatibleBitmap(target, width, height);
    if dc.is_null() || bitmap.is_null() {
        if !bitmap.is_null() {
            DeleteObject(bitmap);
        }
        if !dc.is_null() {
            DeleteDC(dc);
        }
        EndPaint(hwnd, &ps);
        return;
    }
    let previous = SelectObject(dc, bitmap);
    let light = color[0] < 128;
    let background = if light { 0x00f3f3f3 } else { 0x00252525 };
    let brush = CreateSolidBrush(background);
    FillRect(
        dc,
        &RECT {
            left: 0,
            top: 0,
            right: width,
            bottom: height,
        },
        brush,
    );
    DeleteObject(brush);
    if let Some(cell) = hover.and_then(|index| surface.cells.get(index)) {
        let brush = CreateSolidBrush(if light { 0x00e5e5e5 } else { 0x003d3d3d });
        FillRect(
            dc,
            &RECT {
                left: cell.left,
                top: cell.top,
                right: cell.right,
                bottom: cell.bottom,
            },
            brush,
        );
        DeleteObject(brush);
    }
    text(
        dc,
        &text_layout::runs(columns, surface, dpi),
        dpi,
        Some(color),
        CLEARTYPE_QUALITY,
    );
    BitBlt(target, 0, 0, width, height, dc, 0, 0, SRCCOPY);
    SelectObject(dc, previous);
    DeleteObject(bitmap);
    DeleteDC(dc);
    EndPaint(hwnd, &ps);
}

// Opaque painting keeps ClearType on its known background. Transparent painting
// uses DirectWrite; both renderers consume the same physical text-run bounds.
pub unsafe fn text(
    dc: HDC,
    runs: &[Run<'_>],
    dpi: u32,
    foreground: Option<[u8; 3]>,
    quality: u8,
) -> bool {
    SetBkMode(dc, TRANSPARENT as i32);
    let font = CreateFontW(
        -((12 * dpi / 96) as i32),
        0,
        0,
        0,
        500,
        0,
        0,
        0,
        DEFAULT_CHARSET as _,
        OUT_DEFAULT_PRECIS as _,
        CLIP_DEFAULT_PRECIS as _,
        quality as _,
        DEFAULT_PITCH as _,
        w!("Segoe UI"),
    );
    if font.is_null() {
        return false;
    }
    let old_font = SelectObject(dc, font);
    for run in runs {
        // Loading rates have no unit. Never pass an empty Vec's dangling pointer
        // to USER32; some DrawText paths inspect the buffer even with count zero.
        if run.text.is_empty() {
            continue;
        }
        // None requests a white coverage mask; transparent painting applies the
        // exact same run colors after rasterization instead of tinting the mask.
        let color = foreground
            .map(|value| run.ink.rgb(value))
            .unwrap_or([255; 3]);
        SetTextColor(
            dc,
            color[0] as u32 | ((color[1] as u32) << 8) | ((color[2] as u32) << 16),
        );
        let mut value: Vec<u16> = run.text.encode_utf16().chain(Some(0)).collect();
        let mut rect = RECT {
            left: run.bounds.left,
            top: run.bounds.top,
            right: run.bounds.right,
            bottom: run.bounds.bottom,
        };
        let alignment = match run.alignment {
            Alignment::Left => DT_LEFT,
            Alignment::Center => DT_CENTER,
            Alignment::Right => DT_RIGHT,
        };
        DrawTextW(
            dc,
            value.as_mut_ptr(),
            (value.len() - 1) as _,
            &mut rect,
            alignment | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX | DT_END_ELLIPSIS,
        );
    }
    SelectObject(dc, old_font);
    DeleteObject(font);
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::resident::taskbar_display::{layout::Bounds, text_layout::Ink};

    #[test]
    fn compact_text_fits_native_font_at_percentage_and_rate_boundaries() {
        use crate::resident::{
            taskbar_display::{presentation, surface::Surface},
            tray_display::format::{DisplayEntry, DisplayId},
        };
        unsafe {
            let dc = CreateCompatibleDC(std::ptr::null_mut());
            assert!(!dc.is_null());
            for dpi in [96, 120, 144, 192, 240, 288] {
                let font = CreateFontW(
                    -((12 * dpi / 96) as i32),
                    0,
                    0,
                    0,
                    500,
                    0,
                    0,
                    0,
                    DEFAULT_CHARSET as _,
                    OUT_DEFAULT_PRECIS as _,
                    CLIP_DEFAULT_PRECIS as _,
                    CLEARTYPE_QUALITY as _,
                    DEFAULT_PITCH as _,
                    w!("Segoe UI"),
                );
                assert!(!font.is_null());
                let previous = SelectObject(dc, font);
                for (id, text, digits) in [
                    (DisplayId::Cpu, "CPU 100%", "100"),
                    (DisplayId::Memory, "MEM 0%", "0"),
                    (DisplayId::Disk, "DISK —", "—"),
                    (DisplayId::Upload, "↑999.9 TB/s", "999.9"),
                    (DisplayId::Upload, "↑ 99.9 MB/s", "99.9"),
                    (DisplayId::Upload, "↑ 999.0 GB/s", "999"),
                    (DisplayId::Upload, "↑ 0.1 KB/s", "0.1"),
                    (DisplayId::Upload, "↑ 0.0 B/s", "0.0"),
                    (DisplayId::Upload, "↑ —", "—"),
                ] {
                    let columns = presentation::columns(
                        &[DisplayEntry {
                            id,
                            marker: String::new(),
                            digits: digits.into(),
                            text: text.into(),
                            tooltip: String::new(),
                        }],
                        dpi,
                        true,
                    );
                    let surface = Surface::arrange(
                        &columns,
                        Bounds {
                            left: 0,
                            top: 0,
                            right: 3840,
                            bottom: (48 * dpi / 96) as i32,
                        },
                        dpi,
                    )
                    .unwrap();
                    for run in text_layout::runs(&columns, &surface, dpi) {
                        if run.text.is_empty() {
                            continue;
                        }
                        let text: Vec<u16> = run.text.encode_utf16().collect();
                        let mut size = windows_sys::Win32::Foundation::SIZE::default();
                        assert_ne!(
                            GetTextExtentPoint32W(dc, text.as_ptr(), text.len() as i32, &mut size),
                            0
                        );
                        assert!(
                            size.cx <= run.bounds.width(),
                            "dpi={dpi} text={} measured={} field={}",
                            run.text,
                            size.cx,
                            run.bounds.width()
                        );
                    }
                }
                SelectObject(dc, previous);
                DeleteObject(font);
            }
            DeleteDC(dc);
        }
    }

    #[test]
    fn native_loading_rate_with_empty_unit_does_not_crash() {
        // Exercise USER32, not just rectangle calculations: an empty unit exists
        // on the first frame before the network sampler has a baseline.
        unsafe {
            let dc = CreateCompatibleDC(std::ptr::null_mut());
            assert!(!dc.is_null());
            let runs: Vec<_> = ["↑", "—", "", "↓", "0.0", "B/s"]
                .into_iter()
                .map(|text| Run {
                    text,
                    bounds: Bounds {
                        left: 0,
                        top: 0,
                        right: 112,
                        bottom: 18,
                    },
                    alignment: Alignment::Right,
                    ink: Ink::Foreground,
                })
                .collect();
            assert!(text(dc, &runs, 96, Some([0; 3]), CLEARTYPE_QUALITY));
            assert!(text(dc, &runs, 192, None, ANTIALIASED_QUALITY));
            DeleteDC(dc);
        }
    }
}
