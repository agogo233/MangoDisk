//! Cached identities and engine classification from public WDDM metadata queries.
use super::windows::{check, Failure, DXGK_ENGINE_TYPE_VIDEO_CODEC};
use crate::diagnostics::text;
use std::{
    collections::{HashMap, HashSet},
    mem,
    time::{Duration, Instant},
};
use windows_sys::{
    Wdk::Graphics::Direct3D::*,
    Win32::Devices::Display::{
        DisplayConfigGetDeviceInfo, DISPLAYCONFIG_ADAPTER_NAME,
        DISPLAYCONFIG_DEVICE_INFO_GET_ADAPTER_NAME,
    },
    Win32::Foundation::{LUID, STATUS_INVALID_PARAMETER},
    Win32::System::Performance::PDH_INVALID_DATA,
};

pub(super) fn node_enumeration_ended(node: u32, code: u32) -> bool {
    node > 0 && code == STATUS_INVALID_PARAMETER as u32
}

pub(super) fn standard_engine(kind: DXGK_ENGINE_TYPE) -> bool {
    matches!(
        kind,
        DXGK_ENGINE_TYPE_3D
            | DXGK_ENGINE_TYPE_VIDEO_DECODE
            | DXGK_ENGINE_TYPE_VIDEO_ENCODE
            | DXGK_ENGINE_TYPE_VIDEO_PROCESSING
            | DXGK_ENGINE_TYPE_SCENE_ASSEMBLY
            | DXGK_ENGINE_TYPE_COPY
            | DXGK_ENGINE_TYPE_OVERLAY
            | DXGK_ENGINE_TYPE_CRYPTO
            | DXGK_ENGINE_TYPE_VIDEO_CODEC
    )
}

pub(super) fn physical_address_valid(address: &D3DKMT_ADAPTERADDRESS) -> bool {
    address.BusNumber != u32::MAX && address.DeviceNumber <= 31 && address.FunctionNumber <= 7
}

fn device_path_id(units: &[u16]) -> Option<String> {
    let end = units.iter().position(|unit| *unit == 0)?;
    let path = String::from_utf16(&units[..end]).ok()?;
    if !path.starts_with(r"\\?\") || path.len() <= 4 || path.chars().any(char::is_control) {
        return None;
    }
    Some(format!("device:{}", path.to_ascii_lowercase()))
}

fn indirect_display_adapter(flags: u32) -> bool {
    // d3dkmthk.h defines IndirectDisplayDevice at bit 6. Paravirtualized GPUs
    // remain eligible: only display endpoints without their own GPU are excluded.
    flags & (1 << 6) != 0
}

fn adapter_device_id(luid: LUID) -> Result<String, Failure> {
    let mut value = DISPLAYCONFIG_ADAPTER_NAME::default();
    value.header.r#type = DISPLAYCONFIG_DEVICE_INFO_GET_ADAPTER_NAME;
    value.header.size = mem::size_of::<DISPLAYCONFIG_ADAPTER_NAME>() as u32;
    value.header.adapterId = luid;
    let code = unsafe { DisplayConfigGetDeviceInfo(&mut value.header) };
    if code != 0 {
        return Err(Failure {
            stage: "adapter_device_path",
            code: code as u32,
        });
    }
    device_path_id(&value.adapterDevicePath).ok_or(Failure {
        stage: "adapter_device_path_invalid",
        code: PDH_INVALID_DATA,
    })
}

#[derive(Clone, PartialEq, Eq)]
pub(super) struct Node {
    pub kind: super::details::GpuActivityKind,
    pub native_kind: DXGK_ENGINE_TYPE,
    pub name: Option<String>,
    pub standard: bool,
}

pub(super) struct Metadata {
    pub(super) id: String,
    pub(super) name: String,
    pub(super) ordinal: u32,
    pub(super) physical_count: u32,
    pub(super) engines: HashSet<(u32, u32)>,
    pub(super) nodes: HashMap<(u32, u32), Node>,
    pub(super) dedicated_bytes: u64,
    pub(super) memory_capacities: Vec<super::windows_memory::Capacities>,
    pub(super) telemetry: TelemetryReader,
}

