//! Native placement and atomic position persistence for one floating window.
use super::{
    geometry::{self, Area, Bookmark},
    Snapshot,
};
use std::{
    sync::Mutex,
    time::{Duration, Instant},
};
use tauri::Manager;
#[cfg(not(target_os = "linux"))]
use tauri::{PhysicalPosition, PhysicalSize};
pub(super) struct Placement {
    bookmark: Bookmark,
    moved: Option<Instant>,
    applied: Option<(i32, i32)>,
    size: (u32, u32),
    area: Option<Area>,
    columns: Option<Vec<f64>>,
}

impl Placement {
    pub(super) fn load(app: &tauri::AppHandle) -> Self {
        Self {
            bookmark: load_position(app),
            moved: None,
            applied: None,
            size: (0, 0),
            area: None,
            columns: None,
        }
    }
    pub(super) fn finish_drag(&mut self) {
        self.moved = Some(Instant::now());
    }
    pub(super) fn reset(&mut self) {
        self.bookmark = Bookmark::default();
        self.area = None;
        self.moved = None;
    }
    pub(super) fn invalidate_window(&mut self) {
        // A replacement handle needs placement even when its monitor and size are unchanged.
        self.moved = None;
        self.applied = None;
        self.size = (0, 0);
        self.area = None;
        self.columns = None;
    }
    #[cfg(any(windows, test))]
    pub(super) fn is_applied(&self) -> bool {
        self.area.is_some() && self.columns.is_some()
    }
}
pub(super) fn apply(
    window: &tauri::WebviewWindow,
    snapshot: &Snapshot,
    placement: &Mutex<Placement>,
    widths: &[f64],
) -> tauri::Result<Option<Vec<f64>>> {
    // Native move loops own geometry until the pointer is released.
    if super::native::dragging() {
        return Ok(placement
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .columns
            .clone());
    }
    let app = window.app_handle();
    let (bookmark, moved, previous_area, previous_size, previous_point) = {
        let position = placement.lock().unwrap_or_else(|e| e.into_inner());
        (
            position.bookmark.clone(),
            position.moved,
            position.area,
            position.size,
            position.applied,
        )
    };
    if moved.is_some_and(|time| time.elapsed() < Duration::from_millis(350)) {
        return Ok(placement
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .columns
            .clone());
    }
    let monitors = app.available_monitors()?;
    let monitor = if moved.is_some() {
        window.current_monitor()?
    } else {
        None
    }
    .or_else(|| {
        monitors
            .iter()
            .find(|m| m.name().is_some_and(|name| name == &bookmark.monitor))
            .cloned()
    })
    .or(app.primary_monitor()?);
    let Some(monitor) = monitor else {
        return Err(std::io::Error::other("floating monitor work area unavailable").into());
    };
    let work = monitor.work_area();
    let area = Area {
        x: work.position.x,
        y: work.position.y,
        width: work.size.width,
        height: work.size.height,
    };
    let scale = monitor.scale_factor();
    if area.width == 0 || area.height == 0 || !scale.is_finite() || scale <= 0.0 {
        return Err(std::io::Error::other("floating monitor work area or DPI invalid").into());
    }
    let layout = geometry::layout(
        widths,
        snapshot.preferences.layout == crate::resident::preference_schema::FloatingLayout::Vertical,
        area,
        scale,
        snapshot.preferences.font_size.row_height(),
    );
    let size = layout.size;
    let changed = previous_area != Some(area) || previous_size != size;
    #[cfg(target_os = "linux")]
    let remapping = !window.is_visible()?;
    #[cfg(not(target_os = "linux"))]
    let remapping = false;
    let point = if moved.is_some() {
        let point = window.outer_position()?;
        geometry::snap(
            area,
            size,
            (point.x, point.y),
            if snapshot.preferences.snap_to_edges {
                12.0 * scale
            } else {
                0.0
            },
        )
    } else {
        bookmark.restore(area, size)
    };
    // GTK can remap a hidden window at a compositor-selected position even when
    // its size and work area did not change. Reassert the saved anchor first.
    if changed || moved.is_some() || remapping {
        placement.lock().unwrap_or_else(|e| e.into_inner()).applied = Some(point);
        #[cfg(target_os = "linux")]
        super::native::set_bounds(window, point, size)?;
        #[cfg(not(target_os = "linux"))]
        {
            // Move before sizing so a monitor transition cannot use the previous DPI.
            window.set_position(PhysicalPosition::new(point.0, point.1))?;
            #[cfg(windows)]
            crate::resident::windows_frame::refresh(window)?;
            window.set_size(PhysicalSize::new(size.0, size.1))?;
        }
        let captured = bookmark.after_placement(
            monitor.name().cloned().unwrap_or_default(),
            area,
            size,
            moved.map(|_| point),
        );
        {
            let mut position = placement.lock().unwrap_or_else(|e| e.into_inner());
            position.bookmark = captured.clone();
            position.area = Some(area);
            position.size = size;
            position.moved = None;
        }
        if captured != bookmark {
            save_position(app, &captured);
        }
        let previous = previous_point.unwrap_or(point);
        let reason = if moved.is_some() {
            "user_drag"
        } else {
            "geometry"
        };
        log::info!("resident_floating_positioned x={} y={} width={} height={} scale={} reason={} snap_to_edges={} geometry_changed={} previous_x={} previous_y={} work_x={} work_y={} work_width={} work_height={} outcome=clamped", point.0, point.1, size.0, size.1, scale, reason, snapshot.preferences.snap_to_edges, changed, previous.0, previous.1, area.x, area.y, area.width, area.height);
    }
    placement.lock().unwrap_or_else(|e| e.into_inner()).columns = Some(layout.columns.clone());
    Ok(Some(layout.columns))
}

