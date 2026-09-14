use std::{collections::HashMap, ptr};

use windows_sys::Win32::{
    NetworkManagement::{
        IpHelper::{
            FreeMibTable, GetIfTable2, GetIpForwardTable2, GetIpInterfaceEntry, MIB_IF_TABLE2,
            MIB_IPFORWARD_TABLE2, MIB_IPINTERFACE_ROW,
        },
        Ndis::IfOperStatusUp,
    },
    Networking::WinSock::AF_UNSPEC,
};

use super::{InterfaceKind, InterfaceSample, NetworkCounters, NetworkInterface};
use crate::{PlatformError, PlatformErrorCode, PlatformResult};

/// IP Helper owns table allocation; free on every exit, including decoding failures.
struct Table(*mut std::ffi::c_void);
impl Drop for Table {
    fn drop(&mut self) {
        unsafe { FreeMibTable(self.0) };
    }
}

fn default_routes() -> HashMap<u32, u32> {
    let mut result = HashMap::new();
    let mut table: *mut MIB_IPFORWARD_TABLE2 = ptr::null_mut();
    unsafe {
        if GetIpForwardTable2(AF_UNSPEC, &mut table) != 0 || table.is_null() {
            return result;
        }
        let _owned = Table(table.cast());
        for row in std::slice::from_raw_parts((*table).Table.as_ptr(), (*table).NumEntries as usize)
        {
            if row.DestinationPrefix.PrefixLength != 0 {
                continue;
            }
            let mut interface = MIB_IPINTERFACE_ROW {
                Family: row.DestinationPrefix.Prefix.si_family,
                InterfaceIndex: row.InterfaceIndex,
                ..Default::default()
            };
            // Route preference includes both route and interface metrics. IPv4
            // and IPv6 routes for one adapter must never cause double counting.
            let metric = if GetIpInterfaceEntry(&mut interface) == 0 {
                row.Metric.saturating_add(interface.Metric)
            } else {
                row.Metric
            };
            result
                .entry(row.InterfaceIndex)
                .and_modify(|current: &mut u32| *current = (*current).min(metric))
                .or_insert(metric);
        }
    }
    result
}

pub fn read() -> PlatformResult<Vec<InterfaceSample>> {
    let routes = default_routes();
    let mut table: *mut MIB_IF_TABLE2 = ptr::null_mut();
    unsafe {
        if GetIfTable2(&mut table) != 0 || table.is_null() {
            return Err(PlatformError::new(
                PlatformErrorCode::OperationFailed,
                "network counters unavailable",
            ));
        }
        let _owned = Table(table.cast());
        let rows =
            std::slice::from_raw_parts((*table).Table.as_ptr(), (*table).NumEntries as usize);
        Ok(rows
            .iter()
            .map(|row| {
                let physical = row.InterfaceAndOperStatusFlags._bitfield & 1 != 0;
                let kind = match (physical, row.Type) {
                    (true, 71) => InterfaceKind::Wifi,
                    (true, 6) => InterfaceKind::Ethernet,
                    (true, _) => InterfaceKind::Other,
                    (false, _) => InterfaceKind::Virtual,
                };
                let connected = row.OperStatus == IfOperStatusUp;
                let guid = row.InterfaceGuid;
                let suffix = guid
                    .data4
                    .iter()
                    .map(|byte| format!("{byte:02x}"))
                    .collect::<String>();
                InterfaceSample {
                    interface: NetworkInterface {
                        id: format!(
                            "{:08x}-{:04x}-{:04x}-{suffix}",
                            guid.data1, guid.data2, guid.data3
                        ),
                        name: String::from_utf16_lossy(
                            &row.Alias[..row
                                .Alias
                                .iter()
                                .position(|value| *value == 0)
                                .unwrap_or(row.Alias.len())],
                        ),
                        kind,
                        connected,
                        physical,
                        default_route_metric: routes.get(&row.InterfaceIndex).copied(),
                    },
                    counters: connected.then_some(NetworkCounters {
                        received: row.InOctets,
                        transmitted: row.OutOctets,
                    }),
                }
            })
            .collect())
    }
}
