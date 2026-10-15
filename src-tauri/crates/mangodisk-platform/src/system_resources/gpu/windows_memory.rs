//! Physical capacity and allocatable segments are distinct driver facts.
use super::{details::GpuMemoryCapacitySource, windows_metadata::AdapterHandle};
use crate::diagnostics::text;
use windows_sys::{Wdk::Graphics::Direct3D::*, Win32::System::Registry::REG_QWORD};

#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub(super) struct Capacities {
    pub dedicated: Option<u64>,
    pub dedicated_source: Option<GpuMemoryCapacitySource>,
    pub shared: Option<u64>,
    // Retain diagnostic facts too: a new native failure must not be suppressed
    // merely because the resulting capacity is unchanged.
    reported: Option<u64>,
    allocatable: Option<u64>,
    segment_native_code: u32,
    registry_native_code: u32,
    registry_status: D3DDDI_QUERYREGISTRY_STATUS,
}

fn registry_capacity(value: &D3DDDI_QUERYREGISTRY_INFO) -> Option<u64> {
    if value.Status != D3DDDI_QUERYREGISTRY_STATUS_SUCCESS
        || value.ValueType != REG_QWORD
        || value.OutputValueSize != 8
    {
        return None;
    }
    Some(unsafe { value.Anonymous.OutputQword })
}

fn dedicated_capacity(
    reported: Option<u64>,
    allocatable: Option<u64>,
) -> (Option<u64>, Option<GpuMemoryCapacitySource>) {
    match (reported, allocatable) {
        (Some(total), Some(usable)) if total >= usable => {
            (Some(total), Some(GpuMemoryCapacitySource::Reported))
        }
        (Some(total), None) if total > 0 => (Some(total), Some(GpuMemoryCapacitySource::Reported)),
        _ => (
            allocatable,
            allocatable.map(|_| GpuMemoryCapacitySource::Allocatable),
        ),
    }
}

pub(super) fn read(
    handle: &AdapterHandle,
    id: &str,
    physical: u32,
    physical_count: u32,
    previous: Option<Capacities>,
) -> Capacities {
    let mut groups = D3DKMT_SEGMENTGROUPSIZEINFO {
        PhysicalAdapterIndex: physical,
        ..Default::default()
    };
    let group_result = handle.query(
        KMTQAITYPE_GETSEGMENTGROUPSIZE,
        &mut groups,
        "memory_segments",
    );
    let segments = if group_result.is_ok() {
        Some(groups.LegacyInfo)
    } else if physical_count == 1 {
        // Legacy sizes describe the logical adapter, not each member of an LDA.
        let mut legacy = D3DKMT_SEGMENTSIZEINFO::default();
        handle
            .query(
                KMTQAITYPE_GETSEGMENTSIZE,
                &mut legacy,
                "memory_segments_legacy",
            )
            .ok()
            .map(|()| legacy)
    } else {
        None
    };
    let allocatable = segments.and_then(|sizes| {
        sizes
            .DedicatedVideoMemorySize
            .checked_add(sizes.DedicatedSystemMemorySize)
    });
    let mut registry = D3DDDI_QUERYREGISTRY_INFO {
        QueryType: D3DDDI_QUERYREGISTRY_ADAPTERKEY,
        PhysicalAdapterIndex: physical,
        ValueType: REG_QWORD,
        OutputValueSize: 8,
        ..Default::default()
    };
    for (dest, unit) in registry
        .ValueName
        .iter_mut()
        .zip("HardwareInformation.qwMemorySize".encode_utf16())
    {
        *dest = unit;
    }
    // Query through WDDM so the kernel resolves the selected physical adapter,
    // including paravirtualized devices. Never match registry entries by name.
    let registry_result = handle.query(KMTQAITYPE_QUERYREGISTRY, &mut registry, "memory_capacity");
    let reported = registry_result
        .ok()
        .and_then(|()| registry_capacity(&registry));
    let (dedicated, dedicated_source) = dedicated_capacity(reported, allocatable);
    let capacity = Capacities {
        dedicated,
        dedicated_source,
        shared: segments.map(|sizes| sizes.SharedSystemMemorySize),
        reported,
        allocatable,
        segment_native_code: group_result.err().map_or(0, |error| error.code),
        registry_native_code: registry_result.err().map_or(0, |error| error.code),
        registry_status: registry.Status,
    };
    // Older drivers may only expose allocatable segments, excluding hardware
    // reservations. Retain those reported sizes without guessing missing bytes.
    let source = if capacity.dedicated_source == Some(GpuMemoryCapacitySource::Reported) {
        "wddm_registry_qword"
    } else if capacity.dedicated.is_some() {
        "wddm_allocatable_segments"
    } else {
        "unavailable"
    };
    log::log!(if previous == Some(capacity) { log::Level::Debug } else { log::Level::Info }, "gpu_memory_capacity adapter={} physical={physical} source={source} dedicated_total_bytes={:?} shared_total_bytes={:?} reported_dedicated_bytes={reported:?} allocatable_dedicated_bytes={allocatable:?} segment_native_code={:#x} registry_native_code={:#x} registry_status={} policy=prefer_reported_fallback_allocatable", text(id), capacity.dedicated, capacity.shared, group_result.err().map_or(0, |error| error.code), registry_result.err().map_or(0, |error| error.code), registry.Status);
    capacity
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reported_capacity_is_preferred_with_allocatable_fallback() {
        assert_eq!(
            dedicated_capacity(Some(128), Some(113)),
            (Some(128), Some(GpuMemoryCapacitySource::Reported))
        );
        assert_eq!(
            dedicated_capacity(None, Some(113)),
            (Some(113), Some(GpuMemoryCapacitySource::Allocatable))
        );
        assert_eq!(
            dedicated_capacity(Some(100), Some(113)),
            (Some(113), Some(GpuMemoryCapacitySource::Allocatable))
        );
        assert_eq!(
            dedicated_capacity(None, Some(0)),
            (Some(0), Some(GpuMemoryCapacitySource::Allocatable))
        );
        assert_eq!(dedicated_capacity(None, None), (None, None));
        assert_eq!(
            dedicated_capacity(Some(128), None),
            (Some(128), Some(GpuMemoryCapacitySource::Reported))
        );
        assert_eq!(dedicated_capacity(Some(0), None), (None, None));
    }

    #[test]
    fn registry_results_require_success_type_and_exact_size() {
        let mut value = D3DDDI_QUERYREGISTRY_INFO {
            ValueType: REG_QWORD,
            OutputValueSize: 8,
            ..Default::default()
        };
        value.Anonymous.OutputQword = 128;
        assert_eq!(registry_capacity(&value), Some(128));
        value.Status = D3DDDI_QUERYREGISTRY_STATUS_FAIL;
        assert_eq!(registry_capacity(&value), None);
        value.Status = D3DDDI_QUERYREGISTRY_STATUS_SUCCESS;
        value.OutputValueSize = 4;
        assert_eq!(registry_capacity(&value), None);
        value.OutputValueSize = 8;
        value.ValueType = 4;
        assert_eq!(registry_capacity(&value), None);
    }
}