fn load_position(app: &tauri::AppHandle) -> Bookmark {
    let result = (|| -> Result<Option<Bookmark>, Box<dyn std::error::Error>> {
        let path = app.path().app_data_dir()?.join("floating-position.json");
        Ok(crate::resident::preference_file::read(&path)?
            .remove("position")
            .map(serde_json::from_value)
            .transpose()?)
    })();
    match result {
        Ok(Some(bookmark)) if bookmark.valid() => bookmark,
        Ok(None) => Bookmark::default(),
        Ok(Some(_)) => {
            log::warn!("resident_floating_position_load_failed reason=invalid_bookmark outcome=default_position");
            Bookmark::default()
        }
        Err(error) => {
            log::warn!(
                "resident_floating_position_load_failed error={error} outcome=default_position"
            );
            Bookmark::default()
        }
    }
}

fn save_position(app: &tauri::AppHandle, bookmark: &Bookmark) {
    let result = (|| -> Result<(), Box<dyn std::error::Error>> {
        let path = app.path().app_data_dir()?.join("floating-position.json");
        let bytes = serde_json::to_vec(&serde_json::json!({"position": bookmark}))?;
        Ok(crate::resident::preference_file::write(&path, &bytes)?)
    })();
    if let Err(error) = result {
        log::warn!("resident_floating_position_save_failed error={error} outcome=runtime_position_retained");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replacement_window_retains_user_anchor_and_reapplies_native_geometry() {
        let area = Area {
            x: -1920,
            y: 0,
            width: 1920,
            height: 1040,
        };
        let bookmark = Bookmark {
            monitor: "secondary".into(),
            horizontal: 4500,
            vertical: 7000,
            ..Bookmark::default()
        };
        let size = (480, 100);
        let point = bookmark.restore(area, size);
        let mut placement = Placement {
            bookmark: bookmark.clone(),
            moved: Some(Instant::now()),
            applied: Some(point),
            size,
            area: Some(area),
            columns: Some(vec![200.0, 274.0]),
        };
        assert!(placement.is_applied());

        placement.invalidate_window();

        assert!(!placement.is_applied());
        assert_eq!(placement.bookmark, bookmark);
        assert_eq!(placement.bookmark.restore(area, size), point);
        assert_ne!(placement.area, Some(area));
        assert_ne!(placement.size, size);
        assert!(placement.moved.is_none());
        assert!(placement.applied.is_none());
        assert!(placement.columns.is_none());
    }
}
