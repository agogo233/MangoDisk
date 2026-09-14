//! Coalesce presentation requests without making sampling wait for native UI calls.
use super::runtime::{ResidentState, READING_EVENT};
use std::sync::{
    atomic::{AtomicU8, Ordering},
    mpsc::{self, SyncSender},
    Arc,
};
use tauri::{Emitter, Manager};

const READING: u8 = 1;
const DISPLAY: u8 = 2;

pub struct Presentation {
    pending: Arc<AtomicU8>,
    wake: SyncSender<()>,
}

impl Presentation {
    pub fn start(app: tauri::AppHandle, state: Arc<ResidentState>) -> Self {
        let pending = Arc::new(AtomicU8::new(0));
        let requests = pending.clone();
        let (wake, events) = mpsc::sync_channel(1);
        std::thread::spawn(move || {
            let mut stalled = false;
            while events.recv().is_ok() {
                let flags = requests.swap(0, Ordering::AcqRel);
                if flags == 0 {
                    continue;
                }
                let started = std::time::Instant::now();
                // Fetch only the latest snapshot. A blocked shell/WebView must not
                // retain a queue of obsolete histories or hold the sampling lock.
                let reading = state
                    .reading
                    .lock()
                    .unwrap_or_else(|error| error.into_inner())
                    .clone();
                if flags & READING != 0 {
                    if state.panel_open.load(Ordering::Relaxed) {
                        let _ = app.emit_to(super::PANEL_LABEL, READING_EVENT, &reading);
                    }
                    if app
                        .get_webview_window(crate::MAIN_WINDOW_LABEL)
                        .is_some_and(|window| window.is_visible().unwrap_or(false))
                    {
                        let _ = app.emit_to(crate::MAIN_WINDOW_LABEL, READING_EVENT, &reading);
                    }
                }
                if flags & DISPLAY != 0 {
                    // Wait outside the sampling locks. Read settings and values
                    // only after any in-progress commit/rollback has completed.
                    let _update = state
                        .preference_update
                        .lock()
                        .unwrap_or_else(|error| error.into_inner());
                    let preferences = state
                        .preferences
                        .lock()
                        .unwrap_or_else(|error| error.into_inner())
                        .clone();
                    let reading = state
                        .reading
                        .lock()
                        .unwrap_or_else(|error| error.into_inner())
                        .clone();
                    super::tray_display::refresh(&app, &preferences, &reading);
                }
                let elapsed_ms = started.elapsed().as_millis();
                let slow = elapsed_ms > 1000;
                // Report transitions, not every frame during a persistent shell stall.
                if slow && !stalled {
                    log::warn!(
                        "resident_presentation_delayed elapsed_ms={elapsed_ms} flags={flags}"
                    );
                } else if stalled && !slow {
                    log::info!("resident_presentation_recovered elapsed_ms={elapsed_ms}");
                }
                stalled = slow;
            }
        });
        Self { pending, wake }
    }

    fn request(&self, flags: u8) {
        // Flags survive a full wake channel. Swapping them before UI work ensures
        // requests arriving during that work are processed on the next wake.
        self.pending.fetch_or(flags, Ordering::Release);
        let _ = self.wake.try_send(());
    }

    pub fn reading(&self) {
        self.request(READING);
    }

    pub fn display(&self) {
        self.request(DISPLAY);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stalled_presentation_keeps_latest_work_without_blocking_or_losing_flags() {
        let pending = Arc::new(AtomicU8::new(0));
        let (wake, events) = mpsc::sync_channel(1);
        let presentation = Presentation { pending, wake };
        for _ in 0..10_000 {
            presentation.reading();
            presentation.display();
        }
        events.try_recv().unwrap();
        assert!(events.try_recv().is_err());
        assert_eq!(
            presentation.pending.swap(0, Ordering::AcqRel),
            READING | DISPLAY
        );
        // Simulate a sample arriving while the previous native update is blocked.
        presentation.reading();
        events.try_recv().unwrap();
        assert_eq!(presentation.pending.swap(0, Ordering::AcqRel), READING);
    }
}
