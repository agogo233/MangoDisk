//! Physical desktop geometry with monitor-relative persistence and bounded recovery.
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Area {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Bookmark {
    pub schema_version: u32,
    pub monitor: String,
    pub horizontal: u16,
    pub vertical: u16,
}

impl Bookmark {
    pub fn valid(&self) -> bool {
        self.schema_version == 1
            && self.horizontal <= 10_000
            && self.vertical <= 10_000
            && self.monitor.len() <= 1024
            && !self.monitor.contains('\0')
    }

    pub fn restore(&self, area: Area, size: (u32, u32)) -> (i32, i32) {
        let x = area.x as f64
            + area.width.saturating_sub(size.0) as f64 * self.horizontal.min(10_000) as f64
                / 10_000.0;
        let y = area.y as f64
            + area.height.saturating_sub(size.1) as f64 * self.vertical.min(10_000) as f64
                / 10_000.0;
        (x.round() as i32, y.round() as i32)
    }

    pub fn after_placement(
        &self,
        monitor: String,
        area: Area,
        size: (u32, u32),
        dragged_to: Option<(i32, i32)>,
    ) -> Self {
        // Geometry changes restore a rounded pixel position, not a new user
        // preference. Recapturing it would gradually quantize the saved anchor.
        let Some(point) = dragged_to else {
            return Self {
                monitor,
                ..self.clone()
            };
        };
        fn fraction(value: i32, origin: i32, available: u32, previous: u16) -> u16 {
            // A constrained axis has no observable position preference. Retain
            // its last anchor so expanding the work area restores that choice.
            if available == 0 {
                return previous;
            }
            (((value as f64 - origin as f64) / available as f64).clamp(0.0, 1.0) * 10_000.0).round()
                as u16
        }
        Self {
            schema_version: 1,
            monitor,
            horizontal: fraction(
                point.0,
                area.x,
                area.width.saturating_sub(size.0),
                self.horizontal,
            ),
            vertical: fraction(
                point.1,
                area.y,
                area.height.saturating_sub(size.1),
                self.vertical,
            ),
        }
    }
}

impl Default for Bookmark {
    fn default() -> Self {
        Self {
            schema_version: 1,
            monitor: String::new(),
            horizontal: 10_000,
            vertical: 10_000,
        }
    }
}

pub fn snap(area: Area, size: (u32, u32), point: (i32, i32), threshold: f64) -> (i32, i32) {
    fn axis(origin: i32, available: u32, size: u32, value: i32, threshold: f64) -> i32 {
        let start = origin as f64;
        let end = start + available.saturating_sub(size) as f64;
        let value = (value as f64).clamp(start, end);
        if value - start <= threshold {
            start as i32
        } else if end - value <= threshold {
            end as i32
        } else {
            value.round() as i32
        }
    }
    (
        axis(area.x, area.width, size.0, point.0, threshold),
        axis(area.y, area.height, size.1, point.1, threshold),
    )
}

pub(super) struct Layout {
    pub columns: Vec<f64>,
    pub size: (u32, u32),
}

pub(super) fn item_width(label: f64, value: f64) -> f64 {
    label + value + 9.0
}

/// Column widths depend on translated labels and reserved samples, never live readings.
pub(super) fn layout(
    widths: &[f64],
    vertical: bool,
    area: Area,
    scale: f64,
    row_height: f64,
) -> Layout {
    let available_width = area.width as f64 / scale;
    let available_height = area.height as f64 / scale;
    let empty = [108.0];
    let widths = if widths.is_empty() { &empty } else { widths };
    let count = widths.len();
    let mut columns = vec![widths.iter().copied().fold(0.0, f64::max)];
    if !vertical {
        for maximum in (1..=count.min(5)).rev() {
            let rows = count.div_ceil(maximum);
            let column_count = count.div_ceil(rows);
            let mut candidate = vec![0.0_f64; column_count];
            for (index, width) in widths.iter().enumerate() {
                candidate[index % column_count] = candidate[index % column_count].max(*width);
            }
            let rows = count.div_ceil(column_count);
            let gutter = if rows as f64 * row_height + 6.0 > available_height {
                10.0
            } else {
                0.0
            };
            if candidate.iter().sum::<f64>() + 6.0 + gutter <= available_width || column_count == 1
            {
                columns = candidate;
                break;
            }
        }
    }
    let logical_height = count.div_ceil(columns.len()) as f64 * row_height + 6.0;
    // The project scrollbar is 10 CSS pixels wide; do not let it cover readings.
    let gutter = if logical_height > available_height {
        10.0
    } else {
        0.0
    };
    let logical_width = columns.iter().sum::<f64>() + 6.0 + gutter;
    // A very narrow work area keeps the number of columns stable and scrolls rows.
    if columns.len() == 1 {
        columns[0] = columns[0].min((available_width - 6.0 - gutter).max(1.0));
    }
    Layout {
        columns,
        size: (
            (logical_width.min(available_width) * scale)
                .round()
                .max(1.0) as u32,
            (logical_height.min(available_height) * scale)
                .round()
                .max(1.0) as u32,
        ),
    }
}

/// Prefer above/below the entire strip so details do not cover its metric targets.
#[cfg(any(windows, test))]
pub fn detail_position(
    surface: Area,
    work: (f64, f64, f64, f64),
    size: (f64, f64),
    gap: f64,
) -> (f64, f64) {
    let (left, top, width, height) = work;
    let x = surface.x as f64;
    let y = surface.y as f64;
    let centered = x + surface.width as f64 / 2.0 - size.0 / 2.0;
    let (x, y) = if y - gap - size.1 >= top {
        (centered, y - gap - size.1)
    } else if y + surface.height as f64 + gap + size.1 <= top + height {
        (centered, y + surface.height as f64 + gap)
    } else if x + surface.width as f64 + gap + size.0 <= left + width {
        (x + surface.width as f64 + gap, y)
    } else {
        (x - gap - size.0, y)
    };
    (
        x.clamp(left, left + (width - size.0).max(0.0)),
        y.clamp(top, top + (height - size.1).max(0.0)),
    )
}

#[cfg(test)]
mod tests {
    fn dimensions(entries: usize, vertical: bool, area: Area, scale: f64) -> (u32, u32) {
        layout(&vec![108.0; entries], vertical, area, scale, 20.0).size
    }

    use super::*;

    #[test]
    fn resizing_preserves_the_unrounded_position_preference() {
        let area = Area {
            x: 0,
            y: 0,
            width: 1920,
            height: 1080,
        };
        let original = Bookmark {
            monitor: "screen".into(),
            horizontal: 303,
            vertical: 517,
            ..Default::default()
        };
        let mut bookmark = original.clone();
        for _ in 0..5 {
            for size in [(1146, 66), (1466, 66)] {
                bookmark = bookmark.after_placement("screen".into(), area, size, None);
            }
        }
        assert_eq!(bookmark, original);
        let recovered = bookmark.after_placement("replacement".into(), area, (300, 100), None);
        assert_eq!(recovered.monitor, "replacement");
        assert_eq!((recovered.horizontal, recovered.vertical), (303, 517));
    }

    #[test]
    fn font_presets_keep_rows_within_the_work_area_at_fractional_dpi() {
        use crate::resident::preference_schema::FloatingFontSize;
        for (font, expected_row) in [
            (FloatingFontSize::Standard, 20.0),
            (FloatingFontSize::Larger, 23.0),
            (FloatingFontSize::Large, 26.0),
            (FloatingFontSize::ExtraLarge, 30.0),
        ] {
            assert_eq!(font.row_height(), expected_row);
            let widths = vec![80.0 * expected_row / 20.0; 10];
            let result = layout(&widths, false, AREA, 1.25, font.row_height());
            assert_eq!(
                result.size.1,
                ((expected_row * 2.0 + 6.0) * 1.25).round() as u32
            );
            let small = Area {
                width: 320,
                height: 120,
                ..AREA
            };
            let result = layout(&widths, true, small, 1.5, font.row_height());
            assert!(result.size.0 <= small.width && result.size.1 <= small.height);
            assert!(result.columns[0] + 16.0 <= small.width as f64 / 1.5);
        }
    }

    #[test]
    fn translated_columns_pack_individually_and_keep_the_same_grid_across_readings() {
        let chinese = [62.0, 93.0, 64.0, 95.0, 62.0, 62.0, 92.0, 70.0, 80.0, 80.0];
        let english = [62.0, 94.0, 64.0, 96.0, 80.0, 62.0, 98.0, 68.0, 90.0, 106.0];
        let compact = layout(&chinese, false, AREA, 1.0, 20.0);
        assert_eq!(compact.columns, vec![62.0, 93.0, 70.0, 95.0, 80.0]);
        assert_eq!(compact.size, (406, 46));
        let wider_labels = layout(&english, false, AREA, 1.0, 20.0);
        assert!(wider_labels.size.0 > compact.size.0);
        assert!(wider_labels.size.0 < 546);
        for scale in [1.0, 1.25, 1.5, 2.0, 3.0] {
            let result = layout(&chinese, false, AREA, scale, 20.0);
            assert_eq!(result.columns, compact.columns);
            assert_eq!(result.size.0, (406.0 * scale).round() as u32);
        }
    }

    #[test]
    fn constrained_height_reserves_the_scrollbar_without_covering_value_columns() {
        let area = Area {
            width: 190,
            height: 40,
            ..AREA
        };
        let result = layout(&[85.0; 14], false, area, 1.0, 20.0);
        assert_eq!(result.columns, vec![85.0, 85.0]);
        assert_eq!(result.size, (186, 40));
        assert_eq!(item_width(20.0, 49.0), 78.0);
    }

    const AREA: Area = Area {
        x: -2560,
        y: -400,
        width: 2560,
        height: 1400,
    };

    #[test]
    fn relative_position_survives_dpi_resolution_and_negative_monitor_origins() {
        let mark = Bookmark::default().after_placement(
            "external".into(),
            AREA,
            (400, 100),
            Some((-1480, 250)),
        );
        assert_eq!(mark.restore(AREA, (400, 100)), (-1480, 250));
        let smaller = Area {
            x: 0,
            y: 0,
            width: 1920,
            height: 1040,
        };
        assert_eq!(mark.restore(smaller, (400, 100)), (760, 470));
    }

    #[test]
    fn constrained_layout_retains_the_saved_vertical_anchor() {
        let original = Bookmark {
            monitor: "external".into(),
            horizontal: 8500,
            vertical: 9000,
            ..Default::default()
        };
        let small = Area {
            height: 400,
            ..AREA
        };
        let size = dimensions(10, true, small, 2.0);
        assert_eq!(size.1, small.height);
        let captured = original.after_placement(original.monitor.clone(), small, size, None);
        assert_eq!(captured.vertical, original.vertical);
        let restored_size = dimensions(10, true, AREA, 2.0);
        assert_eq!(
            captured.restore(AREA, restored_size),
            original.restore(AREA, restored_size)
        );
    }

    #[test]
    fn a_drag_updates_only_axes_that_have_room_to_move() {
        let original = Bookmark {
            horizontal: 8500,
            vertical: 9000,
            ..Default::default()
        };
        let horizontal_only = original.after_placement(
            "screen".into(),
            AREA,
            (400, AREA.height),
            Some((-1480, AREA.y)),
        );
        assert_eq!(
            (horizontal_only.horizontal, horizontal_only.vertical),
            (5000, 9000)
        );
        let vertical_only = original.after_placement(
            "screen".into(),
            AREA,
            (AREA.width, 100),
            Some((AREA.x, 250)),
        );
        assert_eq!(
            (vertical_only.horizontal, vertical_only.vertical),
            (8500, 5000)
        );
        let full = original.after_placement(
            "screen".into(),
            AREA,
            (AREA.width, AREA.height),
            Some((AREA.x, AREA.y)),
        );
        assert_eq!((full.horizontal, full.vertical), (8500, 9000));
    }

    #[test]
    fn removed_monitors_and_oversized_windows_recover_inside_the_work_area() {
        assert_eq!(
            Bookmark::default().restore(AREA, (3000, 2000)),
            (-2560, -400)
        );
        assert_eq!(snap(AREA, (400, 100), (500, -900), 12.0), (-400, -400));
        assert_eq!(snap(AREA, (400, 100), (-2551, 500), 12.0), (-2560, 500));
        assert_eq!(snap(AREA, (400, 100), (-405, 899), 12.0), (-400, 900));
    }

    #[test]
    fn disabling_magnetic_snapping_keeps_nearby_positions_but_still_bounds_the_window() {
        assert_eq!(snap(AREA, (400, 100), (-2551, 500), 0.0), (-2551, 500));
        assert_eq!(snap(AREA, (400, 100), (-405, 895), 0.0), (-405, 895));
        assert_eq!(snap(AREA, (400, 100), (500, -900), 0.0), (-400, -400));
        assert_eq!(snap(AREA, (400, 100), (-2551, 500), 12.0), (-2560, 500));
    }

    #[test]
    fn compact_cells_reduce_area_without_changing_hit_targets_for_live_values() {
        assert_eq!(dimensions(7, false, AREA, 1.0), (438, 46));
        assert_eq!(dimensions(10, false, AREA, 1.0), (546, 46));
        assert_eq!(dimensions(7, true, AREA, 1.0), (114, 146));
        for scale in [1.0, 1.25, 1.5, 2.0, 3.0] {
            for count in 0..=40 {
                for vertical in [false, true] {
                    let (width, height) = dimensions(count, vertical, AREA, scale);
                    assert!(width > 0 && width <= AREA.width);
                    assert!(height > 0 && height <= AREA.height);
                }
            }
        }
    }

    #[test]
    fn detail_panels_prefer_free_space_and_remain_inside_small_work_areas() {
        let work = (0.0, 0.0, 1920.0, 1040.0);
        let surface = Area {
            x: 1000,
            y: 900,
            width: 600,
            height: 140,
        };
        assert_eq!(
            detail_position(surface, work, (390.0, 610.0), 8.0),
            (1105.0, 282.0)
        );
        let surface = Area {
            x: 0,
            y: 0,
            width: 200,
            height: 100,
        };
        assert_eq!(
            detail_position(surface, work, (390.0, 610.0), 8.0),
            (0.0, 108.0)
        );
        assert_eq!(
            detail_position(surface, (0.0, 0.0, 300.0, 400.0), (300.0, 400.0), 8.0),
            (0.0, 0.0)
        );
    }

    #[test]
    fn many_metrics_wrap_and_small_high_dpi_work_areas_bound_both_layouts() {
        assert_eq!(dimensions(0, false, AREA, 1.0), (114, 26));
        assert_eq!(dimensions(1, false, AREA, 1.0), (114, 26));
        let tiny = Area {
            x: 0,
            y: 0,
            width: 800,
            height: 600,
        };
        for vertical in [false, true] {
            for count in [0, 1, 4, 7, 40] {
                let (w, h) = dimensions(count, vertical, tiny, 2.0);
                assert!(w <= 800 && h <= 600 && w > 0 && h > 0);
            }
        }
        assert!(!Bookmark {
            horizontal: 10_001,
            ..Default::default()
        }
        .valid());
        assert!(!Bookmark {
            schema_version: 2,
            ..Default::default()
        }
        .valid());
    }
}
