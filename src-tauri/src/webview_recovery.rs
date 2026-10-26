//! Recover native Windows views without restarting Core operations or repeating user actions.
use std::time::{Duration, Instant};

const RECOVERY_WINDOW: Duration = Duration::from_secs(300);
const MAX_RECOVERIES: usize = 2;
const MAX_UNRESPONSIVE_OBSERVATION_GAP: Duration = Duration::from_secs(30);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RecoveryAction {
    Observe,
    Wait,
    Reload,
    Recreate,
    Exhausted,
    Suppressed,
}

#[derive(Default)]
struct RecoveryBudget {
    attempts: std::collections::VecDeque<Instant>,
    pending: bool,
    unresponsive_since: Option<(Instant, Instant)>,
    notified: bool,
}

impl RecoveryBudget {
    fn decide(&mut self, kind: i32, now: Instant) -> RecoveryAction {
        let action = match kind {
            0 => RecoveryAction::Recreate,
            1 => RecoveryAction::Reload,
            2 => {
                // WebView2 repeats this event every few seconds while stalled, but
                // does not report responsiveness. Expire isolated observations so
                // a later, unrelated stall receives its own grace period.
                let (since, last) = self.unresponsive_since.get_or_insert((now, now));
                if now.duration_since(*last) > MAX_UNRESPONSIVE_OBSERVATION_GAP {
                    *since = now;
                }
                *last = now;
                if now.duration_since(*since) < Duration::from_secs(15) {
                    return RecoveryAction::Wait;
                }
                RecoveryAction::Reload
            }
            // GPU and utility failures are recovered by WebView2 itself. Subframe
            // failures must not discard an otherwise healthy main document.
            _ => return RecoveryAction::Observe,
        };
        self.attempts
            .retain(|time| now.duration_since(*time) < RECOVERY_WINDOW);
        if self.pending {
            return RecoveryAction::Suppressed;
        }
        if self.attempts.len() >= MAX_RECOVERIES {
            if self.notified {
                return RecoveryAction::Suppressed;
            }
            self.notified = true;
            return RecoveryAction::Exhausted;
        }
        self.notified = false;
        self.attempts.push_back(now);
        self.pending = true;
        action
    }

    fn finish(&mut self) {
        self.pending = false;
        self.unresponsive_since = None;
    }
}

#[cfg(windows)]
pub(crate) use native::{install, RecoveryState};

#[cfg(windows)]
mod native {
    use std::{
        collections::HashMap,
        sync::{
            atomic::{AtomicBool, Ordering},
            Mutex,
        },
    };
    use tauri::Manager;
    use webview2_com::{
        Microsoft::Web::WebView2::Win32::*, NavigationCompletedEventHandler,
        ProcessFailedEventHandler,
    };
    use windows::core::Interface;

    use super::*;

    #[derive(Default)]
    pub(crate) struct RecoveryState {
        windows: Mutex<HashMap<String, RecoveryBudget>>,
        browser: Mutex<RecoveryBudget>,
        notice_open: AtomicBool,
    }

    impl RecoveryState {
        pub(crate) fn recreating(&self) -> bool {
            self.browser
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .pending
        }
    }

