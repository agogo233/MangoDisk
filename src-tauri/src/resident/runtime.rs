use mangodisk_core::system_resources::{
    metrics::{MetricId, MetricStatus},
    readings::{ResourceCache, ResourceReadings},
};
use serde::Serialize;
use std::{
    sync::{
        atomic::{AtomicBool, AtomicU8, Ordering},
        mpsc::{self, SyncSender},
        Arc, Mutex,
    },
    time::{Duration, Instant},
};
use tauri::{Listener, Manager};

use super::{
    preferences::ResidentPreferences,
    sampling_schedule::{Demand, SamplingSlot},
    sampling_workers::{self, Observation, SamplingEvent},
    TRAY_ID,
};

pub const READING_EVENT: &str = "resident-reading";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResidentReading {
    pub revision: u64,
    #[serde(flatten)]
    pub resources: ResourceReadings,
}

pub struct ResidentState {
    // Serialize native preference transactions and periodic display refreshes.
    // Sampling only reads the short-lived committed-preferences lock below.
    pub preference_update: Mutex<()>,
    pub preferences: Mutex<ResidentPreferences>,
    pub panel_open: AtomicBool,
    pub panel_ready: AtomicBool,
    pub panel_creation: Mutex<()>,
    pub panel_action: Mutex<()>,
    pub panel_requested_at: Mutex<Option<Instant>>,
    pub panel_metric: Mutex<MetricId>,
    pub panel_source: Mutex<String>,
    pub reading: Mutex<ResidentReading>,
    catalogue: AtomicU8,
    wake: SyncSender<SamplingEvent>,
}

impl ResidentState {
    fn disk_activity_demand(&self) -> Demand {
        Demand {
            // I/O history must survive closing the popup and switching tabs.
            // Read lightweight counters while resident mode is enabled; detailed
            // process enumeration remains restricted to the memory panel.
            active: self.enabled(),
            ..Default::default()
        }
    }

    pub fn enabled(&self) -> bool {
        self.preferences
            .lock()
            .map(|value| value.enabled)
            .unwrap_or(false)
    }

    pub fn wake(&self) {
        let _ = self.wake.try_send(SamplingEvent::Wake);
    }

    pub fn request_catalogue(&self) {
        self.catalogue.store(3, Ordering::Relaxed);
        self.wake();
    }

    fn demands(&self, warm_icons: bool) -> [Demand; 4] {
        let preferences = self
            .preferences
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let panel_open = self.panel_open.load(Ordering::Relaxed);
        let selected = *self
            .panel_metric
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let catalogue = self.catalogue.load(Ordering::Relaxed);
        MetricId::ALL.map(|metric| Demand {
            // Display choices only control native entries. Keep lightweight history
            // for every overview metric while resident mode is enabled.
            active: preferences.enabled
                || (metric == MetricId::Network && catalogue & 1 != 0)
                || (metric == MetricId::Disk && catalogue & 2 != 0),
            detailed: metric == MetricId::Memory
                && ((panel_open && selected == MetricId::Memory) || warm_icons),
            selection: match metric {
                MetricId::Network => preferences.network_interface.clone(),
                MetricId::Disk => preferences.disk_volume.clone(),
                _ => None,
            },
        })
    }
}

