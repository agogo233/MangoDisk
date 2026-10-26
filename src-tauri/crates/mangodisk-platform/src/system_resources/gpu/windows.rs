//! A persistent PDH query summarizes standard WDDM engines, matching the Windows overview.
use super::observation_diagnostics::ObservationDiagnostics;
use super::windows_counter::Query;
use super::windows_metadata::Metadata;
use super::windows_retry::{native_name, RetryState};
use super::{failed, unsupported, GpuAdapter, GpuAdapterUsage, GpuSample, PlatformResult};
#[cfg(test)]
use super::{
    windows_counter::decode,
    windows_metadata::{node_enumeration_ended, physical_address_valid, standard_engine},
};
use crate::diagnostics::text;
use std::{
    collections::{HashMap, HashSet},
    time::{Duration, Instant},
};
use windows::Win32::Graphics::Dxgi::{
    CreateDXGIFactory1, IDXGIFactory1, DXGI_ADAPTER_FLAG_SOFTWARE,
};
#[cfg(test)]
use windows_sys::Wdk::Graphics::Direct3D::*;
#[cfg(test)]
use windows_sys::Win32::Foundation::STATUS_INVALID_PARAMETER;
use windows_sys::Win32::System::Performance::*;

// pdh.h defines this public formatting flag; windows-sys omits it from metadata.
pub(super) const PDH_FMT_NOCAP100: u32 = 0x8000;
// Newer WDDM headers add this standard type; windows-sys 0.61 predates it.
pub(super) const DXGK_ENGINE_TYPE_VIDEO_CODEC:
    windows_sys::Wdk::Graphics::Direct3D::DXGK_ENGINE_TYPE = 9;
pub(super) const MAX_BUFFER_BYTES: usize = 8 * 1024 * 1024;
pub(super) type Engine = (u32, u32, u32, u32);
type Adapter = (u32, u32, u32);

pub(super) fn engine(name: &str) -> Option<Engine> {
    let (pid, rest) = name.strip_prefix("pid_")?.split_once("_luid_")?;
    pid.parse::<u32>().ok()?;
    let (luid, rest) = rest.split_once("_phys_")?;
    let (high, low) = luid.split_once('_')?;
    let (physical, rest) = rest.split_once("_eng_")?;
    let (index, _) = rest.split_once("_engtype_")?;
    Some((
        u32::from_str_radix(high.strip_prefix("0x")?, 16).ok()?,
        u32::from_str_radix(low.strip_prefix("0x")?, 16).ok()?,
        physical.parse().ok()?,
        index.parse().ok()?,
    ))
}