    pub(crate) fn install(window: &tauri::WebviewWindow) {
        let app = window.app_handle().clone();
        let label = window.label().to_owned();
        let install_label = label.clone();
        if let Err(error) = window.with_webview(move |native| {
            // Tauri invokes this closure on the owning COM apartment. Event handlers
            // are retained by WebView2 and released when its controller is closed.
            let result = unsafe {
                (|| -> windows::core::Result<()> {
                    let view = native.controller().CoreWebView2()?;
                    let mut pid = 0;
                    view.BrowserProcessId(&mut pid)?;
                    let failed_app = app.clone();
                    let failed_label = label.clone();
                    let mut token = 0;
                    view.add_ProcessFailed(
                        &ProcessFailedEventHandler::create(Box::new(move |_, args| {
                            let Some(args) = args else { return Ok(()); };
                            let mut kind = COREWEBVIEW2_PROCESS_FAILED_KIND::default();
                            args.ProcessFailedKind(&mut kind)?;
                            let details = args.cast::<ICoreWebView2ProcessFailedEventArgs2>().ok();
                            let reason = details.as_ref().and_then(|args| {
                                let mut value = COREWEBVIEW2_PROCESS_FAILED_REASON::default();
                                args.Reason(&mut value).ok().map(|()| value.0)
                            });
                            let exit_code = details.as_ref().and_then(|args| {
                                let mut value = 0;
                                args.ExitCode(&mut value).ok().map(|()| value)
                            });
                            let state = failed_app.state::<RecoveryState>();
                            let requested_at = Instant::now();
                            let action = if kind == COREWEBVIEW2_PROCESS_FAILED_KIND_BROWSER_PROCESS_EXITED {
                                state.browser.lock().unwrap_or_else(|e| e.into_inner()).decide(kind.0, requested_at)
                            } else {
                                state.windows.lock().unwrap_or_else(|e| e.into_inner())
                                    .entry(failed_label.clone()).or_default().decide(kind.0, requested_at)
                            };
                            log::warn!("webview_process_failed label={} browser_pid={} kind={} reason={:?} exit_code={:?} action={:?}", failed_label, pid, kind.0, reason, exit_code, action);
                            if matches!(action, RecoveryAction::Reload | RecoveryAction::Recreate) {
                                schedule(failed_app.clone(), failed_label.clone(), action, requested_at);
                            } else if action == RecoveryAction::Exhausted {
                                let app = failed_app.clone();
                                let label = failed_label.clone();
                                tauri::async_runtime::spawn_blocking(move || recovery_failed(&app, &label, "retry_budget", "automatic recovery limit reached"));
                            }
                            Ok(())
                        })), &mut token,
                    )?;
                    let ready_app = app.clone();
                    let ready_label = label.clone();
                    view.add_NavigationCompleted(
                        &NavigationCompletedEventHandler::create(Box::new(move |_, args| {
                            let Some(args) = args else { return Ok(()); };
                            let mut succeeded = windows::core::BOOL::default();
                            args.IsSuccess(&mut succeeded)?;
                            let state = ready_app.state::<RecoveryState>();
                            let mut windows = state.windows.lock().unwrap_or_else(|e| e.into_inner());
                            let recovering = windows.get_mut(&ready_label).is_some_and(|budget| {
                                let pending = budget.pending;
                                if pending { budget.finish(); }
                                pending
                            });
                            drop(windows);
                            if recovering {
                                log::info!("webview_recovery_navigation_finished label={} success={}", ready_label, succeeded.as_bool());
                                if !succeeded.as_bool() {
                                    let app = ready_app.clone();
                                    let label = ready_label.clone();
                                    tauri::async_runtime::spawn_blocking(move || recovery_failed(&app, &label, "navigation", "reloaded navigation failed"));
                                }
                            }
                            Ok(())
                        })), &mut token,
                    )?;
                    log::info!("webview_process_monitor_installed label={} browser_pid={}", label, pid);
                    Ok(())
                })()
            };
            if let Err(error) = result {
                log::warn!("webview_process_monitor_failed label={} native_code={} error={}", label, error.code().0, mangodisk_platform::diagnostics::text(&error.to_string()));
            }
        }) {
            log::warn!("webview_process_monitor_dispatch_failed label={} error={}", install_label, mangodisk_platform::diagnostics::text(&error.to_string()));
        }
    }

