//! Desktop lifecycle adapter for Core's per-interface traffic history.
use std::{sync::Arc, time::Duration};

use mangodisk_core::{system_resources::network_traffic::NetworkTraffic, CoreError, CoreResult};
use tauri::Manager;

use super::runtime::{ResidentReading, ResidentState};

pub fn install(app: &tauri::AppHandle, state: &ResidentState) -> Option<Arc<NetworkTraffic>> {
    let result = app
        .path()
        .app_data_dir()
        .map_err(|error| CoreError::persistence(error.to_string()))
        .and_then(|path| NetworkTraffic::open(path.join("network-traffic.json")));
    match result {
        Ok(traffic) => {
            let traffic = Arc::new(traffic);
            *state
                .network_traffic
                .lock()
                .unwrap_or_else(|error| error.into_inner()) = Some(traffic.clone());
            let weak = Arc::downgrade(&traffic);
            std::thread::spawn(move || {
                let mut failed = false;
                loop {
                    std::thread::sleep(Duration::from_secs(60));
                    let Some(traffic) = weak.upgrade() else {
                        break;
                    };
                    match traffic.flush(false) {
                        Ok(_) => {
                            if failed {
                                log::info!("network_traffic_save_recovered");
                            }
                            failed = false;
                        }
                        Err(error) => {
                            if !failed {
                                log::warn!(
                                    "network_traffic_save_failed error={}",
                                    mangodisk_platform::diagnostics::text(&error)
                                );
                            }
                            failed = true;
                        }
                    }
                }
            });
            log::info!("network_traffic_loaded save_interval_seconds=60");
            Some(traffic)
        }
        Err(error) => {
            log::warn!(
                "network_traffic_load_failed outcome=history_preserved error={}",
                mangodisk_platform::diagnostics::text(&error)
            );
            None
        }
    }
}

pub fn reset(
    state: &ResidentState,
    interface_id: &str,
    started_at_ms: u64,
) -> CoreResult<ResidentReading> {
    let traffic = state
        .network_traffic
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .clone()
        .ok_or_else(|| CoreError::persistence("network traffic history is unavailable"))?;
    // Bind the confirmation to the interface that was displayed, including when
    // preferences changed while the dialog or native command was pending.
    let current = state
        .reading
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .resources
        .network
        .value
        .as_ref()
        .map(|value| value.interface.id.clone());
    if current.as_deref() != Some(interface_id) {
        return Err(CoreError::operation_busy(
            "network traffic selection changed",
        ));
    }
    let total = traffic.reset(
        interface_id,
        started_at_ms,
        super::sampling_workers::timestamp_ms(),
    )?;
    let mut reading = state
        .reading
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    if let Some(network) = reading
        .resources
        .network
        .value
        .as_mut()
        .filter(|value| value.interface.id == interface_id)
    {
        network.cumulative = Some(total);
    }
    reading.revision += 1;
    let reading = reading.clone();
    state.wake();
    log::info!("network_traffic_reset scope=displayed_interface outcome=saved");
    Ok(reading)
}

pub fn close(app: &tauri::AppHandle) {
    let traffic = app.try_state::<Arc<ResidentState>>().and_then(|state| {
        state
            .network_traffic
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .clone()
    });
    if let Some(traffic) = traffic {
        match traffic.flush(true) {
            Ok(written) => log::info!("network_traffic_closed written={written}"),
            Err(error) => log::warn!(
                "network_traffic_close_failed error={}",
                mangodisk_platform::diagnostics::text(&error)
            ),
        }
    }
}