pub(super) struct AdapterHandle(u32);
impl Drop for AdapterHandle {
    fn drop(&mut self) {
        unsafe {
            D3DKMTCloseAdapter(&D3DKMT_CLOSEADAPTER { hAdapter: self.0 });
        }
    }
}
impl AdapterHandle {
    fn open(luid: LUID) -> Result<Self, Failure> {
        let mut value = D3DKMT_OPENADAPTERFROMLUID {
            AdapterLuid: luid,
            hAdapter: 0,
        };
        check(
            "adapter_open",
            unsafe { D3DKMTOpenAdapterFromLuid(&mut value) } as u32,
        )?;
        Ok(Self(value.hAdapter))
    }
    pub(super) fn query<T>(
        &self,
        kind: KMTQUERYADAPTERINFOTYPE,
        value: &mut T,
        stage: &'static str,
    ) -> Result<(), Failure> {
        let mut query = D3DKMT_QUERYADAPTERINFO {
            hAdapter: self.0,
            Type: kind,
            pPrivateDriverData: (value as *mut T).cast(),
            PrivateDriverDataSize: mem::size_of::<T>() as u32,
        };
        check(stage, unsafe { D3DKMTQueryAdapterInfo(&mut query) } as u32)
    }
}

// The discovery cache owns this handle, so detailed sampling needs no additional
// adapter opens. Unsupported drivers are retried at most once per 30 seconds.
#[derive(Default)]
pub(super) struct TelemetryReader {
    handle: Option<AdapterHandle>,
    retry_at: Vec<Option<Instant>>,
    observations: Vec<Option<Result<u8, Failure>>>,
    node_retry_at: Vec<Option<Instant>>,
    node_observations: Vec<Option<Result<bool, Failure>>>,
}

fn temperature_celsius(deci_celsius: u32) -> Option<f64> {
    (1..=1500)
        .contains(&deci_celsius)
        .then_some(f64::from(deci_celsius) / 10.0)
}

impl TelemetryReader {
    pub(super) fn read_temperature(
        &mut self,
        physical: u32,
        id: &str,
    ) -> crate::PlatformResult<f64> {
        if let Some(celsius) = self.read(physical, id, None).temperature_celsius {
            return Ok(celsius);
        }
        match self.observations.get(physical as usize).copied().flatten() {
            Some(Err(error)) if !matches!(error.code, 0xc000000d | 0xc00000bb) => {
                Err(super::failed(error.stage, format!("{:#x}", error.code)))
            }
            _ => Err(crate::PlatformError::new(
                crate::PlatformErrorCode::Unsupported,
                "GPU driver does not expose a valid WDDM temperature",
            )),
        }
    }

    pub(super) fn read(
        &mut self,
        physical: u32,
        id: &str,
        node: Option<u32>,
    ) -> super::details::GpuTelemetry {
        let mut telemetry = super::details::GpuTelemetry::default();
        let Some(handle) = self.handle.as_ref() else {
            return telemetry;
        };
        let index = physical as usize;
        let Some(retry_at) = self.retry_at.get_mut(index) else {
            return telemetry;
        };
        if !retry_at.is_some_and(|at| Instant::now() < at) {
            let mut value = D3DKMT_ADAPTER_PERFDATA {
                PhysicalAdapterIndex: physical,
                ..Default::default()
            };
            let result = handle.query(KMTQAITYPE_ADAPTERPERFDATA, &mut value, "adapter_telemetry");
            if result.is_ok() {
                telemetry.temperature_celsius = temperature_celsius(value.Temperature);
                telemetry.memory_clock_mhz = clock_mhz(value.MemoryFrequency);
            }
            let mask = u8::from(telemetry.temperature_celsius.is_some())
                | (u8::from(telemetry.memory_clock_mhz.is_some()) << 1);
            let observation = result.map(|()| mask);
            if let Some(previous) = self.observations.get_mut(index) {
                if *previous != Some(observation) {
                    match observation {
                        Ok(capabilities) => log::info!("gpu_telemetry_capability adapter={} physical={physical} source=wddm_adapter_perfdata temperature={} memory_clock={} native_deci_celsius={} native_memory_hz={} retry_seconds=30",text(id),capabilities & 1 != 0,capabilities & 2 != 0,value.Temperature,value.MemoryFrequency),
                        Err(error) => log::info!("gpu_telemetry_unavailable adapter={} physical={physical} source=wddm_adapter_perfdata stage={} native_code={:#x} retry_seconds=30",text(id),error.stage,error.code),
                    }
                    *previous = Some(observation);
                }
            }
            *retry_at = (mask == 0).then(|| Instant::now() + Duration::from_secs(30));
        }
        if let (Some(node), Some(retry_at)) = (node, self.node_retry_at.get_mut(index)) {
            if !retry_at.is_some_and(|at| Instant::now() < at) {
                let mut value = D3DKMT_NODE_PERFDATA {
                    NodeOrdinal: node,
                    PhysicalAdapterIndex: physical,
                    ..Default::default()
                };
                let result =
                    handle.query(KMTQAITYPE_NODEPERFDATA, &mut value, "graphics_frequency");
                if result.is_ok() {
                    telemetry.engine_clock_mhz = clock_mhz(value.Frequency);
                }
                let observation = result.map(|()| telemetry.engine_clock_mhz.is_some());
                if let Some(previous) = self.node_observations.get_mut(index) {
                    if *previous != Some(observation) {
                        match observation {
                            Ok(available)=>log::info!("gpu_graphics_frequency_capability adapter={} physical={physical} node={node} source=wddm_node_perfdata available={available} native_hz={} retry_seconds=30",text(id),value.Frequency),
                            Err(error)=>log::info!("gpu_graphics_frequency_unavailable adapter={} physical={physical} node={node} stage={} native_code={:#x} retry_seconds=30",text(id),error.stage,error.code),
                        }
                        *previous = Some(observation);
                    }
                }
                *retry_at = telemetry
                    .engine_clock_mhz
                    .is_none()
                    .then(|| Instant::now() + Duration::from_secs(30));
            }
        }
        telemetry
    }
}
fn clock_mhz(hz: u64) -> Option<f64> {
    (1_000_000..=20_000_000_000)
        .contains(&hz)
        .then_some(hz as f64 / 1_000_000.0)
}