pub fn start(app: &tauri::AppHandle, preferences: ResidentPreferences) -> Arc<ResidentState> {
    let (sender, events) = mpsc::sync_channel(8);
    let state = Arc::new(ResidentState {
        preference_update: Mutex::new(()),
        preferences: Mutex::new(preferences),
        panel_open: AtomicBool::new(false),
        panel_ready: AtomicBool::new(false),
        panel_creation: Mutex::new(()),
        panel_action: Mutex::new(()),
        panel_requested_at: Mutex::new(None),
        panel_metric: Mutex::new(MetricId::Cpu),
        panel_source: Mutex::new(TRAY_ID.into()),
        reading: Mutex::new(ResidentReading {
            revision: 0,
            resources: ResourceReadings::default(),
        }),
        catalogue: AtomicU8::new(0),
        wake: sender.clone(),
    });
    // Publish state before any worker can call a desktop adapter that looks it up.
    app.manage(state.clone());
    let locale_state = state.clone();
    // Store notifications immediately refresh native labels after locale changes.
    // The listener only queues work: reading the store inside its own mutation
    // callback would contend with the plugin's cache lock.
    app.listen("store://change", move |event| {
        if serde_json::from_str::<serde_json::Value>(event.payload())
            .ok()
            .is_some_and(|value| value.get("key").and_then(|key| key.as_str()) == Some("settings"))
        {
            locale_state.wake();
        }
    });
    let worker = state.clone();
    let app = app.clone();
    std::thread::spawn(move || {
        let presentation = super::presentation::Presentation::start(app.clone(), worker.clone());
        let origin = Instant::now();
        let jobs =
            MetricId::ALL.map(|metric| sampling_workers::start(metric, origin, sender.clone()));
        let mut slots: [SamplingSlot; 4] = std::array::from_fn(|_| SamplingSlot::default());
        let mut cache = ResourceCache::default();
        let disk_activity = super::disk_activity::start(origin, sender.clone());
        let mut disk_slot = SamplingSlot::default();
        let mut disk_status = MetricStatus::Loading;
        let mut warm_icons = worker.enabled();
        let mut previous_status = [MetricStatus::Loading; 4];
        let mut durations = Vec::new();
        let mut summary_at = Instant::now();
        let mut dropped = 0u64;
        let mut cpu_baseline = None;
        let mut cpu_retries = 0u8;
        let mut loop_at = Instant::now();
        let mut was_active = worker.enabled();
        let mut loop_max_ms = 0u128;
        let mut loop_delayed = false;
        let mut display_pending: Option<Instant> = None;
        let mut display_updated = Instant::now() - Duration::from_secs(1);
        loop {
            let loop_ms = loop_at.elapsed().as_millis();
            loop_at = Instant::now();
            loop_max_ms = loop_max_ms.max(loop_ms);
            // An idle disabled service can sleep indefinitely; that is intentional.
            // While active, the coordinator should wake at least once per second.
            let active = worker.enabled();
            let delayed = was_active && active && loop_ms > 2000;
            was_active = active;
            if delayed && !loop_delayed {
                log::warn!("resident_sampling_delayed loop_ms={loop_ms}");
            } else if loop_delayed && !delayed {
                log::info!("resident_sampling_recovered");
            }
            loop_delayed = delayed;
            let demands = worker.demands(warm_icons);
            if disk_slot.update(worker.disk_activity_demand()) {
                cache.stop_disk_io();
                log::info!("resident_disk_io_demand active={}", disk_slot.demand.active);
            }
            if let Some(generation) = disk_slot.begin(origin.elapsed().as_millis() as u64, 2000) {
                if disk_activity.send(generation).is_err() {
                    return;
                }
            }
            for (index, metric) in MetricId::ALL.into_iter().enumerate() {
                let selection_changed = slots[index].demand.selection != demands[index].selection;
                if slots[index].update(demands[index].clone()) {
                    if selection_changed {
                        cache.reset(metric);
                    } else if !demands[index].active {
                        cache.suspend(metric);
                    }
                }
                if let Some(generation) =
                    slots[index].begin(origin.elapsed().as_millis() as u64, metric.interval_ms())
                {
                    if jobs[index]
                        .send(sampling_workers::Request {
                            generation,
                            demand: demands[index].clone(),
                        })
                        .is_err()
                    {
                        return;
                    }
                }
            }
            let event = if demands.iter().any(|demand| demand.active)
                || disk_slot.demand.active
                || display_pending.is_some()
            {
                match events.recv_timeout(Duration::from_millis(
                    slots
                        .iter()
                        .map(|slot| slot.wait_ms(origin.elapsed().as_millis() as u64))
                        .min()
                        .unwrap_or(1000)
                        .min(disk_slot.wait_ms(origin.elapsed().as_millis() as u64))
                        .min(display_pending.map_or(1000, |deadline| {
                            (deadline
                                .saturating_duration_since(Instant::now())
                                .as_millis() as u64)
                                .max(1)
                        })),
                )) {
                    Ok(event) => Some(event),
                    Err(mpsc::RecvTimeoutError::Timeout) => None,
                    Err(_) => break,
                }
            } else {
                match events.recv() {
                    Ok(event) => Some(event),
                    Err(_) => break,
                }
            };
            if matches!(event, Some(SamplingEvent::Wake)) {
                presentation.display();
                display_updated = Instant::now();
                display_pending = None;
            }
            let mut changed = false;
            let event = match event {
                Some(SamplingEvent::DiskIo(completion)) => {
                    if disk_slot.update(worker.disk_activity_demand()) {
                        cache.stop_disk_io();
                        log::info!("resident_disk_io_demand active={}", disk_slot.demand.active);
                    }
                    if disk_slot.complete(completion.generation) {
                        changed = true;
                        durations.push(completion.duration_ms);
                        if durations.len() > 1200 {
                            durations.remove(0);
                        }
                        if completion.duration_ms > 5000 {
                            cache.fail_disk_io(MetricStatus::Stale);
                        } else {
                            match completion.result {
                                Ok(devices) => cache.disk_io(
                                    devices,
                                    completion.monotonic_ms,
                                    completion.timestamp_ms,
                                ),
                                Err(error) => {
                                    if disk_status != MetricStatus::Unsupported {
                                        log::warn!(
                                            "resident_disk_io_unavailable code={:?}",
                                            error.code()
                                        );
                                    }
                                    cache.fail_disk_io(MetricStatus::Unsupported);
                                }
                            }
                        }
                    } else {
                        dropped += 1;
                    }
                    None
                }
                event => event,
            };
            if let Some(SamplingEvent::Completed(completion)) = event {
                let index = MetricId::ALL
                    .iter()
                    .position(|metric| *metric == completion.metric)
                    .expect("known metric");
                // Reconcile demand before accepting a completion: disable/selection
                // commands may have raced the queued wake and this native result.
                let demand = worker.demands(warm_icons)[index].clone();
                let selection_changed = slots[index].demand.selection != demand.selection;
                if slots[index].update(demand) {
                    if selection_changed {
                        cache.reset(completion.metric);
                    } else if !slots[index].demand.active {
                        cache.suspend(completion.metric);
                    }
                }
                if slots[index].complete(completion.generation) {
                    changed = true;
                    durations.push(completion.duration_ms);
                    if durations.len() > 1200 {
                        durations.remove(0);
                    }
                    if completion.duration_ms > completion.metric.freshness_ms() {
                        if previous_status[index] != MetricStatus::Stale {
                            log::warn!(
                                "resident_sample_expired metric={:?} duration_ms={}",
                                completion.metric,
                                completion.duration_ms
                            );
                        }
                        cache.fail(completion.metric, MetricStatus::Stale);
                        dropped += 1;
                    } else {
                        match completion.result {
                            Ok(Observation::Cpu(counters)) => {
                                if let Some(reason) = cache.cpu(
                                    counters,
                                    completion.monotonic_ms,
                                    completion.timestamp_ms,
                                ) {
                                    if cpu_baseline != Some(reason) {
                                        log::info!(
                                            "resident_cpu_baseline reason={:?} query_ms={}",
                                            reason,
                                            completion.duration_ms
                                        );
                                    }
                                    cpu_baseline = Some(reason);
                                    // At most two quick retries per recovery episode. Native
                                    // jobs remain serialized and persistent failures return to
                                    // normal cadence rather than creating a tight polling loop.
                                    if cpu_retries < 2 {
                                        cpu_retries += 1;
                                        slots[index]
                                            .retry_after(origin.elapsed().as_millis() as u64, 250);
                                    }
                                } else {
                                    if cpu_baseline.take().is_some() {
                                        log::info!(
                                            "resident_cpu_recovered quick_retries={cpu_retries}"
                                        );
                                    }
                                    cpu_retries = 0;
                                }
                            }
                            Ok(Observation::Memory(snapshot)) => {
                                cache.memory(snapshot.clone());
                                if warm_icons {
                                    if let Some(summary) = snapshot.processes {
                                        let app = app.clone();
                                        // Icon I/O has its own bounded one-time warm-up;
                                        // it cannot block either sample publication or the memory worker.
                                        std::thread::spawn(move || {
                                            super::application_icons::warm(&app, &summary)
                                        });
                                    }
                                    warm_icons = false;
                                }
                            }
                            Ok(Observation::Network(interfaces)) => {
                                cache.network(
                                    interfaces,
                                    slots[index].demand.selection.as_deref(),
                                    completion.monotonic_ms,
                                    completion.timestamp_ms,
                                );
                                worker.catalogue.fetch_and(!1, Ordering::Relaxed);
                            }
                            Ok(Observation::Disk { volumes, selected }) => {
                                cache.volumes(volumes);
                                if let Some((volume, capacity)) = selected {
                                    cache.disk(volume, capacity, completion.timestamp_ms);
                                } else {
                                    cache.fail(MetricId::Disk, MetricStatus::Disconnected);
                                }
                                worker.catalogue.fetch_and(!2, Ordering::Relaxed);
                            }
                            Ok(Observation::Unsupported) => {
                                cache.fail(completion.metric, MetricStatus::Unsupported);
                            }
                            Err(error) => {
                                if completion.metric == MetricId::Memory {
                                    warm_icons = false;
                                }
                                if previous_status[index] != MetricStatus::Failed {
                                    log::warn!(
                                        "resident_sample_failed metric={:?} code={:?}",
                                        completion.metric,
                                        error.code()
                                    );
                                }
                                cache.fail(completion.metric, MetricStatus::Failed);
                                if completion.metric == MetricId::Network {
                                    worker.catalogue.fetch_and(!1, Ordering::Relaxed);
                                }
                                if completion.metric == MetricId::Disk {
                                    worker.catalogue.fetch_and(!2, Ordering::Relaxed);
                                }
                            }
                        }
                    }
                } else {
                    dropped += 1;
                }
            }
            let resources = cache.snapshot(sampling_workers::timestamp_ms());
            if resources.disk_io.status != disk_status {
                changed = true;
                log::info!(
                    "resident_disk_io_state from={:?} to={:?} scope=all_devices",
                    disk_status,
                    resources.disk_io.status
                );
                disk_status = resources.disk_io.status;
            }
            let statuses = [
                resources.cpu.status,
                resources.memory.status,
                resources.network.status,
                resources.disk.status,
            ];
            for index in 0..4 {
                if statuses[index] != previous_status[index] {
                    changed = true;
                    log::info!(
                        "resident_metric_state metric={:?} from={:?} to={:?}",
                        MetricId::ALL[index],
                        previous_status[index],
                        statuses[index]
                    );
                }
            }
            previous_status = statuses;
            if changed {
                {
                    let mut reading = worker
                        .reading
                        .lock()
                        .unwrap_or_else(|error| error.into_inner());
                    if resources.network.status == MetricStatus::Ready {
                        if let Some(network) = &resources.network.value {
                            if reading
                                .resources
                                .network
                                .value
                                .as_ref()
                                .is_none_or(|previous| {
                                    previous.interface.id != network.interface.id
                                })
                            {
                                log::info!(
                                    "resident_network_selected reason={:?}",
                                    network.selection_reason
                                );
                            }
                        }
                    }
                    reading.revision += 1;
                    reading.resources = resources;
                }
                // Independent workers commonly finish within a few milliseconds.
                // Native status-bar layout is expensive. Limit periodic display
                // refreshes to once per second without delaying IPC readings or
                // preference changes; combine independent completion bursts.
                display_pending.get_or_insert_with(|| {
                    (Instant::now() + Duration::from_millis(50))
                        .max(display_updated + Duration::from_secs(1))
                });
                presentation.reading();
            }
            if display_pending.is_some_and(|deadline| Instant::now() >= deadline) {
                presentation.display();
                display_updated = Instant::now();
                display_pending = None;
            }
            if summary_at.elapsed() >= Duration::from_secs(300) {
                durations.sort_unstable();
                let percentile = |percent: usize| {
                    durations
                        .get(durations.len().saturating_sub(1) * percent / 100)
                        .copied()
                        .unwrap_or(0)
                };
                let renders = app
                    .state::<Mutex<super::tray_display::DisplayState>>()
                    .try_lock()
                    .ok()
                    .map(|state| state.renders);
                let handles = super::tray_display::format::DisplayId::ALL
                    .iter()
                    .filter(|id| app.tray_by_id(id.tray_id()).is_some())
                    .count();
                log::info!("resident_sample_summary samples={} p50_ms={} p95_ms={} discarded={} tray_renders={:?} tray_handles={} loop_max_ms={loop_max_ms}", durations.len(), percentile(50), percentile(95), dropped, renders, handles);
                loop_max_ms = 0;
                durations.clear();
                dropped = 0;
                summary_at = Instant::now();
            }
        }
    });
    state
}