    fn schedule(
        app: tauri::AppHandle,
        label: String,
        action: RecoveryAction,
        requested_at: Instant,
    ) {
        // Construction and destruction must leave the native event callback before
        // WebView2 pumps messages. Core jobs and authorization sessions stay alive.
        tauri::async_runtime::spawn_blocking(move || {
            std::thread::sleep(Duration::from_millis(250));
            if action == RecoveryAction::Recreate {
                let result = recreate_windows(&app);
                app.state::<RecoveryState>()
                    .browser
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .finish();
                if let Err(error) = result {
                    recovery_failed(&app, &label, "recreate", &error);
                }
                return;
            }
            let Some(window) = app.get_webview_window(&label) else {
                app.state::<RecoveryState>()
                    .windows
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .entry(label.clone())
                    .or_default()
                    .finish();
                log::info!(
                    "webview_recovery_cancelled label={} reason=window_closed",
                    label
                );
                return;
            };
            let visible = window.is_visible().unwrap_or(false);
            if label == crate::MAIN_WINDOW_LABEL {
                crate::resident::main_window::prepare_webview_recovery(&app, visible);
            }
            let reload_app = app.clone();
            let reload_label = label.clone();
            if let Err(error) = window.with_webview(move |native| {
                // SAFETY: Tauri supplies the controller on its owning UI thread.
                let result = unsafe {
                    native
                        .controller()
                        .CoreWebView2()
                        .and_then(|view| view.Reload())
                };
                match result {
                    Ok(()) => {
                        log::info!("webview_recovery_reload_requested label={}", reload_label)
                    }
                    Err(error) => {
                        reload_app
                            .state::<RecoveryState>()
                            .windows
                            .lock()
                            .unwrap_or_else(|e| e.into_inner())
                            .entry(reload_label.clone())
                            .or_default()
                            .finish();
                        let message = error.to_string();
                        tauri::async_runtime::spawn_blocking(move || {
                            recovery_failed(&reload_app, &reload_label, "reload", &message)
                        });
                    }
                }
            }) {
                recovery_failed(&app, &label, "dispatch_reload", &error);
            }
            // Navigation completion confirms the result. A missing callback must
            // not leave the window permanently marked as recovering.
            std::thread::sleep(Duration::from_secs(15));
            let state = app.state::<RecoveryState>();
            let mut windows = state.windows.lock().unwrap_or_else(|e| e.into_inner());
            let pending = windows.get_mut(&label).is_some_and(|budget| {
                let pending = budget.pending && budget.attempts.back() == Some(&requested_at);
                if pending {
                    budget.finish();
                }
                pending
            });
            drop(windows);
            if pending {
                recovery_failed(
                    &app,
                    &label,
                    "navigation_timeout",
                    "navigation did not complete within 15 seconds",
                );
            }
        });
    }

    fn recreate_windows(app: &tauri::AppHandle) -> tauri::Result<()> {
        let windows = app.webview_windows();
        let mut restore = Vec::new();
        for (label, window) in &windows {
            let visible = window.is_visible().unwrap_or(false);
            let requested = if label == crate::MAIN_WINDOW_LABEL {
                crate::resident::main_window::recovery_requested(app, visible)
            } else {
                visible
            };
            restore.push((label.clone(), requested));
            window.destroy()?;
        }
        drop(windows);
        restore.sort_by_key(|(label, _)| match label.as_str() {
            crate::MAIN_WINDOW_LABEL => 0,
            crate::resident::floating_monitor::LABEL => 1,
            _ => 2,
        });
        // Destroy dispatches native events. Wait for Tauri to remove every old
        // controller before constructing controls that share the browser process.
        let deadline = Instant::now() + Duration::from_secs(5);
        while restore
            .iter()
            .any(|(label, _)| app.get_webview_window(label).is_some())
        {
            if Instant::now() >= deadline {
                return Err(std::io::Error::other("old WebView2 windows did not close").into());
            }
            std::thread::sleep(Duration::from_millis(25));
        }
        for (label, visible) in restore {
            match label.as_str() {
                crate::MAIN_WINDOW_LABEL => {
                    crate::resident::main_window::prepare_webview_recovery(app, visible);
                    if visible {
                        crate::resident::main_window::open(
                            app,
                            crate::resident::main_window::Destination::Main,
                            "webview_recovery",
                        )?;
                    }
                }
                crate::resident::PANEL_LABEL => {
                    crate::resident::panel::recreate_webview(app, visible)?
                }
                crate::resident::floating_monitor::LABEL => {
                    crate::resident::floating_monitor::recreate_webview(app)?;
                }
                crate::resident::memory_window::LABEL => {
                    if visible {
                        crate::resident::memory_window::open(app)?;
                    }
                }
                _ => {
                    log::warn!(
                        "webview_recovery_window_skipped label={} reason=unknown_window",
                        mangodisk_platform::diagnostics::text(&label)
                    );
                    continue;
                }
            }
            log::info!(
                "webview_recovery_window_recreated label={} visible={}",
                label,
                visible
            );
        }
        Ok(())
    }