impl Metadata {
    pub(super) fn same_inventory(&self, previous: &Self) -> bool {
        self.id == previous.id
            && self.name == previous.name
            && self.ordinal == previous.ordinal
            && self.physical_count == previous.physical_count
            && self.dedicated_bytes == previous.dedicated_bytes
            && self.memory_capacities == previous.memory_capacities
            && self.nodes == previous.nodes
    }
    pub(super) fn log_inventory(&self, luid: (u32, u32)) {
        const MAX_LOGGED_NODES: usize = 64;
        let identity_source = if self.id.starts_with("device:") {
            "device_path"
        } else {
            "pci_address"
        };
        log::info!("gpu_adapter_discovered source=dxgi_wddm identity_source={} adapter={} name={} adapter_luid={:08x}:{:08x} ordinal={} physical_count={} dedicated_capacity_bytes={} node_count={} standard_node_count={} summary_policy=peak_standard_engine custom_in_summary=false", identity_source, text(&self.id), text(&self.name), luid.0, luid.1, self.ordinal, self.physical_count, self.dedicated_bytes, self.nodes.len(), self.engines.len());
        let mut nodes: Vec<_> = self.nodes.iter().collect();
        nodes.sort_by_key(|(identity, _)| **identity);
        for ((physical, node), descriptor) in nodes.into_iter().take(MAX_LOGGED_NODES) {
            log::info!("gpu_engine_discovered adapter={} physical={} node={} native_type={} kind={:?} name={} included_in_summary={}", text(&self.id), physical, node, descriptor.native_kind, descriptor.kind, text(descriptor.name.as_deref().unwrap_or("")), descriptor.standard);
        }
        if self.nodes.len() > MAX_LOGGED_NODES {
            log::info!(
                "gpu_engine_inventory_truncated adapter={} logged_nodes={} omitted_nodes={}",
                text(&self.id),
                MAX_LOGGED_NODES,
                self.nodes.len() - MAX_LOGGED_NODES
            );
        }
    }
    // Preserve retry and diagnostic state across the regular metadata refresh.
    pub(super) fn retain_telemetry_state(&mut self, previous: &mut Self) {
        if self.id == previous.id && self.physical_count == previous.physical_count {
            self.telemetry.retry_at = std::mem::take(&mut previous.telemetry.retry_at);
            self.telemetry.observations = std::mem::take(&mut previous.telemetry.observations);
            if self.nodes == previous.nodes {
                self.telemetry.node_retry_at =
                    std::mem::take(&mut previous.telemetry.node_retry_at);
                self.telemetry.node_observations =
                    std::mem::take(&mut previous.telemetry.node_observations);
            }
        }
    }

