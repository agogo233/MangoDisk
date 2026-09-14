# Resident system status

The resident adapter combines independent Core readings into native menu-bar/tray
entries and one reusable detail panel. It does not own filesystem cleanup,
application termination, or memory-reclamation algorithms.

## Ownership

| Area | Responsibility |
| --- | --- |
| Platform `system_resources` | Native counters, interface identity, local volume identity and capacity |
| Core `system_resources` | CPU/network/disk I/O deltas, selection policy, freshness, bounded trends and memory use cases |
| `sampling_workers` / `sampling_schedule` | One bounded worker per metric, demand, deadlines and generation checks |
| `runtime` | Cached snapshots, status transitions and sampling coordination |
| `presentation` | Bounded, coalesced visible-window publication and native display updates |
| `tray_display` | Shared formatting, localization, native entries and Windows bitmap ownership |
| `taskbar_display` | Windows native window, read-only shell geometry, placement, painting and fallback |
| `preferences` / `preference_schema` | Version migration, revision conflicts, native application and persistence rollback |
| `panel` / `main_window` | Independent presentation, source anchoring, focus and background-launch behavior |
| Vue resident settings store | Serialized optimistic edits and rollback to committed preferences |
| Vue settings / tray panel | Configuration preview and presentation of actual readings |

## Contracts and cadence

Fresh installations enable resident display with Logo, CPU and memory selected;
network and disk remain unselected. Windows starts in taskbar mode, preferring
automatic placement with background enabled. Automatic placement prefers right on
Windows 10, left on centered Windows 11 taskbars, and right on left-aligned ones.
The existing shell inspection refreshes this decision every second; manual choices
remain fixed. Manual placement stays in the outer gap on the requested side; if
it cannot fit, tray fallback replaces jumping across the task buttons. Automatic
placement may still use another fitting gap. Unknown environments prefer right,
and collision checks still apply. These defaults do not replace saved choices.

Resident preferences use schema version 7; resource snapshots use version 3.
Version 1 preferences retain background and memory-display choices. Version 2
preferences retain all selections and default to the original Windows tray mode.
Version 3 retains that mode and defaults the new position preference to right.
Version 4 retains all choices and enables the new taskbar background option.
Version 5 retains its saved left/right preference; only new installations default
to automatic placement. Version 6 retains automatic/manual choices and defaults
the new compact mode to off. Compact mode reduces horizontal cells from 50/84
to 38/76 DIP (percentage/network) without reducing the 12-DIP font,
color cues or fixed value/unit fields; paint and hit testing share those bounds.
Unknown persisted
versions are rejected for writes. Memory snapshots and release results retain
their separate version 1 contract.

CPU samples every 2 seconds, memory every 3, network every 1 and disk every 30.
Their freshness limits are respectively 5, 10, 5 and 90 seconds. All base metrics
remain active while resident display is enabled, regardless of the selected native
entries or panel visibility. Disabling resident mode stops periodic collection;
explicit device-catalogue requests may still read network and disk metadata.
The panel has an overview of all four base readings and a separate memory page.
Process details are requested only by the memory page or startup icon warming;
opening the overview does not enumerate processes. Reopening preserves the last
selected tab within the application session; a new process defaults to overview.
A different metric entry can navigate an already open panel.
CPU and network require two valid observations; unavailable values remain `—`.
Each native trend retains at most 96 points over 80 seconds for the 60-second viewport. `observedAtMs` anchors the time
axis even when the latest valid sample is older than the current snapshot.

A slow native query stays in flight until it returns. Changing demand invalidates
its generation without spawning replacement threads. Native panel focus explicitly stops chart animation even when WebView2 leaves
`document.hidden` false. Hidden WebViews receive no periodic reading events and reload the native cache when opened. Native display
updates from one completion burst are coalesced over 50 ms and periodic native
refreshes are limited to once per second. A separate presentation worker uses one
bounded wake slot and reads the latest cached snapshot; a slow native UI operation
cannot block sampling or accumulate old snapshots. Preference changes and periodic
native refreshes share a separate transaction gate; sampling only briefly reads
the committed settings snapshot. Native application, persistence and rollback do
not hold that snapshot lock. Refreshes read settings after acquiring the gate so
an older refresh cannot overwrite a committed display. Failed updates retain the
previous settings, and successful updates log their elapsed time.
`resident_presentation_delayed` and
`resident_sampling_delayed` distinguish UI waits from coordinator stalls; recovery
is logged once, and the periodic sample summary includes maximum loop latency.

macOS interface names and physical-interface classification are cached for at
most 10 seconds, with immediate invalidation when interface topology changes.
Connection state, routes and byte counters are always read on each network tick.
Automatic selection avoids virtual/loopback interfaces; an explicit selection is
never silently replaced. Only local writable/system or user-mounted disks are
listed; a selected volume is identified independently of its current mount name.