    fn recovery_failed(
        app: &tauri::AppHandle,
        label: &str,
        stage: &str,
        error: &(impl std::fmt::Display + ?Sized),
    ) {
        use tauri_plugin_dialog::{DialogExt, MessageDialogButtons, MessageDialogKind};
        log::error!(
            "webview_recovery_failed label={} stage={} error={}",
            label,
            stage,
            mangodisk_platform::diagnostics::text(&error.to_string())
        );
        // A shared fault can affect several controllers. Keep every diagnostic,
        // but show one native notice until the user dismisses it.
        if app
            .state::<RecoveryState>()
            .notice_open
            .swap(true, Ordering::AcqRel)
        {
            log::info!(
                "webview_recovery_notice_suppressed label={} reason=already_open",
                label
            );
            return;
        }
        let labels = crate::services::native_labels::NativeLabels::load(app);
        let notice_app = app.clone();
        app.dialog()
            .message(labels.text("/webviewRuntime/recoveryFailed"))
            .title("MangoDisk")
            .kind(MessageDialogKind::Warning)
            .buttons(MessageDialogButtons::Ok)
            .show(move |_| {
                notice_app
                    .state::<RecoveryState>()
                    .notice_open
                    .store(false, Ordering::Release);
            });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recovery_deduplicates_events_and_bounds_crash_loops() {
        let now = Instant::now();
        let mut budget = RecoveryBudget::default();
        assert_eq!(budget.decide(0, now), RecoveryAction::Recreate);
        assert_eq!(budget.decide(0, now), RecoveryAction::Suppressed);
        budget.finish();
        assert_eq!(budget.decide(1, now), RecoveryAction::Reload);
        budget.finish();
        assert_eq!(
            budget.decide(1, now + Duration::from_secs(299)),
            RecoveryAction::Exhausted
        );
        assert_eq!(
            budget.decide(1, now + RECOVERY_WINDOW),
            RecoveryAction::Reload
        );
    }

    #[test]
    fn temporary_stalls_and_auto_recovered_processes_do_not_reload() {
        let now = Instant::now();
        let mut budget = RecoveryBudget::default();
        for kind in [3, 4, 5, 6, 7, 8, 9, 99] {
            assert_eq!(budget.decide(kind, now), RecoveryAction::Observe);
        }
        assert_eq!(budget.decide(2, now), RecoveryAction::Wait);
        assert_eq!(
            budget.decide(2, now + Duration::from_secs(14)),
            RecoveryAction::Wait
        );
        assert_eq!(
            budget.decide(2, now + Duration::from_secs(15)),
            RecoveryAction::Reload
        );
        budget.finish();
        assert_eq!(
            budget.decide(2, now + Duration::from_secs(16)),
            RecoveryAction::Wait
        );
    }

    #[test]
    fn isolated_stalls_do_not_reuse_an_old_unresponsive_observation() {
        let now = Instant::now();
        let mut budget = RecoveryBudget::default();
        assert_eq!(budget.decide(2, now), RecoveryAction::Wait);
        let later = now + Duration::from_secs(120);
        assert_eq!(budget.decide(2, later), RecoveryAction::Wait);
        assert_eq!(
            budget.decide(2, later + Duration::from_secs(15)),
            RecoveryAction::Reload
        );
    }
}