    pub(super) fn read(
        description: &windows::Win32::Graphics::Dxgi::DXGI_ADAPTER_DESC1,
        ordinal: u32,
        previous: Option<&Self>,
    ) -> Result<Option<Self>, Failure> {
        let handle = AdapterHandle::open(LUID {
            HighPart: description.AdapterLuid.HighPart,
            LowPart: description.AdapterLuid.LowPart,
        })?;
        let mut address = D3DKMT_ADAPTERADDRESS::default();
        let address_result =
            handle.query(KMTQAITYPE_ADAPTERADDRESS, &mut address, "adapter_address");
        let id = if address_result.is_ok() && physical_address_valid(&address) {
            format!(
                "pci:{:04x}:{:04x}:{}:{}:{}",
                description.VendorId,
                description.DeviceId,
                address.BusNumber,
                address.DeviceNumber,
                address.FunctionNumber
            )
        } else {
            let mut adapter_type = D3DKMT_ADAPTERTYPE::default();
            handle.query(KMTQAITYPE_ADAPTERTYPE, &mut adapter_type, "adapter_type")?;
            if indirect_display_adapter(unsafe { adapter_type.Anonymous.Value }) {
                return Ok(None);
            }
            // Virtual adapters need not have a PCI location. Use their device interface
            // path instead of a repeated name or a LUID that changes across reboots.
            adapter_device_id(LUID {
                HighPart: description.AdapterLuid.HighPart,
                LowPart: description.AdapterLuid.LowPart,
            })?
        };
        let mut count = D3DKMT_PHYSICAL_ADAPTER_COUNT::default();
        handle.query(
            KMTQAITYPE_PHYSICALADAPTERCOUNT,
            &mut count,
            "physical_count",
        )?;
        if !(1..=16).contains(&count.Count) {
            return Err(Failure {
                stage: "physical_count_limit",
                code: PDH_INVALID_DATA,
            });
        }
        let mut engines = HashSet::new();
        let mut nodes = HashMap::new();
        for physical in 0..count.Count {
            let mut ended = false;
            for node in 0..256 {
                let mut metadata = D3DKMT_NODEMETADATA {
                    NodeOrdinalAndAdapterIndex: node | (physical << 16),
                    ..Default::default()
                };
                if let Err(error) =
                    handle.query(KMTQAITYPE_NODEMETADATA, &mut metadata, "node_metadata")
                {
                    if !node_enumeration_ended(node, error.code) {
                        return Err(error);
                    }
                    ended = true;
                    break;
                }
                // Custom nodes remain outside this summary even if their driver
                // name resembles a standard engine. They may still execute work.
                let native_kind = metadata.NodeData.EngineType;
                let units = metadata.NodeData.FriendlyName;
                let end = units
                    .iter()
                    .position(|unit| *unit == 0)
                    .unwrap_or(units.len());
                let name = String::from_utf16_lossy(&units[..end]);
                use super::details::GpuActivityKind as Kind;
                let kind = match native_kind {
                    DXGK_ENGINE_TYPE_3D => Kind::Graphics,
                    DXGK_ENGINE_TYPE_COPY => Kind::Copy,
                    DXGK_ENGINE_TYPE_VIDEO_DECODE => Kind::VideoDecode,
                    DXGK_ENGINE_TYPE_VIDEO_ENCODE => Kind::VideoEncode,
                    DXGK_ENGINE_TYPE_VIDEO_PROCESSING => Kind::VideoProcessing,
                    _ => Kind::Other,
                };
                nodes.insert(
                    (physical, node),
                    Node {
                        kind,
                        native_kind,
                        name: (!name.is_empty()).then_some(name),
                        standard: standard_engine(native_kind),
                    },
                );
                if standard_engine(native_kind) {
                    engines.insert((physical, node));
                }
            }
            if !ended {
                return Err(Failure {
                    stage: "node_count_limit",
                    code: PDH_INVALID_DATA,
                });
            }
        }
        let length = description
            .Description
            .iter()
            .position(|unit| *unit == 0)
            .unwrap_or(description.Description.len());
        let memory_capacities = (0..count.Count)
            .map(|physical| {
                super::windows_memory::read(
                    &handle,
                    &id,
                    physical,
                    count.Count,
                    previous
                        .filter(|old| old.id == id)
                        .and_then(|old| old.memory_capacities.get(physical as usize))
                        .copied(),
                )
            })
            .collect();
        Ok(Some(Self {
            id,
            name: String::from_utf16_lossy(&description.Description[..length]),
            ordinal,
            physical_count: count.Count,
            engines,
            nodes,
            dedicated_bytes: description.DedicatedVideoMemory as u64,
            memory_capacities,
            telemetry: TelemetryReader {
                handle: Some(handle),
                retry_at: vec![None; count.Count as usize],
                observations: vec![None; count.Count as usize],
                node_retry_at: vec![None; count.Count as usize],
                node_observations: vec![None; count.Count as usize],
            },
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn indirect_display_endpoints_are_not_paravirtualized_gpus() {
        assert!(indirect_display_adapter(0x242));
        assert!(indirect_display_adapter(1 << 6));
        assert!(!indirect_display_adapter(0xb));
        assert!(!indirect_display_adapter((1 << 7) | 3));
    }

    #[test]
    fn non_pci_device_paths_preserve_identity_without_adapter_names_or_luids() {
        fn id(path: &str) -> Option<String> {
            device_path_id(&path.encode_utf16().chain([0]).collect::<Vec<_>>())
        }
        let first = id(r"\\?\ACPI#PRL4005#0#{adapter}").unwrap();
        assert_eq!(first, r"device:\\?\acpi#prl4005#0#{adapter}");
        assert_eq!(Some(first.clone()), id(r"\\?\acpi#prl4005#0#{adapter}"));
        assert_ne!(Some(first), id(r"\\?\ACPI#PRL4005#1#{adapter}"));
        assert_eq!(id("Parallels Display Adapter"), None);
        assert_eq!(id(r"\\?\"), None);
        assert_eq!(id(""), None);
        assert_eq!(id("\\\\?\\invalid\npath"), None);
        assert_eq!(device_path_id(&[0xd800, 0]), None);
        assert_eq!(device_path_id(&[1; 128]), None);
    }

    #[test]
    fn native_clock_is_hertz_and_zero_never_becomes_a_frequency() {
        assert_eq!(clock_mhz(210_000_000), Some(210.0));
        assert_eq!(clock_mhz(405_000_000), Some(405.0));
        assert_eq!(clock_mhz(0), None);
        assert_eq!(clock_mhz(u64::MAX), None);
        assert_eq!(clock_mhz(999_999), None);
    }
    #[test]
    fn temperature_converts_deci_celsius_and_rejects_unavailable_values() {
        assert_eq!(temperature_celsius(420), Some(42.0));
        assert_eq!(temperature_celsius(425), Some(42.5));
        assert_eq!(temperature_celsius(1500), Some(150.0));
        assert_eq!(temperature_celsius(0), None);
        assert_eq!(temperature_celsius(1501), None);
        assert_eq!(temperature_celsius(u32::MAX), None);
    }

    #[test]
    fn metadata_refresh_retains_temperature_backoff_and_diagnostic_history() {
        fn adapter(id: &str) -> Metadata {
            Metadata {
                id: id.into(),
                name: "GPU".into(),
                ordinal: 0,
                physical_count: 1,
                engines: HashSet::new(),
                nodes: HashMap::new(),
                dedicated_bytes: 0,
                memory_capacities: Vec::new(),
                telemetry: TelemetryReader {
                    retry_at: vec![None],
                    observations: vec![None],
                    ..Default::default()
                },
            }
        }
        let retry_at = Instant::now() + Duration::from_secs(30);
        let mut previous = adapter("pci:1");
        previous.telemetry.retry_at[0] = Some(retry_at);
        previous.telemetry.observations[0] = Some(Ok(0));
        let mut refreshed = adapter("pci:1");
        refreshed.retain_telemetry_state(&mut previous);
        assert_eq!(refreshed.telemetry.retry_at[0], Some(retry_at));
        assert_eq!(refreshed.telemetry.observations[0], Some(Ok(0)));
        let mut different = adapter("pci:2");
        different.retain_telemetry_state(&mut refreshed);
        assert_eq!(different.telemetry.retry_at[0], None);
        assert_eq!(different.telemetry.observations[0], None);
    }
    #[test]
    fn absent_adapter_and_invalid_physical_index_have_no_temperature() {
        let mut reader = TelemetryReader::default();
        assert_eq!(
            reader.read(0, "unavailable", None).temperature_celsius,
            None
        );
        reader.retry_at = vec![None];
        assert_eq!(
            reader.read(0, "unavailable", None).temperature_celsius,
            None
        );
        assert_eq!(
            reader.read(1, "unavailable", None).temperature_celsius,
            None
        );
    }
}