## Native and interaction boundaries

macOS uses one combined entry with a Retina-aware drawing-handler image. Labels
and values occupy two rows; network directions retain colored arrows and explicit
units. Rate digits are right-aligned in a fixed field; units have a separate fixed
origin so digit-count and unit changes cannot move neighboring text. Native column geometry keeps numeric changes
from resizing the entry (220 pt for all metrics plus the brand). Native appearance
and recreated buttons invalidate the image cache; accessibility retains the textual
summary. The adapter
uses Tauri 2.11 native tray access; its minor version is constrained so an upgrade
receives native UI regression checks. Windows uses at most six retained tray handles and
bounded native-size bitmaps. Logical entry IDs are stable within MangoDisk; they
are not Windows notification GUIDs and do not guarantee retained shell placement
across restarts or changed selections. Windows controls icon order and overflow.
Hidden Windows entries are absent from the notification area even while their
handles are retained. Update their tooltip only after showing them again; a
native modify call on a hidden entry fails and can reject a preference change.
When opening a Windows panel, move it to its target monitor before applying
physical dimensions; a hidden window can still carry the previous monitor DPI.
The current Windows CPU source reports `unsupported` for multiple processor groups
instead of presenting a partial group as whole-machine usage.

The settings display card owns the resident enable switch; disabling it collapses
its options without resetting metric, source or appearance preferences. Login
startup is configured independently in General. A login launch stays hidden only
when resident display is enabled. Disabling display keeps the main window open;
Windows closes the application when that window is subsequently closed, while
macOS preserves Dock reopening. Explicit Quit always exits the application.

Settings apply directly to the native status surface without a duplicate simulated
preview. The brand toggle is a fixed card
beside the reorderable metric cards; it is not part of metric ordering. macOS metric reordering
uses local pointer events because native file-drop handling is also needed by
cleanup pages. Keyboard reordering restores focus after keyed DOM moves.

Diagnostics record status transitions, network selection reasons, failure
stages/codes, sampling percentiles, discarded generations, tray updates and tray-handle counts. Device identities,
private mount paths and raw machine reports do not belong in application logs or
committed validation reports.

Run `pnpm check`, Core tests, Platform system-resource tests and resident-adapter
tests on applicable macOS and Windows environments. Native acceptance includes
cold/warm opening, overflow anchors, focus, lifecycle/login startup, disconnection,
volume removal, native-size readability and release-build resource measurements.
Exercise all 32 metric/brand combinations in the real notification area, including
brand-hidden transitions and the all-disabled fallback; model-only tests cannot
verify the native entry lifecycle.


## Optional Windows taskbar display

Windows can show the same readings in either retained tray icons or one native
Win32 window beside taskbar controls. The window is not parented to Explorer and
does not resize Explorer children, reserve shell space or alter process DPI.
A top-level owner relationship is established at window creation. The native
surface is recreated after Explorer restarts; no WebView or sampler is recreated.
Z-order is repaired only when its topmost state is lost, so routine updates do
not raise the taskbar over native shell menus. Positioning checks actual shell
occlusion as well as the API result before hiding the fallback tray entries.
The primary taskbar supports all four screen edges. Horizontal bars use columns;
vertical bars stack cells and split network values/units into four lines. Painting,
hit testing and panel anchors share these rectangles. Window regions are resized
before occlusion checks, since SetWindowPos does not resize a previous rounded
region when switching orientation. Insufficient size or free
space uses the existing tray entries and exposes a typed status to settings. The
saved mode stays unchanged, so a usable layout can recover automatically.

A dedicated MTA thread reads cached UI Automation control bounds once per second.
While the taskbar is offscreen, it checks only its bounds every 200 ms and skips
UI Automation; this normal auto-hide state does not activate tray fallback.
A separate native window thread handles input, paints a small GDI backbuffer and
checks visibility every 100 ms while enabled. Geometry older than three seconds
is rejected. Shell calls cannot block Tauri's event loop or resource samplers.
Model updates replace one bounded snapshot, and GDI objects are released after
painting. Disabled taskbar presentation stops the window timer. The shell query
thread performs no inspection while tray mode is selected or resident display is disabled.

Placement excludes occupied controls with a margin. Manual left/right selects
only the first/last free gap and aligns to its outer edge (top/bottom on a vertical
taskbar). A too-narrow gap activates tray fallback with a compact-mode suggestion;
it does not move the strip to the opposite side. Automatic placement may choose
another fitting gap. With a free screen edge, Left retains only the normal 4 DIP
margin instead of following centered task buttons. No system control is moved to
manufacture space. A changed
shell layout is detected on the next inspection; no undocumented taskbar-width
mutation is used. Temporary fullscreen/auto-hide visibility differs from a layout
failure, which activates tray fallback. Native status transitions and query stages
are logged without collecting control names or application titles.