#[cfg(test)]
mod overview_tests {
    use super::*;

    fn test_state() -> ResidentState {
        let (wake, _) = mpsc::sync_channel(1);
        let mut preferences = ResidentPreferences::default();
        for metric in &mut preferences.metrics {
            metric.enabled = false;
        }
        ResidentState {
            preference_update: Mutex::new(()),
            preferences: Mutex::new(preferences),
            panel_open: AtomicBool::new(true),
            panel_ready: AtomicBool::new(true),
            panel_creation: Mutex::new(()),
            panel_action: Mutex::new(()),
            panel_requested_at: Mutex::new(None),
            panel_metric: Mutex::new(MetricId::Cpu),
            panel_source: Mutex::new(String::new()),
            reading: Mutex::new(ResidentReading {
                revision: 0,
                resources: ResourceReadings::default(),
            }),
            catalogue: AtomicU8::new(0),
            wake,
        }
    }

    #[test]
    fn stalled_preference_update_does_not_block_sampling_demand() {
        let state = Arc::new(test_state());
        let update = state.preference_update.lock().unwrap();
        let sampler = state.clone();
        let (sampled, samples) = mpsc::channel();
        let (committed, commit) = mpsc::channel();
        let thread = std::thread::spawn(move || {
            sampled
                .send((sampler.demands(false), sampler.disk_activity_demand()))
                .unwrap();
            commit.recv().unwrap();
            (sampler.demands(false), sampler.disk_activity_demand())
        });
        // Keep the same gate held by native application/save/rollback. The
        // sampler must finish before it is released, even with every entry off.
        // Release before asserting so a regression cannot strand a test thread.
        let before = samples.recv_timeout(Duration::from_secs(2));
        state.preferences.lock().unwrap().enabled = false;
        drop(update);
        committed.send(()).unwrap();
        let (after, disk_after) = thread.join().unwrap();
        let (before, disk_before) = before.expect("sampling waited for the preference transaction");
        assert!(before
            .iter()
            .all(|demand| demand.active && !demand.detailed));
        assert!(disk_before.active);
        assert!(after.iter().all(|demand| !demand.active));
        assert!(!disk_after.active);
    }

    #[test]
    fn overview_limits_process_work_and_keeps_resident_disk_history() {
        let state = test_state();
        for selected in [MetricId::Cpu, MetricId::Network, MetricId::Disk] {
            *state.panel_metric.lock().unwrap() = selected;
            assert!(state.disk_activity_demand().active);
            assert!(state
                .demands(false)
                .iter()
                .all(|demand| demand.active && !demand.detailed));
        }
        *state.panel_metric.lock().unwrap() = MetricId::Memory;
        assert!(state.disk_activity_demand().active);
        for (metric, demand) in MetricId::ALL.into_iter().zip(state.demands(false)) {
            assert!(demand.active);
            assert_eq!(demand.detailed, metric == MetricId::Memory);
        }
        state.panel_open.store(false, Ordering::Relaxed);
        assert!(state.disk_activity_demand().active);
        assert!(state
            .demands(false)
            .iter()
            .all(|demand| demand.active && !demand.detailed));
        state.preferences.lock().unwrap().enabled = false;
        assert!(!state.disk_activity_demand().active);
        assert!(state
            .demands(false)
            .iter()
            .all(|demand| !demand.active && !demand.detailed));
    }
}
