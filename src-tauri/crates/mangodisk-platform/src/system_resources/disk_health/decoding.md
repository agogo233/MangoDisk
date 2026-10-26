# SMART decoding

Health reads remain read-only and use the resident five-minute cache. Parsing is
shared by macOS and Windows; model identification never requires a privileged
helper, an installed monitoring program, or an online model lookup.

## Standard versus vendor fields

The [NVMe health log](https://learn.microsoft.com/en-us/windows/win32/api/nvme/ns-nvme-nvme_health_info_log)
and [NVMe Base Specification, SMART / Health Information Log](https://nvmexpress.org/wp-content/uploads/NVM-Express-Base-Specification-2.0d-2024.01.11-Ratified.pdf)
define offsets and units. Data units read/written count 512,000 bytes per unit;
zero means not reported. Other lifetime counters remain decimal strings so JSON
does not lose 128-bit precision. Additional fields include host command counts,
controller busy minutes, unsafe shutdowns, error-log entries, thermal warning
durations, eight optional temperature sensors, and thermal-management counters.
Invalid or absent temperatures remain unavailable.

[INCITS TR-54 (SMART Attribute Descriptions)](https://webstore.ansi.org/standards/incits/INCITSTR542021)
documents relationships between ATA device statistics and SMART attributes; it
does not remove the need to identify vendor-specific encodings.
ATA's attribute sector has thirty twelve-byte slots and a checksum. Attribute
meanings and raw units are vendor-specific. Preserve every nonempty slot, including
unknown IDs, flags, current/worst normalized values, six raw bytes, and the final
reserved byte. Reject truncated, corrupt, empty, or duplicate-ID tables. Raw
bytes are never implicitly interpreted as a health percentage.

Windows [failure prediction](https://learn.microsoft.com/en-us/windows-hardware/drivers/ddi/ntddstor/ni-ntddstor-ioctl_storage_predict_failure)
provides both a status flag and a vendor-specific sector. Parse the latter as ATA
only on an identified ATA/SATA bus. SCSI management status does not imply ATA
attributes. macOS uses its existing ATA SMART read interface and IOKit product
metadata. No serial numbers are collected.

## Verified ATA profiles

| Profile | Selection | Summary decoding |
| --- | --- | --- |
| Samsung consumer | Explicit 830/840/850/860/870 families | 9: hours; 12: cycles; 177: normalized remaining life; 241/242: 512-byte units |
| Legacy Silicon Motion | Complete legacy table prefix plus a supported model; Acer AS500A additionally requires verified firmware | 9: low 24-bit hours; 12: cycles; 169: raw remaining percent; 241/242: host I/O; 245/246: TLC/SLC writes, all in 32 MiB units |
| Seagate BarraCuda | Explicit CMR/SMR series | 9: hours with model-specific packed-byte handling; 12: cycles; 241/242: logical sectors |
| Unknown | No verified match | Raw attributes and valid temperature only; no invented life or byte totals |

Known profiles also decode available sector/block, erase, program, error,
load-cycle and interface CRC counters with explicit metric/unit codes.
Missing attributes remain absent; supported zero counts remain zero.
Do not equate spare block counts with NVMe's spare percentage or ATA error
counts with NVMe media/data-integrity errors. The OS/drive failure-prediction
status remains separate from individual counters and estimated remaining life.

Samsung's [SMART application note](https://semiconductor.samsung.com/resources/others/SSD_Application_Note_SMART_final.pdf)
and [endurance white paper](https://s7d2.scene7.com/is/content/SamsungUS/b2b/resource/2016/05/31/b2b_resource_WHP-SSD-SSDSMARTATTRIBUTES-APR16J.pdf)
describe its raw-versus-normalized wear and fixed 512-byte write units.
The [smartmontools drive database](https://github.com/smartmontools/smartmontools/blob/main/drivedb/drivedb.h)
documents model-specific units and hour encodings. Legacy SMI table detection
and raw life/32 MiB interpretation are cross-checked against
[CrystalDiskInfo's reader](https://github.com/hiyohiyo/CrystalDiskInfo/blob/master/AtaSmart.cpp).
[Seagate's guidance](https://github.com/Seagate/openSeaChest/wiki/Drive-Health-and-SMART)
explains why generic vendor counters cannot be compared directly.

New profiles need an authoritative definition or a documented independent
decoder, exact model/firmware/layout boundaries, and regression cases for
unrecognized variants. The drive brand alone is insufficient. If a logical
sector size is unavailable, Seagate byte totals stay absent. macOS may therefore
expose fewer ATA summaries while retaining the full attribute table.

Current readings use schema 27. The panel displays concise, decoded lifetime
summaries; the engineering probe additionally emits the complete raw/decoded
ATA table and extended NVMe fields. Those diagnostics do not add polling.