Start, Search and Quick Settings temporarily hide the strip while their shell
process owns foreground; closing the system flyout restores it. This avoids
forcing focus away from protected shell surfaces or leaving our panel behind them.
Foreground executable names are checked only when the foreground window changes;
paths are never logged. Ordinary clicks use the existing focused detail panel,
anchored to the clicked column; a second click toggles it closed. The product
main window is not revealed. Right click exposes Open, Settings and Quit. Window callbacks guard
against synchronous Win32 message reentry. Settings enable pointer/keyboard reordering only where the chosen surface
can honor it (macOS and Windows taskbar mode).


The Windows taskbar background option defaults to opaque. Transparent mode uses
a layered window with DirectWrite grayscale text and premultiplied BGRA. A cached
software Direct2D DC target renders colored glyphs directly into alpha, avoiding
the previous white-on-black GDI intensity-to-coverage conversion. Regular Segoe UI
keeps stroke weight close to the opaque reference; both paths retain the same
physical font size and cell rectangles. Factories, target and DPI-specific text
format stay on the native window thread; a failed frame discards them for recovery. Background pixels use alpha 1/255
rather than zero so clicks still reach the entire cell; hover raises that alpha
to 28/255. ClearType remains enabled only for the opaque, known-background path.
Mode transitions change the native window style and invalidate its surface.
The transparent surface is presented before occlusion hit testing, because a
newly layered window has no hit-testable pixels. Allocation/presentation failure
hides the surface and activates the existing tray fallback; diagnostics record
the failing stage and recovery, not every frame.

Windows taskbar network columns keep a fixed 84 DIP width, or 76 DIP in compact mode. The arrow, right-aligned
value and unit occupy independent fields; upload arrows are red and download arrows
blue, matching macOS. Side taskbars retain separate value/unit lines. Shared text-run
geometry drives both opaque GDI drawing and transparent DirectWrite drawing. Taskbar rates
use one decimal below 100, omit trailing `.0`, and round larger rates to integers.
The 28-DIP numeric field also fits rounded 1000 without clipping or changing units;
shared tray text and tooltips keep their existing precision.

### Overview history and disk activity

Resource readings use schema version 3; frontend adapters reject mismatched versions. Memory history records occupancy from the existing three-second sampler. CPU and memory use a fixed 0–100% scale. Network and disk activity share a symmetric scale: upload/write above zero, download/read below it. Gaps remain blank. The frontend buffers one sampling interval plus 250 ms before revealing each completed segment from the right; numeric readings remain live. Core retains up to 80 seconds / 96 samples so a reopened chart can reconstruct the buffered minute and offscreen endpoints. The frontend retains two additional intervals at the left edge. During a brief delivery delay, the playhead waits for completed data and catches up at no more than 1.1× speed; genuinely expired data still scrolls out. Pausing demand preserves existing readings and history with their original timestamps, while source changes clear the corresponding history. Rate scales hold their range for 30 seconds before a substantial reduction, and range changes ease over 600 ms using a shared SVG group. Horizontal scrolling uses that group’s native transform instead of a composited CSS bitmap, preserving vector strokes at fractional positions. Reduced-motion mode applies scale changes immediately and disables continuous scrolling; hidden or fully expired charts stop their frame loop.

Disk capacity belongs to the selected volume. Disk activity is explicitly system-wide block-device I/O, sampled independently every two seconds while resident mode is enabled. macOS reads IOKit block-storage driver counters once per driver; Windows reads localized-independent PDH PhysicalDisk counters once per instance, excluding `_Total`. These counters describe block storage, not per-volume or application file traffic. Missing counters show unavailable rather than zero. Device-set changes, counter rollback, and sleep invalidate the monotonic rate baseline.

`resident_disk_io_state` records capability/freshness transitions and scope; failures log typed codes without device identities. Sampling durations join the bounded periodic summary. Closing the panel or entering Memory preserves disk activity demand and its baseline. Disabling resident mode stops the worker requests and discards the baseline. Healthy idle intervals remain zero-valued samples; unavailable intervals remain gaps.

CPU baseline-only observations retain the last reading until its normal five-second expiry instead of clearing the chart. Recovery can bring forward at most two serialized queries by 250 ms, then returns to normal cadence until a valid interval is available. `resident_cpu_baseline` identifies first observations, invalid intervals, counter resets, and stalled counters; `resident_cpu_recovered` records bounded recovery attempts without raw counters or machine identifiers.