fn adapters(
    engines: &HashMap<Engine, f64>,
    names: &HashMap<(u32, u32), Metadata>,
    software: &HashSet<(u32, u32)>,
) -> Vec<GpuAdapterUsage> {
    let mut usage: HashMap<Adapter, f64> = HashMap::new();
    for (&(high, low, physical, node), &value) in engines {
        if software.contains(&(high, low))
            || !names
                .get(&(high, low))
                .is_some_and(|metadata| metadata.engines.contains(&(physical, node)))
        {
            continue;
        }
        let percent = usage.entry((high, low, physical)).or_default();
        *percent = percent.max(value.min(100.0));
    }
    let mut values: Vec<_> = usage
        .into_iter()
        .map(|((high, low, physical), used_percent)| GpuAdapterUsage {
            id: names
                .get(&(high, low))
                .map(|value| format!("{}:{physical}", value.id))
                .unwrap_or_else(|| format!("{high:08x}:{low:08x}:{physical}")),
            name: names
                .get(&(high, low))
                .map(|value| value.name.clone())
                .unwrap_or_else(|| format!("GPU {high:08x}:{low:08x}:{physical}")),
            used_percent,
            details: None,
        })
        .collect();
    values.sort_by(|a, b| a.id.cmp(&b.id));
    values
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Failure {
    pub(super) stage: &'static str,
    pub(super) code: u32,
}
pub(super) fn check(stage: &'static str, code: u32) -> Result<(), Failure> {
    if code == 0 {
        Ok(())
    } else {
        Err(Failure { stage, code })
    }
}

#[derive(Default)]
pub struct GpuReader {
    query: Option<Query>,
    temperature_due: Option<Instant>,
    temperature_diagnostics: HashMap<String, String>,
    fan_diagnostics: HashMap<String, String>,
    fan_fallback: super::windows_fans::Reader,
    retry: RetryState,
    names: HashMap<(u32, u32), Metadata>,
    named_at: Option<Instant>,
    software: HashSet<(u32, u32)>,
    indirect_displays: HashSet<(u32, u32)>,
    metadata_failure: Option<i32>,
    adapter_failures: HashMap<(u32, u32), Failure>,
    memory_failure: Option<Failure>,
    memory_retry_at: Option<Instant>,
    diagnostics: ObservationDiagnostics,
    inventory_counts: Option<(usize, usize, usize, usize)>,
    enumeration_failure: Option<(u32, Failure)>,
}
impl GpuReader {
    pub fn reset(&mut self) {
        if let Some(query) = self.query.as_mut() {
            query.previous = None;
        }
    }

    fn discover(&mut self) {
        if self
            .named_at
            .is_some_and(|at| at.elapsed() < Duration::from_secs(30))
        {
            return;
        }
        self.named_at = Some(Instant::now());
        // Cached metadata defines stable identities and which engines enter the summary.
        let factory: Result<IDXGIFactory1, _> = unsafe { CreateDXGIFactory1() };
        let factory = match factory {
            Ok(factory) => factory,
            Err(error) => {
                let code = error.code().0;
                if self.metadata_failure != Some(code) {
                    log::warn!("gpu_metadata_unavailable source=dxgi stage=factory native_code={code:#x} outcome=retain_previous_names retry_seconds=30");
                }
                self.metadata_failure = Some(code);
                return;
            }
        };
        if self.metadata_failure.take().is_some() {
            log::info!("gpu_metadata_recovered source=dxgi");
        }
        let mut software = HashSet::new();
        let mut indirect_displays = HashSet::new();
        let mut names = HashMap::new();
        let mut present = HashSet::new();
        let mut enumeration_failure = None;
        for index in 0..64 {
            let adapter = match unsafe { factory.EnumAdapters1(index) } {
                Ok(adapter) => adapter,
                Err(error) => {
                    if error.code() != windows::Win32::Graphics::Dxgi::DXGI_ERROR_NOT_FOUND {
                        enumeration_failure = Some((
                            index,
                            Failure {
                                stage: "enumerate",
                                code: error.code().0 as u32,
                            },
                        ));
                    }
                    break;
                }
            };
            let description = match unsafe { adapter.GetDesc1() } {
                Ok(description) => description,
                Err(error) => {
                    enumeration_failure = Some((
                        index,
                        Failure {
                            stage: "description",
                            code: error.code().0 as u32,
                        },
                    ));
                    continue;
                }
            };
            let identity = (
                description.AdapterLuid.HighPart as u32,
                description.AdapterLuid.LowPart,
            );
            present.insert(identity);
            if description.Flags & DXGI_ADAPTER_FLAG_SOFTWARE.0 as u32 != 0 {
                self.adapter_failures.remove(&identity);
                software.insert(identity);
                continue;
            }
            match Metadata::read(&description, names.len() as u32, self.names.get(&identity)) {
                Ok(None) => {
                    if !self.indirect_displays.contains(&identity) {
                        log::info!("gpu_adapter_excluded source=wddm_adapter_type adapter_luid={:08x}:{:08x} reason=indirect_display_device outcome=excluded", identity.0, identity.1);
                    }
                    self.adapter_failures.remove(&identity);
                    indirect_displays.insert(identity);
                }
                Ok(Some(mut metadata)) => {
                    if self
                        .names
                        .get(&identity)
                        .is_none_or(|previous| !metadata.same_inventory(previous))
                    {
                        metadata.log_inventory(identity);
                    }
                    if let Some(previous) = self.names.get_mut(&identity) {
                        metadata.retain_telemetry_state(previous);
                    }
                    if self.adapter_failures.remove(&identity).is_some() {
                        log::info!(
                            "gpu_adapter_metadata_recovered adapter_luid={:08x}:{:08x}",
                            identity.0,
                            identity.1
                        );
                    }
                    names.insert(identity, metadata);
                }
                Err(error) => {
                    if self.adapter_failures.insert(identity, error) != Some(error) {
                        log::warn!("gpu_adapter_metadata_unavailable adapter_luid={:08x}:{:08x} stage={} native_code={:#x} outcome=unavailable retry_seconds=30", identity.0, identity.1, error.stage, error.code);
                    }
                }
            }
        }
        if self.enumeration_failure != enumeration_failure {
            match enumeration_failure {
                Some((index, error)) => log::warn!("gpu_inventory_incomplete source=dxgi adapter_index={} stage={} native_code={:#x} outcome=partial_inventory retry_seconds=30", index, error.stage, error.code),
                None => log::info!("gpu_inventory_recovered source=dxgi"),
            }
            self.enumeration_failure = enumeration_failure;
        }
        for (luid, previous) in &self.names {
            if !names.contains_key(luid) {
                log::info!(
                    "gpu_adapter_removed source=dxgi_wddm adapter={} name={} metadata_failed={}",
                    text(&previous.id),
                    text(&previous.name),
                    self.adapter_failures.contains_key(luid)
                );
            }
        }
        self.adapter_failures
            .retain(|luid, _| present.contains(luid));
        let counts = (
            names.len(),
            software.len(),
            self.adapter_failures.len(),
            indirect_displays.len(),
        );
        if self.inventory_counts != Some(counts) {
            log::info!("gpu_inventory source=dxgi_wddm hardware_adapters={} excluded_software_adapters={} failed_metadata_adapters={} excluded_indirect_display_adapters={}", counts.0, counts.1, counts.2, counts.3);
            self.inventory_counts = Some(counts);
        }
        self.diagnostics.retain(|id| {
            names.values().any(|adapter| {
                (0..adapter.physical_count)
                    .any(|physical| id == format!("{}:{physical}", adapter.id))
            })
        });
        self.names = names;
        self.software = software;
        self.indirect_displays = indirect_displays;
    }

    pub fn catalogue(&mut self) -> PlatformResult<Vec<GpuAdapter>> {
        self.discover();
        Ok(self.adapters())
    }

    pub fn adapters(&self) -> Vec<GpuAdapter> {
        let mut values: Vec<_> = self
            .names
            .values()
            .flat_map(|value| {
                (0..value.physical_count).map(|physical| GpuAdapter {
                    id: format!("{}:{physical}", value.id),
                    name: value.name.clone(),
                })
            })
            .collect();
        values.sort_by(|a, b| a.id.cmp(&b.id));
        for (index, adapter) in values.iter_mut().enumerate() {
            adapter.name = format!("GPU{index} · {}", adapter.name);
        }
        values
    }

    pub fn pause_fans(&mut self) {
        self.fan_fallback = Default::default();
        self.fan_diagnostics.clear();
    }

    /// Read only actual GPU tachometers; motherboard and case fans are outside WDDM.
    pub fn read_fans(&mut self) -> PlatformResult<crate::system_resources::fans::FanSample> {
        use crate::system_resources::fans::{FanSample, FanSpeed, MAX_FANS};
        self.discover();
        let present = self.adapters();
        self.fan_diagnostics
            .retain(|id, _| present.iter().any(|adapter| adapter.id == *id));
        let mut fans = Vec::new();
        let mut failure = None;
        let mut unavailable = None;
        let mut adapters = self.names.values_mut().collect::<Vec<_>>();
        adapters.sort_by(|a, b| a.id.cmp(&b.id));
        for adapter in adapters {
            for physical in 0..adapter.physical_count {
                let id = format!("{}:{physical}", adapter.id);
                let (reading, source) = match adapter.telemetry.read_fan(physical, &id) {
                    Err(error) if error.code() == crate::PlatformErrorCode::Unsupported => (
                        self.fan_fallback
                            .read(&adapter.id, adapter.physical_count)
                            .map_err(|fallback| {
                                crate::PlatformError::new(
                                    fallback.code(),
                                    format!(
                                        "gpu_fan primary_source=wddm_adapter_perfdata primary_error={} fallback_source=nvapi fallback_error={}",
                                        text(error.diagnostic()),
                                        text(fallback.diagnostic())
                                    ),
                                )
                            }),
                        "nvapi",
                    ),
                    reading => (reading.map(|rpm| vec![rpm]), "wddm_adapter_perfdata"),
                };
                let state = match &reading {
                    Ok(_) => format!("ready:{source}"),
                    Err(error) => format!("{:?}:{}", error.code(), error.diagnostic()),
                };
                if self.fan_diagnostics.get(&id) != Some(&state) {
                    log::info!(
                        "windows_fan_state adapter={} name={} source={source} outcome={} rpm_values={:?}",
                        text(&id),
                        text(&adapter.name),
                        text(&state),
                        reading.as_ref().ok()
                    );
                    self.fan_diagnostics.insert(id, state);
                }
                match reading {
                    Ok(speeds) => {
                        for rpm in speeds {
                            if fans.len() >= MAX_FANS as usize {
                                break;
                            }
                            fans.push(FanSpeed {
                                id: fans.len() as u8,
                                rpm: Some(rpm),
                            });
                        }
                    }
                    Err(error) if error.code() != crate::PlatformErrorCode::Unsupported => {
                        failure = Some(error)
                    }
                    Err(error) => unavailable = Some(error),
                }
            }
        }
        if fans.is_empty() {
            return Err(failure.or(unavailable).unwrap_or_else(|| crate::PlatformError::new(crate::PlatformErrorCode::Unsupported, "fan source=wddm_adapter_perfdata stage=discovery reason=no_actual_tachometers motherboard_and_case_not_exposed")));
        }
        Ok(FanSample { fans })
    }

    pub fn read_temperatures(
        &mut self,
    ) -> Option<PlatformResult<Vec<super::temperature::GpuTemperatureSample>>> {
        use super::temperature::{
            GpuTemperature, GpuTemperatureKind, GpuTemperatureSample, GpuTemperatureSource,
            GPU_TEMPERATURE_INTERVAL_MS,
        };
        if self.temperature_due.is_some_and(|due| Instant::now() < due) {
            return None;
        }
        self.temperature_due =
            Some(Instant::now() + Duration::from_millis(GPU_TEMPERATURE_INTERVAL_MS));
        self.discover();
        if self.names.is_empty() {
            if let Some(code) = self.metadata_failure {
                return Some(Err(failed("temperature_discovery", format!("{code:#x}"))));
            }
            if let Some((_, error)) = self.enumeration_failure {
                return Some(Err(failed(error.stage, format!("{:#x}", error.code))));
            }
        }
        let diagnostics = &mut self.temperature_diagnostics;
        let samples: Vec<_> = self
            .names
            .values_mut()
            .flat_map(|adapter| {
                (0..adapter.physical_count)
                    .map(|physical| {
                        let id = format!("{}:{physical}", adapter.id);
                        let reading =
                            adapter
                                .telemetry
                                .read_temperature(physical, &id)
                                .map(|celsius| GpuTemperature {
                                    adapter_id: id.clone(),
                                    adapter_name: adapter.name.clone(),
                                    celsius,
                                    kind: GpuTemperatureKind::Device,
                                    source: GpuTemperatureSource::Driver,
                                    sensor_count: 1,
                                });
                        let diagnostic = match &reading {
                            Ok(_) => "ready".to_owned(),
                            Err(error) => format!("{:?}:{}", error.code(), error),
                        };
                        if diagnostics.get(&id) != Some(&diagnostic) {
                            match &reading {
                                Ok(value) => log::info!("gpu_temperature_ready adapter={} name={} source=wddm_adapter_perfdata kind={:?} sensor_count={} interval_ms={}",
                                    text(&id), text(&adapter.name), value.kind, value.sensor_count, GPU_TEMPERATURE_INTERVAL_MS),
                                Err(error) if error.code() == crate::PlatformErrorCode::Unsupported => log::info!("gpu_temperature_unsupported adapter={} name={} source=wddm_adapter_perfdata reason={}", text(&id), text(&adapter.name), text(error)),
                                Err(error) => log::warn!("gpu_temperature_read_failed adapter={} name={} source=wddm_adapter_perfdata code={:?} error={} retry_after_ms=30000", text(&id), text(&adapter.name), error.code(), text(error)),
                            }
                            diagnostics.insert(id.clone(), diagnostic);
                        }
                        GpuTemperatureSample {
                            adapter_id: id,
                            reading,
                        }
                    })
                    .collect::<Vec<_>>()
            })
            .collect();
        diagnostics.retain(|id, _| samples.iter().any(|sample| &sample.adapter_id == id));
        Some(Ok(samples))
    }

    pub fn read(&mut self) -> PlatformResult<GpuSample> {
        self.sample(false)
    }
    pub fn read_detailed(&mut self) -> PlatformResult<GpuSample> {
        self.sample(true)
    }
    fn sample(&mut self, detailed: bool) -> PlatformResult<GpuSample> {
        self.discover();
        if self.names.is_empty() {
            self.query = None;
            return Err(unsupported());
        }
        if let Some(failure) = self.retry.pending(Instant::now()) {
            self.retry.sample_failed();
            return Err(self.error(failure));
        }
        let attempt = self.retry.read(
            || {
                if self.query.is_none() {
                    self.query = Some(Query::open()?);
                }
                let query = self.query.as_mut().expect("query was acquired");
                let memory_enabled =
                    detailed && self.memory_retry_at.is_none_or(|at| Instant::now() >= at);
                if let Err(error) = query.set_memory(memory_enabled) {
                    if self.memory_failure != Some(error) {
                        log::warn!(
                            "gpu_memory_unavailable stage={} native_code={:#x} retry_seconds=30",
                            error.stage,
                            error.code
                        );
                    }
                    self.memory_failure = Some(error);
                    self.memory_retry_at = Some(Instant::now() + Duration::from_secs(30));
                }
                query.read()
            },
            std::thread::sleep,
        );
        if !attempt.retain_query {
            self.query = None;
        }
        match attempt.result {
            Ok(values) => {
                let Some(values) = values else {
                    self.retry.baseline();
                    return Ok(GpuSample::Baseline);
                };
                if let Some(recovery) = self.retry.recovered(Instant::now()) {
                    log::info!("gpu_source_recovered source=pdh_gpu_engine previous_stage={} previous_native_code={:#x} previous_native_name={} failed_attempts={} quick_retries={} failed_samples={} elapsed_ms={} outcome=valid_sample", recovery.failure.stage, recovery.failure.code, native_name(recovery.failure.code), recovery.attempts, attempt.quick_retries, recovery.failed_samples, recovery.elapsed.as_millis());
                }
                let memory = detailed.then(|| {
                    if self.memory_retry_at.is_some_and(|at| Instant::now() < at) {
                        return Err(self.memory_failure.expect("memory backoff retains failure"));
                    }
                    let result = self.query.as_mut().expect("query was acquired").memory();
                    match &result {
                        Ok(_) => {
                            if self.memory_failure.take().is_some() { log::info!("gpu_memory_recovered source=pdh_adapter_memory"); }
                            self.memory_retry_at = None;
                        }
                        Err(error) => {
                            if self.memory_failure != Some(*error) { log::warn!("gpu_memory_unavailable stage={} native_code={:#x} retry_seconds=30", error.stage, error.code); }
                            self.memory_failure = Some(*error);
                            self.memory_retry_at = Some(Instant::now() + Duration::from_secs(30));
                        }
                    }
                    result
                });
                let mut readings = adapters(&values, &self.names, &self.software);
                if let Some(memory) = memory {
                    super::windows_details::attach(
                        &mut readings,
                        &values,
                        &mut self.names,
                        &memory,
                    );
                }
                for adapter in self.names.values() {
                    for physical in 0..adapter.physical_count {
                        let id = format!("{}:{physical}", adapter.id);
                        let result = readings
                            .iter()
                            .find(|reading| reading.id == id)
                            .map(|reading| reading.details.as_ref())
                            .ok_or("no_classified_engine_observation");
                        self.diagnostics
                            .observe(&id, "pdh_gpu_engine_peak_standard", result);
                    }
                }
                Ok(GpuSample::Usage(readings))
            }
            Err(error) => Err(self.error(error)),
        }
    }

    fn error(&self, error: Failure) -> crate::PlatformError {
        if matches!(error.code, PDH_CSTATUS_NO_OBJECT | PDH_CSTATUS_NO_COUNTER) {
            unsupported()
        } else {
            failed(error.stage, format!("{:#x}", error.code))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ptr;
    fn test_metadata(id: &str, name: &str, engines: &[(u32, u32)]) -> Metadata {
        Metadata {
            id: id.into(),
            name: name.into(),
            ordinal: 0,
            physical_count: 2,
            engines: engines.iter().copied().collect(),
            nodes: HashMap::new(),
            dedicated_bytes: 0,
            memory_capacities: Vec::new(),
            telemetry: Default::default(),
        }
    }
    #[test]
    fn unsupported_fan_fallback_preserves_the_primary_driver_reason() {
        let mut metadata = test_metadata("pci:8086:9a49:0:2:0", "Intel GPU", &[]);
        metadata.physical_count = 1;
        let mut reader = GpuReader {
            names: HashMap::from([((0, 1), metadata)]),
            named_at: Some(Instant::now()),
            ..Default::default()
        };
        let error = reader.read_fans().unwrap_err();
        assert_eq!(error.code(), crate::PlatformErrorCode::Unsupported);
        assert!(
            error.diagnostic().contains("no_adapter_handle"),
            "the primary WDDM reason must survive an unavailable fallback: {}",
            error.diagnostic()
        );
        assert!(error.diagnostic().contains("wddm_adapter_perfdata"));
        assert!(error.diagnostic().contains("nvapi"));
        assert!(error
            .diagnostic()
            .contains("no_unambiguous_nvidia_pci_device"));
    }

    #[test]
    fn catalogue_numbers_devices_sequentially_after_identity_sorting() {
        let mut reader = GpuReader::default();
        let mut linked = test_metadata("a", "Linked GPU", &[]);
        linked.ordinal = 7;
        let mut discrete = test_metadata("z", "Discrete GPU", &[]);
        discrete.ordinal = 2;
        discrete.physical_count = 1;
        reader.names.insert((0, 1), linked);
        reader.names.insert((0, 2), discrete);
        let catalogue = reader.adapters();
        assert_eq!(
            catalogue
                .iter()
                .map(|adapter| adapter.id.as_str())
                .collect::<Vec<_>>(),
            ["a:0", "a:1", "z:0"]
        );
        assert_eq!(
            catalogue
                .iter()
                .map(|adapter| adapter.name.as_str())
                .collect::<Vec<_>>(),
            [
                "GPU0 · Linked GPU",
                "GPU1 · Linked GPU",
                "GPU2 · Discrete GPU"
            ]
        );
    }

    #[test]
    fn session_adapter_addresses_cannot_alias_hardware_selections() {
        assert!(physical_address_valid(&D3DKMT_ADAPTERADDRESS {
            BusNumber: 1,
            DeviceNumber: 0,
            FunctionNumber: 0,
        }));
        assert!(!physical_address_valid(&D3DKMT_ADAPTERADDRESS {
            BusNumber: u32::MAX,
            DeviceNumber: u16::MAX as u32,
            FunctionNumber: u16::MAX as u32,
        }));
        assert!(!physical_address_valid(&D3DKMT_ADAPTERADDRESS {
            BusNumber: 1,
            DeviceNumber: 32,
            FunctionNumber: 0,
        }));
        assert!(!physical_address_valid(&D3DKMT_ADAPTERADDRESS {
            BusNumber: 1,
            DeviceNumber: 0,
            FunctionNumber: 8,
        }));
    }
    #[test]
    fn unexpected_metadata_errors_cannot_silently_truncate_the_summary() {
        assert!(!node_enumeration_ended(0, STATUS_INVALID_PARAMETER as u32));
        assert!(node_enumeration_ended(20, STATUS_INVALID_PARAMETER as u32));
        assert!(!node_enumeration_ended(20, 0xc0000022));
    }
    #[test]
    fn catalogue_discovery_never_acquires_a_pdh_query() {
        let mut reader = GpuReader::default();
        reader.catalogue().unwrap();
        assert!(reader.query.is_none());
    }
    #[test]
    fn custom_nodes_do_not_replace_the_windows_aggregate() {
        assert!(!standard_engine(DXGK_ENGINE_TYPE_OTHER));
        assert!(standard_engine(DXGK_ENGINE_TYPE_VIDEO_ENCODE));
        assert!(!standard_engine(i32::MAX));
        assert!(standard_engine(DXGK_ENGINE_TYPE_VIDEO_CODEC));
        let values = adapters(
            &HashMap::from([((0, 1, 0, 0), 3.0), ((0, 1, 0, 12), 56.0)]),
            &HashMap::from([((0, 1), test_metadata("pci:1", "NVIDIA", &[(0, 0)]))]),
            &HashSet::new(),
        );
        assert_eq!(values[0].used_percent, 3.0);
        let graphics = adapters(
            &HashMap::from([((0, 1, 0, 0), 85.0), ((0, 1, 0, 2), 10.0)]),
            &HashMap::from([((0, 1), test_metadata("pci:1", "NVIDIA", &[(0, 0), (0, 2)]))]),
            &HashSet::new(),
        );
        assert_eq!(graphics[0].used_percent, 85.0);
    }
    #[test]
    fn instance_identity_distinguishes_adapters_and_physical_engines() {
        assert_eq!(
            engine("pid_123_luid_0x00000000_0x00001234_phys_0_eng_2_engtype_Compute"),
            Some((0, 0x1234, 0, 2))
        );
        assert_eq!(engine("pid_1_luid_invalid_phys_0_eng_0_engtype_3D"), None);
    }
    #[test]
    fn busiest_engine_sums_processes_without_summing_independent_engines() {
        let mut engines = HashMap::new();
        *engines.entry((0, 1, 0, 0)).or_default() += 40.0;
        *engines.entry((0, 1, 0, 0)).or_default() += 30.0;
        engines.insert((0, 1, 0, 1), 50.0);
        engines.insert((0, 2, 0, 0), 12.0);
        engines.insert((0, 2, 1, 0), 110.0);
        let result = adapters(
            &engines,
            &HashMap::from([
                ((0, 1), test_metadata("pci:1", "NVIDIA", &[(0, 0), (0, 1)])),
                ((0, 2), test_metadata("pci:2", "GPU 2", &[(0, 0), (1, 0)])),
            ]),
            &HashSet::new(),
        );
        assert_eq!(result.len(), 3);
        assert_eq!(
            result
                .iter()
                .find(|value| value.id == "pci:1:0")
                .unwrap()
                .used_percent,
            70.0
        );
        assert_eq!(
            result
                .iter()
                .find(|value| value.name == "NVIDIA")
                .unwrap()
                .used_percent,
            70.0
        );
        assert_eq!(
            result
                .iter()
                .find(|value| value.id == "pci:2:0")
                .unwrap()
                .used_percent,
            12.0
        );
        assert_eq!(
            result
                .iter()
                .find(|value| value.id == "pci:2:1")
                .unwrap()
                .used_percent,
            100.0
        );
    }
    #[test]
    fn software_adapters_cannot_be_reported_as_hardware_usage() {
        let values = adapters(
            &HashMap::from([((0, 1, 0, 0), 50.0), ((0, 2, 0, 0), 95.0)]),
            &HashMap::from([
                ((0, 1), test_metadata("pci:1", "GPU 1", &[(0, 0)])),
                ((0, 2), test_metadata("pci:2", "GPU 2", &[(0, 0)])),
            ]),
            &HashSet::from([(0, 2)]),
        );
        assert_eq!(values.len(), 1);
        assert_eq!(values[0].used_percent, 50.0);
        assert_eq!(
            engine("pid_invalid_luid_0x0_0x1_phys_0_eng_0_engtype_3D"),
            None
        );
    }

    #[test]
    fn non_pci_adapters_with_identical_names_remain_separate_and_filter_software() {
        let values = adapters(
            &HashMap::from([
                ((0, 1, 0, 0), 25.0),
                ((0, 2, 0, 0), 75.0),
                ((0, 3, 0, 0), 99.0),
            ]),
            &HashMap::from([
                (
                    (0, 1),
                    test_metadata("device:path0", "Virtual GPU", &[(0, 0)]),
                ),
                (
                    (0, 2),
                    test_metadata("device:path1", "Virtual GPU", &[(0, 0)]),
                ),
                (
                    (0, 3),
                    test_metadata("device:path2", "Software GPU", &[(0, 0)]),
                ),
            ]),
            &HashSet::from([(0, 3)]),
        );
        assert_eq!(values.len(), 2);
        for (id, expected) in [("device:path0:0", 25.0), ("device:path1:0", 75.0)] {
            assert_eq!(
                values
                    .iter()
                    .find(|value| value.id == id)
                    .unwrap()
                    .used_percent,
                expected
            );
        }
    }

    #[test]
    fn malformed_native_buffers_are_rejected_before_reading_names() {
        let mut buffer = vec![0u64; 8];
        let bytes = buffer.len() * 8;
        assert_eq!(decode(&buffer, bytes + 1, 1).unwrap_err().stage, "bounds");
        assert_eq!(
            decode(&buffer, bytes, u32::MAX).unwrap_err().stage,
            "bounds"
        );
        let item = buffer.as_mut_ptr().cast::<PDH_FMT_COUNTERVALUE_ITEM_W>();
        unsafe {
            (*item).FmtValue.CStatus = PDH_CSTATUS_VALID_DATA;
            (*item).szName = ptr::null_mut();
        }
        assert_eq!(decode(&buffer, bytes, 1).unwrap_err().stage, "name_bounds");
        unsafe {
            (*item).szName = (buffer.as_ptr() as usize + 1) as *mut u16;
        }
        assert_eq!(decode(&buffer, bytes, 1).unwrap_err().stage, "name_bounds");
        let last_unit = (buffer.as_ptr() as usize + bytes - 2) as *mut u16;
        unsafe {
            *last_unit = 1;
            (*item).szName = last_unit;
        }
        assert_eq!(decode(&buffer, bytes, 1).unwrap_err().stage, "name_length");
    }
}
